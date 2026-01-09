use std::collections::HashMap;

use tempfile::TempDir;

use sift_core::config::ownership_store::OwnershipStore;

#[test]
fn ownership_store_roundtrip() {
    let temp = TempDir::new().unwrap();
    let store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let mut ownership = HashMap::new();
    ownership.insert("alpha".to_string(), "hash-a".to_string());
    ownership.insert("beta".to_string(), "hash-b".to_string());

    let config_path = temp.path().join("config.json");
    store.save(&config_path, &ownership).unwrap();

    let loaded = store.load(&config_path).unwrap();

    assert_eq!(loaded.get("alpha").map(String::as_str), Some("hash-a"));
    assert_eq!(loaded.get("beta").map(String::as_str), Some("hash-b"));
}

#[test]
fn ownership_store_missing_returns_empty() {
    let temp = TempDir::new().unwrap();
    let store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let config_path = temp.path().join("config.json");
    let loaded = store.load(&config_path).unwrap();

    assert!(loaded.is_empty());
}

#[test]
fn ownership_store_roundtrip_with_field() {
    let temp = TempDir::new().unwrap();
    let store = OwnershipStore::new(temp.path().to_path_buf(), None);

    let mut ownership = HashMap::new();
    ownership.insert("gamma".to_string(), "hash-g".to_string());

    let config_path = temp.path().join("config.json");
    store
        .save_for_field(&config_path, "mcpServers", &ownership)
        .unwrap();

    let loaded = store.load_for_field(&config_path, "mcpServers").unwrap();

    assert_eq!(loaded.get("gamma").map(String::as_str), Some("hash-g"));
}
