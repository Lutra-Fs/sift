//! Install command implementation.
//!
//! Orchestrates installing MCP servers and skills from registries or local sources,
//! updating configuration, resolving versions, and writing to client configs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use crate::client::ClientAdapter;
use crate::client::ClientContext;
use crate::client::claude_code::ClaudeCodeClient;
use crate::config::{
    ConfigScope, ConfigStore, McpConfigEntry, OwnershipStore, SkillConfigEntry, merge_configs,
};
use crate::fs::LinkMode;
use crate::install::orchestrator::{InstallMcpRequest, InstallOrchestrator};
use crate::install::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, resolve_scope,
};
use crate::mcp::spec::McpResolvedServer;
use crate::skills::installer::{GitSkillMetadata, SkillInstaller};
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

impl InstallOptions {
    /// Create new install options for an MCP server
    pub fn mcp(name: impl Into<String>) -> Self {
        Self {
            target: InstallTarget::Mcp,
            name: name.into(),
            source: None,
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
            let (resolved_name, resolved_source, source_is_registry, source_explicit) =
                self.resolve_name_and_source(&options.name, options.source.as_deref())?;
            name = resolved_name;
            warnings = self.registry_warnings(source_is_registry, source_explicit)?;
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
        let (name, source, source_is_registry, source_explicit) =
            self.resolve_name_and_source(&options.name, options.source.as_deref())?;
        let mut warnings = self.registry_warnings(source_is_registry, source_explicit)?;

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

        let cache_dir = self.state_dir.join("cache").join("skills").join(&name);

        let mut git_metadata = None;
        let mut resolved_version = options
            .version
            .clone()
            .unwrap_or_else(|| DEFAULT_VERSION.to_string());
        let mut constraint = resolved_version.clone();
        let mut registry = "default".to_string();

        if is_git_source(&source) {
            ensure_git_version()?;
            let spec = parse_git_source(&source)?;
            let lockfile = LockfileStore::load(
                Some(self.project_root.clone()),
                self.state_dir.join("locks"),
            )?;
            let existing = lockfile.skills.get(&name);
            let is_cache_ready = cache_dir.join("SKILL.md").exists();

            let commit = if !options.force && is_cache_ready {
                if let Some(locked) = existing {
                    locked.resolved_version.clone()
                } else {
                    ensure_git_cache(&cache_dir, &spec, &self.state_dir, options.force)?
                }
            } else {
                ensure_git_cache(&cache_dir, &spec, &self.state_dir, options.force)?
            };

            resolved_version = commit;
            constraint = spec.reference.clone().unwrap_or_else(|| "HEAD".to_string());
            registry = source.clone();
            git_metadata = Some(GitSkillMetadata {
                repo: spec.repo.clone(),
                reference: spec.reference.clone(),
                subdir: spec.subdir.clone(),
            });
        } else {
            std::fs::create_dir_all(&cache_dir).with_context(|| {
                format!("Failed to create cache directory: {}", cache_dir.display())
            })?;
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
            &client,
            &ctx,
            &name,
            entry,
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
            name,
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

        // Build a resolved server spec from the entry
        // In a full implementation, this would resolve the package from the registry
        let runtime = entry.runtime.as_deref().unwrap_or(DEFAULT_RUNTIME);
        let command = runtime.to_string();

        // For registry sources, the args would be resolved from the registry
        // For now, we use a placeholder
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
    ) -> anyhow::Result<(String, String, bool, bool)> {
        if let Some(explicit) = source {
            let is_registry = explicit.starts_with("registry:");
            return Ok((input.to_string(), explicit.to_string(), is_registry, true));
        }

        if let Some(inferred) = self.infer_local_source(input) {
            return Ok((inferred.name, inferred.source, false, false));
        }

        if let Some(inferred) = self.infer_git_source(input) {
            return Ok((inferred.name, inferred.source, false, false));
        }

        Ok((
            input.to_string(),
            format!("registry:{}", input),
            true,
            false,
        ))
    }

    fn infer_local_source(&self, input: &str) -> Option<ResolvedSource> {
        if !is_local_path(input, &self.project_root) {
            return None;
        }
        let name = derive_name_from_path(input).ok()?;
        Some(ResolvedSource {
            name,
            source: format!("local:{}", input),
        })
    }

    fn infer_git_source(&self, input: &str) -> Option<ResolvedSource> {
        if !is_git_like(input) {
            return None;
        }
        let source = normalize_git_source(input);
        let name = derive_name_from_git_source(&source).ok()?;
        Some(ResolvedSource { name, source })
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
                "Multiple registries are configured; use --source to select the desired registry."
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
struct ResolvedSource {
    name: String,
    source: String,
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

#[derive(Debug)]
struct GitSourceSpec {
    repo: String,
    reference: Option<String>,
    subdir: Option<String>,
}

fn is_git_source(source: &str) -> bool {
    source.starts_with("git:") || source.starts_with("github:")
}

fn ensure_git_version() -> anyhow::Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .context("Failed to invoke git --version")?;
    if !output.status.success() {
        anyhow::bail!("Failed to run git --version");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| anyhow::anyhow!("Unexpected git version output: {}", stdout))?;
    let mut parts = version.split('.');
    let major: u32 = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid git version: {}", version))?
        .parse()?;
    let minor: u32 = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid git version: {}", version))?
        .parse()?;
    if major > 2 || (major == 2 && minor >= 25) {
        return Ok(());
    }
    anyhow::bail!("Git 2.25+ is required for sparse checkout. Please upgrade git.");
}

fn parse_git_source(source: &str) -> anyhow::Result<GitSourceSpec> {
    let raw = source.strip_prefix("git:").unwrap_or(source);
    let raw = if let Some(stripped) = raw.strip_prefix("github:") {
        format!("https://github.com/{}", stripped)
    } else {
        raw.to_string()
    };

    if let Some((repo, reference, subdir)) = split_tree_path(&raw) {
        if subdir.is_empty() {
            anyhow::bail!("Git URL is missing a path after /tree/<ref>/");
        }
        return Ok(GitSourceSpec {
            repo,
            reference: Some(reference),
            subdir: Some(subdir),
        });
    }

    Ok(GitSourceSpec {
        repo: raw,
        reference: None,
        subdir: None,
    })
}

fn split_tree_path(raw: &str) -> Option<(String, String, String)> {
    let marker = "/tree/";
    let idx = raw.find(marker)?;
    let repo = raw[..idx].to_string();
    let rest = &raw[idx + marker.len()..];
    let mut parts = rest.splitn(2, '/');
    let reference = parts.next()?.to_string();
    let subdir = parts.next().unwrap_or("").to_string();
    Some((repo, reference, subdir))
}

fn ensure_git_cache(
    cache_dir: &Path,
    spec: &GitSourceSpec,
    state_dir: &Path,
    force: bool,
) -> anyhow::Result<String> {
    let skill_marker = cache_dir.join("SKILL.md");
    if cache_dir.exists() && skill_marker.exists() && !force {
        let bare_dir = ensure_bare_repo(state_dir, spec, false)?;
        return resolve_git_commit(&bare_dir, spec, false);
    }
    if cache_dir.exists() {
        if force || is_empty_dir(cache_dir)? {
            std::fs::remove_dir_all(cache_dir).with_context(|| {
                format!("Failed to remove cache directory: {}", cache_dir.display())
            })?;
        } else {
            anyhow::bail!(
                "Skill cache exists but is missing SKILL.md: {}. Use --force to refresh.",
                cache_dir.display()
            );
        }
    }

    let bare_dir = ensure_bare_repo(state_dir, spec, force)?;
    let commit = resolve_git_commit(&bare_dir, spec, force)?;
    export_git_subdir(&bare_dir, cache_dir, spec, &commit)?;
    Ok(commit)
}

fn ensure_bare_repo(
    state_dir: &Path,
    spec: &GitSourceSpec,
    force: bool,
) -> anyhow::Result<PathBuf> {
    let bare_dir = bare_repo_dir(state_dir, &spec.repo);
    if bare_dir.exists() {
        if force && spec.reference.is_none() {
            std::fs::remove_dir_all(&bare_dir)
                .with_context(|| format!("Failed to remove bare repo: {}", bare_dir.display()))?;
        } else {
            return Ok(bare_dir);
        }
    }

    std::fs::create_dir_all(
        bare_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Bare repo directory has no parent"))?,
    )
    .with_context(|| {
        format!(
            "Failed to create git cache directory: {}",
            bare_dir.display()
        )
    })?;
    run_git(
        None,
        &[
            "clone",
            "--filter=blob:none",
            "--bare",
            &spec.repo,
            bare_dir
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid bare repo dir"))?,
        ],
    )?;
    Ok(bare_dir)
}

fn resolve_git_commit(
    bare_dir: &Path,
    spec: &GitSourceSpec,
    force: bool,
) -> anyhow::Result<String> {
    let reference = spec.reference.as_deref().unwrap_or("HEAD");
    if force && spec.reference.is_some() {
        run_git(
            Some(bare_dir),
            &["fetch", "--filter=blob:none", "origin", reference],
        )?;
        return git_rev_parse(Some(bare_dir), "FETCH_HEAD");
    }

    match git_rev_parse(Some(bare_dir), reference) {
        Ok(commit) => Ok(commit),
        Err(err) => {
            if spec.reference.is_some() {
                run_git(
                    Some(bare_dir),
                    &["fetch", "--filter=blob:none", "origin", reference],
                )?;
                git_rev_parse(Some(bare_dir), "FETCH_HEAD")
            } else {
                Err(err)
            }
        }
    }
}

fn export_git_subdir(
    bare_dir: &Path,
    cache_dir: &Path,
    spec: &GitSourceSpec,
    commit: &str,
) -> anyhow::Result<()> {
    let worktree_dir = unique_temp_repo_dir(cache_dir)?;
    run_git(
        Some(bare_dir),
        &[
            "worktree",
            "add",
            "--detach",
            worktree_dir
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid worktree dir"))?,
            commit,
        ],
    )?;

    if let Some(subdir) = &spec.subdir {
        run_git(Some(&worktree_dir), &["sparse-checkout", "init", "--cone"])?;
        run_git(Some(&worktree_dir), &["sparse-checkout", "set", subdir])?;
        run_git(Some(&worktree_dir), &["checkout", commit])?;
    }

    let src_root = if let Some(subdir) = &spec.subdir {
        worktree_dir.join(subdir)
    } else {
        worktree_dir.clone()
    };
    if !src_root.exists() {
        anyhow::bail!(
            "Git checkout did not create expected path: {}",
            src_root.display()
        );
    }

    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("Failed to create cache directory: {}", cache_dir.display()))?;
    copy_tree_filtered(&src_root, cache_dir)?;

    run_git(
        Some(bare_dir),
        &[
            "worktree",
            "remove",
            "--force",
            worktree_dir
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid worktree dir"))?,
        ],
    )?;
    let _ = std::fs::remove_dir_all(&worktree_dir);
    Ok(())
}

fn run_git(cwd: Option<&Path>, args: &[&str]) -> anyhow::Result<()> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .with_context(|| format!("Failed to run git {:?}", args))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Git command failed {:?}: {}", args, stderr.trim());
    }
    Ok(())
}

fn git_rev_parse(cwd: Option<&Path>, rev: &str) -> anyhow::Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", rev]);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .with_context(|| format!("Failed to run git rev-parse {}", rev))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git rev-parse {} failed: {}", rev, stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_empty_dir(path: &Path) -> anyhow::Result<bool> {
    let mut entries = std::fs::read_dir(path)
        .with_context(|| format!("Failed to read directory: {}", path.display()))?;
    Ok(entries.next().is_none())
}

fn bare_repo_dir(state_dir: &Path, repo_url: &str) -> PathBuf {
    let hash = blake3::hash(repo_url.as_bytes()).to_hex().to_string();
    state_dir.join("git").join(format!("{}.git", hash))
}

fn unique_temp_repo_dir(state_dir: &Path) -> anyhow::Result<PathBuf> {
    let worktree_base = state_dir.join("worktrees");
    std::fs::create_dir_all(&worktree_base).with_context(|| {
        format!(
            "Failed to create worktrees directory: {}",
            worktree_base.display()
        )
    })?;
    for attempt in 0..100 {
        let thread_id = format!("{:?}", std::thread::current().id());
        let name = format!("{}.{}.{}", std::process::id(), thread_id, attempt);
        let candidate = worktree_base.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "Failed to allocate a temp worktree directory in {}",
        worktree_base.display()
    );
}

fn copy_tree_filtered(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in
        std::fs::read_dir(src).with_context(|| format!("Failed to read dir: {}", src.display()))?
    {
        let entry =
            entry.with_context(|| format!("Failed to read dir entry: {}", src.display()))?;
        let ty = entry
            .file_type()
            .with_context(|| format!("Failed to stat dir entry: {}", entry.path().display()))?;
        let from = entry.path();
        let file_name = entry.file_name();
        if file_name == ".git" {
            continue;
        }
        let to = dst.join(&file_name);

        if ty.is_dir() {
            std::fs::create_dir_all(&to)
                .with_context(|| format!("Failed to create directory: {}", to.display()))?;
            copy_tree_filtered(&from, &to)?;
        } else if ty.is_file() {
            std::fs::copy(&from, &to).with_context(|| {
                format!(
                    "Failed to copy file from {} to {}",
                    from.display(),
                    to.display()
                )
            })?;
        } else {
            anyhow::bail!("Unsupported filesystem entry type at {}", from.display());
        }
    }
    Ok(())
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

        let cmd = InstallCommand::with_global_config_dir(
            home,
            project.clone(),
            state,
            global_config,
            LinkMode::Copy,
        );

        (temp, cmd)
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
    fn test_unique_temp_repo_dir_allocates_under_worktrees() {
        // Safe unwrap: tempdir creation failure should fail the test immediately.
        let temp = TempDir::new().unwrap();
        let state_dir = temp.path().join("state");
        // Safe unwrap: test environment expects writable temp dir.
        std::fs::create_dir_all(&state_dir).unwrap();

        let candidate = unique_temp_repo_dir(&state_dir).unwrap();

        assert!(
            candidate.starts_with(state_dir.join("worktrees")),
            "Expected {:?} under worktrees",
            candidate
        );
        assert!(!candidate.exists());
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
        let (_temp, cmd) = setup_test_env();

        let opts = InstallOptions::skill("demo-skill")
            .with_scope(ConfigScope::PerProjectShared)
            .with_source("registry:demo/skill");

        let report = cmd.execute(&opts).unwrap();

        assert_eq!(report.name, "demo-skill");
        assert!(report.changed);
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
    fn test_with_defaults_uses_link_mode_from_config() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
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

        let original_dir = std::env::current_dir().unwrap();
        let original_config = std::env::var("XDG_CONFIG_HOME").ok();
        let original_home = std::env::var("HOME").ok();
        std::env::set_current_dir(&project).unwrap();
        unsafe {
            // Setting process-level env vars is unsafe in Rust 2024; scoped to test.
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        }

        let cmd = InstallCommand::with_defaults().unwrap();
        assert_eq!(cmd.link_mode, LinkMode::Copy);

        std::env::set_current_dir(original_dir).unwrap();
        if let Some(value) = original_config {
            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", value);
            }
        } else {
            unsafe {
                std::env::remove_var("XDG_CONFIG_HOME");
            }
        }
        if let Some(value) = original_home {
            unsafe {
                std::env::set_var("HOME", value);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
    }
}
