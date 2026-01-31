//! Uninstall command implementation.
//!
//! Removes MCP servers and skills from configuration, client configs, and lockfiles.

use std::path::PathBuf;

use crate::client::claude_code::ClaudeCodeClient;
use crate::client::{ClientAdapter, ClientContext};
use crate::config::{ConfigStore, merge_configs};
use crate::deploy::UninstallOrchestrator;
use crate::deploy::scope::{RepoStatus, ResourceKind, ScopeRequest, resolve_scope};
use crate::fs::LinkMode;
use crate::lockfile::{LockfileService, LockfileStore};
use crate::types::ConfigScope;

/// What to uninstall: MCP server or skill
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallTarget {
    /// Uninstall an MCP server
    Mcp,
    /// Uninstall a skill
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallScope {
    Auto,
    Scope(ConfigScope),
    All,
}

/// Options for the uninstall command
#[derive(Debug, Clone)]
pub struct UninstallOptions {
    /// Target type (mcp or skill)
    pub target: UninstallTarget,
    /// Name of the package to uninstall
    pub name: String,
    /// Scope selection
    pub scope: UninstallScope,
}

impl UninstallOptions {
    /// Create new uninstall options for an MCP server
    pub fn mcp(name: impl Into<String>) -> Self {
        Self {
            target: UninstallTarget::Mcp,
            name: name.into(),
            scope: UninstallScope::Auto,
        }
    }

    /// Create new uninstall options for a skill
    pub fn skill(name: impl Into<String>) -> Self {
        Self {
            target: UninstallTarget::Skill,
            name: name.into(),
            scope: UninstallScope::Auto,
        }
    }

    /// Set the scope
    pub fn with_scope(mut self, scope: ConfigScope) -> Self {
        self.scope = UninstallScope::Scope(scope);
        self
    }

    /// Set the scope to all
    pub fn with_scope_all(mut self) -> Self {
        self.scope = UninstallScope::All;
        self
    }
}

/// Report from an uninstall operation
#[derive(Debug, Clone)]
pub struct UninstallReport {
    /// Name of the removed package
    pub name: String,
    /// Whether the uninstall changed anything
    pub changed: bool,
    /// Any warnings generated during uninstall
    pub warnings: Vec<String>,
}

/// Uninstall command orchestrator
#[derive(Debug)]
pub struct UninstallCommand {
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

impl UninstallCommand {
    /// Create a new uninstall command
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

    /// Create a new uninstall command with custom global config directory
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

    /// Create an uninstall command with default paths
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

    /// Execute the uninstall command
    pub fn execute(&self, options: &UninstallOptions) -> anyhow::Result<UninstallReport> {
        let client = ClaudeCodeClient::new();
        let ctx = ClientContext::new(self.home_dir.clone(), self.project_root.clone());
        let repo_status = RepoStatus::from_project_root(&ctx.project_root);
        let lockfile = self.load_lockfile()?;

        let scopes = self.resolve_scopes(options, &lockfile)?;
        let mut warnings = Vec::new();
        let mut changed = false;

        for scope in scopes {
            if matches!(options.scope, UninstallScope::All) {
                let support = match options.target {
                    UninstallTarget::Mcp => client.capabilities().mcp,
                    UninstallTarget::Skill => client.capabilities().skills,
                };
                if !is_scope_supported(scope, support) {
                    warnings.push(format!(
                        "Skipping unsupported scope {:?} for {} uninstall",
                        scope,
                        target_label(options.target)
                    ));
                    continue;
                }
            }

            let scope = self.normalize_scope(options, scope, &client, repo_status)?;
            let config_store = self.create_config_store(scope)?;
            let lockfile_service = self.create_lockfile_service();
            let orchestrator = UninstallOrchestrator::new(config_store, lockfile_service);

            let report = match options.target {
                UninstallTarget::Mcp => orchestrator.uninstall_mcp(&client, &ctx, &options.name)?,
                UninstallTarget::Skill => {
                    orchestrator.uninstall_skill(&client, &ctx, &options.name)?
                }
            };

            changed |= report.changed;
            warnings.extend(report.warnings);
        }

        if !changed {
            anyhow::bail!(
                "{} '{}' is not installed",
                target_label(options.target),
                options.name
            );
        }

        Ok(UninstallReport {
            name: options.name.clone(),
            changed,
            warnings,
        })
    }

    fn resolve_scopes(
        &self,
        options: &UninstallOptions,
        lockfile: &crate::lockfile::Lockfile,
    ) -> anyhow::Result<Vec<ConfigScope>> {
        match options.scope {
            UninstallScope::All => Ok(all_scopes()),
            UninstallScope::Scope(scope) => Ok(vec![scope]),
            UninstallScope::Auto => {
                if let Some(scope) = scope_from_lockfile(options, lockfile) {
                    return Ok(vec![scope]);
                }
                if let Some(scope) = self.find_scope_in_config(options)? {
                    return Ok(vec![scope]);
                }
                anyhow::bail!(
                    "{} '{}' is not installed",
                    target_label(options.target),
                    options.name
                );
            }
        }
    }

    fn normalize_scope(
        &self,
        options: &UninstallOptions,
        scope: ConfigScope,
        client: &ClaudeCodeClient,
        repo: RepoStatus,
    ) -> anyhow::Result<ConfigScope> {
        match options.scope {
            UninstallScope::Scope(requested) => {
                let support = match options.target {
                    UninstallTarget::Mcp => client.capabilities().mcp,
                    UninstallTarget::Skill => client.capabilities().skills,
                };
                let resource = match options.target {
                    UninstallTarget::Mcp => ResourceKind::Mcp,
                    UninstallTarget::Skill => ResourceKind::Skill,
                };
                let resolution =
                    resolve_scope(resource, ScopeRequest::Explicit(requested), support, repo)?;
                match resolution {
                    crate::deploy::scope::ScopeResolution::Apply(decision) => Ok(decision.scope),
                    crate::deploy::scope::ScopeResolution::Skip { warning } => {
                        anyhow::bail!("{warning}")
                    }
                }
            }
            UninstallScope::All => Ok(scope),
            UninstallScope::Auto => Ok(scope),
        }
    }

    fn find_scope_in_config(
        &self,
        options: &UninstallOptions,
    ) -> anyhow::Result<Option<ConfigScope>> {
        for scope in all_scopes() {
            let store = self.create_config_store(scope)?;
            let service = crate::deploy::UninstallService::new(store);
            let exists = match options.target {
                UninstallTarget::Mcp => service.contains_mcp(&options.name)?,
                UninstallTarget::Skill => service.contains_skill(&options.name)?,
            };
            if exists {
                return Ok(Some(scope));
            }
        }
        Ok(None)
    }

    fn create_config_store(&self, scope: ConfigScope) -> anyhow::Result<ConfigStore> {
        Ok(ConfigStore::from_paths(
            scope,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        ))
    }

    fn create_lockfile_service(&self) -> LockfileService {
        LockfileService::new(
            self.state_dir.join("locks"),
            Some(self.project_root.clone()),
        )
    }

    fn load_lockfile(&self) -> anyhow::Result<crate::lockfile::Lockfile> {
        LockfileStore::load(
            Some(self.project_root.clone()),
            self.state_dir.join("locks"),
        )
    }
}

fn all_scopes() -> Vec<ConfigScope> {
    vec![
        ConfigScope::Global,
        ConfigScope::PerProjectShared,
        ConfigScope::PerProjectLocal,
    ]
}

fn target_label(target: UninstallTarget) -> &'static str {
    match target {
        UninstallTarget::Mcp => "MCP",
        UninstallTarget::Skill => "Skill",
    }
}

fn scope_from_lockfile(
    options: &UninstallOptions,
    lockfile: &crate::lockfile::Lockfile,
) -> Option<ConfigScope> {
    match options.target {
        UninstallTarget::Mcp => lockfile
            .get_mcp_server(&options.name)
            .map(|locked| locked.scope),
        UninstallTarget::Skill => lockfile.get_skill(&options.name).map(|locked| locked.scope),
    }
}

fn is_scope_supported(scope: ConfigScope, support: crate::client::ScopeSupport) -> bool {
    match scope {
        ConfigScope::Global => support.global,
        ConfigScope::PerProjectShared => support.project,
        ConfigScope::PerProjectLocal => support.local,
    }
}
