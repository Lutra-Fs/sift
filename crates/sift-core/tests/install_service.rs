use std::collections::HashMap;

use tempfile::TempDir;

use sift_core::config::{ConfigScope, ConfigStore, McpConfigEntry, SkillConfigEntry};
use sift_core::orchestration::{InstallOutcome, InstallService};

#[test]
fn install_skill_adds_entry() {
    let temp = TempDir::new().unwrap();
    let store = ConfigStore::from_paths(
        ConfigScope::Global,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let service = InstallService::new(store);

    let entry = SkillConfigEntry {
        source: "registry:demo/skill".to_string(),
        version: Some("1.0.0".to_string()),
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    let outcome = service.install_skill("demo", entry.clone(), false).unwrap();

    assert_eq!(outcome, InstallOutcome::Changed);

    let loaded = service.config_store().load().unwrap();
    assert!(loaded.skill.contains_key("demo"));
}

#[test]
fn install_skill_is_idempotent() {
    let temp = TempDir::new().unwrap();
    let store = ConfigStore::from_paths(
        ConfigScope::Global,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let service = InstallService::new(store);

    let entry = SkillConfigEntry {
        source: "registry:demo/skill".to_string(),
        version: Some("1.0.0".to_string()),
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    service.install_skill("demo", entry.clone(), false).unwrap();
    let outcome = service.install_skill("demo", entry, false).unwrap();

    assert_eq!(outcome, InstallOutcome::NoOp);
}

#[test]
fn install_skill_rejects_conflict_without_force() {
    let temp = TempDir::new().unwrap();
    let store = ConfigStore::from_paths(
        ConfigScope::Global,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let service = InstallService::new(store);

    let first = SkillConfigEntry {
        source: "registry:demo/skill".to_string(),
        version: Some("1.0.0".to_string()),
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    let second = SkillConfigEntry {
        source: "registry:demo/skill".to_string(),
        version: Some("2.0.0".to_string()),
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    service.install_skill("demo", first, false).unwrap();
    let result = service.install_skill("demo", second, false);

    assert!(result.is_err());
}

#[test]
fn install_mcp_adds_entry() {
    let temp = TempDir::new().unwrap();
    let store = ConfigStore::from_paths(
        ConfigScope::Global,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let service = InstallService::new(store);

    let entry = McpConfigEntry {
        transport: Some("stdio".to_string()),
        source: "registry:demo-mcp".to_string(),
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
    };

    let outcome = service.install_mcp("demo-mcp", entry, false).unwrap();

    assert_eq!(outcome, InstallOutcome::Changed);
}
