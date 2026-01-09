//! Ownership-aware config merging for Sift-managed entries.

use std::collections::HashMap;

use serde_json::{Map, Value};

pub fn hash_json(value: &Value) -> String {
    let normalized = normalize_json(value);
    let bytes = serde_json::to_vec(&normalized).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

pub fn merge_owned_map(
    existing: &Map<String, Value>,
    desired: &Map<String, Value>,
    ownership: &HashMap<String, String>,
    force: bool,
) -> anyhow::Result<Map<String, Value>> {
    let mut merged = existing.clone();

    for (key, desired_value) in desired {
        if let Some(expected_hash) = ownership.get(key) {
            if let Some(existing_value) = existing.get(key) {
                let existing_hash = hash_json(existing_value);
                if existing_hash != *expected_hash && !force {
                    anyhow::bail!("Refusing to overwrite user-modified entry: {}", key);
                }
                merged.insert(key.clone(), desired_value.clone());
            } else {
                merged.insert(key.clone(), desired_value.clone());
            }
        } else if existing.contains_key(key) {
            if !force {
                anyhow::bail!("Entry '{}' already exists and is not managed by Sift", key);
            }
            merged.insert(key.clone(), desired_value.clone());
        } else {
            merged.insert(key.clone(), desired_value.clone());
        }
    }

    for (key, expected_hash) in ownership {
        if desired.contains_key(key) {
            continue;
        }
        if let Some(existing_value) = existing.get(key) {
            let existing_hash = hash_json(existing_value);
            if existing_hash != *expected_hash && !force {
                anyhow::bail!("Refusing to remove user-modified entry: {}", key);
            }
            merged.remove(key);
        }
    }

    Ok(merged)
}

fn normalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let mut normalized = Map::new();
            for key in keys {
                if let Some(child) = map.get(key) {
                    normalized.insert(key.clone(), normalize_json(child));
                }
            }
            Value::Object(normalized)
        }
        Value::Array(items) => Value::Array(items.iter().map(normalize_json).collect()),
        _ => value.clone(),
    }
}
