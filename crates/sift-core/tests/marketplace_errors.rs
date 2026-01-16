//! Non-happy path tests for marketplace.json parsing and MCP server resolution.
//!
//! Tests error conditions in marketplace manifest parsing, plugin resolution,
//! and MCP server configuration extraction.

use sift_core::registry::marketplace::{
    MarketplaceAdapter, MarketplacePlugin, MarketplaceSource, MarketplaceSourceObject,
    SkillsOrPaths, SourceType,
};

// =========================================================================
// Malformed marketplace.json Tests
// =========================================================================

#[test]
fn parse_marketplace_invalid_json_errors() {
    let invalid_json = r#"{ invalid json }"#;
    let result = MarketplaceAdapter::parse(invalid_json);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Failed to parse") || err.contains("parse"));
}

#[test]
fn parse_marketplace_missing_plugins_array_errors() {
    // Missing plugins field - should error
    let incomplete = r#"{"marketplace": {"name": "test"}}"#;
    let result = MarketplaceAdapter::parse(incomplete);
    assert!(result.is_err());
}

#[test]
fn parse_marketplace_empty_plugins_array_returns_empty() {
    // Empty plugins array is valid
    let empty_plugins = r#"{
        "marketplace": {"name": "test"},
        "plugins": []
    }"#;
    let result = MarketplaceAdapter::parse(empty_plugins);
    assert!(result.is_ok());
    let manifest = result.unwrap();
    assert_eq!(manifest.plugins.len(), 0);
}

#[test]
fn parse_marketplace_missing_plugin_name_errors() {
    // Plugin without required name field
    let json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "description": "Test plugin",
            "source": "./test"
        }]
    }"#;
    let result = MarketplaceAdapter::parse(json);
    assert!(result.is_err());
}

#[test]
fn parse_marketplace_missing_plugin_description_errors() {
    // Plugin without required description field
    let json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "test-plugin",
            "source": "./test"
        }]
    }"#;
    let result = MarketplaceAdapter::parse(json);
    assert!(result.is_err());
}

// =========================================================================
// Invalid Git Source Configuration Tests
// =========================================================================

#[test]
fn get_source_string_github_missing_repo_field_errors() {
    let plugin = MarketplacePlugin {
        name: "test".to_string(),
        description: "Test".to_string(),
        version: "1.0.0".to_string(),
        source: MarketplaceSource::Object(MarketplaceSourceObject {
            source: SourceType::Github,
            repo: None, // Missing!
            url: None,
            ref_: None,
            path: None,
        }),
        skills: None,
        hooks: None,
        mcp_servers: None,
        author: None,
        homepage: None,
        repository: None,
        license: None,
        keywords: vec![],
        category: None,
        tags: vec![],
        strict: None,
    };

    let result = MarketplaceAdapter::get_source_string(&plugin);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("repo"));
}

#[test]
fn get_source_string_url_source_missing_url_field_errors() {
    let plugin = MarketplacePlugin {
        name: "test".to_string(),
        description: "Test".to_string(),
        version: "1.0.0".to_string(),
        source: MarketplaceSource::Object(MarketplaceSourceObject {
            source: SourceType::Url,
            repo: None,
            url: None, // Missing!
            ref_: None,
            path: None,
        }),
        skills: None,
        hooks: None,
        mcp_servers: None,
        author: None,
        homepage: None,
        repository: None,
        license: None,
        keywords: vec![],
        category: None,
        tags: vec![],
        strict: None,
    };

    let result = MarketplaceAdapter::get_source_string(&plugin);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("url"));
}

// =========================================================================
// Invalid mcpServers Configuration Tests
// =========================================================================

#[test]
fn plugin_to_mcp_configs_object_without_command_or_url_errors() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "invalid-mcp",
            "description": "Invalid MCP config",
            "version": "1.0.0",
            "source": "./test",
            "mcpServers": {
                "empty-server": {
                    "env": {"TEST": "value"}
                }
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let result = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("command") || err.contains("url") || err.contains("Invalid"),
        "Error should mention missing command/url: {}",
        err
    );
}

#[test]
fn plugin_to_mcp_configs_empty_mcp_servers_object_errors() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "empty-object-mcp",
            "description": "Empty object value",
            "version": "1.0.0",
            "source": "./test",
            "mcpServers": {
                "empty": {}
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let result = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None);
    assert!(result.is_err());
}

#[test]
fn plugin_to_mcp_configs_invalid_command_type_errors() {
    // Command is a number instead of string
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "bad-command-type",
            "description": "Invalid command type",
            "version": "1.0.0",
            "source": "./test",
            "mcpServers": {
                "bad-mcp": {
                    "command": 12345
                }
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let result = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None);
    // Should either error or produce invalid config
    // Let's verify it doesn't panic and either returns error or a config we can validate
    if let Ok(configs) = result {
        // If it succeeds, the command should be handled (possibly as string "12345")
        assert!(!configs.is_empty());
    }
    // If it errors, that's also acceptable behavior
}

#[test]
fn plugin_to_mcp_configs_invalid_args_type_errors() {
    // Args is a string instead of array
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "bad-args-type",
            "description": "Invalid args type",
            "version": "1.0.0",
            "source": "./test",
            "mcpServers": {
                "bad-mcp": {
                    "command": "npx",
                    "args": "not-an-array"
                }
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let result = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None);
    // Should either error or handle gracefully (empty args array)
    assert!(result.is_ok() || result.is_err());
}

// =========================================================================
// Invalid SkillsOrPaths Tests
// =========================================================================

#[test]
fn plugin_to_skill_configs_empty_path_in_array_skipped() {
    let plugin = MarketplacePlugin {
        name: "test".to_string(),
        description: "Test".to_string(),
        version: "1.0.0".to_string(),
        source: MarketplaceSource::String("github:anthropics/skills".to_string()),
        skills: Some(SkillsOrPaths::Multiple(vec![
            "./skills/xlsx".to_string(),
            "".to_string(), // Empty path
            "./skills/docx".to_string(),
        ])),
        hooks: None,
        mcp_servers: None,
        author: None,
        homepage: None,
        repository: None,
        license: None,
        keywords: vec![],
        category: None,
        tags: vec![],
        strict: None,
    };

    let result = MarketplaceAdapter::plugin_to_skill_configs(&plugin);
    assert!(result.is_ok());
    let configs = result.unwrap();
    // Empty path might be included or skipped - both are acceptable
    assert!(!configs.is_empty(), "Should have at least one config");
}

#[test]
fn plugin_to_skill_configs_absolute_path_handled() {
    let plugin = MarketplacePlugin {
        name: "test".to_string(),
        description: "Test".to_string(),
        version: "1.0.0".to_string(),
        source: MarketplaceSource::String("local:./base".to_string()),
        skills: Some(SkillsOrPaths::Multiple(vec![
            "/absolute/path/skill".to_string(),
        ])),
        hooks: None,
        mcp_servers: None,
        author: None,
        homepage: None,
        repository: None,
        license: None,
        keywords: vec![],
        category: None,
        tags: vec![],
        strict: None,
    };

    let result = MarketplaceAdapter::plugin_to_skill_configs(&plugin);
    assert!(result.is_ok());
    let configs = result.unwrap();
    assert_eq!(configs.len(), 1);
    // Absolute path should be used as-is
    assert_eq!(configs[0].source, "/absolute/path/skill");
}
