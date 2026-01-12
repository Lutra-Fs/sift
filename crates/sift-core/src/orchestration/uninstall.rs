//! Uninstall/remove orchestration for config entries.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde_json::{Map, Value};

use crate::client::{ClientAdapter, ClientContext};
use crate::config::managed_json::read_json_map_at_path;
use crate::config::{ConfigScope, ConfigStore, OwnershipStore};
use crate::orchestration::orchestrator::resolve_plan_path;
use crate::version::store::LockfileStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallOutcome {
    Changed,
    NoOp,
}

#[derive(Debug)]
pub struct UninstallService {
    store: ConfigStore,
}

impl UninstallService {
    pub fn new(store: ConfigStore) -> Self {
        Self { store }
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.store
    }

    pub fn contains_mcp(&self, name: &str) -> anyhow::Result<bool> {
        let config = self.store.load()?;
        let scope = self.store.scope();
        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            return Ok(config
                .projects
                .get(&project_key)
                .map(|project| project.mcp.contains_key(name))
                .unwrap_or(false));
        }
        Ok(config.mcp.contains_key(name))
    }

    pub fn contains_skill(&self, name: &str) -> anyhow::Result<bool> {
        let config = self.store.load()?;
        let scope = self.store.scope();
        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            return Ok(config
                .projects
                .get(&project_key)
                .map(|project| project.skill.contains_key(name))
                .unwrap_or(false));
        }
        Ok(config.skill.contains_key(name))
    }

    pub fn remove_mcp(&self, name: &str) -> anyhow::Result<UninstallOutcome> {
        let mut config = self.store.load()?;
        let scope = self.store.scope();
        let mut removed = false;

        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            if let Some(project) = config.projects.get_mut(&project_key) {
                if project.mcp.remove(name).is_some() {
                    removed = true;
                }
            }
            if removed {
                cleanup_project_entry(&mut config, &project_key);
            }
        } else if config.mcp.remove(name).is_some() {
            removed = true;
        }

        if removed {
            self.store.save(&config)?;
            Ok(UninstallOutcome::Changed)
        } else {
            Ok(UninstallOutcome::NoOp)
        }
    }

    pub fn remove_skill(&self, name: &str) -> anyhow::Result<UninstallOutcome> {
        let mut config = self.store.load()?;
        let scope = self.store.scope();
        let mut removed = false;

        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            if let Some(project) = config.projects.get_mut(&project_key) {
                if project.skill.remove(name).is_some() {
                    removed = true;
                }
            }
            if removed {
                cleanup_project_entry(&mut config, &project_key);
            }
        } else if config.skill.remove(name).is_some() {
            removed = true;
        }

        if removed {
            self.store.save(&config)?;
            Ok(UninstallOutcome::Changed)
        } else {
            Ok(UninstallOutcome::NoOp)
        }
    }
}

fn cleanup_project_entry(config: &mut crate::config::SiftConfig, project_key: &str) {
    let is_empty = config
        .projects
        .get(project_key)
        .map(|project| {
            project.mcp.is_empty()
                && project.skill.is_empty()
                && project.mcp_overrides.is_empty()
                && project.skill_overrides.is_empty()
        })
        .unwrap_or(false);
    if is_empty {
        config.projects.remove(project_key);
    }
}

#[derive(Debug, Default)]
pub struct UninstallReport {
    pub changed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct UninstallOrchestrator {
    uninstall: UninstallService,
    ownership_store: OwnershipStore,
}

impl UninstallOrchestrator {
    pub fn new(store: ConfigStore, ownership_store: OwnershipStore) -> Self {
        Self {
            uninstall: UninstallService::new(store),
            ownership_store,
        }
    }

    pub fn config_store(&self) -> &ConfigStore {
        self.uninstall.config_store()
    }

    pub fn uninstall_mcp(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
    ) -> anyhow::Result<UninstallReport> {
        let outcome = self.uninstall.remove_mcp(name)?;
        let mut warnings = Vec::new();
        let managed_changed = self.remove_managed_mcp_entry(client, ctx, name, &mut warnings)?;
        let lockfile_removed = remove_mcp_lockfile(&self.ownership_store, ctx, name)?;
        let changed =
            matches!(outcome, UninstallOutcome::Changed) || managed_changed || lockfile_removed;
        Ok(UninstallReport { changed, warnings })
    }

    pub fn uninstall_skill(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
    ) -> anyhow::Result<UninstallReport> {
        let outcome = self.uninstall.remove_skill(name)?;
        let removed_dir = self.remove_skill_dir(client, ctx, name)?;
        let lockfile_removed = remove_skill_lockfile(&self.ownership_store, ctx, name)?;
        let changed =
            matches!(outcome, UninstallOutcome::Changed) || removed_dir || lockfile_removed;
        Ok(UninstallReport {
            changed,
            warnings: Vec::new(),
        })
    }

    fn remove_managed_mcp_entry(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
        warnings: &mut Vec<String>,
    ) -> anyhow::Result<bool> {
        let scope = self.uninstall.config_store().scope();
        let plan = client.plan_mcp(ctx, scope, &[])?;
        let config_path = resolve_plan_path(ctx, plan.root, &plan.relative_path)?;
        let path: Vec<&str> = plan.json_path.iter().map(|s| s.as_str()).collect();
        let ownership_key = plan.json_path.join(".");
        let ownership = self
            .ownership_store
            .load_for_field(&config_path, &ownership_key)?;
        if ownership.is_empty() && !config_path.exists() {
            return Ok(false);
        }

        let existing = read_json_map_at_path(&config_path, &path)?;
        let has_entry = existing.contains_key(name);
        let is_owned = ownership.contains_key(name);

        if has_entry && !is_owned {
            warnings.push(format!(
                "Client config entry '{}' is not managed by Sift; skipping removal",
                name
            ));
            return Ok(false);
        }

        let desired = filter_managed_entries(&existing, &ownership, name);
        if !is_owned && desired.is_empty() {
            return Ok(false);
        }

        crate::config::managed_json::apply_managed_entries_in_path(
            &config_path,
            &path,
            &desired,
            &self.ownership_store,
            false,
        )
        .with_context(|| format!("Failed to remove MCP '{}' from client config", name))?;

        Ok(is_owned)
    }

    fn remove_skill_dir(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
    ) -> anyhow::Result<bool> {
        let scope = self.uninstall.config_store().scope();
        let plan = client.plan_skill(ctx, scope)?;
        let root = resolve_plan_path(ctx, plan.root, &plan.relative_path)?;
        let dst_dir = root.join(name);
        remove_path_if_exists(&dst_dir)
            .with_context(|| format!("Failed to remove skill directory: {}", dst_dir.display()))
    }

    /// Clean up skill delivery artifacts (filesystem + lockfile) without touching sift.toml.
    ///
    /// Used by `install --force` to ensure a clean slate before re-installing.
    pub fn cleanup_skill_delivery(
        &self,
        client: &dyn ClientAdapter,
        ctx: &ClientContext,
        name: &str,
    ) -> anyhow::Result<bool> {
        let removed_dir = self.remove_skill_dir(client, ctx, name)?;
        let lockfile_removed = remove_skill_lockfile(&self.ownership_store, ctx, name)?;
        Ok(removed_dir || lockfile_removed)
    }
}

fn filter_managed_entries(
    existing: &Map<String, Value>,
    ownership: &HashMap<String, String>,
    remove_name: &str,
) -> Map<String, Value> {
    let mut desired = Map::new();
    for (key, value) in existing {
        if key == remove_name {
            continue;
        }
        if ownership.contains_key(key) {
            desired.insert(key.clone(), value.clone());
        }
    }
    desired
}

/// Remove a path (file or directory) if it exists.
///
/// Returns `Ok(true)` if something was removed, `Ok(false)` if path didn't exist.
pub fn remove_path_if_exists(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("Failed to read metadata: {}", path.display()))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove directory: {}", path.display()))?;
    } else {
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove file: {}", path.display()))?;
    }
    Ok(true)
}

fn remove_mcp_lockfile(
    ownership_store: &OwnershipStore,
    ctx: &ClientContext,
    name: &str,
) -> anyhow::Result<bool> {
    let store_dir = ownership_store.store_dir().to_path_buf();
    let mut lockfile = LockfileStore::load(Some(ctx.project_root.clone()), store_dir.clone())?;
    let removed = lockfile.remove_mcp_server(name).is_some();
    if removed {
        LockfileStore::save(Some(ctx.project_root.clone()), store_dir, &lockfile)?;
    }
    Ok(removed)
}

fn remove_skill_lockfile(
    ownership_store: &OwnershipStore,
    ctx: &ClientContext,
    name: &str,
) -> anyhow::Result<bool> {
    let store_dir = ownership_store.store_dir().to_path_buf();
    let mut lockfile = LockfileStore::load(Some(ctx.project_root.clone()), store_dir.clone())?;
    let removed = lockfile.remove_skill(name).is_some();
    if removed {
        LockfileStore::save(Some(ctx.project_root.clone()), store_dir, &lockfile)?;
    }
    Ok(removed)
}
