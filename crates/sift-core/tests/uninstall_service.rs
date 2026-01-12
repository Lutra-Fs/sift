//! Unit tests for UninstallService
//!
//! Tests edge cases and behavior of the UninstallService including:
//! - Non-existent MCP/skill handling
//! - Empty project entry cleanup
//! - Multiple entries in same scope
//! - Scope-specific lookup behavior

use std::collections::HashMap;

use sift_core::config::{
    ConfigScope, ConfigStore, McpConfigEntry, ProjectConfig, SiftConfig, SkillConfigEntry,
};
use sift_core::orchestration::uninstall::{UninstallOutcome, UninstallService};
use tempfile::TempDir;

/// Creates a temp directory and ConfigStore for testing
fn setup_test_store(scope: ConfigScope) -> (TempDir, ConfigStore) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let global_config = temp.path().join("config");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&global_config).expect("Failed to create config dir");
    std::fs::create_dir_all(&project_root).expect("Failed to create project dir");

    let store = ConfigStore::from_paths(scope, global_config, project_root);
    (temp, store)
}

/// Creates a minimal config with one MCP entry
fn create_config_with_mcp(name: &str, store: &ConfigStore, scope: ConfigScope) -> SiftConfig {
    let mcp_entry = McpConfigEntry {
        transport: None,
        source: format!("registry:{}", name),
        runtime: None,
        args: Vec::new(),
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

    if scope == ConfigScope::PerProjectLocal {
        let project_key = store.project_root().to_string_lossy().to_string();
        let mut project = ProjectConfig::default();
        project.mcp.insert(name.to_string(), mcp_entry);
        let mut projects = HashMap::new();
        projects.insert(project_key, project);
        SiftConfig {
            mcp: HashMap::new(),
            skill: HashMap::new(),
            projects,
            link_mode: None,
            clients: HashMap::new(),
            registry: HashMap::new(),
        }
    } else {
        let mut mcp = HashMap::new();
        mcp.insert(name.to_string(), mcp_entry);
        SiftConfig {
            mcp,
            skill: HashMap::new(),
            projects: HashMap::new(),
            link_mode: None,
            clients: HashMap::new(),
            registry: HashMap::new(),
        }
    }
}

/// Creates a minimal config with one skill entry
fn create_config_with_skill(name: &str, store: &ConfigStore, scope: ConfigScope) -> SiftConfig {
    let skill_entry = SkillConfigEntry {
        source: format!("local:{}", name),
        version: None,
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    if scope == ConfigScope::PerProjectLocal {
        let project_key = store.project_root().to_string_lossy().to_string();
        let mut project = ProjectConfig::default();
        project.skill.insert(name.to_string(), skill_entry);
        let mut projects = HashMap::new();
        projects.insert(project_key, project);
        SiftConfig {
            mcp: HashMap::new(),
            skill: HashMap::new(),
            projects,
            link_mode: None,
            clients: HashMap::new(),
            registry: HashMap::new(),
        }
    } else {
        let mut skill = HashMap::new();
        skill.insert(name.to_string(), skill_entry);
        SiftConfig {
            mcp: HashMap::new(),
            skill,
            projects: HashMap::new(),
            link_mode: None,
            clients: HashMap::new(),
            registry: HashMap::new(),
        }
    }
}

// ============================================================================
// Tests for remove_mcp()
// ============================================================================

#[test]
fn remove_mcp_nonexistent_returns_noop() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_mcp("nonexistent")
        .expect("remove_mcp should not error");

    assert_eq!(
        result,
        UninstallOutcome::NoOp,
        "Removing non-existent MCP should return NoOp"
    );
}

#[test]
fn remove_mcp_global_scope_removes_entry() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    let config = create_config_with_mcp("test-mcp", &store, ConfigScope::Global);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_mcp("test-mcp")
        .expect("remove_mcp should succeed");

    assert_eq!(
        result,
        UninstallOutcome::Changed,
        "Removing existing MCP should return Changed"
    );

    let loaded = store.load().expect("Failed to load config");
    assert!(
        !loaded.mcp.contains_key("test-mcp"),
        "MCP should be removed from config"
    );
}

#[test]
fn remove_mcp_local_scope_removes_project_entry() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    let config = create_config_with_mcp("test-mcp", &store, ConfigScope::PerProjectLocal);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_mcp("test-mcp")
        .expect("remove_mcp should succeed");

    assert_eq!(
        result,
        UninstallOutcome::Changed,
        "Removing local MCP should return Changed"
    );

    let loaded = store.load().expect("Failed to load config");
    let project_key = store.project_root().to_string_lossy().to_string();
    assert!(
        !loaded.projects.contains_key(&project_key)
            || loaded
                .projects
                .get(&project_key)
                .is_none_or(|p| p.mcp.is_empty()),
        "Project entry should be cleaned up or empty"
    );
}

#[test]
fn remove_mcp_local_scope_no_project_returns_noop() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_mcp("test-mcp")
        .expect("remove_mcp should not error");

    assert_eq!(
        result,
        UninstallOutcome::NoOp,
        "Removing MCP with no project entry should return NoOp"
    );
}

#[test]
fn remove_mcp_multiple_in_same_scope_only_removes_target() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    let mut config = SiftConfig::default();
    config.mcp.insert(
        "mcp1".to_string(),
        McpConfigEntry {
            transport: None,
            source: "registry:mcp1".to_string(),
            runtime: None,
            args: Vec::new(),
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
    config.mcp.insert(
        "mcp2".to_string(),
        McpConfigEntry {
            transport: None,
            source: "registry:mcp2".to_string(),
            runtime: None,
            args: Vec::new(),
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
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_mcp("mcp1")
        .expect("remove_mcp should succeed");

    assert_eq!(result, UninstallOutcome::Changed);

    let loaded = store.load().expect("Failed to load config");
    assert!(!loaded.mcp.contains_key("mcp1"), "mcp1 should be removed");
    assert!(loaded.mcp.contains_key("mcp2"), "mcp2 should still exist");
}

// ============================================================================
// Tests for remove_skill()
// ============================================================================

#[test]
fn remove_skill_nonexistent_returns_noop() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_skill("nonexistent")
        .expect("remove_skill should not error");

    assert_eq!(
        result,
        UninstallOutcome::NoOp,
        "Removing non-existent skill should return NoOp"
    );
}

#[test]
fn remove_skill_global_scope_removes_entry() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    let config = create_config_with_skill("test-skill", &store, ConfigScope::Global);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_skill("test-skill")
        .expect("remove_skill should succeed");

    assert_eq!(
        result,
        UninstallOutcome::Changed,
        "Removing existing skill should return Changed"
    );

    let loaded = store.load().expect("Failed to load config");
    assert!(
        !loaded.skill.contains_key("test-skill"),
        "Skill should be removed from config"
    );
}

#[test]
fn remove_skill_local_scope_removes_project_entry() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    let config = create_config_with_skill("test-skill", &store, ConfigScope::PerProjectLocal);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_skill("test-skill")
        .expect("remove_skill should succeed");

    assert_eq!(
        result,
        UninstallOutcome::Changed,
        "Removing local skill should return Changed"
    );

    let loaded = store.load().expect("Failed to load config");
    let project_key = store.project_root().to_string_lossy().to_string();
    assert!(
        !loaded.projects.contains_key(&project_key)
            || loaded
                .projects
                .get(&project_key)
                .is_none_or(|p| p.skill.is_empty()),
        "Project entry should be cleaned up or empty"
    );
}

#[test]
fn remove_skill_multiple_in_same_scope_only_removes_target() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    let mut config = SiftConfig::default();
    config.skill.insert(
        "skill1".to_string(),
        SkillConfigEntry {
            source: "local:skill1".to_string(),
            version: None,
            targets: None,
            ignore_targets: None,
            reset_version: false,
        },
    );
    config.skill.insert(
        "skill2".to_string(),
        SkillConfigEntry {
            source: "local:skill2".to_string(),
            version: None,
            targets: None,
            ignore_targets: None,
            reset_version: false,
        },
    );
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store.clone());
    let result = service
        .remove_skill("skill1")
        .expect("remove_skill should succeed");

    assert_eq!(result, UninstallOutcome::Changed);

    let loaded = store.load().expect("Failed to load config");
    assert!(
        !loaded.skill.contains_key("skill1"),
        "skill1 should be removed"
    );
    assert!(
        loaded.skill.contains_key("skill2"),
        "skill2 should still exist"
    );
}

// ============================================================================
// Tests for contains_mcp()
// ============================================================================

#[test]
fn contains_mcp_global_scope_returns_true_when_present() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    let config = create_config_with_mcp("test-mcp", &store, ConfigScope::Global);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_mcp("test-mcp")
        .expect("contains_mcp should succeed");

    assert!(result, "Should return true when MCP exists in global scope");
}

#[test]
fn contains_mcp_global_scope_returns_false_when_absent() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_mcp("test-mcp")
        .expect("contains_mcp should succeed");

    assert!(!result, "Should return false when MCP does not exist");
}

#[test]
fn contains_mcp_local_scope_returns_true_when_present() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    let config = create_config_with_mcp("test-mcp", &store, ConfigScope::PerProjectLocal);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_mcp("test-mcp")
        .expect("contains_mcp should succeed");

    assert!(result, "Should return true when MCP exists in local scope");
}

#[test]
fn contains_mcp_local_scope_no_project_entry_returns_false() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_mcp("test-mcp")
        .expect("contains_mcp should succeed");

    assert!(!result, "Should return false when no project entry exists");
}

#[test]
fn contains_mcp_local_scope_project_exists_but_no_mcp() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    let project_key = store.project_root().to_string_lossy().to_string();
    let mut project = ProjectConfig::default();
    project.skill.insert(
        "some-skill".to_string(),
        SkillConfigEntry {
            source: "local:some-skill".to_string(),
            version: None,
            targets: None,
            ignore_targets: None,
            reset_version: false,
        },
    );
    let mut projects = HashMap::new();
    projects.insert(project_key.clone(), project);
    let config = SiftConfig {
        mcp: HashMap::new(),
        skill: HashMap::new(),
        projects,
        link_mode: None,
        clients: HashMap::new(),
        registry: HashMap::new(),
    };
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_mcp("test-mcp")
        .expect("contains_mcp should succeed");

    assert!(
        !result,
        "Should return false when project exists but has no MCPs"
    );
}

// ============================================================================
// Tests for contains_skill()
// ============================================================================

#[test]
fn contains_skill_global_scope_returns_true_when_present() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    let config = create_config_with_skill("test-skill", &store, ConfigScope::Global);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_skill("test-skill")
        .expect("contains_skill should succeed");

    assert!(
        result,
        "Should return true when skill exists in global scope"
    );
}

#[test]
fn contains_skill_global_scope_returns_false_when_absent() {
    let (_temp, store) = setup_test_store(ConfigScope::Global);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_skill("test-skill")
        .expect("contains_skill should succeed");

    assert!(!result, "Should return false when skill does not exist");
}

#[test]
fn contains_skill_local_scope_returns_true_when_present() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    let config = create_config_with_skill("test-skill", &store, ConfigScope::PerProjectLocal);
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_skill("test-skill")
        .expect("contains_skill should succeed");

    assert!(
        result,
        "Should return true when skill exists in local scope"
    );
}

#[test]
fn contains_skill_local_scope_no_project_entry_returns_false() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    store
        .save(&SiftConfig::default())
        .expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_skill("test-skill")
        .expect("contains_skill should succeed");

    assert!(!result, "Should return false when no project entry exists");
}

#[test]
fn contains_skill_local_scope_project_exists_but_no_skill() {
    let (_temp, store) = setup_test_store(ConfigScope::PerProjectLocal);
    let project_key = store.project_root().to_string_lossy().to_string();
    let mut project = ProjectConfig::default();
    project.mcp.insert(
        "some-mcp".to_string(),
        McpConfigEntry {
            transport: None,
            source: "registry:some-mcp".to_string(),
            runtime: None,
            args: Vec::new(),
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
    let mut projects = HashMap::new();
    projects.insert(project_key.clone(), project);
    let config = SiftConfig {
        mcp: HashMap::new(),
        skill: HashMap::new(),
        projects,
        link_mode: None,
        clients: HashMap::new(),
        registry: HashMap::new(),
    };
    store.save(&config).expect("Failed to save config");

    let service = UninstallService::new(store);
    let result = service
        .contains_skill("test-skill")
        .expect("contains_skill should succeed");

    assert!(
        !result,
        "Should return false when project exists but has no skills"
    );
}
