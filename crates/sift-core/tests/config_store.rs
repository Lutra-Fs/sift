use std::collections::HashMap;

use tempfile::TempDir;

use sift_core::config::SiftConfig;
use sift_core::config::store::ConfigStore;
use sift_core::types::ConfigScope;

#[test]
fn load_missing_returns_empty_config() {
    let temp = TempDir::new().unwrap();
    let store = ConfigStore::from_paths(
        ConfigScope::Global,
        temp.path().join("config"),
        temp.path().join("project"),
    );

    let config = store.load().unwrap();

    assert!(config.mcp.is_empty());
    assert!(config.skill.is_empty());
}

#[test]
fn save_then_load_roundtrip() {
    let temp = TempDir::new().unwrap();
    let store = ConfigStore::from_paths(
        ConfigScope::Global,
        temp.path().join("config"),
        temp.path().join("project"),
    );

    let mut config = SiftConfig::new();
    config.mcp.insert(
        "test".to_string(),
        sift_core::config::schema::McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: "registry:test".to_string(),
            runtime: Some("node".to_string()),
            args: vec!["--flag".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        },
    );

    store.save(&config).unwrap();
    let loaded = store.load().unwrap();

    assert!(loaded.mcp.contains_key("test"));
}
