//! Skill installation orchestration.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::client::{ClientAdapter, PathRoot};
use crate::config::SkillConfigEntry;
use crate::context::AppContext;
use crate::deploy::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, resolve_scope,
};
use crate::deploy::targeting::TargetingPolicy;
use crate::fs::{LinkMode, hash_tree};
use crate::lockfile::LockfileService;
use crate::lockfile::{LockedSkill, ResolvedOrigin};
use crate::skills::linker::{LinkerOptions, deliver_dir_managed};
use crate::source::ResolvedSource;
use crate::types::ConfigScope;

#[derive(Debug)]
pub struct SkillInstallResult {
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct GitSkillMetadata {
    pub repo: String,
    pub reference: Option<String>,
    pub subdir: Option<String>,
}

#[derive(Debug)]
pub struct SkillInstaller {
    service: LockfileService,
}

impl SkillInstaller {
    pub fn new(store_dir: PathBuf, project_root: Option<PathBuf>) -> Self {
        Self {
            service: LockfileService::new(store_dir, project_root),
        }
    }

    /// Create from an existing LockfileService (for sharing).
    pub fn from_service(service: LockfileService) -> Self {
        Self { service }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn install(
        &self,
        name: &str,
        cache_dir: &Path,
        dst_dir: &Path,
        mode: LinkMode,
        force: bool,
        allow_symlink: bool,
        resolved_version: &str,
        constraint: &str,
        registry: &str,
        scope: ConfigScope,
        git_metadata: Option<GitSkillMetadata>,
        origin: Option<ResolvedOrigin>,
    ) -> anyhow::Result<SkillInstallResult> {
        let cache_hash = hash_tree(cache_dir)
            .with_context(|| format!("Failed to hash cache: {}", cache_dir.display()))?;

        let lockfile = self.service.load()?;
        let existing = lockfile.get_skill(name);
        let expected_hash = if force {
            &cache_hash
        } else {
            existing
                .and_then(|locked| locked.tree_hash.as_deref())
                .unwrap_or(&cache_hash)
        };

        let options = LinkerOptions {
            mode,
            force,
            allow_symlink,
        };

        let report = deliver_dir_managed(cache_dir, dst_dir, &options, existing, expected_hash)?;

        let mut locked = LockedSkill::new(
            name.to_string(),
            resolved_version.to_string(),
            constraint.to_string(),
            registry.to_string(),
            scope,
        )
        .with_install_state(
            dst_dir.to_path_buf(),
            cache_dir.to_path_buf(),
            report.mode,
            cache_hash,
        );
        if let Some(origin) = origin {
            locked = locked.with_origin(origin);
        }
        if let Some(metadata) = git_metadata {
            locked = locked.with_git_metadata(metadata.repo, metadata.reference, metadata.subdir);
        }

        self.service.add_skill(name, locked)?;

        Ok(SkillInstallResult {
            changed: report.changed,
        })
    }
}

/// Request for skill installation pipeline.
#[derive(Debug)]
pub struct SkillPipelineRequest {
    pub name: String,
    pub entry: SkillConfigEntry,
    pub force: bool,
}

/// Report from skill installation pipeline.
#[derive(Debug)]
pub struct SkillPipelineReport {
    pub changed: bool,
    pub applied: bool,
    pub warnings: Vec<String>,
}

/// Full skill installation pipeline.
///
/// Coordinates: source resolution → fetch → config write → deploy → lockfile.
pub struct SkillInstallPipeline<'a> {
    ctx: &'a AppContext,
    scope: ConfigScope,
}

impl<'a> SkillInstallPipeline<'a> {
    pub fn new(ctx: &'a AppContext, scope: ConfigScope) -> Self {
        Self { ctx, scope }
    }

    pub fn install(
        &self,
        client: &dyn ClientAdapter,
        request: SkillPipelineRequest,
    ) -> anyhow::Result<SkillPipelineReport> {
        let mut warnings = Vec::new();

        // 1. Resolve scope
        let capabilities = client.capabilities();
        let repo_status = RepoStatus::from_project_root(self.ctx.project_root());
        let resolution = resolve_scope(
            ResourceKind::Skill,
            ScopeRequest::Explicit(self.scope),
            capabilities.skills,
            repo_status,
        )?;

        let (deploy_scope, use_git_exclude) = match resolution {
            ScopeResolution::Skip { warning } => {
                warnings.push(warning);
                return Ok(SkillPipelineReport {
                    changed: false,
                    applied: false,
                    warnings,
                });
            }
            ScopeResolution::Apply(decision) => (decision.scope, decision.use_git_exclude),
        };

        // 2. Check targeting
        let targeting = TargetingPolicy::new(
            request.entry.targets.clone(),
            request.entry.ignore_targets.clone(),
        );
        if !targeting.should_deploy_to(client.id()) {
            warnings.push(format!(
                "Skipping deployment to '{}': not in target clients",
                client.id()
            ));
            self.write_config(&request)?;
            return Ok(SkillPipelineReport {
                changed: true,
                applied: false,
                warnings,
            });
        }

        // 3. Resolve source and get cache path
        let source_resolver = self.ctx.source_resolver(HashMap::new());
        let (resolved, _metadata) = source_resolver.resolve_with_metadata(&request.entry.source)?;
        let cache_dir = match resolved {
            ResolvedSource::Local(spec) => spec.path,
            ResolvedSource::Git(spec) => {
                let fetcher = self.ctx.git_fetcher();
                let result = fetcher.fetch(&spec, &request.name, request.force)?;
                result.cache_dir
            }
            ResolvedSource::Mcpb(_) => {
                anyhow::bail!("MCPB sources not supported for skills");
            }
        };

        // 4. Write sift.toml
        self.write_config(&request)?;

        // 5. Get delivery plan and compute destination
        let client_ctx = self.ctx.client_context();
        let plan = client.plan_skill(&client_ctx, deploy_scope)?;

        let dst_dir = match plan.root {
            PathRoot::Project => self
                .ctx
                .project_root()
                .join(&plan.relative_path)
                .join(&request.name),
            PathRoot::User => self
                .ctx
                .home_dir()
                .join(&plan.relative_path)
                .join(&request.name),
        };

        // Handle git exclude if needed
        if (use_git_exclude || plan.use_git_exclude)
            && let Some(rel_str) = plan.relative_path.to_str()
        {
            crate::git::ensure_git_exclude(self.ctx.project_root(), rel_str)?;
        }

        // 6. Install skill files
        let installer = SkillInstaller::from_service(self.ctx.lockfile_service());
        let install_result = installer.install(
            &request.name,
            &cache_dir,
            &dst_dir,
            self.ctx.link_mode(),
            request.force,
            capabilities.supports_symlinked_skills,
            "local",
            "local",
            &request.entry.source,
            deploy_scope,
            None,
            None,
        )?;

        Ok(SkillPipelineReport {
            changed: install_result.changed,
            applied: true,
            warnings,
        })
    }

    fn write_config(&self, request: &SkillPipelineRequest) -> anyhow::Result<()> {
        let store = self.ctx.config_store(self.scope);
        let mut config = store.load()?;
        config
            .skill
            .insert(request.name.clone(), request.entry.clone());
        store.save(&config)?;
        Ok(())
    }
}
