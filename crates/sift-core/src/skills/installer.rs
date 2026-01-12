//! Skill installation orchestration.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::fs::{LinkMode, hash_tree};
use crate::lockfile::LockfileService;
use crate::lockfile::{LockedSkill, ResolvedOrigin};
use crate::skills::linker::{LinkerOptions, deliver_dir_managed};
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
