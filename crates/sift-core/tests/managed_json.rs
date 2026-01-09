use std::collections::HashMap;

use serde_json::{Map, json};
use tempfile::TempDir;

use sift_core::config::managed_json::apply_managed_entries;
use sift_core::config::ownership::hash_json;
use sift_core::config::ownership_store::OwnershipStore;

#[test]
fn apply_managed_entries_preserves_user_entries() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.json");
    let ownership_store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let existing = json!({
        "user": {"command": "echo"}
    });
    std::fs::write(&config_path, serde_json::to_vec_pretty(&existing).unwrap()).unwrap();

    let mut desired = Map::new();
    desired.insert(
        "managed".to_string(),
        json!({"command": "npx", "args": ["pkg@1.2.3"]}),
    );

    let result = apply_managed_entries(&config_path, &desired, &ownership_store, false).unwrap();

    assert!(result.merged.contains_key("user"));
    assert!(result.merged.contains_key("managed"));
    assert!(result.ownership.contains_key("managed"));
}

#[test]
fn apply_managed_entries_refuses_modified_managed_entry() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.json");
    let ownership_store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let existing = json!({
        "managed": {"command": "npx", "args": ["pkg@1.0.0"]}
    });
    std::fs::write(&config_path, serde_json::to_vec_pretty(&existing).unwrap()).unwrap();

    let mut ownership = HashMap::new();
    ownership.insert(
        "managed".to_string(),
        hash_json(&json!({"command": "npx", "args": ["pkg@0.9.0"]})),
    );
    ownership_store.save(&config_path, &ownership).unwrap();

    let mut desired = Map::new();
    desired.insert(
        "managed".to_string(),
        json!({"command": "npx", "args": ["pkg@1.2.3"]}),
    );

    let result = apply_managed_entries(&config_path, &desired, &ownership_store, false);

    assert!(result.is_err());
}

#[test]
fn apply_managed_entries_in_field() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.json");
    let ownership_store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let existing = json!({
        "mcpServers": {
            "user": {"command": "echo"}
        }
    });
    std::fs::write(&config_path, serde_json::to_vec_pretty(&existing).unwrap()).unwrap();

    let mut desired = Map::new();
    desired.insert(
        "managed".to_string(),
        json!({"command": "npx", "args": ["pkg@1.2.3"]}),
    );

    let result = sift_core::config::managed_json::apply_managed_entries_in_field(
        &config_path,
        "mcpServers",
        &desired,
        &ownership_store,
        false,
    )
    .unwrap();

    let field = result
        .merged
        .get("mcpServers")
        .and_then(|value| value.as_object())
        .unwrap();

    assert!(field.contains_key("user"));
    assert!(field.contains_key("managed"));
}

#[test]
fn apply_managed_entries_in_nested_path() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.json");
    let ownership_store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let mut desired = Map::new();
    desired.insert(
        "local".to_string(),
        json!({"command": "npx", "args": ["pkg@1.2.3"]}),
    );

    let result = sift_core::config::managed_json::apply_managed_entries_in_path(
        &config_path,
        &["projects", "/tmp/project", "mcpServers"],
        &desired,
        &ownership_store,
        false,
    )
    .unwrap();

    let projects = result
        .merged
        .get("projects")
        .and_then(|value| value.as_object())
        .unwrap();
    let project = projects
        .get("/tmp/project")
        .and_then(|value| value.as_object())
        .unwrap();
    let servers = project
        .get("mcpServers")
        .and_then(|value| value.as_object())
        .unwrap();

    assert!(servers.contains_key("local"));
}
