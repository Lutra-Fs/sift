//! Core install orchestration across clients.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::client::{ClientAdapter, ClientContext, PathRoot};
use crate::config::managed_json::apply_managed_entries_in_path;
use crate::config::{ConfigStore, McpConfigEntry, SkillConfigEntry};
use crate::fs::LinkMode;
use crate::git::{FetchResult, GitFetcher, ensure_git_exclude};
use crate::lockfile::LockfileService;
use crate::lockfile::{LockedMcpServer, ResolvedOrigin};
use crate::orchestration::scope::ScopeResolution;
use crate::orchestration::service::{InstallOutcome, InstallService};
use crate::orchestration::uninstall::remove_path_if_exists;
use crate::skills::installer::{GitSkillMetadata, SkillInstallResult, SkillInstaller};
use crate::source::{RegistryMetadata, ResolvedSource, SourceResolver};

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
    lockfile_service: LockfileService,
    skill_installer: SkillInstaller,
    source_resolver: SourceResolver,
    git_fetcher: GitFetcher,
    link_mode: LinkMode,
}

/// Result of preparing a skill source for installation.
#[derive(Debug)]
struct PreparedSkillSource {
    cache_dir: PathBuf,
    resolved_version: String,
    constraint: String,
    registry: String,
    git_metadata: Option<GitSkillMetadata>,
    origin: Option<ResolvedOrigin>,
}

impl InstallOrchestrator {
    pub fn new(
        store: ConfigStore,
        lockfile_service: LockfileService,
        skill_installer: SkillInstaller,
        source_resolver: SourceResolver,
        git_fetcher: GitFetcher,
        link_mode: LinkMode,
    ) -> Self {
        Self {
            install: InstallService::new(store),
            lockfile_service,
            skill_installer,
            source_resolver,
            git_fetcher,
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
                self.update_mcp_lockfile(req.name, &entry, req.declared_version)?;
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
                    &self.lockfile_service,
                    req.force,
                )
                .with_context(|| format!("Failed to apply MCP config for {}", req.name))?;
                self.update_mcp_lockfile(req.name, &entry, req.declared_version)?;

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
        name: &str,
        entry: &McpConfigEntry,
        declared_version: Option<&str>,
    ) -> anyhow::Result<()> {
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

        self.lockfile_service.add_mcp(name, locked)
    }

    /// Install a skill from a source string.
    ///
    /// This is the "active" installation method that resolves the source,
    /// fetches from git if needed, and handles lockfile caching.
    ///
    /// # Arguments
    /// - `client`: Client adapter for delivery planning
    /// - `ctx`: Client context with paths
    /// - `name`: Skill name
    /// - `entry`: Config entry to write to sift.toml
    /// - `source`: Normalized source string (e.g., "registry:...", "git:...", "local:...")
    /// - `resolution`: Scope resolution from resolve_scope()
    /// - `force`: Force overwrite existing
    #[allow(clippy::too_many_arguments)]
    pub fn install_skill_from_source(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
        entry: SkillConfigEntry,
        source: &str,
        resolution: ScopeResolution,
        force: bool,
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
                let prepared = self.prepare_skill_source(name, source, force)?;
                let outcome = self.install.install_skill(name, entry, force)?;

                let mut plan = client.plan_skill(ctx, decision.scope)?;
                if decision.use_git_exclude {
                    plan.use_git_exclude = true;
                }

                let root = resolve_plan_path(ctx, plan.root, &plan.relative_path)?;
                let dst_dir = root.join(name);

                if force {
                    self.cleanup_skill_delivery(&dst_dir, name)?;
                }

                if plan.use_git_exclude {
                    let rel = plan.relative_path.to_string_lossy();
                    ensure_git_exclude(&ctx.project_root, rel.as_ref())
                        .context("Failed to update git exclude")?;
                }

                let allow_symlink = client.capabilities().supports_symlinked_skills;
                let install = self.skill_installer.install(
                    name,
                    &prepared.cache_dir,
                    &dst_dir,
                    self.link_mode,
                    force,
                    allow_symlink,
                    &prepared.resolved_version,
                    &prepared.constraint,
                    &prepared.registry,
                    decision.scope,
                    prepared.git_metadata,
                    prepared.origin,
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

    /// Prepare a skill source for installation.
    ///
    /// Resolves the source string, checks lockfile for cached versions,
    /// and fetches from git if needed.
    fn prepare_skill_source(
        &self,
        name: &str,
        source: &str,
        force: bool,
    ) -> anyhow::Result<PreparedSkillSource> {
        let (resolved_source, registry_metadata) =
            self.source_resolver.resolve_with_metadata(source)?;

        match resolved_source {
            ResolvedSource::Git(spec) => {
                self.prepare_git_source(name, source, &spec, registry_metadata, force)
            }
            ResolvedSource::Local(spec) => {
                if !spec.path.exists() {
                    anyhow::bail!("Local skill path does not exist: {}", spec.path.display());
                }
                Ok(PreparedSkillSource {
                    cache_dir: spec.path,
                    resolved_version: "local".to_string(),
                    constraint: "local".to_string(),
                    registry: source.to_string(),
                    git_metadata: None,
                    origin: None,
                })
            }
        }
    }

    /// Prepare a git source, checking lockfile for cached versions.
    fn prepare_git_source(
        &self,
        name: &str,
        source: &str,
        spec: &crate::git::GitSpec,
        registry_metadata: Option<RegistryMetadata>,
        force: bool,
    ) -> anyhow::Result<PreparedSkillSource> {
        GitFetcher::ensure_git_version()?;

        let existing = self.lockfile_service.get_skill(name)?;

        let fetch_result = if !force {
            if let Some(locked) = existing.as_ref().filter(|e| e.is_installed()) {
                FetchResult {
                    cache_dir: locked
                        .cache_src_path
                        .clone()
                        .unwrap_or_else(|| self.git_fetcher_cache_dir(name)),
                    commit_sha: locked.resolved_version.clone(),
                }
            } else {
                self.git_fetcher.fetch(spec, name, force)?
            }
        } else {
            self.git_fetcher.fetch(spec, name, force)?
        };

        let constraint = spec.reference.clone().unwrap_or_else(|| "HEAD".to_string());
        let registry = registry_metadata
            .as_ref()
            .map(|m| m.original_source.clone())
            .unwrap_or_else(|| source.to_string());

        let git_metadata = Some(GitSkillMetadata {
            repo: spec.repo_url.clone(),
            reference: spec.reference.clone(),
            subdir: spec.subdir.clone(),
        });
        let origin = registry_metadata.as_ref().map(registry_metadata_to_origin);

        Ok(PreparedSkillSource {
            cache_dir: fetch_result.cache_dir,
            resolved_version: fetch_result.commit_sha,
            constraint,
            registry,
            git_metadata,
            origin,
        })
    }

    /// Get the default cache directory for a skill.
    fn git_fetcher_cache_dir(&self, skill_name: &str) -> PathBuf {
        PathBuf::from(&format!(
            "{}/cache/skills/{}",
            self.git_fetcher.state_dir().display(),
            skill_name
        ))
    }

    /// Clean up skill delivery artifacts (filesystem + lockfile) without touching sift.toml.
    fn cleanup_skill_delivery(&self, dst_dir: &Path, name: &str) -> anyhow::Result<()> {
        // Remove destination directory if exists
        remove_path_if_exists(dst_dir)
            .with_context(|| format!("Failed to remove skill directory: {}", dst_dir.display()))?;

        // Remove lockfile entry via service
        let _ = self.lockfile_service.remove_skill(name)?;
        Ok(())
    }
}

fn registry_metadata_to_origin(metadata: &RegistryMetadata) -> ResolvedOrigin {
    ResolvedOrigin {
        original_source: metadata.original_source.clone(),
        registry_key: metadata.registry_key.clone(),
        registry_version: Some(metadata.marketplace_version.clone()),
        aliases: metadata.aliases.clone(),
        parent: metadata.parent_plugin.clone(),
        is_group: metadata.is_group,
    }
}

pub(crate) fn resolve_plan_path(
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

pub(crate) fn ensure_relative_path(path: &Path) -> anyhow::Result<()> {
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
