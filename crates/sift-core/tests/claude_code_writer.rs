use std::collections::HashMap;

use serde_json::Value;
use tempfile::TempDir;

use sift_core::client::claude_code::{ClaudeCodeMcpWriter, ClaudeCodePaths};
use sift_core::config::ownership_store::OwnershipStore;
use sift_core::mcp::spec::McpResolvedServer;

#[test]
fn claude_code_project_writer_creates_mcp_json() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let paths = ClaudeCodePaths::new(home.clone(), project.clone());
    let ownership_store = OwnershipStore::new(temp.path().join("state"), Some(project.clone()));
    let writer = ClaudeCodeMcpWriter::new(paths, ownership_store);

    let servers = vec![McpResolvedServer::stdio(
        "local".to_string(),
        "npx".to_string(),
        vec!["pkg@1.2.3".to_string()],
        HashMap::new(),
    )];

    writer.apply_project_servers(&servers, false).unwrap();

    let mcp_path = project.join(".mcp.json");
    let bytes = std::fs::read(&mcp_path).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let servers_obj = value.get("mcpServers").and_then(|v| v.as_object()).unwrap();

    assert!(servers_obj.contains_key("local"));
}

#[test]
fn claude_code_user_writer_creates_claude_json() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let paths = ClaudeCodePaths::new(home.clone(), project.clone());
    let ownership_store = OwnershipStore::new(temp.path().join("state"), Some(project.clone()));
    let writer = ClaudeCodeMcpWriter::new(paths, ownership_store);

    let servers = vec![McpResolvedServer::http(
        "remote".to_string(),
        "https://api.example.com/mcp".to_string(),
        HashMap::new(),
    )];

    writer.apply_user_servers(&servers, false).unwrap();

    let config_path = home.join(".claude.json");
    let bytes = std::fs::read(&config_path).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let servers_obj = value.get("mcpServers").and_then(|v| v.as_object()).unwrap();

    assert!(servers_obj.contains_key("remote"));
}

#[test]
fn claude_code_local_writer_creates_project_entry() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let paths = ClaudeCodePaths::new(home.clone(), project.clone());
    let ownership_store = OwnershipStore::new(temp.path().join("state"), Some(project.clone()));
    let writer = ClaudeCodeMcpWriter::new(paths, ownership_store);

    let servers = vec![McpResolvedServer::stdio(
        "local".to_string(),
        "npx".to_string(),
        vec!["pkg@1.2.3".to_string()],
        HashMap::new(),
    )];

    writer.apply_local_servers(&servers, false).unwrap();

    let config_path = home.join(".claude.json");
    let bytes = std::fs::read(&config_path).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();

    let projects = value.get("projects").and_then(|v| v.as_object()).unwrap();
    let project_key = project.to_string_lossy();
    let project_entry = projects
        .get(project_key.as_ref())
        .and_then(|v| v.as_object())
        .unwrap();
    let servers_obj = project_entry
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .unwrap();

    assert!(servers_obj.contains_key("local"));
}

#[test]
fn claude_code_apply_scope_routes_to_project() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let paths = ClaudeCodePaths::new(home.clone(), project.clone());
    let ownership_store = OwnershipStore::new(temp.path().join("state"), Some(project.clone()));
    let writer = ClaudeCodeMcpWriter::new(paths, ownership_store);

    let servers = vec![McpResolvedServer::stdio(
        "local".to_string(),
        "npx".to_string(),
        vec!["pkg@1.2.3".to_string()],
        HashMap::new(),
    )];

    writer
        .apply_servers_for_scope(
            sift_core::config::ConfigScope::PerProjectShared,
            &servers,
            false,
        )
        .unwrap();

    assert!(project.join(".mcp.json").exists());
    assert!(!home.join(".claude.json").exists());
}
