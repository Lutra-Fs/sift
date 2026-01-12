//! Uninstall/remove orchestration for config entries.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde_json::{Map, Value};

use crate::client::{ClientAdapter, ClientContext};
use crate::config::ConfigStore;
use crate::config::managed_json::read_json_map_at_path;
use crate::lockfile::LockfileService;
use crate::orchestration::install::resolve_plan_path;
use crate::orchestration::service::{UninstallOutcome, UninstallService};

#[derive(Debug, Default)]
pub struct UninstallReport {
    pub changed: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct UninstallOrchestrator {
    uninstall: UninstallService,
    lockfile_service: LockfileService,
}

impl UninstallOrchestrator {
    pub fn new(store: ConfigStore, lockfile_service: LockfileService) -> Self {
        Self {
            uninstall: UninstallService::new(store),
            lockfile_service,
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
        let lockfile_removed = self.lockfile_service.remove_mcp(name)?;
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
        let lockfile_removed = self.lockfile_service.remove_skill(name)?;
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
            .lockfile_service
            .load_ownership(&config_path, Some(&ownership_key))?;
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
            &self.lockfile_service,
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
        let lockfile_removed = self.lockfile_service.remove_skill(name)?;
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
