//! Integration tests for McpInstaller.

use std::collections::HashMap;

use sift_core::client::claude_code::ClaudeCodeClient;
use sift_core::config::McpConfigEntry;
use sift_core::context::AppContext;
use sift_core::fs::LinkMode;
use sift_core::mcp::installer::{McpInstallRequest, McpInstaller};
use sift_core::types::ConfigScope;
use tempfile::TempDir;

fn create_test_entry() -> McpConfigEntry {
    McpConfigEntry {
        transport: Some("stdio".to_string()),
        source: "registry:demo".to_string(),
        runtime: Some("shell".to_string()),
        args: vec![],
        url: None,
        headers: HashMap::new(),
        targets: None,
        ignore_targets: None,
        env: HashMap::new(),
        reset_targets: false,
        reset_ignore_targets: false,
        reset_env: None,
        reset_env_all: false,
    }
}

#[test]
fn mcp_installer_writes_toml_and_client_config() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let ctx = AppContext::with_global_config_dir(
        home.clone(),
        project.clone(),
        temp.path().join("state"),
        temp.path().join("config"),
        LinkMode::Auto,
    );

    let installer = McpInstaller::new(&ctx, ConfigScope::PerProjectShared);
    let client = ClaudeCodeClient::new();

    let request = McpInstallRequest {
        name: "test-mcp".to_string(),
        entry: create_test_entry(),
        version: None,
        force: false,
    };

    let report = installer.install(&client, request).unwrap();

    assert!(report.changed);
    assert!(report.applied);

    // Verify sift.toml was written
    let toml_path = project.join("sift.toml");
    assert!(toml_path.exists());
    let content = std::fs::read_to_string(&toml_path).unwrap();
    assert!(content.contains("test-mcp"));

    // Verify client config was written
    assert!(project.join(".mcp.json").exists());
}

#[test]
fn mcp_installer_skips_non_targeted_client() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let ctx = AppContext::with_global_config_dir(
        home.clone(),
        project.clone(),
        temp.path().join("state"),
        temp.path().join("config"),
        LinkMode::Auto,
    );

    let installer = McpInstaller::new(&ctx, ConfigScope::PerProjectShared);
    let client = ClaudeCodeClient::new();

    let mut entry = create_test_entry();
    entry.targets = Some(vec!["amp".to_string()]); // Only target amp, not claude-code

    let request = McpInstallRequest {
        name: "targeted-mcp".to_string(),
        entry,
        version: None,
        force: false,
    };

    let report = installer.install(&client, request).unwrap();

    assert!(report.changed); // sift.toml was written
    assert!(!report.applied); // client config was NOT written
    assert!(!report.warnings.is_empty()); // should have warning about skipping

    // sift.toml should exist
    assert!(project.join("sift.toml").exists());
    // client config should NOT exist
    assert!(!project.join(".mcp.json").exists());
}
