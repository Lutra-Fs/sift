use std::collections::HashMap;

use serde_json::json;

use sift_core::config::ownership::{hash_json, merge_owned_map};

#[test]
fn merge_preserves_user_entries() {
    let existing = json!({
        "user-server": {"command": "echo", "args": ["hello"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let desired = json!({
        "sift-server": {"command": "bunx", "args": ["pkg@1.2.3"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let ownership: HashMap<String, String> = HashMap::new();

    let merged = merge_owned_map(&existing, &desired, &ownership, false).unwrap();

    assert!(merged.contains_key("user-server"));
    assert!(merged.contains_key("sift-server"));
}

#[test]
fn merge_updates_managed_entry_when_hash_matches() {
    let existing = json!({
        "sift-server": {"command": "npx", "args": ["pkg@1.0.0"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let desired = json!({
        "sift-server": {"command": "npx", "args": ["pkg@1.2.3"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let mut ownership: HashMap<String, String> = HashMap::new();
    let existing_hash = hash_json(existing.get("sift-server").unwrap());
    ownership.insert("sift-server".to_string(), existing_hash);

    let merged = merge_owned_map(&existing, &desired, &ownership, false).unwrap();

    assert_eq!(
        merged
            .get("sift-server")
            .and_then(|value| value.get("args"))
            .and_then(|value| value.as_array())
            .unwrap()[0]
            .as_str()
            .unwrap(),
        "pkg@1.2.3"
    );
}

#[test]
fn merge_rejects_user_modified_managed_entry() {
    let existing = json!({
        "sift-server": {"command": "npx", "args": ["pkg@1.0.0"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let desired = json!({
        "sift-server": {"command": "npx", "args": ["pkg@1.2.3"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let mut ownership: HashMap<String, String> = HashMap::new();
    let stale_hash = hash_json(&json!({"command": "npx", "args": ["pkg@0.9.0"]}));
    ownership.insert("sift-server".to_string(), stale_hash);

    let result = merge_owned_map(&existing, &desired, &ownership, false);

    assert!(result.is_err());
}

#[test]
fn merge_restores_missing_managed_entry() {
    let existing = json!({
        "user-server": {"command": "echo"}
    })
    .as_object()
    .unwrap()
    .clone();

    let desired = json!({
        "sift-server": {"command": "bunx", "args": ["pkg@1.2.3"]}
    })
    .as_object()
    .unwrap()
    .clone();

    let mut ownership: HashMap<String, String> = HashMap::new();
    ownership.insert("sift-server".to_string(), "previous-hash".to_string());

    let merged = merge_owned_map(&existing, &desired, &ownership, false).unwrap();

    assert!(merged.contains_key("sift-server"));
}
