//! Managed JSON file merge with ownership tracking.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use serde_json::{Map, Value};

use super::ownership::{hash_json, merge_owned_map};
use crate::lockfile::LockfileService;

#[derive(Debug)]
pub struct ManagedJsonResult {
    pub merged: Map<String, Value>,
    pub ownership: HashMap<String, String>,
}

pub fn apply_managed_entries(
    config_path: &Path,
    desired: &Map<String, Value>,
    lockfile_service: &LockfileService,
    force: bool,
) -> anyhow::Result<ManagedJsonResult> {
    let existing = load_json_map(config_path)?;
    let ownership = lockfile_service.load_ownership(config_path, None)?;

    let merged = merge_owned_map(&existing, desired, &ownership, force)?;

    let mut updated_ownership = HashMap::new();
    for (key, value) in desired {
        updated_ownership.insert(key.clone(), hash_json(value));
    }

    write_json_map(config_path, &merged)?;
    lockfile_service.save_ownership(config_path, None, &updated_ownership)?;

    Ok(ManagedJsonResult {
        merged,
        ownership: updated_ownership,
    })
}

pub fn apply_managed_entries_in_field(
    config_path: &Path,
    field: &str,
    desired: &Map<String, Value>,
    lockfile_service: &LockfileService,
    force: bool,
) -> anyhow::Result<ManagedJsonResult> {
    apply_managed_entries_in_path(config_path, &[field], desired, lockfile_service, force)
}

pub fn apply_managed_entries_in_path(
    config_path: &Path,
    path: &[&str],
    desired: &Map<String, Value>,
    lockfile_service: &LockfileService,
    force: bool,
) -> anyhow::Result<ManagedJsonResult> {
    if path.is_empty() {
        anyhow::bail!("Path for managed entries cannot be empty");
    }

    let mut root = load_json_map(config_path)?;
    let existing = extract_map_at_path(&root, path)?;

    let field_key = path.join(".");
    let ownership = lockfile_service.load_ownership(config_path, Some(&field_key))?;
    let merged_field = merge_owned_map(&existing, desired, &ownership, force)?;

    set_map_at_path(&mut root, path, merged_field.clone())?;

    let mut updated_ownership = HashMap::new();
    for (key, value) in desired {
        updated_ownership.insert(key.clone(), hash_json(value));
    }

    write_json_map(config_path, &root)?;
    lockfile_service.save_ownership(config_path, Some(&field_key), &updated_ownership)?;

    Ok(ManagedJsonResult {
        merged: root,
        ownership: updated_ownership,
    })
}

pub fn read_json_map_at_path(
    config_path: &Path,
    path: &[&str],
) -> anyhow::Result<Map<String, Value>> {
    if path.is_empty() {
        anyhow::bail!("Path for managed entries cannot be empty");
    }
    if !config_path.exists() {
        return Ok(Map::new());
    }
    let root = load_json_map(config_path)?;
    extract_map_at_path(&root, path)
}

fn load_json_map(path: &Path) -> anyhow::Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| "Failed to parse JSON config")?;
    match value {
        Value::Object(map) => Ok(map),
        _ => anyhow::bail!("Expected JSON object at root: {}", path.display()),
    }
}

fn write_json_map(path: &Path, map: &Map<String, Value>) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(map).context("Failed to serialize JSON config")?;
    std::fs::write(path, bytes)
        .with_context(|| format!("Failed to write config file: {}", path.display()))?;
    Ok(())
}

fn extract_map_at_path(
    root: &Map<String, Value>,
    path: &[&str],
) -> anyhow::Result<Map<String, Value>> {
    let mut current = root;
    for (idx, segment) in path.iter().enumerate() {
        let value = match current.get(*segment) {
            Some(value) => value,
            None => return Ok(Map::new()),
        };
        if idx == path.len() - 1 {
            return match value {
                Value::Object(map) => Ok(map.clone()),
                _ => anyhow::bail!("Expected '{}' to be a JSON object", segment),
            };
        }
        match value {
            Value::Object(map) => current = map,
            _ => anyhow::bail!("Expected '{}' to be a JSON object", segment),
        }
    }
    Ok(Map::new())
}

fn set_map_at_path(
    root: &mut Map<String, Value>,
    path: &[&str],
    map: Map<String, Value>,
) -> anyhow::Result<()> {
    let mut current = root;
    for (idx, segment) in path.iter().enumerate() {
        if idx == path.len() - 1 {
            current.insert(segment.to_string(), Value::Object(map));
            return Ok(());
        }
        if !current.contains_key(*segment) {
            current.insert(segment.to_string(), Value::Object(Map::new()));
        }
        let next = current.get_mut(*segment).unwrap();
        match next {
            Value::Object(map) => current = map,
            _ => anyhow::bail!("Expected '{}' to be a JSON object", segment),
        }
    }
    Ok(())
}
