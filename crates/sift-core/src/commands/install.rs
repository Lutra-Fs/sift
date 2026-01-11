//! Install command implementation.
//!
//! Orchestrates installing MCP servers and skills from registries or local sources,
//! updating configuration, resolving versions, and writing to client configs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::client::ClientAdapter;
use crate::client::ClientContext;
use crate::client::claude_code::ClaudeCodeClient;
use crate::config::{
    ConfigScope, ConfigStore, McpConfigEntry, OwnershipStore, SkillConfigEntry, merge_configs,
};
use crate::fs::LinkMode;
use crate::git::GitFetcher;
use crate::install::orchestrator::{InstallMcpRequest, InstallOrchestrator};
use crate::install::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, resolve_scope,
};
use crate::mcp::spec::McpResolvedServer;
use crate::skills::installer::{GitSkillMetadata, SkillInstaller};
use crate::source::{ResolvedSource, SourceResolver};
use crate::version::store::LockfileStore;

/// Default runtime for MCP servers when not specified
const DEFAULT_RUNTIME: &str = "npx";

/// Default version constraint when not specified
const DEFAULT_VERSION: &str = "latest";

/// What to install: MCP server or skill
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallTarget {
    /// Install an MCP server
    Mcp,
    /// Install a skill
    Skill,
}

/// Options for the install command
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Target type (mcp or skill)
    pub target: InstallTarget,
    /// Name of the package to install
    pub name: String,
    /// Source specification (e.g., "registry:name" or "local:/path")
    pub source: Option<String>,
    /// Registry name for disambiguation
    pub registry: Option<String>,
    /// Version constraint
    pub version: Option<String>,
    /// Configuration scope
    pub scope: Option<ConfigScope>,
    /// Force overwrite existing entries
    pub force: bool,
    /// Runtime type for MCP servers (node, bun, docker, etc.)
    pub runtime: Option<String>,
    /// Transport type for MCP servers (stdio or http)
    pub transport: Option<String>,
    /// HTTP URL for MCP servers
    pub url: Option<String>,
    /// Environment variables for MCP servers (KEY=VALUE)
    pub env: Vec<String>,
    /// HTTP headers for MCP servers (KEY=VALUE)
    pub headers: Vec<String>,
    /// Explicit stdio command for MCP servers
    pub command: Vec<String>,
}

struct ContinueSkillInstallRequest<'a> {
    name: &'a str,
    source: &'a str,
    entry: &'a SkillConfigEntry,
    options: &'a InstallOptions,
    resolved_source: &'a ResolvedSource,
    registry_metadata: Option<&'a crate::source::RegistryMetadata>,
    warnings: Vec<String>,
    client: &'a ClaudeCodeClient,
    ctx: &'a ClientContext,
}

impl InstallOptions {
    /// Create new install options for an MCP server
    pub fn mcp(name: impl Into<String>) -> Self {
        Self {
            target: InstallTarget::Mcp,
            name: name.into(),
            source: None,
            registry: None,
            version: None,
            scope: None,
            force: false,
            runtime: None,
            transport: None,
            url: None,
            env: Vec::new(),
            headers: Vec::new(),
            command: Vec::new(),
        }
    }

    /// Create new install options for a skill
    pub fn skill(name: impl Into<String>) -> Self {
        Self {
            target: InstallTarget::Skill,
            name: name.into(),
            source: None,
            registry: None,
            version: None,
            scope: None,
            force: false,
            runtime: None,
            transport: None,
            url: None,
            env: Vec::new(),
            headers: Vec::new(),
            command: Vec::new(),
        }
    }

    /// Set the source specification
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set the registry name for disambiguation
    pub fn with_registry(mut self, registry: impl Into<String>) -> Self {
        self.registry = Some(registry.into());
        self
    }

    /// Set the version constraint
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the configuration scope
    pub fn with_scope(mut self, scope: ConfigScope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Set the force flag
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Set the runtime type
    pub fn with_runtime(mut self, runtime: impl Into<String>) -> Self {
        self.runtime = Some(runtime.into());
        self
    }

    /// Set the transport type for MCP servers
    pub fn with_transport(mut self, transport: impl Into<String>) -> Self {
        self.transport = Some(transport.into());
        self
    }

    /// Set the HTTP URL for MCP servers
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Add an environment variable for MCP servers (KEY=VALUE)
    pub fn with_env(mut self, env: impl Into<String>) -> Self {
        self.env.push(env.into());
        self
    }

    /// Add an HTTP header for MCP servers (KEY=VALUE)
    pub fn with_header(mut self, header: impl Into<String>) -> Self {
        self.headers.push(header.into());
        self
    }

    /// Set an explicit stdio command for MCP servers
    pub fn with_command<I, S>(mut self, command: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.command = command.into_iter().map(Into::into).collect();
        self
    }
}

/// Report from an install operation
#[derive(Debug, Clone)]
pub struct InstallReport {
    /// Name of the installed package
    pub name: String,
    /// Whether the installation changed anything
    pub changed: bool,
    /// Whether the config was applied to client configs
    pub applied: bool,
    /// Any warnings generated during installation
    pub warnings: Vec<String>,
}

/// Install command orchestrator
#[derive(Debug)]
pub struct InstallCommand {
    /// Home directory
    home_dir: PathBuf,
    /// Project root directory
    project_root: PathBuf,
    /// State directory for lockfiles
    state_dir: PathBuf,
    /// Global config directory
    global_config_dir: PathBuf,
    /// Link mode for skills
    link_mode: LinkMode,
}

impl InstallCommand {
    /// Create a new install command
    pub fn new(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        link_mode: LinkMode,
    ) -> Self {
        // Derive global_config_dir from system defaults when not explicitly provided
        let global_config_dir = dirs::config_dir()
            .map(|p| p.join("sift"))
            .unwrap_or_else(|| home_dir.join(".config").join("sift"));
        Self::with_global_config_dir(
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            link_mode,
        )
    }

    /// Create a new install command with custom global config directory
    pub fn with_global_config_dir(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        global_config_dir: PathBuf,
        link_mode: LinkMode,
    ) -> Self {
        Self {
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            link_mode,
        }
    }

    /// Create an install command with default paths
    pub fn with_defaults() -> anyhow::Result<Self> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        let project_root = std::env::current_dir()?;
        let state_dir = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .ok_or_else(|| anyhow::anyhow!("Could not determine state directory"))?
            .join("sift");
        let global_store = ConfigStore::from_scope(ConfigScope::Global)?;
        let project_store = ConfigStore::from_scope(ConfigScope::PerProjectShared)?;
        let global = global_store.load()?;
        let project = project_store.load()?;
        let merged = merge_configs(Some(global), Some(project), &project_root)?;
        let link_mode = merged.link_mode.unwrap_or(LinkMode::Auto);
        let global_config_dir = global_store
            .config_path()
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .to_path_buf();

        Self::with_defaults_from_paths(home_dir, project_root, state_dir, global_config_dir).map(
            |mut cmd| {
                cmd.link_mode = link_mode;
                cmd
            },
        )
    }

    pub fn with_defaults_from_paths(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        global_config_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let global_store = ConfigStore::from_paths(
            ConfigScope::Global,
            global_config_dir.clone(),
            project_root.clone(),
        );
        let project_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            global_config_dir.clone(),
            project_root.clone(),
        );
        let global = global_store.load()?;
        let project = project_store.load()?;
        let merged = merge_configs(Some(global), Some(project), &project_root)?;
        let link_mode = merged.link_mode.unwrap_or(LinkMode::Auto);

        Ok(Self::with_global_config_dir(
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            link_mode,
        ))
    }

    /// Execute the install command
    pub fn execute(&self, options: &InstallOptions) -> anyhow::Result<InstallReport> {
        match options.target {
            InstallTarget::Mcp => self.install_mcp(options),
            InstallTarget::Skill => self.install_skill(options),
        }
    }

    /// Install an MCP server
    fn install_mcp(&self, options: &InstallOptions) -> anyhow::Result<InstallReport> {
        let env = parse_key_values(&options.env, "env")?;
        let headers = parse_key_values(&options.headers, "header")?;
        let has_command = !options.command.is_empty();
        let has_url = options.url.as_ref().is_some_and(|url| !url.is_empty());
        let transport = resolve_transport(options.transport.as_deref(), has_command, has_url)?;
        let mut warnings = Vec::new();
        let mut name = options.name.clone();
        let mut args = Vec::new();
        let mut runtime = options.runtime.clone();
        let url = options.url.clone();
        let mut version = options.version.clone();

        let source = if has_command || has_url {
            if options.source.is_some() {
                warnings.push(
                    "Ignoring --source because an explicit command or URL was provided."
                        .to_string(),
                );
            }
            if options.registry.is_some() {
                warnings.push(
                    "Ignoring --registry because an explicit command or URL was provided."
                        .to_string(),
                );
            }
            if options.runtime.is_some() {
                warnings.push(
                    "Ignoring --runtime because an explicit command or URL was provided."
                        .to_string(),
                );
                runtime = None;
            }
            if version.is_some() {
                warnings.push(
                    "Ignoring version because an explicit command or URL was provided.".to_string(),
                );
                version = None;
            }
            if has_command {
                let command = options
                    .command
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("stdio command cannot be empty"))?;
                runtime = Some("shell".to_string());
                args = options.command.iter().skip(1).cloned().collect();
                format!("local:{}", command)
            } else {
                format!("local:{}", name)
            }
        } else {
            let resolved = self.resolve_name_and_source(
                &options.name,
                options.source.as_deref(),
                options.registry.as_deref(),
            )?;
            let ResolvedNameAndSource {
                name: resolved_name,
                source: resolved_source,
                source_is_registry,
                source_explicit,
                warnings: resolved_warnings,
            } = resolved;
            name = resolved_name;
            warnings.extend(resolved_warnings);
            warnings.extend(self.registry_warnings(source_is_registry, source_explicit)?);
            if source_is_registry
                && version.is_some()
                && let Some(false) = self.registry_supports_version_pinning(&resolved_source)?
            {
                warnings.push(
                    "Registry does not support version pinning; ignoring requested version."
                        .to_string(),
                );
                version = None;
            }
            resolved_source
        };

        let entry = McpConfigEntry {
            transport: Some(transport.to_string()),
            source: source.clone(),
            runtime,
            args,
            url,
            headers,
            targets: None,
            ignore_targets: None,
            env,
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        // Create client adapter and context
        let client = ClaudeCodeClient::new();
        let ctx = ClientContext::new(self.home_dir.clone(), self.project_root.clone());

        // Build resolved server spec (simplified for now)
        let servers = self.build_mcp_servers(&name, &source, &entry, version.as_deref())?;

        // Determine scope request
        let scope_request = match options.scope {
            Some(s) => ScopeRequest::Explicit(s),
            None => ScopeRequest::Auto,
        };

        let repo = RepoStatus::from_project_root(&ctx.project_root);
        let resolution = resolve_scope(
            ResourceKind::Mcp,
            scope_request,
            client.capabilities().mcp,
            repo,
        )?;

        let config_scope = match &resolution {
            ScopeResolution::Apply(decision) => decision.scope,
            ScopeResolution::Skip { .. } => options.scope.unwrap_or(ConfigScope::PerProjectShared),
        };

        // Create orchestrator
        let config_store = self.create_config_store(config_scope)?;
        let ownership_store = self.create_ownership_store();
        let skill_installer = self.create_skill_installer();
        let orchestrator = InstallOrchestrator::new(
            config_store,
            ownership_store,
            skill_installer,
            self.link_mode,
        );

        // Execute installation
        let report = orchestrator.install_mcp(
            &client,
            &ctx,
            InstallMcpRequest {
                name: &name,
                entry,
                servers: &servers,
                resolution,
                force: options.force,
                declared_version: version.as_deref(),
            },
        )?;

        warnings.extend(report.warnings);
        Ok(InstallReport {
            name,
            changed: matches!(report.outcome, crate::install::InstallOutcome::Changed),
            applied: report.applied,
            warnings,
        })
    }

    /// Install a skill
    fn install_skill(&self, options: &InstallOptions) -> anyhow::Result<InstallReport> {
        // Build config entry
        let resolved = self.resolve_name_and_source(
            &options.name,
            options.source.as_deref(),
            options.registry.as_deref(),
        )?;
        let ResolvedNameAndSource {
            name,
            source,
            source_is_registry,
            source_explicit,
            warnings: resolved_warnings,
        } = resolved;
        let mut warnings = Vec::new();
        warnings.extend(resolved_warnings);
        warnings.extend(self.registry_warnings(source_is_registry, source_explicit)?);

        let entry = SkillConfigEntry {
            source: source.clone(),
            version: options.version.clone(),
            targets: None,
            ignore_targets: None,
            reset_version: false,
        };

        // Create client adapter and context
        let client = ClaudeCodeClient::new();
        let ctx = ClientContext::new(self.home_dir.clone(), self.project_root.clone());

        // Resolve source to a fetchable specification
        let source_resolver = self.create_source_resolver()?;

        // Check if this is a registry source that might have nested marketplaces
        if let Some(registry_part) = source.strip_prefix("registry:") {
            // Try to expand for nested marketplaces
            let resolutions = source_resolver.resolve_registry_with_expansion(registry_part)?;

            // Check for collisions
            crate::source::SourceResolver::detect_collisions(&resolutions)
                .map_err(|e| anyhow::anyhow!("Name collision detected: {}", e))?;

            // If first resolution is a group, install all nested plugins directly
            if resolutions
                .first()
                .map(|r| r.metadata.is_group)
                .unwrap_or(false)
            {
                let mut changed = false;
                let mut applied = false;
                let mut nested_warnings = warnings;

                for resolution in &resolutions {
                    if resolution.metadata.is_group {
                        continue; // Skip the parent group entry
                    }

                    // Install each nested plugin directly using the resolved GitSpec
                    // Use short name (first alias) for filesystem path, canonical name for identification
                    let nested_name = resolution
                        .metadata
                        .aliases
                        .first()
                        .cloned()
                        .unwrap_or_else(|| resolution.metadata.skill_name.clone());
                    let resolved_source = ResolvedSource::Git(resolution.git_spec.clone());
                    let registry_metadata = Some(resolution.metadata.clone());

                    // Build config entry for this nested plugin
                    let nested_entry = SkillConfigEntry {
                        source: resolution.metadata.original_source.clone(),
                        version: None, // Use latest/default
                        targets: None,
                        ignore_targets: None,
                        reset_version: false,
                    };

                    // Create options for nested plugin
                    let mut nested_options = options.clone();
                    nested_options.name = nested_name.clone();
                    nested_options.source = Some(resolution.metadata.original_source.clone());

                    let request = ContinueSkillInstallRequest {
                        name: &nested_name,
                        source: &resolution.metadata.original_source,
                        entry: &nested_entry,
                        options: &nested_options,
                        resolved_source: &resolved_source,
                        registry_metadata: registry_metadata.as_ref(),
                        warnings: Vec::new(),
                        client: &client,
                        ctx: &ctx,
                    };
                    match self.continue_skill_install(request) {
                        Ok(report) => {
                            changed = changed || report.changed;
                            applied = applied || report.applied;
                            nested_warnings.extend(report.warnings);
                        }
                        Err(e) => {
                            // If installation fails, report but continue
                            nested_warnings.push(format!(
                                "Failed to install nested plugin '{}': {}",
                                nested_name, e
                            ));
                        }
                    }
                }

                // Build combined name for all installed nested plugins
                let nested_names: Vec<_> = resolutions
                    .iter()
                    .filter(|r| !r.metadata.is_group)
                    .map(|r| r.metadata.skill_name.clone())
                    .collect();
                let combined_name = format!("{} ({})", name, nested_names.join(", "));

                // Return combined report
                return Ok(InstallReport {
                    name: combined_name,
                    changed,
                    applied,
                    warnings: nested_warnings,
                });
            }

            // No nested marketplace or single plugin - use first resolution
            let first_resolution = &resolutions[0];
            let resolved_source = ResolvedSource::Git(first_resolution.git_spec.clone());
            let registry_metadata = Some(first_resolution.metadata.clone());

            let request = ContinueSkillInstallRequest {
                name: &name,
                source: &source,
                entry: &entry,
                options,
                resolved_source: &resolved_source,
                registry_metadata: registry_metadata.as_ref(),
                warnings,
                client: &client,
                ctx: &ctx,
            };
            return self.continue_skill_install(request);
        }

        // Non-registry source - use normal resolution
        let (resolved_source, registry_metadata) =
            source_resolver.resolve_with_metadata(&source)?;

        let request = ContinueSkillInstallRequest {
            name: &name,
            source: &source,
            entry: &entry,
            options,
            resolved_source: &resolved_source,
            registry_metadata: registry_metadata.as_ref(),
            warnings,
            client: &client,
            ctx: &ctx,
        };
        self.continue_skill_install(request)
    }

    /// Continue skill installation after source resolution.
    ///
    /// This helper method handles the actual installation logic after the source
    /// has been resolved (either normally or via nested marketplace expansion).
    fn continue_skill_install(
        &self,
        request: ContinueSkillInstallRequest<'_>,
    ) -> anyhow::Result<InstallReport> {
        let ContinueSkillInstallRequest {
            name,
            source,
            entry,
            options,
            resolved_source,
            registry_metadata,
            mut warnings,
            client,
            ctx,
        } = request;
        let mut git_metadata = None;
        let mut resolved_version = options
            .version
            .clone()
            .unwrap_or_else(|| DEFAULT_VERSION.to_string());
        let mut constraint = resolved_version.clone();
        let mut registry = "default".to_string();
        let cache_dir;

        match resolved_source {
            ResolvedSource::Git(spec) => {
                GitFetcher::ensure_git_version()?;

                let fetcher = GitFetcher::new(self.state_dir.clone());

                // Check lockfile for existing resolved version
                let lockfile = LockfileStore::load(
                    Some(self.project_root.clone()),
                    self.state_dir.join("locks"),
                )?;
                let existing = lockfile.skills.get(name);

                let result = if !options.force {
                    if let Some(locked) = existing.filter(|e| e.is_installed()) {
                        // Use cached version from lockfile
                        crate::git::FetchResult {
                            cache_dir: locked.cache_src_path.clone().unwrap_or_else(|| {
                                self.state_dir.join("cache").join("skills").join(name)
                            }),
                            commit_sha: locked.resolved_version.clone(),
                        }
                    } else {
                        fetcher.fetch(spec, name, options.force)?
                    }
                } else {
                    fetcher.fetch(spec, name, options.force)?
                };

                cache_dir = result.cache_dir;
                resolved_version = result.commit_sha;
                constraint = spec.reference.clone().unwrap_or_else(|| "HEAD".to_string());
                registry = if let Some(meta) = registry_metadata {
                    meta.original_source.clone()
                } else {
                    source.to_string()
                };
                git_metadata = Some(GitSkillMetadata {
                    repo: spec.repo_url.clone(),
                    reference: spec.reference.clone(),
                    subdir: spec.subdir.clone(),
                });
            }
            ResolvedSource::Local(spec) => {
                cache_dir = spec.path.clone();
                // For local sources, we don't fetch - just use the path directly
                if !cache_dir.exists() {
                    anyhow::bail!("Local skill path does not exist: {}", cache_dir.display());
                }
            }
        }

        // Determine scope request
        let scope_request = match options.scope {
            Some(s) => ScopeRequest::Explicit(s),
            None => ScopeRequest::Auto,
        };

        let repo = RepoStatus::from_project_root(&ctx.project_root);
        let resolution = resolve_scope(
            ResourceKind::Skill,
            scope_request,
            client.capabilities().skills,
            repo,
        )?;

        let config_scope = match &resolution {
            ScopeResolution::Apply(decision) => decision.scope,
            ScopeResolution::Skip { .. } => options.scope.unwrap_or(ConfigScope::PerProjectShared),
        };

        // Create orchestrator
        let config_store = self.create_config_store(config_scope)?;
        let ownership_store = self.create_ownership_store();
        let skill_installer = self.create_skill_installer();
        let orchestrator = InstallOrchestrator::new(
            config_store,
            ownership_store,
            skill_installer,
            self.link_mode,
        );

        // Execute installation
        let report = orchestrator.install_skill(
            client,
            ctx,
            name,
            entry.clone(),
            &cache_dir,
            resolution,
            options.force,
            &resolved_version,
            &constraint,
            &registry,
            git_metadata,
        )?;

        warnings.extend(report.warnings);
        Ok(InstallReport {
            name: name.to_string(),
            changed: matches!(report.outcome, crate::install::InstallOutcome::Changed),
            applied: report.applied,
            warnings,
        })
    }

    // Helper methods

    fn create_config_store(&self, scope: ConfigScope) -> anyhow::Result<ConfigStore> {
        Ok(ConfigStore::from_paths(
            scope,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        ))
    }

    fn create_ownership_store(&self) -> OwnershipStore {
        OwnershipStore::new(
            self.state_dir.join("locks"),
            Some(self.project_root.clone()),
        )
    }

    fn create_skill_installer(&self) -> SkillInstaller {
        SkillInstaller::new(
            self.state_dir.join("locks"),
            Some(self.project_root.clone()),
        )
    }

    fn create_source_resolver(&self) -> anyhow::Result<SourceResolver> {
        // Load registry configurations from global and project configs
        let global_store = ConfigStore::from_paths(
            ConfigScope::Global,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let project_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let global = global_store.load()?;
        let project = project_store.load()?;
        let merged = merge_configs(Some(global), Some(project), &self.project_root)?;

        // Convert registry entries to RegistryConfig
        let mut registries = std::collections::HashMap::new();
        for (key, entry) in merged.registry {
            let config: crate::registry::RegistryConfig = entry.try_into()?;
            registries.insert(key, config);
        }

        Ok(SourceResolver::new(
            self.state_dir.clone(),
            self.project_root.clone(),
            registries,
        ))
    }

    fn build_mcp_servers(
        &self,
        name: &str,
        _source: &str,
        entry: &McpConfigEntry,
        version: Option<&str>,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        if entry.transport.as_deref() == Some("http") {
            let url = entry
                .url
                .clone()
                .ok_or_else(|| anyhow::anyhow!("HTTP transport requires a URL"))?;
            return Ok(vec![McpResolvedServer::http(
                name.to_string(),
                url,
                entry.headers.clone(),
            )]);
        }

        if entry.runtime.as_deref() == Some("shell") {
            let command = entry
                .source
                .strip_prefix("local:")
                .unwrap_or(&entry.source)
                .to_string();
            return Ok(vec![McpResolvedServer::stdio(
                name.to_string(),
                command,
                entry.args.clone(),
                entry.env.clone(),
            )]);
        }

        // Build a resolved server spec from the entry.
        // NOTE: Registry resolution is not implemented yet; we pass name@version as args.
        let runtime = entry.runtime.as_deref().unwrap_or(DEFAULT_RUNTIME);
        let command = runtime.to_string();

        // For registry sources, args should be derived from registry metadata.
        // TODO: Resolve from configured registries instead of relying on runtime tools.
        let resolved = version.unwrap_or(DEFAULT_VERSION);
        let mut args = vec![format!("{}@{}", name, resolved)];
        args.extend(entry.args.clone());

        Ok(vec![McpResolvedServer::stdio(
            name.to_string(),
            command,
            args,
            entry.env.clone(),
        )])
    }

    fn resolve_name_and_source(
        &self,
        input: &str,
        source: Option<&str>,
        registry: Option<&str>,
    ) -> anyhow::Result<ResolvedNameAndSource> {
        let mut warnings = Vec::new();

        if let Some(explicit) = source {
            let (normalized, normalized_warning) = self.normalize_explicit_source(explicit)?;
            if let Some(warning) = normalized_warning {
                warnings.push(warning);
            }
            if registry.is_some() {
                warnings.push("Ignoring --registry because --source was provided.".to_string());
            }
            let is_registry = normalized.starts_with("registry:");
            return Ok(ResolvedNameAndSource {
                name: input.to_string(),
                source: normalized,
                source_is_registry: is_registry,
                source_explicit: true,
                warnings,
            });
        }

        if let Some(inferred) = self.infer_local_source(input) {
            return Ok(inferred);
        }

        if let Some(inferred) = self.infer_git_source(input) {
            return Ok(inferred);
        }

        let source = if let Some(selected) = registry {
            format!("registry:{}/{}", selected, input)
        } else {
            format!("registry:{}", input)
        };

        Ok(ResolvedNameAndSource {
            name: input.to_string(),
            source,
            source_is_registry: true,
            source_explicit: registry.is_some(),
            warnings,
        })
    }

    fn infer_local_source(&self, input: &str) -> Option<ResolvedNameAndSource> {
        if !is_local_path(input, &self.project_root) {
            return None;
        }
        let name = derive_name_from_path(input).ok()?;
        Some(ResolvedNameAndSource {
            name,
            source: format!("local:{}", input),
            source_is_registry: false,
            source_explicit: false,
            warnings: Vec::new(),
        })
    }

    fn infer_git_source(&self, input: &str) -> Option<ResolvedNameAndSource> {
        if !is_git_like(input) {
            return None;
        }
        let source = normalize_git_source(input);
        let name = derive_name_from_git_source(&source).ok()?;
        Some(ResolvedNameAndSource {
            name,
            source,
            source_is_registry: false,
            source_explicit: false,
            warnings: Vec::new(),
        })
    }

    fn normalize_explicit_source(&self, source: &str) -> anyhow::Result<(String, Option<String>)> {
        if source.starts_with("registry:")
            || source.starts_with("local:")
            || source.starts_with("github:")
            || source.starts_with("git:")
        {
            return Ok((source.to_string(), None));
        }

        if is_local_path(source, &self.project_root) {
            let normalized = format!("local:{}", source);
            return Ok((
                normalized.clone(),
                Some(format!(
                    "Normalized source '{}' to '{}'.",
                    source, normalized
                )),
            ));
        }

        if is_git_like(source) {
            let normalized = normalize_git_source(source);
            return Ok((
                normalized.clone(),
                Some(format!(
                    "Normalized source '{}' to '{}'.",
                    source, normalized
                )),
            ));
        }

        anyhow::bail!(
            "Invalid source format: must be 'registry:', 'local:', 'github:', 'git:', a path, or a git URL"
        )
    }

    fn registry_warnings(
        &self,
        source_is_registry: bool,
        source_explicit: bool,
    ) -> anyhow::Result<Vec<String>> {
        if !source_is_registry || source_explicit {
            return Ok(Vec::new());
        }
        let global_store = ConfigStore::from_paths(
            ConfigScope::Global,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let project_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let global = global_store.load()?;
        let project = project_store.load()?;
        let merged = merge_configs(Some(global), Some(project), &self.project_root)?;
        if merged.registry.len() > 1 {
            return Ok(vec![
                "Multiple registries are configured; use --registry or an explicit registry source."
                    .to_string(),
            ]);
        }
        Ok(Vec::new())
    }

    fn registry_supports_version_pinning(&self, source: &str) -> anyhow::Result<Option<bool>> {
        if !source.starts_with("registry:") {
            return Ok(None);
        }
        let registry_key = source
            .strip_prefix("registry:")
            .and_then(|value| value.split('/').next());
        let global_store = ConfigStore::from_paths(
            ConfigScope::Global,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let project_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let global = global_store.load()?;
        let project = project_store.load()?;
        let merged = merge_configs(Some(global), Some(project), &self.project_root)?;
        let entry = registry_key
            .and_then(|key| merged.registry.get(key))
            .or_else(|| {
                if merged.registry.len() == 1 {
                    merged.registry.values().next()
                } else {
                    None
                }
            });
        let Some(entry) = entry else {
            return Ok(None);
        };
        let config: crate::registry::RegistryConfig = entry.clone().try_into()?;
        Ok(Some(
            crate::registry::capabilities_for(&config).supports_version_pinning,
        ))
    }
}

#[derive(Debug)]
struct ResolvedNameAndSource {
    name: String,
    source: String,
    source_is_registry: bool,
    source_explicit: bool,
    warnings: Vec<String>,
}

fn is_local_path(input: &str, project_root: &Path) -> bool {
    if input.starts_with("./")
        || input.starts_with("../")
        || input.starts_with('/')
        || input.starts_with("~/")
    {
        return true;
    }
    project_root.join(input).exists()
}

fn is_git_like(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git://")
        || input.starts_with("git+")
        || input.starts_with("github:")
        || input.starts_with("git:")
        || input.starts_with("git@")
}

fn normalize_git_source(input: &str) -> String {
    if let Some(stripped) = input.strip_prefix("git+") {
        return format!("git:{}", stripped);
    }
    if input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git://")
        || input.starts_with("git@")
    {
        return format!("git:{}", input);
    }
    input.to_string()
}

fn derive_name_from_path(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim_end_matches('/');
    let file_name = Path::new(trimmed)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid path for skill name: {}", input))?;
    Ok(file_name.to_string_lossy().to_string())
}

fn derive_name_from_git_source(source: &str) -> anyhow::Result<String> {
    let raw = source
        .strip_prefix("git:")
        .or_else(|| source.strip_prefix("github:"))
        .unwrap_or(source)
        .trim_end_matches('/');
    let segment = raw
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .rsplit(':')
        .next()
        .unwrap_or(raw);
    let name = segment.strip_suffix(".git").unwrap_or(segment);
    if name.is_empty() {
        anyhow::bail!("Invalid git source for skill name: {}", source);
    }
    Ok(name.to_string())
}

fn resolve_transport(
    transport: Option<&str>,
    has_command: bool,
    has_url: bool,
) -> anyhow::Result<String> {
    if has_command && has_url {
        anyhow::bail!("Cannot specify both stdio command and HTTP URL");
    }

    if let Some(raw) = transport {
        let transport = raw.to_lowercase();
        match transport.as_str() {
            "stdio" => {
                if has_url {
                    anyhow::bail!("HTTP URL requires transport 'http'");
                }
                Ok(transport)
            }
            "http" => {
                if has_command {
                    anyhow::bail!("Stdio command requires transport 'stdio'");
                }
                if !has_url {
                    anyhow::bail!("HTTP transport requires a URL");
                }
                Ok(transport)
            }
            _ => anyhow::bail!("Invalid transport: {}", raw),
        }
    } else if has_url {
        Ok("http".to_string())
    } else {
        Ok("stdio".to_string())
    }
}

fn parse_key_values(pairs: &[String], label: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for pair in pairs {
        let (key, value) = pair.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("Invalid {} entry (expected KEY=VALUE): {}", label, pair)
        })?;
        if key.is_empty() {
            anyhow::bail!("Invalid {} entry (empty key): {}", label, pair);
        }
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, InstallCommand) {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state = temp.path().join("state");
        let global_config = temp.path().join("config");

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&global_config).unwrap();

        // Add minimal registry configuration for testing
        let config_file = global_config.join("sift.toml");
        std::fs::write(
            &config_file,
            r#"
[registry.demo]
type = "claude-marketplace"
source = "github:anthropics/skills"

[registry.anthropic-skills]
type = "claude-marketplace"
source = "github:anthropics/skills"
"#,
        )
        .unwrap();

        let cmd = InstallCommand::with_global_config_dir(
            home,
            project.clone(),
            state,
            global_config,
            LinkMode::Copy,
        );

        (temp, cmd)
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap_or_else(|err| panic!("Failed to run git {:?}: {}", args, err));
        assert!(status.success(), "git {:?} failed", args);
    }

    fn init_git_repo(dir: &std::path::Path) {
        std::fs::create_dir_all(dir).unwrap();
        run_git(dir, &["init"]);
        run_git(dir, &["config", "user.email", "test@example.com"]);
        run_git(dir, &["config", "user.name", "Test User"]);
        run_git(dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(
            dir.join("SKILL.md"),
            "name: demo-skill\n\nTest instructions.\n",
        )
        .unwrap();
        run_git(dir, &["add", "."]);
        run_git(dir, &["commit", "-m", "init"]);
    }

    #[test]
    fn test_install_options_builder() {
        let opts = InstallOptions::mcp("postgres")
            .with_source("registry:postgres-mcp")
            .with_version("^1.0")
            .with_scope(ConfigScope::Global)
            .with_force(true)
            .with_runtime("docker");

        assert_eq!(opts.target, InstallTarget::Mcp);
        assert_eq!(opts.name, "postgres");
        assert_eq!(opts.source, Some("registry:postgres-mcp".to_string()));
        assert_eq!(opts.version, Some("^1.0".to_string()));
        assert_eq!(opts.scope, Some(ConfigScope::Global));
        assert!(opts.force);
        assert_eq!(opts.runtime, Some("docker".to_string()));
    }

    #[test]
    fn test_install_skill_options_builder() {
        let opts = InstallOptions::skill("pdf-processing")
            .with_source("registry:anthropic/pdf")
            .with_version("latest");

        assert_eq!(opts.target, InstallTarget::Skill);
        assert_eq!(opts.name, "pdf-processing");
        assert_eq!(opts.source, Some("registry:anthropic/pdf".to_string()));
        assert_eq!(opts.version, Some("latest".to_string()));
    }

    #[test]
    fn test_install_mcp_creates_config_entry() {
        let (_temp, cmd) = setup_test_env();

        let opts = InstallOptions::mcp("demo-mcp")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source("registry:demo-mcp");

        let report = cmd.execute(&opts).unwrap();

        assert_eq!(report.name, "demo-mcp");
        assert!(report.changed);
        assert!(report.applied);

        // Verify config was created
        let config_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            cmd.home_dir.parent().unwrap().join("config"),
            cmd.project_root.clone(),
        );
        let config = config_store.load().unwrap();
        assert!(config.mcp.contains_key("demo-mcp"));
    }

    #[test]
    fn test_install_mcp_is_idempotent() {
        let (_temp, cmd) = setup_test_env();

        let opts = InstallOptions::mcp("idempotent-mcp")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source("registry:idempotent-mcp");

        // First install
        let report1 = cmd.execute(&opts).unwrap();
        assert!(report1.changed);

        // Second install (same config)
        let report2 = cmd.execute(&opts).unwrap();
        assert!(!report2.changed);
    }

    #[test]
    fn test_install_skill_creates_config_entry() {
        let (temp, cmd) = setup_test_env();

        // Create a local skill directory relative to project root
        let skill_dir = temp
            .path()
            .join("project")
            .join("skills")
            .join("demo-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: demo-skill
description: A test skill
---

Test instructions."#,
        )
        .unwrap();

        let opts = InstallOptions::skill("demo-skill")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source("local:./skills/demo-skill");

        let report = cmd.execute(&opts).unwrap();

        assert_eq!(report.name, "demo-skill");
        assert!(report.changed);
    }

    #[test]
    fn test_install_skill_normalizes_explicit_local_source() {
        let (temp, cmd) = setup_test_env();

        let skill_dir = temp
            .path()
            .join("project")
            .join("skills")
            .join("demo-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "name: demo-skill\n\nTest instructions.\n",
        )
        .unwrap();

        let opts = InstallOptions::skill("demo-skill")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source("./skills/demo-skill");

        let report = cmd.execute(&opts).unwrap();

        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("Normalized source")),
            "expected normalization warning"
        );

        let config_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            cmd.home_dir.parent().unwrap().join("config"),
            cmd.project_root.clone(),
        );
        let config = config_store.load().unwrap();
        let entry = config.skill.get("demo-skill").unwrap();
        assert_eq!(entry.source, "local:./skills/demo-skill");
    }

    #[test]
    fn test_install_skill_normalizes_explicit_git_source() {
        let (temp, cmd) = setup_test_env();

        let repo_dir = temp.path().join("git-skill");
        init_git_repo(&repo_dir);

        let raw_source = format!("git+{}", repo_dir.display());
        let opts = InstallOptions::skill("demo-skill")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source(raw_source);

        let report = cmd.execute(&opts).unwrap();

        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("Normalized source")),
            "expected normalization warning"
        );

        let config_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            cmd.home_dir.parent().unwrap().join("config"),
            cmd.project_root.clone(),
        );
        let config = config_store.load().unwrap();
        let entry = config.skill.get("demo-skill").unwrap();
        assert_eq!(entry.source, format!("git:{}", repo_dir.display()));
    }

    #[test]
    fn test_install_report_includes_warnings() {
        let (_temp, cmd) = setup_test_env();

        // Install to global scope which may generate warnings for some clients
        let opts = InstallOptions::mcp("warning-test")
            .with_scope(ConfigScope::Global)
            .with_source("registry:warning-test");

        let report = cmd.execute(&opts).unwrap();

        // Report should exist even with empty warnings
        assert_eq!(report.name, "warning-test");
    }

    #[test]
    fn test_install_mcp_explicit_command_warns_on_runtime() {
        let (_temp, cmd) = setup_test_env();

        let opts = InstallOptions::mcp("demo-mcp")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source("registry:demo-mcp")
            .with_runtime("node")
            .with_command(["echo", "hello"]);

        let report = cmd.execute(&opts).unwrap();

        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("Ignoring --runtime")),
            "expected runtime warning"
        );
    }

    #[test]
    fn test_with_defaults_uses_link_mode_from_config() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state = temp.path().join("state");
        // On macOS, dirs::config_dir() returns $HOME/Library/Application Support
        // On Linux, it uses $XDG_CONFIG_HOME (or $HOME/.config)
        #[cfg(target_os = "macos")]
        let config_dir = home
            .join("Library")
            .join("Application Support")
            .join("sift");
        #[cfg(not(target_os = "macos"))]
        let config_dir = home.join(".config").join("sift");

        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        let config_path = config_dir.join("sift.toml");
        std::fs::write(&config_path, "link_mode = \"copy\"").unwrap();

        let cmd =
            InstallCommand::with_defaults_from_paths(home, project, state, config_dir).unwrap();
        assert_eq!(cmd.link_mode, LinkMode::Copy);
    }
}
