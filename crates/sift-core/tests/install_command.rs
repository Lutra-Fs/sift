//! Integration tests for the install command

use tempfile::TempDir;

use sift_core::commands::{InstallCommand, InstallOptions};
use sift_core::config::ConfigScope;
use sift_core::fs::LinkMode;

fn setup_isolated_install_command() -> (TempDir, InstallCommand) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let state = temp.path().join("state");
    let global_config = temp.path().join("config");

    std::fs::create_dir_all(&home).expect("Failed to create home dir");
    std::fs::create_dir_all(&project).expect("Failed to create project dir");
    std::fs::create_dir_all(&state).expect("Failed to create state dir");
    std::fs::create_dir_all(&global_config).expect("Failed to create global config dir");

    let cmd = InstallCommand::with_global_config_dir(
        home,
        project,
        state,
        global_config,
        LinkMode::Copy,
    );

    (temp, cmd)
}

#[test]
fn install_mcp_server_creates_config_and_applies() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("postgres")
        .with_source("registry:postgres-mcp")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");

    // Verify report
    assert_eq!(report.name, "postgres");
    assert!(report.changed);
    assert!(report.applied);

    // Verify config file was created
    let config_path = temp.path().join("project").join("sift.toml");
    assert!(config_path.exists(), "sift.toml should be created");

    // Verify .mcp.json was created for Claude Code client
    let mcp_json_path = temp.path().join("project").join(".mcp.json");
    assert!(mcp_json_path.exists(), ".mcp.json should be created");

    // Verify content
    let content = std::fs::read_to_string(&mcp_json_path).expect("Should read .mcp.json");
    assert!(content.contains("postgres"), "Should contain postgres server");
}

#[test]
fn install_skill_creates_config_and_directory() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::skill("commit")
        .with_source("registry:official/commit")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");

    // Verify report
    assert_eq!(report.name, "commit");
    assert!(report.changed);

    // Verify config file was created
    let config_path = temp.path().join("project").join("sift.toml");
    assert!(config_path.exists(), "sift.toml should be created");

    // Parse and verify config content
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("[skill.commit]"),
        "Should contain skill entry"
    );
}

#[test]
fn install_is_idempotent() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("idempotent-test")
        .with_source("registry:test")
        .with_scope(ConfigScope::PerProjectShared);

    // First install
    let report1 = cmd.execute(&opts).expect("First install should succeed");
    assert!(report1.changed, "First install should change config");

    // Second install (identical)
    let report2 = cmd.execute(&opts).expect("Second install should succeed");
    assert!(!report2.changed, "Second install should be a no-op");
}

#[test]
fn install_with_force_overwrites_existing() {
    let (_temp, cmd) = setup_isolated_install_command();

    // First install
    let opts1 = InstallOptions::mcp("force-test")
        .with_source("registry:source1")
        .with_scope(ConfigScope::PerProjectShared);
    cmd.execute(&opts1).expect("First install should succeed");

    // Second install with different source (without force - should fail)
    let opts2 = InstallOptions::mcp("force-test")
        .with_source("registry:source2")
        .with_scope(ConfigScope::PerProjectShared);
    let result = cmd.execute(&opts2);
    assert!(result.is_err(), "Should fail without force flag");

    // Third install with force flag
    let opts3 = InstallOptions::mcp("force-test")
        .with_source("registry:source2")
        .with_scope(ConfigScope::PerProjectShared)
        .with_force(true);
    let report = cmd.execute(&opts3).expect("Force install should succeed");
    assert!(report.changed, "Force install should change config");
}

#[test]
fn install_global_scope_writes_to_global_config() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("global-test")
        .with_source("registry:global-test")
        .with_scope(ConfigScope::Global);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    // Verify global config was created
    let global_config_path = temp.path().join("config").join("sift.toml");
    assert!(
        global_config_path.exists(),
        "Global sift.toml should be created"
    );

    let content = std::fs::read_to_string(&global_config_path).expect("Should read global config");
    assert!(
        content.contains("global-test"),
        "Should contain global-test entry"
    );
}

#[test]
fn install_with_version_constraint() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::skill("versioned-skill")
        .with_source("registry:test/versioned")
        .with_version("^1.0.0")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    // Verify config contains version
    let config_path = temp.path().join("project").join("sift.toml");
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("version = \"^1.0.0\""),
        "Should contain version constraint"
    );
}

#[test]
fn install_mcp_with_runtime() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("docker-mcp")
        .with_source("registry:docker-mcp")
        .with_runtime("docker")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    // Verify config contains runtime
    let config_path = temp.path().join("project").join("sift.toml");
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("runtime = \"docker\""),
        "Should contain runtime"
    );
}
