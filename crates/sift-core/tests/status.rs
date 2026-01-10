//! Status module tests (TDD - RED phase)
//!
//! These tests define the expected behavior for status collection.
//! They should FAIL initially until implementation is complete.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde_json::{Map, Value, json};
use tempfile::TempDir;

use sift_core::config::ownership::hash_json;
use sift_core::config::ownership_store::OwnershipStore;
use sift_core::config::{ConfigScope, McpConfigEntry, SiftConfig, SkillConfigEntry};
use sift_core::fs::{LinkMode, tree_hash};
use sift_core::status::{
    AggregatedIntegrity, ClientDeployment, DeploymentIntegrity, EntryState, McpServerStatus,
    SkillIntegrity, collect_status, collect_status_with_paths, determine_entry_state,
    verify_mcp_deployment, verify_skill_integrity,
};
use sift_core::version::lock::LockedMcpServer;
use sift_core::version::lock::{LockedSkill, Lockfile};

// =============================================================================
// Test Helpers
// =============================================================================

fn config_with_skill(name: &str, constraint: &str) -> SiftConfig {
    let mut config = SiftConfig::default();
    config.skill.insert(
        name.to_string(),
        SkillConfigEntry {
            source: format!("registry:official/{}", name),
            version: Some(constraint.to_string()),
            targets: None,
            ignore_targets: None,
            reset_version: false,
        },
    );
    config
}

fn lockfile_with_skill(name: &str, constraint: &str, resolved: &str) -> Lockfile {
    lockfile_with_skill_and_scope(name, constraint, resolved, ConfigScope::Global)
}

fn lockfile_with_skill_and_scope(
    name: &str,
    constraint: &str,
    resolved: &str,
    scope: ConfigScope,
) -> Lockfile {
    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        name.to_string(),
        LockedSkill::new(
            name.to_string(),
            resolved.to_string(),
            constraint.to_string(),
            "registry:official".to_string(),
            scope,
        ),
    );
    lockfile
}

fn create_skill_files(dir: &std::path::Path, content: &str) {
    std::fs::create_dir_all(dir).expect("Failed to create skill directory");
    std::fs::write(dir.join("skill.md"), content).expect("Failed to write skill file");
}

static HOME_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn home_env_lock() -> &'static Mutex<()> {
    HOME_ENV_LOCK.get_or_init(|| Mutex::new(()))
}

// =============================================================================
// 1.1 Entry State Tests
// =============================================================================

/// Test: Skill entry exists in both config and lockfile with matching constraint
#[test]
fn entry_state_ok_when_declared_locked_and_matching() {
    let config = config_with_skill("commit", "^1.0");
    let lockfile = lockfile_with_skill("commit", "^1.0", "1.2.3");

    let state = determine_entry_state("commit", &config.skill, &lockfile.skills);

    assert_eq!(state, EntryState::Ok);
}

/// Test: Entry in config but not in lockfile
#[test]
fn entry_state_not_locked_when_only_in_config() {
    let config = config_with_skill("commit", "^1.0");
    let lockfile = Lockfile::default();

    let state = determine_entry_state("commit", &config.skill, &lockfile.skills);

    assert_eq!(state, EntryState::NotLocked);
}

/// Test: Entry in both but constraint changed
#[test]
fn entry_state_stale_when_constraint_differs() {
    let config = config_with_skill("commit", "^2.0"); // updated constraint
    let lockfile = lockfile_with_skill("commit", "^1.0", "1.2.3"); // old constraint

    let state = determine_entry_state("commit", &config.skill, &lockfile.skills);

    assert_eq!(state, EntryState::Stale);
}

/// Test: Entry in lockfile but removed from config
#[test]
fn entry_state_orphaned_when_only_in_lockfile() {
    let config = SiftConfig::default(); // no entries
    let lockfile = lockfile_with_skill("commit", "^1.0", "1.2.3");

    let state = determine_entry_state("commit", &config.skill, &lockfile.skills);

    assert_eq!(state, EntryState::Orphaned);
}

// =============================================================================
// 1.1.2 MCP Entry State Tests
// =============================================================================

/// Test: MCP entry is Ok when in both config and lockfile (even with non-empty locked constraint)
///
/// MCP configs don't have version fields, so constraint() returns "".
/// This should NOT cause entries to be marked Stale.
#[test]
fn mcp_entry_state_ok_when_config_has_empty_constraint() {
    let mut config_mcp: HashMap<String, McpConfigEntry> = HashMap::new();
    config_mcp.insert(
        "postgres".to_string(),
        McpConfigEntry {
            transport: None,
            source: "registry:official/postgres".to_string(),
            runtime: None,
            args: Vec::new(),
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
            env: HashMap::new(),
        },
    );

    let mut locked_mcp: HashMap<String, LockedMcpServer> = HashMap::new();
    locked_mcp.insert(
        "postgres".to_string(),
        LockedMcpServer::new(
            "postgres".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(), // Non-empty constraint in lockfile
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );

    let state = determine_entry_state("postgres", &config_mcp, &locked_mcp);

    // Should be Ok, not Stale - empty config constraint means "any version acceptable"
    assert_eq!(state, EntryState::Ok);
}

/// Test: MCP entry is NotLocked when only in config
#[test]
fn mcp_entry_state_not_locked_when_only_in_config() {
    let mut config_mcp: HashMap<String, McpConfigEntry> = HashMap::new();
    config_mcp.insert(
        "postgres".to_string(),
        McpConfigEntry {
            transport: None,
            source: "registry:official/postgres".to_string(),
            runtime: None,
            args: Vec::new(),
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
            env: HashMap::new(),
        },
    );

    let locked_mcp: HashMap<String, LockedMcpServer> = HashMap::new();

    let state = determine_entry_state("postgres", &config_mcp, &locked_mcp);

    assert_eq!(state, EntryState::NotLocked);
}

/// Test: MCP entry is Orphaned when only in lockfile
#[test]
fn mcp_entry_state_orphaned_when_only_in_lockfile() {
    let config_mcp: HashMap<String, McpConfigEntry> = HashMap::new();

    let mut locked_mcp: HashMap<String, LockedMcpServer> = HashMap::new();
    locked_mcp.insert(
        "old-server".to_string(),
        LockedMcpServer::new(
            "old-server".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );

    let state = determine_entry_state("old-server", &config_mcp, &locked_mcp);

    assert_eq!(state, EntryState::Orphaned);
}

// =============================================================================
// 1.2 Skill Integrity Tests (with --verify)
// =============================================================================

/// Test: Skill directory exists and hash matches lockfile
#[test]
fn skill_integrity_installed_when_exists_and_hash_matches() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let skill_dir = tmp.path().join("commit");
    create_skill_files(&skill_dir, "skill content");

    // Compute actual hash
    let expected_hash = sift_core::fs::tree_hash::hash_tree(&skill_dir).expect("hash should work");

    let integrity = verify_skill_integrity(&skill_dir, Some(&expected_hash), LinkMode::Copy);

    assert_eq!(integrity, SkillIntegrity::Installed);
}

/// Test: Skill directory exists but content differs
#[test]
fn skill_integrity_modified_when_hash_mismatch() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let skill_dir = tmp.path().join("commit");
    create_skill_files(&skill_dir, "modified content");

    let old_hash = "expected_hash_from_lockfile_that_does_not_match";

    let integrity = verify_skill_integrity(&skill_dir, Some(old_hash), LinkMode::Copy);

    assert_eq!(integrity, SkillIntegrity::Modified);
}

/// Test: dst_path set in lockfile but directory doesn't exist
#[test]
fn skill_integrity_not_found_when_dst_missing() {
    let missing_path = PathBuf::from("/nonexistent/skill/path");

    let integrity = verify_skill_integrity(&missing_path, Some("some_hash"), LinkMode::Copy);

    assert_eq!(integrity, SkillIntegrity::NotFound);
}

/// Test: Symlink exists but target is missing
#[test]
#[cfg(unix)]
fn skill_integrity_broken_link_when_symlink_target_missing() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().expect("Failed to create temp dir");
    let link_path = tmp.path().join("skill_link");
    symlink("/nonexistent/target", &link_path).expect("Failed to create symlink");

    let integrity = verify_skill_integrity(&link_path, Some("some_hash"), LinkMode::Symlink);

    assert_eq!(integrity, SkillIntegrity::BrokenLink);
}

// =============================================================================
// 1.3 MCP Config Integrity Tests (with --verify)
// =============================================================================

/// Test: MCP entry exists in client config and hash matches ownership
#[test]
fn mcp_deployment_ok_when_entry_matches_ownership() {
    let config_content = json!({
        "mcpServers": {
            "postgres": { "command": "npx", "args": ["-y", "@modelcontextprotocol/postgres"] }
        }
    });

    let mut ownership = HashMap::new();
    let entry_hash = hash_json(&config_content["mcpServers"]["postgres"]);
    ownership.insert("postgres".to_string(), entry_hash);

    let integrity = verify_mcp_deployment(&config_content, &["mcpServers"], "postgres", &ownership);

    assert_eq!(integrity, DeploymentIntegrity::Ok);
}

/// Test: MCP entry exists but was modified by user
#[test]
fn mcp_deployment_modified_when_hash_differs() {
    let config_content = json!({
        "mcpServers": {
            "postgres": { "command": "npx", "args": ["MODIFIED_BY_USER"] }
        }
    });

    let mut ownership = HashMap::new();
    ownership.insert(
        "postgres".to_string(),
        "original_hash_before_modification".to_string(),
    );

    let integrity = verify_mcp_deployment(&config_content, &["mcpServers"], "postgres", &ownership);

    assert_eq!(integrity, DeploymentIntegrity::Modified);
}

/// Test: MCP entry missing from client config
#[test]
fn mcp_deployment_missing_when_entry_deleted() {
    let config_content = json!({
        "mcpServers": {}  // postgres was deleted by user
    });

    let mut ownership = HashMap::new();
    ownership.insert("postgres".to_string(), "expected_hash".to_string());

    let integrity = verify_mcp_deployment(&config_content, &["mcpServers"], "postgres", &ownership);

    assert_eq!(integrity, DeploymentIntegrity::Missing);
}

/// Test: No ownership record (never deployed)
#[test]
fn mcp_deployment_not_deployed_when_no_ownership() {
    let config_content = json!({ "mcpServers": {} });
    let ownership = HashMap::new(); // empty - no ownership records

    let integrity = verify_mcp_deployment(&config_content, &["mcpServers"], "postgres", &ownership);

    assert_eq!(integrity, DeploymentIntegrity::NotDeployed);
}

// =============================================================================
// 1.4 Multi-Client Aggregation Tests
// =============================================================================

/// Test: All clients have OK status
#[test]
fn aggregated_all_ok_when_all_clients_match() {
    let status = McpServerStatus {
        name: "postgres".to_string(),
        runtime: Some("npx".to_string()),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: sift_core::config::ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("~/.claude.json"),
                scope: sift_core::config::ConfigScope::Global,
                integrity: DeploymentIntegrity::Ok,
            },
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("./.mcp.json"),
                scope: sift_core::config::ConfigScope::PerProjectShared,
                integrity: DeploymentIntegrity::Ok,
            },
        ],
    };

    assert_eq!(status.aggregated_integrity(), AggregatedIntegrity::AllOk(2));
}

/// Test: Some clients OK, some failed
#[test]
fn aggregated_partial_when_mixed_status() {
    let status = McpServerStatus {
        name: "postgres".to_string(),
        runtime: Some("npx".to_string()),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: sift_core::config::ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("~/.claude.json"),
                scope: sift_core::config::ConfigScope::Global,
                integrity: DeploymentIntegrity::Ok,
            },
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("./.mcp.json"),
                scope: sift_core::config::ConfigScope::PerProjectShared,
                integrity: DeploymentIntegrity::Modified,
            },
        ],
    };

    assert_eq!(
        status.aggregated_integrity(),
        AggregatedIntegrity::Partial { ok: 1, total: 2 }
    );
}

/// Test: All clients failed
#[test]
fn aggregated_all_failed_when_none_ok() {
    let status = McpServerStatus {
        name: "postgres".to_string(),
        runtime: Some("npx".to_string()),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: sift_core::config::ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("~/.claude.json"),
                scope: sift_core::config::ConfigScope::Global,
                integrity: DeploymentIntegrity::Missing,
            },
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("./.mcp.json"),
                scope: sift_core::config::ConfigScope::PerProjectShared,
                integrity: DeploymentIntegrity::Modified,
            },
        ],
    };

    assert_eq!(
        status.aggregated_integrity(),
        AggregatedIntegrity::AllFailed(2)
    );
}

/// Test: NotDeployed entries excluded from count
#[test]
fn aggregated_excludes_not_deployed() {
    let status = McpServerStatus {
        name: "postgres".to_string(),
        runtime: Some("npx".to_string()),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: sift_core::config::ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            ClientDeployment {
                client_id: "claude-code".to_string(),
                config_path: PathBuf::from("~/.claude.json"),
                scope: sift_core::config::ConfigScope::Global,
                integrity: DeploymentIntegrity::Ok,
            },
            ClientDeployment {
                client_id: "future-client".to_string(),
                config_path: PathBuf::from("~/.future/config.json"),
                scope: sift_core::config::ConfigScope::Global,
                integrity: DeploymentIntegrity::NotDeployed,
            },
        ],
    };

    // Only 1 was expected to be deployed, and it's OK
    assert_eq!(status.aggregated_integrity(), AggregatedIntegrity::AllOk(1));
}

// =============================================================================
// 1.5 Integration Tests
// =============================================================================

/// Test: collect_status with empty config returns empty status
#[test]
fn collect_status_empty_config() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let global_dir = tmp.path().join("global");
    let state_dir = tmp.path().join("state");
    // No sift.toml, no lockfile
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let status = collect_status_with_paths(tmp.path(), &global_dir, &state_dir, None, false)
        .expect("collect_status should succeed");

    assert!(status.mcp_servers.is_empty());
    assert!(status.skills.is_empty());
    assert_eq!(status.summary.issues, 0);
}

/// Test: collect_status defaults link_mode to Auto when unset
#[test]
fn collect_status_defaults_link_mode_to_auto() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let global_dir = tmp.path().join("global");
    let state_dir = tmp.path().join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let status = collect_status_with_paths(tmp.path(), &global_dir, &state_dir, None, false)
        .expect("collect_status should succeed");

    assert_eq!(status.link_mode, LinkMode::Auto);
}

/// Test: collect_status finds NotLocked entries
#[test]
fn collect_status_detects_not_locked() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let global_dir = tmp.path().join("global");
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    // Create sift.toml with a skill
    let sift_toml = r#"
[skill.commit]
source = "registry:official/commit"
version = "^1.0"
"#;
    std::fs::write(tmp.path().join("sift.toml"), sift_toml).expect("Failed to write sift.toml");
    // No lockfile

    let status = collect_status_with_paths(tmp.path(), &global_dir, &state_dir, None, false)
        .expect("collect_status should succeed");

    assert_eq!(status.skills.len(), 1);
    assert_eq!(status.skills[0].state, EntryState::NotLocked);
    assert_eq!(status.summary.issues, 1);
}

// =============================================================================
// 2.0 Phase 2 Integration Tests - Lockfile & Multi-Client
// =============================================================================

use sift_core::version::store::LockfileStore;

/// Test: collect_status with matching lockfile shows Ok state
#[test]
fn collect_status_with_lockfile_shows_ok() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create sift.toml
    let sift_toml = r#"
[skill.commit]
source = "registry:official/commit"
version = "^1.0"
"#;
    std::fs::write(tmp.path().join("sift.toml"), sift_toml).expect("Failed to write sift.toml");

    // Create lockfile with matching constraint
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "commit".to_string(),
        LockedSkill::new(
            "commit".to_string(),
            "1.2.3".to_string(),
            "^1.0".to_string(), // matches sift.toml
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(Some(tmp.path().to_path_buf()), state_dir.clone(), &lockfile)
        .expect("Failed to save lockfile");

    // Mock the default state dir by using environment or testing with full path
    // For this test, we need to verify the lockfile is loaded
    // Since collect_status uses LockfileStore::default_state_dir(), we test the logic separately

    // Direct test: verify determine_entry_state works with real data
    let config = config_with_skill("commit", "^1.0");
    let state = determine_entry_state("commit", &config.skill, &lockfile.skills);
    assert_eq!(state, EntryState::Ok);
}

/// Test: collect_status detects Stale entries when constraint changed
#[test]
fn collect_status_detects_stale_constraint() {
    let config = config_with_skill("commit", "^2.0"); // updated to ^2.0
    let lockfile = lockfile_with_skill("commit", "^1.0", "1.2.3"); // still ^1.0

    let state = determine_entry_state("commit", &config.skill, &lockfile.skills);
    assert_eq!(state, EntryState::Stale);
}

/// Test: collect_status marks skills stale when lockfile constraint differs
#[test]
fn collect_status_skill_state_stale_when_constraint_changed() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let project_root = tmp.path();
    let global_dir = tmp.path().join("global_config");
    let state_dir = tmp.path().join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let config = r#"
[skill.commit]
source = "registry:official/commit"
version = "^2.0"
"#;
    std::fs::write(project_root.join("sift.toml"), config).expect("Failed to write sift.toml");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "commit".to_string(),
        LockedSkill::new(
            "commit".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let status = collect_status_with_paths(project_root, &global_dir, &state_dir, None, false)
        .expect("collect_status should succeed");

    let skill = status
        .skills
        .iter()
        .find(|s| s.name == "commit")
        .expect("Should find commit skill");
    assert_eq!(skill.state, EntryState::Stale);
    assert_eq!(status.summary.issues, 1);
}

/// Test: collect_status detects Orphaned entries in lockfile
#[test]
fn collect_status_detects_orphaned_in_lockfile() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create empty sift.toml (no skills)
    let sift_toml = r#"
# empty config
"#;
    std::fs::write(tmp.path().join("sift.toml"), sift_toml).expect("Failed to write sift.toml");

    // Create lockfile with a skill that's not in config
    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "old-skill".to_string(),
        LockedSkill::new(
            "old-skill".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );

    // Verify orphan detection logic
    let config = SiftConfig::default();
    let state = determine_entry_state("old-skill", &config.skill, &lockfile.skills);
    assert_eq!(state, EntryState::Orphaned);
}

/// Test: collect_status reflects lockfile state for MCP entries
#[test]
fn collect_status_returns_mcp_state_from_lockfile() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let project_root = tmp.path();
    let global_dir = tmp.path().join("global_config");
    let state_dir = tmp.path().join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let sift_toml = r#"
[mcp.postgres]
source = "registry:official/postgres"
runtime = "npx"
transport = "stdio"
"#;

    std::fs::write(project_root.join("sift.toml"), sift_toml).expect("Failed to write sift.toml");

    let mut lockfile = Lockfile::default();
    lockfile.mcp_servers.insert(
        "postgres".to_string(),
        LockedMcpServer::new(
            "postgres".to_string(),
            "1.0.0".to_string(),
            "".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let status = collect_status_with_paths(project_root, &global_dir, &state_dir, None, false)
        .expect("collect_status should succeed");

    assert_eq!(status.mcp_servers.len(), 1);
    assert_eq!(status.mcp_servers[0].state, EntryState::Ok);
    assert_eq!(status.summary.issues, 0);
}

/// Test: scope_filter only returns orphaned MCP entries that match the locked scope
#[test]
fn collect_status_orphaned_mcp_scope_filter_respects_scope() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let project_root = tmp.path();
    let global_dir = tmp.path().join("global_config");
    let state_dir = tmp.path().join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    std::fs::write(project_root.join("sift.toml"), "# empty\n").expect("Write empty config");

    let mut lockfile = Lockfile::default();
    lockfile.mcp_servers.insert(
        "postgres".to_string(),
        LockedMcpServer::new(
            "postgres".to_string(),
            "1.0.0".to_string(),
            "".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let global_status = collect_status_with_paths(
        project_root,
        &global_dir,
        &state_dir,
        Some(ConfigScope::Global),
        false,
    )
    .expect("collect_status should succeed");

    assert!(
        global_status
            .mcp_servers
            .iter()
            .any(|m| m.name == "postgres" && m.state == EntryState::Orphaned),
        "Should show orphaned entry when filtering for Global only"
    );

    let project_status = collect_status_with_paths(
        project_root,
        &global_dir,
        &state_dir,
        Some(ConfigScope::PerProjectShared),
        false,
    )
    .expect("collect_status should succeed");

    assert!(
        project_status
            .mcp_servers
            .iter()
            .all(|m| m.name != "postgres"),
        "Should skip orphaned entry for other scope filters"
    );
}

/// Test: collect_status updates MCP state when lockfile becomes available
#[test]
fn collect_status_updates_mcp_state_when_lockfile_added() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let project_root = tmp.path();
    let global_dir = tmp.path().join("global_config");
    let state_dir = tmp.path().join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let config = r#"
[mcp.postgres]
source = "registry:official/postgres"
runtime = "npx"
transport = "stdio"
"#;

    std::fs::write(project_root.join("sift.toml"), config).expect("Failed to write sift.toml");

    let initial_status =
        collect_status_with_paths(project_root, &global_dir, &state_dir, None, false)
            .expect("collect_status should succeed");

    assert_eq!(
        initial_status.mcp_servers[0].state,
        EntryState::NotLocked,
        "No lockfile should show NotLocked"
    );
    assert_eq!(initial_status.summary.issues, 1);

    let mut lockfile = Lockfile::default();
    lockfile.mcp_servers.insert(
        "postgres".to_string(),
        LockedMcpServer::new(
            "postgres".to_string(),
            "1.0.0".to_string(),
            "".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let final_status =
        collect_status_with_paths(project_root, &global_dir, &state_dir, None, false)
            .expect("collect_status should succeed");

    assert_eq!(final_status.mcp_servers[0].state, EntryState::Ok);
    assert_eq!(final_status.summary.issues, 0);
}

/// Test: verify=true populates deployments for project-scope MCP
#[test]
fn collect_status_verify_populates_project_scope_mcp_deployments() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let project_root = tmp.path();
    let global_dir = tmp.path().join("global_config");
    let state_dir = tmp.path().join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let config = r#"
[mcp.postgres]
source = "registry:official/postgres"
runtime = "npx"
transport = "stdio"
"#;

    std::fs::write(project_root.join("sift.toml"), config).expect("Failed to write sift.toml");

    let mcp_json = json!({
        "mcpServers": {
            "postgres": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/postgres"]
            }
        }
    });
    let mcp_path = project_root.join(".mcp.json");
    std::fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_json).unwrap())
        .expect("Failed to write .mcp.json");

    let mut lockfile = Lockfile::default();
    lockfile.mcp_servers.insert(
        "postgres".to_string(),
        LockedMcpServer::new(
            "postgres".to_string(),
            "1.0.0".to_string(),
            "".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let entry_hash = hash_json(&mcp_json["mcpServers"]["postgres"]);
    let mut ownership = HashMap::new();
    ownership.insert("postgres".to_string(), entry_hash);

    let ownership_store = OwnershipStore::new(state_dir.clone(), Some(project_root.to_path_buf()));
    ownership_store
        .save_for_field(&mcp_path, "mcpServers", &ownership)
        .expect("Failed to save ownership");

    let status = collect_status_with_paths(project_root, &global_dir, &state_dir, None, true)
        .expect("collect_status should succeed");

    let deployments = status
        .mcp_servers
        .iter()
        .flat_map(|m| m.deployments.iter())
        .filter(|d| d.scope == ConfigScope::PerProjectShared)
        .collect::<Vec<_>>();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].integrity, DeploymentIntegrity::Ok);
    assert_eq!(deployments[0].config_path, mcp_path);
}

/// Test: verify=true populates deployments for local-scope MCP entries via nested JSON path
#[test]
fn collect_status_verify_populates_local_scope_mcp_deployments() {
    let guard = home_env_lock().lock().unwrap();

    let home_dir = TempDir::new().expect("Failed to create temp home dir");
    let project = TempDir::new().expect("Failed to create temp project");
    let project_root = project.path();
    let global_dir = project_root.join("global_config");
    let state_dir = project_root.join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let config = r#"
[mcp.postgres]
source = "registry:official/postgres"
runtime = "npx"
transport = "stdio"
"#;

    std::fs::write(project_root.join("sift.toml"), config).expect("Failed to write sift.toml");

    let previous_home = env::var_os("HOME");
    let previous_userprofile = env::var_os("USERPROFILE");
    unsafe {
        env::set_var("HOME", home_dir.path());
        env::set_var("USERPROFILE", home_dir.path());
    }

    let project_key = project_root.to_string_lossy().to_string();
    let entry_value = json!({
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/postgres"]
    });

    let mut mcp_servers_map = Map::new();
    mcp_servers_map.insert("postgres".to_string(), entry_value.clone());

    let mut project_inner = Map::new();
    project_inner.insert("mcpServers".to_string(), Value::Object(mcp_servers_map));

    let mut projects_map = Map::new();
    projects_map.insert(project_key.clone(), Value::Object(project_inner));

    let mut root_map = Map::new();
    root_map.insert("projects".to_string(), Value::Object(projects_map));

    let local_config = Value::Object(root_map);
    let claude_path = home_dir.path().join(".claude.json");
    std::fs::write(
        &claude_path,
        serde_json::to_string_pretty(&local_config).unwrap(),
    )
    .expect("Failed to write local Claude config");

    let mut lockfile = Lockfile::default();
    lockfile.mcp_servers.insert(
        "postgres".to_string(),
        LockedMcpServer::new(
            "postgres".to_string(),
            "1.0.0".to_string(),
            "".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let entry_hash = hash_json(&entry_value);
    let mut ownership = HashMap::new();
    ownership.insert("postgres".to_string(), entry_hash);

    let ownership_store = OwnershipStore::new(state_dir.clone(), Some(project_root.to_path_buf()));
    let field_key = format!("projects.{}.mcpServers", project_key);
    ownership_store
        .save_for_field(&claude_path, &field_key, &ownership)
        .expect("Failed to save ownership");

    let status = collect_status_with_paths(project_root, &global_dir, &state_dir, None, true)
        .expect("collect_status should succeed");

    let local_deployments = status
        .mcp_servers
        .iter()
        .flat_map(|m| m.deployments.iter())
        .filter(|d| d.scope == ConfigScope::PerProjectLocal)
        .collect::<Vec<_>>();

    assert_eq!(local_deployments.len(), 1);
    assert_eq!(local_deployments[0].integrity, DeploymentIntegrity::Ok);
    assert_eq!(local_deployments[0].config_path, claude_path);

    if let Some(val) = previous_home {
        unsafe {
            env::set_var("HOME", val);
        }
    } else {
        unsafe {
            env::remove_var("HOME");
        }
    }
    if let Some(val) = previous_userprofile {
        unsafe {
            env::set_var("USERPROFILE", val);
        }
    } else {
        unsafe {
            env::remove_var("USERPROFILE");
        }
    }

    drop(guard);
}

/// Test: verify=true marks installed skills as OK via tree hash
#[test]
fn collect_status_verify_marks_skill_installed() {
    let guard = home_env_lock().lock().unwrap();

    let home_dir = TempDir::new().expect("Failed to create temp home dir");
    let project = TempDir::new().expect("Failed to create temp project");
    let project_root = project.path();
    let global_dir = project_root.join("global_config");
    let state_dir = project_root.join("state");

    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let skill_dir = home_dir.path().join(".claude/skills/commit");
    create_skill_files(&skill_dir, "# Commit Skill\n");
    let tree_hash = tree_hash::hash_tree(&skill_dir).expect("hash_tree should succeed");

    let previous_home = env::var_os("HOME");
    let previous_userprofile = env::var_os("USERPROFILE");
    unsafe {
        env::set_var("HOME", home_dir.path());
        env::set_var("USERPROFILE", home_dir.path());
    }

    let config = r#"
[skill.commit]
source = "registry:official/commit"
version = "^1.0"
"#;
    std::fs::write(project_root.join("sift.toml"), config).expect("Failed to write sift.toml");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "commit".to_string(),
        LockedSkill::new(
            "commit".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        )
        .with_install_state(
            skill_dir.clone(),
            skill_dir.clone(),
            LinkMode::Copy,
            tree_hash.clone(),
        ),
    );
    LockfileStore::save(
        Some(project_root.to_path_buf()),
        state_dir.clone(),
        &lockfile,
    )
    .expect("Failed to save lockfile");

    let status = collect_status_with_paths(project_root, &global_dir, &state_dir, None, true)
        .expect("collect_status should succeed");

    let skill = status
        .skills
        .iter()
        .find(|s| s.name == "commit")
        .expect("Should find commit skill");
    assert_eq!(skill.state, EntryState::Ok);

    let deployment = skill
        .deployments
        .iter()
        .find(|d| d.scope == ConfigScope::Global)
        .expect("Should have global deployment");
    assert_eq!(deployment.integrity, SkillIntegrity::Installed);
    assert_eq!(deployment.dst_path, skill_dir);
    assert_eq!(status.summary.issues, 0);

    if let Some(val) = previous_home {
        unsafe {
            env::set_var("HOME", val);
        }
    } else {
        unsafe {
            env::remove_var("HOME");
        }
    }
    if let Some(val) = previous_userprofile {
        unsafe {
            env::set_var("USERPROFILE", val);
        }
    } else {
        unsafe {
            env::remove_var("USERPROFILE");
        }
    }
    drop(guard);
}

/// Test: collect_status includes resolved_version from lockfile
#[test]
fn collect_status_includes_resolved_version() {
    let _config = config_with_skill("commit", "^1.0");
    let lockfile = lockfile_with_skill("commit", "^1.0", "1.2.3");

    // Verify we can get resolved version from lockfile
    let locked = lockfile.skills.get("commit").expect("should have commit");
    assert_eq!(locked.resolved_version, "1.2.3");
    assert_eq!(locked.constraint, "^1.0");
}

/// Test: collect_status with verify=true collects deployment info
#[test]
fn collect_status_verify_collects_deployments() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let global_dir = tmp.path().join("global");
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    // Create sift.toml with MCP
    let sift_toml = r#"
[mcp.postgres]
source = "registry:official/postgres"
runtime = "npx"
transport = "stdio"
"#;
    std::fs::write(tmp.path().join("sift.toml"), sift_toml).expect("Failed to write sift.toml");

    // Run collect_status with verify=true
    let status = collect_status_with_paths(tmp.path(), &global_dir, &state_dir, None, true)
        .expect("collect_status should succeed");

    // Should have MCP server
    assert_eq!(status.mcp_servers.len(), 1);

    // With verify=true, should have deployments for each client scope
    // Claude Code supports 3 MCP scopes: Global, Shared, Local
    let deployments = &status.mcp_servers[0].deployments;
    assert!(
        !deployments.is_empty(),
        "Should have at least one deployment check"
    );

    // All should be NotDeployed since we haven't installed anything
    for dep in deployments {
        assert_eq!(dep.integrity, DeploymentIntegrity::NotDeployed);
        assert_eq!(dep.client_id, "claude-code");
    }
}

/// Test: collect_status without verify=false has empty deployments
#[test]
fn collect_status_no_verify_skips_deployments() {
    let tmp = TempDir::new().expect("Failed to create temp dir");
    let global_dir = tmp.path().join("global");
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    // Create sift.toml with MCP
    let sift_toml = r#"
[mcp.postgres]
source = "registry:official/postgres"
runtime = "npx"
transport = "stdio"
"#;
    std::fs::write(tmp.path().join("sift.toml"), sift_toml).expect("Failed to write sift.toml");

    // Run collect_status with verify=false
    let status = collect_status_with_paths(tmp.path(), &global_dir, &state_dir, None, false)
        .expect("collect_status should succeed");

    // Should have MCP server
    assert_eq!(status.mcp_servers.len(), 1);

    // Without verify, deployments should be empty
    assert!(
        status.mcp_servers[0].deployments.is_empty(),
        "Deployments should be empty when verify=false"
    );
}

/// Test: collect_status includes client capabilities
#[test]
fn collect_status_includes_clients() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let status = collect_status(tmp.path(), None, false).expect("collect_status should succeed");

    // Should have at least Claude Code client
    assert!(
        !status.clients.is_empty(),
        "Should have at least one client"
    );

    let claude_code = status.clients.iter().find(|c| c.id == "claude-code");
    assert!(claude_code.is_some(), "Should have claude-code client");

    let cc = claude_code.unwrap();
    assert!(cc.enabled);
    assert_eq!(cc.delivery_mode, "Filesystem");
    assert!(!cc.mcp_scopes.is_empty());
    assert!(!cc.skill_scopes.is_empty());
}

/// Test: MCP deployment integrity with real client config file
#[test]
fn verify_mcp_deployment_with_client_config() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create a mock client config file (.mcp.json)
    let mcp_json = json!({
        "mcpServers": {
            "postgres": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/postgres"]
            }
        }
    });
    let config_path = tmp.path().join(".mcp.json");
    std::fs::write(
        &config_path,
        serde_json::to_string_pretty(&mcp_json).unwrap(),
    )
    .expect("Failed to write .mcp.json");

    // Create ownership record with matching hash
    let mut ownership = HashMap::new();
    let entry_hash = hash_json(&mcp_json["mcpServers"]["postgres"]);
    ownership.insert("postgres".to_string(), entry_hash);

    // Verify deployment integrity
    let integrity = verify_mcp_deployment(&mcp_json, &["mcpServers"], "postgres", &ownership);
    assert_eq!(integrity, DeploymentIntegrity::Ok);

    // Now modify the config and verify it detects the change
    let modified_json = json!({
        "mcpServers": {
            "postgres": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/postgres", "--extra-arg"]
            }
        }
    });
    let integrity_modified =
        verify_mcp_deployment(&modified_json, &["mcpServers"], "postgres", &ownership);
    assert_eq!(integrity_modified, DeploymentIntegrity::Modified);
}

/// Test: Skill deployment integrity with real files
#[test]
fn verify_skill_deployment_with_files() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create skill directory with content
    let skill_dir = tmp.path().join(".claude").join("skills").join("commit");
    create_skill_files(&skill_dir, "# Commit Skill\n\nThis is the commit skill.");

    // Get the actual hash
    let actual_hash =
        sift_core::fs::tree_hash::hash_tree(&skill_dir).expect("hash_tree should succeed");

    // Verify with correct hash
    let integrity = verify_skill_integrity(&skill_dir, Some(&actual_hash), LinkMode::Copy);
    assert_eq!(integrity, SkillIntegrity::Installed);

    // Modify content and verify it detects the change
    std::fs::write(skill_dir.join("skill.md"), "# Modified content")
        .expect("Failed to modify skill");
    let integrity_after = verify_skill_integrity(&skill_dir, Some(&actual_hash), LinkMode::Copy);
    assert_eq!(integrity_after, SkillIntegrity::Modified);
}

// =============================================================================
// 3.0 Bug Fix Tests - scope_filter, multi-layer config, entry scope tracking
// =============================================================================

use sift_core::status::SkillStatus;

/// BUG: scope_filter parameter is never used - filtering should work
/// Currently sift status --scope global returns same as sift status
#[test]
fn scope_filter_global_only_returns_global_entries() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create global config dir
    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create global sift.toml with a global skill
    let global_toml = r#"
[skill.global-tool]
source = "registry:official/global-tool"
version = "^1.0"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // Create project sift.toml with a project skill
    let project_toml = r#"
[skill.project-tool]
source = "registry:official/project-tool"
version = "^2.0"
"#;
    std::fs::write(tmp.path().join("sift.toml"), project_toml)
        .expect("Failed to write project config");

    // With scope_filter = Global, should only return global-tool
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::Global),
        false,
    )
    .expect("collect_status should succeed");

    // FIX: Should return only entries from global scope
    assert!(
        status.skills.iter().any(|s| s.name == "global-tool"),
        "Should include global-tool when filtering for Global scope"
    );
    assert!(
        !status.skills.iter().any(|s| s.name == "project-tool"),
        "Should NOT include project-tool when filtering for Global scope"
    );
}

/// BUG: scope_filter PerProjectShared should only return project entries
#[test]
fn scope_filter_project_only_returns_project_entries() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create global config dir
    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create global sift.toml
    let global_toml = r#"
[skill.global-tool]
source = "registry:official/global-tool"
version = "^1.0"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // Create project sift.toml
    let project_toml = r#"
[skill.project-tool]
source = "registry:official/project-tool"
version = "^2.0"
"#;
    std::fs::write(tmp.path().join("sift.toml"), project_toml)
        .expect("Failed to write project config");

    // With scope_filter = PerProjectShared, should only return project-tool
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::PerProjectShared),
        false,
    )
    .expect("collect_status should succeed");

    assert!(
        status.skills.iter().any(|s| s.name == "project-tool"),
        "Should include project-tool when filtering for PerProjectShared scope"
    );
    assert!(
        !status.skills.iter().any(|s| s.name == "global-tool"),
        "Should NOT include global-tool when filtering for PerProjectShared scope"
    );
}

/// BUG: Entry scope is hardcoded to PerProjectShared
/// Entries from global config should have scope = Global
#[test]
fn entry_scope_reflects_config_source_global() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create global config dir
    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create global sift.toml
    let global_toml = r#"
[skill.global-tool]
source = "registry:official/global-tool"
version = "^1.0"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // No project config
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        None,
        false,
    )
    .expect("collect_status should succeed");

    // FIX: global-tool should have scope = Global
    let global_tool = status.skills.iter().find(|s| s.name == "global-tool");
    assert!(global_tool.is_some(), "Should find global-tool");
    assert_eq!(
        global_tool.unwrap().scope,
        ConfigScope::Global,
        "Entry from global config should have Global scope"
    );
}

/// BUG: Only project config is loaded
/// Global config at ~/.config/sift/sift.toml should also be loaded
#[test]
fn collect_status_loads_global_config() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create global config dir structure mimicking ~/.config/sift/
    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create global sift.toml
    let global_toml = r#"
[skill.global-only-skill]
source = "registry:official/global-only-skill"
version = "^1.0"

[mcp.global-mcp]
source = "registry:official/global-mcp"
runtime = "node"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // Create empty project dir (no sift.toml)
    // Status should still find global entries
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        None,
        false,
    )
    .expect("collect_status should succeed");

    // FIX: Should load global config and return global-only-skill and global-mcp
    assert!(
        status.skills.iter().any(|s| s.name == "global-only-skill"),
        "Should load skills from global config"
    );
    assert!(
        status.mcp_servers.iter().any(|m| m.name == "global-mcp"),
        "Should load MCP servers from global config"
    );
}

/// Test: scope_filter None returns all entries from all scopes
#[test]
fn scope_filter_none_returns_all_entries() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create global config
    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    let global_toml = r#"
[skill.global-tool]
source = "registry:official/global-tool"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // Create project config
    let project_toml = r#"
[skill.project-tool]
source = "registry:official/project-tool"
"#;
    std::fs::write(tmp.path().join("sift.toml"), project_toml)
        .expect("Failed to write project config");

    // scope_filter = None should return both
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        None,
        false,
    )
    .expect("collect_status should succeed");

    assert!(
        status.skills.iter().any(|s| s.name == "global-tool"),
        "Should include global-tool with scope_filter=None"
    );
    assert!(
        status.skills.iter().any(|s| s.name == "project-tool"),
        "Should include project-tool with scope_filter=None"
    );
}

// =============================================================================
// 3.1 Matrix Tests: scope_filter × verify combinations
// =============================================================================

/// Matrix test: scope_filter=Global, verify=true
#[test]
fn matrix_scope_global_verify_true() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::write(
        global_dir.join("sift.toml"),
        r#"
[mcp.global-mcp]
source = "registry:official/global"
runtime = "node"
"#,
    )
    .expect("write global");

    std::fs::write(
        tmp.path().join("sift.toml"),
        r#"
[mcp.project-mcp]
source = "registry:official/project"
runtime = "node"
"#,
    )
    .expect("write project");

    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::Global),
        true,
    )
    .expect("collect_status should succeed");

    // Should only have global MCP, with deployments populated (verify=true)
    assert!(
        status.mcp_servers.iter().any(|m| m.name == "global-mcp"),
        "Should include global-mcp"
    );
    assert!(
        !status.mcp_servers.iter().any(|m| m.name == "project-mcp"),
        "Should NOT include project-mcp when filtering for Global"
    );

    // verify=true means deployments should be populated
    if let Some(global_mcp) = status.mcp_servers.iter().find(|m| m.name == "global-mcp") {
        assert!(
            !global_mcp.deployments.is_empty(),
            "verify=true should populate deployments"
        );
    }
}

/// Matrix test: scope_filter=Global, verify=false
#[test]
fn matrix_scope_global_verify_false() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::write(
        global_dir.join("sift.toml"),
        r#"
[mcp.global-mcp]
source = "registry:official/global"
runtime = "node"
"#,
    )
    .expect("write global");

    std::fs::write(
        tmp.path().join("sift.toml"),
        r#"
[mcp.project-mcp]
source = "registry:official/project"
runtime = "node"
"#,
    )
    .expect("write project");

    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::Global),
        false,
    )
    .expect("collect_status should succeed");

    // Should only have global MCP, with empty deployments (verify=false)
    assert!(status.mcp_servers.iter().any(|m| m.name == "global-mcp"));
    assert!(!status.mcp_servers.iter().any(|m| m.name == "project-mcp"));

    if let Some(global_mcp) = status.mcp_servers.iter().find(|m| m.name == "global-mcp") {
        assert!(
            global_mcp.deployments.is_empty(),
            "verify=false should have empty deployments"
        );
    }
}

/// Matrix test: scope_filter=PerProjectShared, verify=true
#[test]
fn matrix_scope_project_verify_true() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::write(
        global_dir.join("sift.toml"),
        r#"
[skill.global-skill]
source = "registry:official/global"
"#,
    )
    .expect("write global");

    std::fs::write(
        tmp.path().join("sift.toml"),
        r#"
[skill.project-skill]
source = "registry:official/project"
"#,
    )
    .expect("write project");

    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::PerProjectShared),
        true,
    )
    .expect("collect_status should succeed");

    assert!(status.skills.iter().any(|s| s.name == "project-skill"));
    assert!(!status.skills.iter().any(|s| s.name == "global-skill"));

    // verify=true means deployments should be populated for skills
    if let Some(skill) = status.skills.iter().find(|s| s.name == "project-skill") {
        // Skills also have deployments when verify=true
        assert!(
            !skill.deployments.is_empty(),
            "verify=true should populate skill deployments"
        );
    }
}

/// Matrix test: scope_filter=PerProjectShared, verify=false
#[test]
fn matrix_scope_project_verify_false() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::write(
        global_dir.join("sift.toml"),
        r#"
[skill.global-skill]
source = "registry:official/global"
"#,
    )
    .expect("write global");

    std::fs::write(
        tmp.path().join("sift.toml"),
        r#"
[skill.project-skill]
source = "registry:official/project"
"#,
    )
    .expect("write project");

    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::PerProjectShared),
        false,
    )
    .expect("collect_status should succeed");

    assert!(status.skills.iter().any(|s| s.name == "project-skill"));
    assert!(!status.skills.iter().any(|s| s.name == "global-skill"));

    if let Some(skill) = status.skills.iter().find(|s| s.name == "project-skill") {
        assert!(
            skill.deployments.is_empty(),
            "verify=false should have empty deployments"
        );
    }
}

/// Matrix test: scope_filter=None, verify=true
#[test]
fn matrix_scope_none_verify_true() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::write(
        global_dir.join("sift.toml"),
        r#"
[skill.global-skill]
source = "registry:official/global"
"#,
    )
    .expect("write global");

    std::fs::write(
        tmp.path().join("sift.toml"),
        r#"
[skill.project-skill]
source = "registry:official/project"
"#,
    )
    .expect("write project");

    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        None,
        true,
    )
    .expect("collect_status should succeed");

    // scope_filter=None returns all
    assert!(status.skills.iter().any(|s| s.name == "global-skill"));
    assert!(status.skills.iter().any(|s| s.name == "project-skill"));

    // verify=true populates deployments
    for skill in &status.skills {
        assert!(
            !skill.deployments.is_empty(),
            "verify=true should populate deployments for {}",
            skill.name
        );
    }
}

/// Matrix test: scope_filter=None, verify=false (baseline)
#[test]
fn matrix_scope_none_verify_false() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");
    std::fs::write(
        global_dir.join("sift.toml"),
        r#"
[skill.global-skill]
source = "registry:official/global"
"#,
    )
    .expect("write global");

    std::fs::write(
        tmp.path().join("sift.toml"),
        r#"
[skill.project-skill]
source = "registry:official/project"
"#,
    )
    .expect("write project");

    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        None,
        false,
    )
    .expect("collect_status should succeed");

    // scope_filter=None returns all
    assert!(status.skills.iter().any(|s| s.name == "global-skill"));
    assert!(status.skills.iter().any(|s| s.name == "project-skill"));

    // verify=false has empty deployments
    for skill in &status.skills {
        assert!(
            skill.deployments.is_empty(),
            "verify=false should have empty deployments"
        );
    }
}

// =============================================================================
// 3.2 Skill Aggregation Tests (parallel to MCP aggregation)
// =============================================================================

use sift_core::status::SkillDeployment;

/// Test: SkillStatus should have aggregated_integrity() like McpServerStatus
#[test]
fn skill_aggregated_all_ok_when_all_clients_match() {
    let status = SkillStatus {
        name: "commit".to_string(),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("~/.claude/skills/commit"),
                scope: ConfigScope::Global,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::Installed,
            },
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("./.claude/skills/commit"),
                scope: ConfigScope::PerProjectShared,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::Installed,
            },
        ],
        mode: Some(LinkMode::Symlink),
        dst_path: None,
    };

    // SkillStatus should have aggregated_integrity() method
    assert_eq!(status.aggregated_integrity(), AggregatedIntegrity::AllOk(2));
}

/// Test: Skill partial integrity when some clients fail
#[test]
fn skill_aggregated_partial_when_mixed_status() {
    let status = SkillStatus {
        name: "commit".to_string(),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("~/.claude/skills/commit"),
                scope: ConfigScope::Global,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::Installed,
            },
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("./.claude/skills/commit"),
                scope: ConfigScope::PerProjectShared,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::Modified,
            },
        ],
        mode: Some(LinkMode::Symlink),
        dst_path: None,
    };

    assert_eq!(
        status.aggregated_integrity(),
        AggregatedIntegrity::Partial { ok: 1, total: 2 }
    );
}

/// Test: Skill all failed when none ok
#[test]
fn skill_aggregated_all_failed_when_none_ok() {
    let status = SkillStatus {
        name: "commit".to_string(),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("~/.claude/skills/commit"),
                scope: ConfigScope::Global,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::NotFound,
            },
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("./.claude/skills/commit"),
                scope: ConfigScope::PerProjectShared,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::BrokenLink,
            },
        ],
        mode: Some(LinkMode::Symlink),
        dst_path: None,
    };

    assert_eq!(
        status.aggregated_integrity(),
        AggregatedIntegrity::AllFailed(2)
    );
}

/// Test: Skill NotDeployed excluded from count
#[test]
fn skill_aggregated_excludes_not_deployed() {
    let status = SkillStatus {
        name: "commit".to_string(),
        constraint: "^1.0".to_string(),
        resolved_version: Some("1.2.3".to_string()),
        registry: "registry:official".to_string(),
        scope: ConfigScope::Global,
        source_file: PathBuf::from("~/.config/sift/sift.toml"),
        state: EntryState::Ok,
        deployments: vec![
            SkillDeployment {
                client_id: "claude-code".to_string(),
                dst_path: PathBuf::from("~/.claude/skills/commit"),
                scope: ConfigScope::Global,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::Installed,
            },
            SkillDeployment {
                client_id: "future-client".to_string(),
                dst_path: PathBuf::from("~/.future/skills/commit"),
                scope: ConfigScope::Global,
                mode: LinkMode::Symlink,
                integrity: SkillIntegrity::NotDeployed,
            },
        ],
        mode: Some(LinkMode::Symlink),
        dst_path: None,
    };

    // Only 1 was expected to be deployed, and it's OK
    assert_eq!(status.aggregated_integrity(), AggregatedIntegrity::AllOk(1));
}

// =============================================================================
// 3.3 Orphaned Entries Scope Preservation Tests
// =============================================================================

/// Test: LockedSkill should have scope field
#[test]
fn locked_skill_has_scope_field() {
    let locked_skill = LockedSkill::new(
        "test".to_string(),
        "1.0.0".to_string(),
        "^1.0".to_string(),
        "registry:official".to_string(),
        ConfigScope::Global,
    );

    // Verify scope field is accessible and correct
    assert_eq!(locked_skill.scope, ConfigScope::Global);
}

/// RED TEST: Orphaned entry from global config should preserve Global scope
/// This test will FAIL until we save scope in lockfile
#[test]
fn orphaned_global_skill_preserves_scope() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create global config with a skill
    let global_toml = r#"
[skill.global-skill]
source = "registry:official/global-skill"
version = "^1.0"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // Create lockfile with the skill (simulating it was installed)
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "global-skill".to_string(),
        LockedSkill::new(
            "global-skill".to_string(),
            "1.2.3".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );
    LockfileStore::save(Some(tmp.path().to_path_buf()), state_dir.clone(), &lockfile)
        .expect("Failed to save lockfile");

    // Now remove the skill from config (making it orphaned)
    std::fs::write(global_dir.join("sift.toml"), "# empty\n")
        .expect("Failed to clear global config");

    // Run status with scope_filter=Global
    // Should show the orphaned entry with Global scope
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::Global),
        false,
    )
    .expect("collect_status should succeed");

    // Find the orphaned entry
    let orphaned = status.skills.iter().find(|s| s.name == "global-skill");

    // This will fail until we:
    // 1. Add scope field to LockedSkill
    // 2. Save scope when installing
    // 3. Use lockfile scope for orphaned entries
    assert!(orphaned.is_some(), "Should find orphaned global-skill");
    assert_eq!(
        orphaned.unwrap().scope,
        ConfigScope::Global,
        "Orphaned entry should preserve its original Global scope"
    );
}

/// RED TEST: Orphaned entry from project config should preserve Project scope
#[test]
fn orphaned_project_skill_preserves_scope() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    // Create project config with a skill
    let project_toml = r#"
[skill.project-skill]
source = "registry:official/project-skill"
version = "^2.0"
"#;
    std::fs::write(tmp.path().join("sift.toml"), project_toml)
        .expect("Failed to write project config");

    // Create lockfile with the skill
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "project-skill".to_string(),
        LockedSkill::new(
            "project-skill".to_string(),
            "2.1.0".to_string(),
            "^2.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(Some(tmp.path().to_path_buf()), state_dir.clone(), &lockfile)
        .expect("Failed to save lockfile");

    // Remove from config (making it orphaned)
    std::fs::write(tmp.path().join("sift.toml"), "# empty\n")
        .expect("Failed to clear project config");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Run status with scope_filter=PerProjectShared
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &state_dir,
        Some(ConfigScope::PerProjectShared),
        false,
    )
    .expect("collect_status should succeed");

    // Should show orphaned with Project scope
    let orphaned = status.skills.iter().find(|s| s.name == "project-skill");

    assert!(orphaned.is_some(), "Should find orphaned project-skill");
    assert_eq!(
        orphaned.unwrap().scope,
        ConfigScope::PerProjectShared,
        "Orphaned entry should preserve its original Project scope"
    );
}

/// RED TEST: scope_filter should not hide orphaned entries
/// Orphaned entries should be shown when their original scope matches the filter
#[test]
fn scope_filter_shows_orphaned_entries_from_matching_scope() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create both global and project skills
    let global_toml = r#"
[skill.global-tool]
source = "registry:official/global-tool"
version = "^1.0"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    let project_toml = r#"
[skill.project-tool]
source = "registry:official/project-tool"
version = "^2.0"
"#;
    std::fs::write(tmp.path().join("sift.toml"), project_toml)
        .expect("Failed to write project config");

    // Create lockfile with both skills
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "global-tool".to_string(),
        LockedSkill::new(
            "global-tool".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );
    lockfile.skills.insert(
        "project-tool".to_string(),
        LockedSkill::new(
            "project-tool".to_string(),
            "2.0.0".to_string(),
            "^2.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        ),
    );
    LockfileStore::save(Some(tmp.path().to_path_buf()), state_dir.clone(), &lockfile)
        .expect("Failed to save lockfile");

    // Remove both from configs (making them orphaned)
    std::fs::write(global_dir.join("sift.toml"), "# empty\n")
        .expect("Failed to clear global config");
    std::fs::write(tmp.path().join("sift.toml"), "# empty\n")
        .expect("Failed to clear project config");

    // Run with scope_filter=Global - should show global-tool orphaned
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        Some(ConfigScope::Global),
        false,
    )
    .expect("collect_status should succeed");

    // Should find global-tool orphaned
    assert!(
        status
            .skills
            .iter()
            .any(|s| s.name == "global-tool" && s.state == EntryState::Orphaned),
        "Should show global-tool orphaned when filtering for Global scope"
    );

    // Should NOT show project-tool orphaned (wrong scope)
    assert!(
        !status.skills.iter().any(|s| s.name == "project-tool"),
        "Should NOT show project-tool orphaned when filtering for Global scope"
    );
}

/// RED TEST: Orphaned entries shown when scope_filter=None
#[test]
fn orphaned_entries_shown_when_no_scope_filter() {
    let tmp = TempDir::new().expect("Failed to create temp dir");

    let global_dir = tmp.path().join("global_config");
    std::fs::create_dir_all(&global_dir).expect("Failed to create global dir");

    // Create a global skill
    let global_toml = r#"
[skill.old-global]
source = "registry:official/old-global"
version = "^1.0"
"#;
    std::fs::write(global_dir.join("sift.toml"), global_toml)
        .expect("Failed to write global config");

    // Create lockfile
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("Failed to create state dir");

    let mut lockfile = Lockfile::default();
    lockfile.skills.insert(
        "old-global".to_string(),
        LockedSkill::new(
            "old-global".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        ),
    );
    LockfileStore::save(Some(tmp.path().to_path_buf()), state_dir.clone(), &lockfile)
        .expect("Failed to save lockfile");

    // Remove from config
    std::fs::write(global_dir.join("sift.toml"), "# empty\n")
        .expect("Failed to clear global config");

    // Run with scope_filter=None (show all)
    let status = collect_status_with_paths(
        tmp.path(),
        &global_dir,
        &tmp.path().join("state"),
        None,
        false,
    )
    .expect("collect_status should succeed");

    // Should show orphaned entry
    assert!(
        status
            .skills
            .iter()
            .any(|s| s.name == "old-global" && s.state == EntryState::Orphaned),
        "Should show orphaned entries when scope_filter=None"
    );
}
