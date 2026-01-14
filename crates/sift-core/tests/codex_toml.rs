//! Integration tests for Codex TOML configuration support.

use serde_json::json;
use sift_core::client::codex::CodexClient;
use sift_core::client::{ClientAdapter, ClientContext, McpConfigFormat};
use sift_core::config::client_config::{ConfigFormat, ConfigSerializer, TomlSerializer};
use sift_core::mcp::spec::{McpResolvedServer, McpTransport};
use sift_core::types::ConfigScope;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_context(temp_dir: &TempDir) -> ClientContext {
    ClientContext {
        home_dir: temp_dir.path().to_path_buf(),
        project_root: temp_dir.path().join("project"),
    }
}

#[test]
fn codex_client_uses_toml_format() {
    let client = CodexClient::new();
    let caps = client.capabilities();

    assert_eq!(caps.mcp_config_format, McpConfigFormat::Toml);
}

#[test]
fn codex_plan_mcp_returns_toml_format() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let ctx = create_test_context(&temp_dir);
    let client = CodexClient::new();

    let servers = vec![McpResolvedServer {
        name: "test-server".to_string(),
        transport: McpTransport::Stdio,
        command: Some("npx".to_string()),
        args: vec!["-y".to_string(), "@test/mcp".to_string()],
        env: std::collections::HashMap::new(),
        url: None,
        headers: std::collections::HashMap::new(),
    }];

    let plan = client
        .plan_mcp(&ctx, ConfigScope::Global, &servers)
        .expect("plan_mcp should succeed");

    assert_eq!(plan.format, McpConfigFormat::Toml);
    assert_eq!(plan.relative_path, PathBuf::from(".codex/config.toml"));
    assert_eq!(plan.config_path, vec!["mcp_servers".to_string()]);
}

#[test]
fn toml_serializer_writes_codex_format() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_path = temp_dir.path().join("config.toml");

    let serializer = TomlSerializer;

    // Build a Codex-style MCP config
    let mut mcp_servers = serde_json::Map::new();

    // STDIO server
    let mut server = serde_json::Map::new();
    server.insert("command".to_string(), json!("npx"));
    server.insert("args".to_string(), json!(["-y", "@upstash/context7-mcp"]));
    let mut env = serde_json::Map::new();
    env.insert("API_KEY".to_string(), json!("secret"));
    server.insert("env".to_string(), serde_json::Value::Object(env));
    mcp_servers.insert("context7".to_string(), serde_json::Value::Object(server));

    let mut root = serde_json::Map::new();
    root.insert(
        "mcp_servers".to_string(),
        serde_json::Value::Object(mcp_servers),
    );

    serializer
        .save(&config_path, &root)
        .expect("save should succeed");

    // Verify the written TOML content
    let content = std::fs::read_to_string(&config_path).expect("read file");

    // Should contain proper TOML structure
    assert!(
        content.contains("mcp_servers"),
        "Should have mcp_servers section"
    );
    assert!(content.contains("context7"), "Should have context7 server");
    assert!(content.contains("command"), "Should have command field");
    assert!(content.contains("npx"), "Should have npx value");

    // Verify roundtrip
    let loaded = serializer.load(&config_path).expect("load should succeed");
    assert_eq!(root, loaded);
}

#[test]
fn config_format_converts_from_mcp_config_format() {
    assert_eq!(
        ConfigFormat::from(McpConfigFormat::Toml),
        ConfigFormat::Toml
    );
    assert_eq!(
        ConfigFormat::from(McpConfigFormat::Generic),
        ConfigFormat::Json
    );
    assert_eq!(
        ConfigFormat::from(McpConfigFormat::ClaudeCode),
        ConfigFormat::Json
    );
    assert_eq!(
        ConfigFormat::from(McpConfigFormat::ClaudeDesktop),
        ConfigFormat::Json
    );
}

#[test]
fn toml_serializer_handles_http_server() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let config_path = temp_dir.path().join("config.toml");

    let serializer = TomlSerializer;

    let mut mcp_servers = serde_json::Map::new();

    // HTTP server
    let mut server = serde_json::Map::new();
    server.insert("url".to_string(), json!("https://mcp.figma.com/mcp"));
    let mut headers = serde_json::Map::new();
    headers.insert("X-Figma-Region".to_string(), json!("us-east-1"));
    server.insert(
        "http_headers".to_string(),
        serde_json::Value::Object(headers),
    );
    mcp_servers.insert("figma".to_string(), serde_json::Value::Object(server));

    let mut root = serde_json::Map::new();
    root.insert(
        "mcp_servers".to_string(),
        serde_json::Value::Object(mcp_servers),
    );

    serializer.save(&config_path, &root).expect("save");

    let content = std::fs::read_to_string(&config_path).expect("read");
    assert!(content.contains("figma"));
    assert!(content.contains("url"));
    assert!(content.contains("https://mcp.figma.com/mcp"));

    let loaded = serializer.load(&config_path).expect("load");
    assert_eq!(root, loaded);
}
