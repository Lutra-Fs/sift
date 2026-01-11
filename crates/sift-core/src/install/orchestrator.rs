//! Core install orchestration across clients.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::client::{ClientAdapter, ClientContext, PathRoot};
use crate::config::managed_json::apply_managed_entries_in_path;
use crate::config::{ConfigStore, McpConfigEntry, SkillConfigEntry};
use crate::fs::LinkMode;
use crate::install::git_exclude::ensure_git_exclude;
use crate::install::scope::ScopeResolution;
use crate::install::{InstallOutcome, InstallService};
use crate::skills::installer::{GitSkillMetadata, SkillInstallResult, SkillInstaller};
use crate::version::lock::LockedMcpServer;
use crate::version::store::LockfileStore;

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub outcome: InstallOutcome,
    pub warnings: Vec<String>,
    pub applied: bool,
}

#[derive(Debug)]
pub struct InstallMcpRequest<'a> {
    pub name: &'a str,
    pub entry: McpConfigEntry,
    pub servers: &'a [crate::mcp::spec::McpResolvedServer],
    pub resolution: ScopeResolution,
    pub force: bool,
    pub declared_version: Option<&'a str>,
}

#[derive(Debug)]
pub struct SkillInstallReport {
    pub outcome: InstallOutcome,
    pub warnings: Vec<String>,
    pub applied: bool,
    pub install: Option<SkillInstallResult>,
}

#[derive(Debug)]
pub struct InstallOrchestrator {
    install: InstallService,
    ownership_store: crate::config::OwnershipStore,
    skill_installer: SkillInstaller,
    link_mode: LinkMode,
}

impl InstallOrchestrator {
    pub fn new(
        store: ConfigStore,
        ownership_store: crate::config::OwnershipStore,
        skill_installer: SkillInstaller,
        link_mode: LinkMode,
    ) -> Self {
        Self {
            install: InstallService::new(store),
            ownership_store,
            skill_installer,
            link_mode,
        }
    }

    pub fn config_store(&self) -> &ConfigStore {
        self.install.config_store()
    }

    pub fn install_mcp(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        req: InstallMcpRequest<'_>,
    ) -> anyhow::Result<InstallReport> {
        match req.resolution {
            ScopeResolution::Skip { warning } => {
                let entry = req.entry.clone();
                let outcome = self.install.install_mcp(req.name, req.entry, req.force)?;
                self.update_mcp_lockfile(ctx, req.name, &entry, req.declared_version)?;
                Ok(InstallReport {
                    outcome,
                    warnings: vec![warning],
                    applied: false,
                })
            }
            ScopeResolution::Apply(decision) => {
                let entry = req.entry.clone();
                let outcome = self.install.install_mcp(req.name, req.entry, req.force)?;
                let plan = client.plan_mcp(ctx, decision.scope, req.servers)?;
                let config_path = resolve_plan_path(ctx, plan.root, &plan.relative_path)?;
                let path: Vec<&str> = plan.json_path.iter().map(|s| s.as_str()).collect();
                apply_managed_entries_in_path(
                    &config_path,
                    &path,
                    &plan.entries,
                    &self.ownership_store,
                    req.force,
                )
                .with_context(|| format!("Failed to apply MCP config for {}", req.name))?;
                self.update_mcp_lockfile(ctx, req.name, &entry, req.declared_version)?;

                Ok(InstallReport {
                    outcome,
                    warnings: Vec::new(),
                    applied: true,
                })
            }
        }
    }

    fn update_mcp_lockfile(
        &self,
        ctx: &ClientContext,
        name: &str,
        entry: &McpConfigEntry,
        declared_version: Option<&str>,
    ) -> anyhow::Result<()> {
        let store_dir = self.ownership_store.store_dir().to_path_buf();
        let project_root = self
            .ownership_store
            .project_root()
            .cloned()
            .unwrap_or_else(|| ctx.project_root.clone());
        let project_root = Some(project_root);
        let mut lockfile = LockfileStore::load(project_root.clone(), store_dir.clone())?;

        let is_registry = entry.source.starts_with("registry:");
        let constraint = if is_registry {
            declared_version.unwrap_or("latest")
        } else {
            "unmanaged"
        };
        // TODO: Registry resolution is not implemented yet; resolved_version is a placeholder.
        let resolved_version = if is_registry { "todo" } else { "unmanaged" };
        let registry = if is_registry {
            entry.source.clone()
        } else {
            "local".to_string()
        };

        let locked = LockedMcpServer::new(
            name.to_string(),
            resolved_version.to_string(),
            constraint.to_string(),
            registry,
            self.install.config_store().scope(),
        );

        lockfile.add_mcp_server(name.to_string(), locked);
        LockfileStore::save(project_root, store_dir, &lockfile)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn install_skill(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
        entry: SkillConfigEntry,
        cache_dir: &Path,
        resolution: ScopeResolution,
        force: bool,
        resolved_version: &str,
        constraint: &str,
        registry: &str,
        git_metadata: Option<GitSkillMetadata>,
    ) -> anyhow::Result<SkillInstallReport> {
        match resolution {
            ScopeResolution::Skip { warning } => {
                let outcome = self.install.install_skill(name, entry, force)?;
                Ok(SkillInstallReport {
                    outcome,
                    warnings: vec![warning],
                    applied: false,
                    install: None,
                })
            }
            ScopeResolution::Apply(decision) => {
                let outcome = self.install.install_skill(name, entry, force)?;
                let mut plan = client.plan_skill(ctx, decision.scope)?;
                if decision.use_git_exclude {
                    plan.use_git_exclude = true;
                }

                let root = resolve_plan_path(ctx, plan.root, &plan.relative_path)?;
                let dst_dir = root.join(name);

                if plan.use_git_exclude {
                    let rel = plan.relative_path.to_string_lossy();
                    ensure_git_exclude(&ctx.project_root, rel.as_ref())
                        .context("Failed to update git exclude")?;
                }

                let allow_symlink = client.capabilities().supports_symlinked_skills;
                let install = self.skill_installer.install(
                    name,
                    cache_dir,
                    &dst_dir,
                    self.link_mode,
                    force,
                    allow_symlink,
                    resolved_version,
                    constraint,
                    registry,
                    decision.scope,
                    git_metadata,
                )?;

                Ok(SkillInstallReport {
                    outcome,
                    warnings: Vec::new(),
                    applied: true,
                    install: Some(install),
                })
            }
        }
    }
}

fn resolve_plan_path(
    ctx: &ClientContext,
    root: PathRoot,
    relative: &Path,
) -> anyhow::Result<PathBuf> {
    ensure_relative_path(relative)?;
    let base = match root {
        PathRoot::User => &ctx.home_dir,
        PathRoot::Project => &ctx.project_root,
    };
    Ok(base.join(relative))
}

fn ensure_relative_path(path: &Path) -> anyhow::Result<()> {
    if path.is_absolute() {
        anyhow::bail!("Absolute paths are not allowed in install plans");
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                anyhow::bail!("Path traversal is not allowed in install plans");
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                anyhow::bail!("Absolute paths are not allowed in install plans");
            }
            _ => {}
        }
    }
    Ok(())
}
