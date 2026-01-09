use std::collections::HashMap;

use serde_json::json;

use sift_core::client::claude_code::{ClaudeCodeClient, ClaudeCodeScope};
use sift_core::mcp::spec::McpResolvedServer;

#[test]
fn claude_code_skill_paths() {
    let client = ClaudeCodeClient::new();

    let global = client.skill_path(ClaudeCodeScope::User).unwrap();
    assert!(global.ends_with("/.claude/skills"));

    let project = client.skill_path(ClaudeCodeScope::Project).unwrap();
    assert_eq!(project, "./.claude/skills");
}

#[test]
fn claude_code_project_mcp_config() {
    let client = ClaudeCodeClient::new();

    let mut env = HashMap::new();
    env.insert("TOKEN".to_string(), "secret".to_string());

    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer token".to_string());

    let servers = vec![
        McpResolvedServer::stdio(
            "local".to_string(),
            "npx".to_string(),
            vec!["pkg@1.2.3".to_string()],
            env,
        ),
        McpResolvedServer::http(
            "remote".to_string(),
            "https://api.example.com/mcp".to_string(),
            headers,
        ),
    ];

    let rendered = client.render_project_mcp_config(&servers).unwrap();

    let expected = json!({
        "mcpServers": {
            "local": {
                "command": "npx",
                "args": ["pkg@1.2.3"],
                "env": {"TOKEN": "secret"}
            },
            "remote": {
                "type": "http",
                "url": "https://api.example.com/mcp",
                "headers": {"Authorization": "Bearer token"}
            }
        }
    });

    assert_eq!(rendered, expected);
}
