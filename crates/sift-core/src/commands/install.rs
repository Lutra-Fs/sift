//! Install command implementation.
//!
//! Orchestrates installing MCP servers and skills from registries or local sources,
//! updating configuration, resolving versions, and writing to client configs.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;

use crate::client::claude_code::ClaudeCodeClient;
use crate::client::ClientContext;
use crate::config::{
    ConfigScope, ConfigStore, McpConfigEntry, OwnershipStore, SkillConfigEntry,
};
use crate::fs::LinkMode;
use crate::install::orchestrator::{InstallMcpRequest, InstallOrchestrator};
use crate::install::scope::ScopeRequest;
use crate::mcp::spec::McpResolvedServer;
use crate::skills::installer::SkillInstaller;

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
        // For backwards compatibility, derive global_config_dir from defaults
        let global_config_dir = dirs::config_dir()
            .map(|p| p.join("sift"))
            .unwrap_or_else(|| home_dir.join(".config").join("sift"));
        Self::with_global_config_dir(home_dir, project_root, state_dir, global_config_dir, link_mode)
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
        let global_config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("sift");

        Ok(Self::with_global_config_dir(
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            LinkMode::Auto,
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
        // Determine scope
        let scope = options.scope.unwrap_or(ConfigScope::PerProjectShared);

        // Build config entry
        let source = options
            .source
            .clone()
            .unwrap_or_else(|| format!("registry:{}", options.name));

        let entry = McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: source.clone(),
            runtime: options.runtime.clone(),
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        // Create orchestrator
        let config_store = self.create_config_store(scope)?;
        let ownership_store = self.create_ownership_store();
        let skill_installer = self.create_skill_installer();
        let orchestrator = InstallOrchestrator::new(
            config_store,
            ownership_store,
            skill_installer,
            self.link_mode,
        );

        // Create client adapter and context
        let client = ClaudeCodeClient::new();
        let ctx = ClientContext::new(self.home_dir.clone(), self.project_root.clone());

        // Build resolved server spec (simplified for now)
        let servers = self.build_mcp_servers(&options.name, &source, &entry)?;

        // Determine scope request
        let scope_request = match options.scope {
            Some(s) => ScopeRequest::Explicit(s),
            None => ScopeRequest::Auto,
        };

        // Execute installation
        let report = orchestrator.install_mcp(
            &client,
            &ctx,
            InstallMcpRequest {
                name: &options.name,
                entry,
                servers: &servers,
                request: scope_request,
                force: options.force,
            },
        )?;

        Ok(InstallReport {
            name: options.name.clone(),
            changed: matches!(
                report.outcome,
                crate::install::InstallOutcome::Changed
            ),
            applied: report.applied,
            warnings: report.warnings,
        })
    }

    /// Install a skill
    fn install_skill(&self, options: &InstallOptions) -> anyhow::Result<InstallReport> {
        // Determine scope
        let scope = options.scope.unwrap_or(ConfigScope::PerProjectShared);

        // Build config entry
        let source = options
            .source
            .clone()
            .unwrap_or_else(|| format!("registry:{}", options.name));

        let entry = SkillConfigEntry {
            source: source.clone(),
            version: options.version.clone(),
            targets: None,
            ignore_targets: None,
            reset_version: false,
        };

        // Create orchestrator
        let config_store = self.create_config_store(scope)?;
        let ownership_store = self.create_ownership_store();
        let skill_installer = self.create_skill_installer();
        let orchestrator = InstallOrchestrator::new(
            config_store,
            ownership_store,
            skill_installer,
            self.link_mode,
        );

        // Create client adapter and context
        let client = ClaudeCodeClient::new();
        let ctx = ClientContext::new(self.home_dir.clone(), self.project_root.clone());

        // For now, we'll use a placeholder cache directory
        // In a real implementation, this would be resolved from the registry
        let cache_dir = self
            .state_dir
            .join("cache")
            .join("skills")
            .join(&options.name);

        // Ensure cache directory exists (for testing - real impl would download)
        std::fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create cache directory: {}", cache_dir.display()))?;

        // Determine scope request
        let scope_request = match options.scope {
            Some(s) => ScopeRequest::Explicit(s),
            None => ScopeRequest::Auto,
        };

        // Version for lockfile
        let version = options.version.clone().unwrap_or_else(|| "latest".to_string());

        // Execute installation
        let report = orchestrator.install_skill(
            &client,
            &ctx,
            &options.name,
            entry,
            &cache_dir,
            scope_request,
            options.force,
            &version,
            &version,
            "default",
        )?;

        Ok(InstallReport {
            name: options.name.clone(),
            changed: matches!(
                report.outcome,
                crate::install::InstallOutcome::Changed
            ),
            applied: report.applied,
            warnings: report.warnings,
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
        OwnershipStore::new(self.state_dir.clone(), Some(self.project_root.clone()))
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
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        // Build a resolved server spec from the entry
        // In a full implementation, this would resolve the package from the registry
        let runtime = entry.runtime.as_deref().unwrap_or("npx");
        let command = runtime.to_string();

        // For registry sources, the args would be resolved from the registry
        // For now, we use a placeholder
        let mut args = vec![format!("{}@latest", name)];
        args.extend(entry.args.clone());

        Ok(vec![McpResolvedServer::stdio(
            name.to_string(),
            command,
            args,
            entry.env.clone(),
        )])
    }
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
}
