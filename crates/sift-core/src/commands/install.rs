//! Install command implementation.
//!
//! Orchestrates installing MCP servers and skills from registries or local sources,
//! updating configuration, resolving versions, and writing to client configs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::client::ClientAdapter;
use crate::client::ClientContext;
use crate::config::{ConfigStore, McpConfigEntry, SkillConfigEntry};
use crate::context::AppContext;
use crate::deploy::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, resolve_scope,
};
use crate::deploy::{InstallMcpRequest, InstallOrchestrator};
use crate::fs::LinkMode;
use crate::mcp::McpServerBuilder;
use crate::source::{ResolvedInput, SourceResolver};
use crate::types::ConfigScope;

use super::context::InstallContext;

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
    /// Target clients (whitelist) - only deploy to these clients
    pub targets: Option<Vec<String>>,
    /// Ignore clients (blacklist) - deploy to all clients except these
    pub ignore_targets: Option<Vec<String>>,
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
            targets: None,
            ignore_targets: None,
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
            targets: None,
            ignore_targets: None,
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

    /// Set target clients (whitelist) - only deploy to these clients
    pub fn with_targets<I, S>(mut self, targets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.targets = Some(targets.into_iter().map(Into::into).collect());
        self
    }

    /// Set ignored clients (blacklist) - deploy to all clients except these
    pub fn with_ignore_targets<I, S>(mut self, ignore_targets: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignore_targets = Some(ignore_targets.into_iter().map(Into::into).collect());
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
pub struct InstallCommand {
    /// Shared context for dependency injection
    ctx: InstallContext,
}

impl InstallCommand {
    /// Create a new install command
    pub fn new(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        link_mode: LinkMode,
    ) -> Self {
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
            ctx: InstallContext::new(
                home_dir,
                project_root,
                state_dir,
                global_config_dir,
                link_mode,
            ),
        }
    }

    /// Create an install command with default paths
    pub fn with_defaults() -> anyhow::Result<Self> {
        Ok(Self {
            ctx: InstallContext::with_defaults()?,
        })
    }

    /// Create an install command from explicit paths, loading link_mode from config.
    pub fn with_defaults_from_paths(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        global_config_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            ctx: InstallContext::from_paths(home_dir, project_root, state_dir, global_config_dir)?,
        })
    }

    /// Create from AppContext (preferred).
    ///
    /// This is the preferred constructor when using the unified AppContext pattern.
    /// CLI/TUI/GUI frontends should create an AppContext once and pass it here.
    pub fn from_context(ctx: AppContext) -> Self {
        Self {
            ctx: InstallContext::from_app_context(ctx),
        }
    }

    // --- Accessors for backward compatibility ---

    /// Get the home directory path.
    pub fn home_dir(&self) -> &Path {
        self.ctx.home_dir()
    }

    /// Get the project root path.
    pub fn project_root(&self) -> &Path {
        self.ctx.project_root()
    }

    /// Get the link mode.
    pub fn link_mode(&self) -> LinkMode {
        self.ctx.link_mode()
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
            let ResolvedInput {
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
            targets: options.targets.clone(),
            ignore_targets: options.ignore_targets.clone(),
            env,
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        // Create client adapter and context
        let registry = self.ctx.client_registry();
        let client = registry
            .get("claude-code")
            .ok_or_else(|| anyhow::anyhow!("claude-code client not found in registry"))?;
        let client_ctx = self.ctx.client_context();

        // Build resolved server spec (simplified for now)
        let servers = self.create_mcp_builder()?.build(
            &name,
            &source,
            &entry,
            version.as_deref(),
            options.force,
        )?;

        // Determine scope request
        let scope_request = match options.scope {
            Some(s) => ScopeRequest::Explicit(s),
            None => ScopeRequest::Auto,
        };

        let repo = RepoStatus::from_project_root(&client_ctx.project_root);
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
        let config_store = self.create_config_store(config_scope);
        let lockfile_service = self.create_lockfile_service();
        let skill_installer = self.create_skill_installer();
        let source_resolver = self.create_source_resolver()?;
        let git_fetcher = self.create_git_fetcher();
        let orchestrator = InstallOrchestrator::new(
            config_store,
            lockfile_service,
            skill_installer,
            source_resolver,
            git_fetcher,
            self.ctx.link_mode(),
        );

        // Execute installation
        let report = orchestrator.install_mcp(
            client,
            &client_ctx,
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
            changed: matches!(report.outcome, crate::deploy::InstallOutcome::Changed),
            applied: report.applied,
            warnings,
        })
    }

    /// Install a skill
    fn install_skill(&self, options: &InstallOptions) -> anyhow::Result<InstallReport> {
        // Resolve name and source
        let resolved = self.resolve_name_and_source(
            &options.name,
            options.source.as_deref(),
            options.registry.as_deref(),
        )?;
        let ResolvedInput {
            name,
            source,
            source_is_registry,
            source_explicit,
            warnings: resolved_warnings,
        } = resolved;
        let mut warnings = Vec::new();
        warnings.extend(resolved_warnings);
        warnings.extend(self.registry_warnings(source_is_registry, source_explicit)?);

        // Create client adapter and context
        let registry = self.ctx.client_registry();
        let client = registry
            .get("claude-code")
            .ok_or_else(|| anyhow::anyhow!("claude-code client not found in registry"))?;
        let client_ctx = self.ctx.client_context();

        // Check if this is a registry source that might have nested marketplaces
        if let Some(registry_part) = source.strip_prefix("registry:") {
            let source_resolver = self.create_source_resolver()?;
            let resolutions = source_resolver.resolve_registry_with_expansion(registry_part)?;

            // Check for collisions
            crate::source::SourceResolver::detect_collisions(&resolutions)
                .map_err(|e| anyhow::anyhow!("Name collision detected: {}", e))?;

            // If first resolution is a group, install all nested plugins
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

                    // Use short name (first alias) for filesystem path
                    let nested_name = resolution
                        .metadata
                        .aliases
                        .first()
                        .cloned()
                        .unwrap_or_else(|| resolution.metadata.skill_name.clone());

                    let nested_entry = SkillConfigEntry {
                        source: resolution.metadata.original_source.clone(),
                        version: None,
                        targets: options.targets.clone(),
                        ignore_targets: options.ignore_targets.clone(),
                        reset_version: false,
                    };

                    match self.install_skill_with_orchestrator(
                        client,
                        &client_ctx,
                        &nested_name,
                        nested_entry,
                        &resolution.metadata.original_source,
                        options.scope,
                        options.force,
                    ) {
                        Ok(report) => {
                            changed = changed || report.changed;
                            applied = applied || report.applied;
                            nested_warnings.extend(report.warnings);
                        }
                        Err(e) => {
                            nested_warnings.push(format!(
                                "Failed to install nested plugin '{}': {}",
                                nested_name, e
                            ));
                        }
                    }
                }

                let nested_names: Vec<_> = resolutions
                    .iter()
                    .filter(|r| !r.metadata.is_group)
                    .map(|r| r.metadata.skill_name.clone())
                    .collect();
                let combined_name = format!("{} ({})", name, nested_names.join(", "));

                return Ok(InstallReport {
                    name: combined_name,
                    changed,
                    applied,
                    warnings: nested_warnings,
                });
            }
        }

        // Standard installation path (non-group or non-registry sources)
        let entry = SkillConfigEntry {
            source: source.clone(),
            version: options.version.clone(),
            targets: options.targets.clone(),
            ignore_targets: options.ignore_targets.clone(),
            reset_version: false,
        };

        let report = self.install_skill_with_orchestrator(
            client,
            &client_ctx,
            &name,
            entry,
            &source,
            options.scope,
            options.force,
        )?;

        let mut all_warnings = warnings;
        all_warnings.extend(report.warnings);
        Ok(InstallReport {
            name,
            changed: report.changed,
            applied: report.applied,
            warnings: all_warnings,
        })
    }

    /// Helper to install a skill using the orchestrator's active method.
    #[allow(clippy::too_many_arguments)]
    fn install_skill_with_orchestrator(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
        entry: SkillConfigEntry,
        source: &str,
        scope: Option<ConfigScope>,
        force: bool,
    ) -> anyhow::Result<InstallReport> {
        // Determine scope request
        let scope_request = match scope {
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
            ScopeResolution::Skip { .. } => scope.unwrap_or(ConfigScope::PerProjectShared),
        };

        // Create orchestrator with all active dependencies
        let config_store = self.create_config_store(config_scope);
        let lockfile_service = self.create_lockfile_service();
        let skill_installer = self.create_skill_installer();
        let source_resolver = self.create_source_resolver()?;
        let git_fetcher = self.create_git_fetcher();
        let orchestrator = InstallOrchestrator::new(
            config_store,
            lockfile_service,
            skill_installer,
            source_resolver,
            git_fetcher,
            self.ctx.link_mode(),
        );

        // Use the new active installation method
        let report = orchestrator
            .install_skill_from_source(client, ctx, name, entry, source, resolution, force)?;

        Ok(InstallReport {
            name: name.to_string(),
            changed: matches!(report.outcome, crate::deploy::InstallOutcome::Changed),
            applied: report.applied,
            warnings: report.warnings,
        })
    }

    // Helper methods - delegate to InstallContext

    fn create_config_store(&self, scope: ConfigScope) -> ConfigStore {
        self.ctx.config_store(scope)
    }

    fn create_lockfile_service(&self) -> crate::lockfile::LockfileService {
        self.ctx.lockfile_service()
    }

    fn create_skill_installer(&self) -> crate::skills::installer::SkillInstaller {
        self.ctx.skill_installer()
    }

    fn create_git_fetcher(&self) -> crate::git::GitFetcher {
        self.ctx.git_fetcher()
    }

    fn create_source_resolver(&self) -> anyhow::Result<SourceResolver> {
        self.ctx.source_resolver()
    }

    /// Create an MCP server builder with optional source resolver.
    fn create_mcp_builder(&self) -> anyhow::Result<McpServerBuilder<'_>> {
        let builder = McpServerBuilder::new(self.ctx.state_dir());
        match self.create_source_resolver() {
            Ok(resolver) => Ok(builder.with_source_resolver(resolver)),
            Err(_) => Ok(builder),
        }
    }

    /// Resolve user input to a name and source using the SourceResolver.
    ///
    /// Delegates to SourceResolver for source inference and normalization.
    fn resolve_name_and_source(
        &self,
        input: &str,
        source: Option<&str>,
        registry: Option<&str>,
    ) -> anyhow::Result<ResolvedInput> {
        let resolver = self.create_source_resolver()?;
        resolver.resolve_input(input, source, registry)
    }

    fn registry_warnings(
        &self,
        source_is_registry: bool,
        source_explicit: bool,
    ) -> anyhow::Result<Vec<String>> {
        if !source_is_registry || source_explicit {
            return Ok(Vec::new());
        }
        let merged = self.ctx.merged_config()?;
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
        let merged = self.ctx.merged_config()?;
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
    use crate::context::AppContext;
    use std::process::Command;
    use tempfile::TempDir;

    const GIT_ENV_OVERRIDES: [&str; 4] = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ];

    fn git_command() -> Command {
        let mut cmd = Command::new("git");
        for key in GIT_ENV_OVERRIDES {
            cmd.env_remove(key);
        }
        cmd
    }

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
        let status = git_command()
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
            cmd.home_dir().parent().unwrap().join("config"),
            cmd.project_root().to_path_buf(),
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
            cmd.home_dir().parent().unwrap().join("config"),
            cmd.project_root().to_path_buf(),
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
            cmd.home_dir().parent().unwrap().join("config"),
            cmd.project_root().to_path_buf(),
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
        assert_eq!(cmd.link_mode(), LinkMode::Copy);
    }

    #[test]
    fn test_install_command_from_context() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        let app_ctx = AppContext::with_global_config_dir(
            home.clone(),
            project.clone(),
            temp.path().join("state"),
            temp.path().join("config"),
            LinkMode::Auto,
        );

        let cmd = InstallCommand::from_context(app_ctx);

        assert_eq!(cmd.home_dir(), &home);
        assert_eq!(cmd.project_root(), &project);
        assert_eq!(cmd.link_mode(), LinkMode::Auto);
    }
}
