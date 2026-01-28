//! Tests for the deploy executor module.

use std::collections::HashMap;

use sift_core::client::ClientContext;
use sift_core::client::claude_code::ClaudeCodeClient;
use sift_core::deploy::executor::deploy_mcp_to_client;
use sift_core::lockfile::LockfileService;
use sift_core::mcp::spec::McpResolvedServer;
use sift_core::types::ConfigScope;
use tempfile::TempDir;

#[test]
fn deploy_mcp_to_client_writes_config_file() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let ctx = ClientContext::new(home.clone(), project.clone());
    let client = ClaudeCodeClient::new();
    let lockfile = LockfileService::new(temp.path().join("state"), Some(project.clone()));

    let servers = vec![McpResolvedServer::stdio(
        "test-server".to_string(),
        "npx".to_string(),
        vec!["pkg@1.0".to_string()],
        HashMap::new(),
    )];

    let report = deploy_mcp_to_client(
        &client,
        &ctx,
        ConfigScope::PerProjectShared,
        &servers,
        &lockfile,
        false, // force
    )
    .unwrap();

    assert!(report.applied);
    assert!(project.join(".mcp.json").exists());
}
