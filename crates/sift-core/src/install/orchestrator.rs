//! Core install orchestration across clients.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::client::{ClientAdapter, ClientContext, PathRoot};
use crate::config::managed_json::apply_managed_entries_in_path;
use crate::config::{ConfigStore, McpConfigEntry, SkillConfigEntry};
use crate::fs::LinkMode;
use crate::install::git_exclude::ensure_git_exclude;
use crate::install::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, resolve_scope,
};
use crate::install::{InstallOutcome, InstallService};
use crate::skills::installer::{SkillInstallResult, SkillInstaller};

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
    pub request: ScopeRequest,
    pub force: bool,
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
        let support = client.capabilities().mcp;
        let repo = RepoStatus::from_project_root(&ctx.project_root);
        let resolution = resolve_scope(ResourceKind::Mcp, req.request, support, repo)?;

        match resolution {
            ScopeResolution::Skip { warning } => {
                let outcome = self.install.install_mcp(req.name, req.entry, req.force)?;
                Ok(InstallReport {
                    outcome,
                    warnings: vec![warning],
                    applied: false,
                })
            }
            ScopeResolution::Apply(decision) => {
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

                Ok(InstallReport {
                    outcome,
                    warnings: Vec::new(),
                    applied: true,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn install_skill(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
        entry: SkillConfigEntry,
        cache_dir: &Path,
        request: ScopeRequest,
        force: bool,
        resolved_version: &str,
        constraint: &str,
        registry: &str,
    ) -> anyhow::Result<SkillInstallReport> {
        let support = client.capabilities().skills;
        let repo = RepoStatus::from_project_root(&ctx.project_root);
        let resolution = resolve_scope(ResourceKind::Skill, request, support, repo)?;

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
