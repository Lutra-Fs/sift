//! Integration tests for MCP server resolution from marketplace registries
//!
//! Tests that MCP servers defined in marketplace.json with MCPB bundle URLs
//! are correctly resolved and installed.

use sift_core::mcp::McpConfig;
use sift_core::registry::marketplace::MarketplaceAdapter;
use sift_core::source::McpRegistryResolution;

// =========================================================================
// Marketplace Plugin MCP Config Extraction Tests (Unit-level, no network)
// =========================================================================

#[test]
fn plugin_to_mcp_configs_extracts_mcpb_url_string() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "test-mcp",
            "description": "Test MCP server",
            "version": "1.0.0",
            "source": "./test-mcp",
            "mcpServers": "https://example.com/releases/test-mcp.mcpb"
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)
        .expect("Should extract MCP configs");

    assert_eq!(configs.len(), 1);
    let (name, config) = &configs[0];
    assert_eq!(name, "test-mcp");
    assert_eq!(
        config.source,
        "mcpb:https://example.com/releases/test-mcp.mcpb"
    );
}

#[test]
fn plugin_to_mcp_configs_extracts_http_url_string() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "remote-api",
            "description": "Remote HTTP MCP",
            "version": "1.0.0",
            "source": "./remote-api",
            "mcpServers": "https://api.example.com/mcp"
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)
        .expect("Should extract MCP configs");

    assert_eq!(configs.len(), 1);
    let (name, config) = &configs[0];
    assert_eq!(name, "remote-api");
    assert_eq!(config.url, Some("https://api.example.com/mcp".to_string()));
}

#[test]
fn plugin_to_mcp_configs_extracts_named_server_objects() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "multi-server",
            "description": "Multiple MCP servers",
            "version": "1.0.0",
            "source": "./multi-server",
            "mcpServers": {
                "primary": {
                    "command": "npx",
                    "args": ["@example/mcp-server"]
                },
                "secondary": "https://backup.example.com/mcp"
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)
        .expect("Should extract MCP configs");

    assert_eq!(configs.len(), 2);

    let names: Vec<_> = configs.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"primary"));
    assert!(names.contains(&"secondary"));
}

#[test]
fn plugin_to_mcp_configs_named_mcpb_url_string_is_mcpb_source() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "named-mcpb",
            "description": "Named MCPB server",
            "version": "1.0.0",
            "source": "./named-mcpb",
            "mcpServers": {
                "primary": "https://example.com/server.mcpb"
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)
        .expect("Should extract MCP configs");

    assert_eq!(configs.len(), 1);
    let (name, config) = &configs[0];
    assert_eq!(name, "primary");
    assert!(config.source.starts_with("mcpb:"));
    assert!(config.url.is_none());
    assert_eq!(config.transport, sift_core::mcp::TransportType::Stdio);
}

// =========================================================================
// MCP Registry Resolution Tests
// =========================================================================

#[test]
fn mcp_registry_resolution_returns_mcpb_source() {
    let manifest_json = r#"{
        "marketplace": {"name": "test-registry"},
        "plugins": [{
            "name": "bundled-server",
            "description": "MCPB bundled server",
            "version": "2.0.0",
            "source": "./bundled-server",
            "mcpServers": "https://github.com/org/repo/releases/v2.0.0/bundled-server.mcpb"
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin =
        MarketplaceAdapter::find_plugin(&manifest, "bundled-server").expect("Plugin should exist");

    let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)
        .expect("Should extract MCP configs");

    let (_, config) = &configs[0];

    // The source should be normalized to mcpb: prefix
    assert!(
        config.source.starts_with("mcpb:"),
        "MCPB URL should be normalized to mcpb: source, got: {}",
        config.source
    );
}

#[test]
fn mcp_registry_resolution_plugin_not_found() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "existing-plugin",
            "description": "Exists",
            "version": "1.0.0",
            "source": "./existing"
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = MarketplaceAdapter::find_plugin(&manifest, "non-existent");

    assert!(plugin.is_none(), "Plugin should not be found");
}

#[test]
fn mcp_registry_resolution_plugin_without_mcp_servers() {
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "skill-only",
            "description": "Skills only, no MCP",
            "version": "1.0.0",
            "source": "./skill-only",
            "skills": ["./SKILL.md"]
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin = &manifest.plugins[0];

    let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)
        .expect("Should succeed even without MCP");

    assert!(
        configs.is_empty(),
        "Plugin without mcpServers should return empty configs"
    );
}

// =========================================================================
// McpRegistryResolution Structure Tests
// =========================================================================

#[test]
fn mcp_registry_resolution_structure_contains_metadata() {
    // Test that McpRegistryResolution contains the right fields
    let resolution = McpRegistryResolution {
        mcp_config: McpConfig {
            transport: sift_core::mcp::TransportType::Stdio,
            source: "mcpb:https://example.com/server.mcpb".to_string(),
            runtime: sift_core::mcp::RuntimeType::Node,
            args: vec![],
            url: None,
            headers: std::collections::HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: std::collections::HashMap::new(),
        },
        registry_key: "anthropic-skills".to_string(),
        plugin_name: "test-server".to_string(),
        plugin_version: "1.0.0".to_string(),
    };

    assert_eq!(resolution.registry_key, "anthropic-skills");
    assert_eq!(resolution.plugin_name, "test-server");
    assert_eq!(resolution.plugin_version, "1.0.0");
    assert!(resolution.mcp_config.source.starts_with("mcpb:"));
}

// =========================================================================
// SourceResolver.resolve_mcp_registry Tests
// =========================================================================

#[test]
fn source_resolver_resolve_mcp_registry_returns_mcpb_config() {
    use sift_core::source::SourceResolver;
    use std::collections::HashMap;
    use tempfile::TempDir;

    // Setup: Create a minimal marketplace with an MCPB server
    let temp = TempDir::new().expect("Failed to create temp dir");
    let state_dir = temp.path().join("state");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&state_dir).expect("mkdir state");
    std::fs::create_dir_all(&project_root).expect("mkdir project");

    // The resolver needs registries configured - for this test we'll call the
    // method directly after manually creating a manifest, simulating what would
    // happen after git fetch
    let registries = HashMap::new();

    let resolver = SourceResolver::new(state_dir, project_root, registries);

    // resolve_mcp_from_plugin is a helper we need to implement
    // It takes a plugin and returns McpRegistryResolution
    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "bundled-mcp",
            "description": "MCPB bundled MCP server",
            "version": "1.0.0",
            "source": "./bundled-mcp",
            "mcpServers": "https://example.com/bundled-mcp.mcpb"
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin =
        MarketplaceAdapter::find_plugin(&manifest, "bundled-mcp").expect("Plugin should exist");

    // Call the new method we need to implement
    let resolutions = resolver
        .resolve_mcp_from_plugin(plugin, "test-registry")
        .expect("Should resolve MCP from plugin");

    assert_eq!(resolutions.len(), 1);
    let resolution = &resolutions[0];
    assert_eq!(resolution.plugin_name, "bundled-mcp");
    assert_eq!(resolution.registry_key, "test-registry");
    assert!(
        resolution.mcp_config.source.starts_with("mcpb:"),
        "Should have mcpb: source, got: {}",
        resolution.mcp_config.source
    );
}

#[test]
fn source_resolver_resolve_mcp_from_plugin_handles_multiple_servers() {
    use sift_core::source::SourceResolver;
    use std::collections::HashMap;
    use tempfile::TempDir;

    let temp = TempDir::new().expect("Failed to create temp dir");
    let state_dir = temp.path().join("state");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&state_dir).expect("mkdir state");
    std::fs::create_dir_all(&project_root).expect("mkdir project");

    let resolver = SourceResolver::new(state_dir, project_root, HashMap::new());

    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "multi-mcp",
            "description": "Multiple MCP servers",
            "version": "2.0.0",
            "source": "./multi-mcp",
            "mcpServers": {
                "primary": "https://example.com/primary.mcpb",
                "secondary": {
                    "command": "npx",
                    "args": ["@example/secondary-mcp"]
                }
            }
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin =
        MarketplaceAdapter::find_plugin(&manifest, "multi-mcp").expect("Plugin should exist");

    let resolutions = resolver
        .resolve_mcp_from_plugin(plugin, "test-registry")
        .expect("Should resolve MCP from plugin");

    assert_eq!(resolutions.len(), 2, "Should have 2 MCP servers");

    let names: Vec<_> = resolutions.iter().map(|r| r.plugin_name.as_str()).collect();
    assert!(names.contains(&"primary"));
    assert!(names.contains(&"secondary"));
}

#[test]
fn source_resolver_resolve_mcp_from_plugin_empty_for_skill_only() {
    use sift_core::source::SourceResolver;
    use std::collections::HashMap;
    use tempfile::TempDir;

    let temp = TempDir::new().expect("Failed to create temp dir");
    let state_dir = temp.path().join("state");
    let project_root = temp.path().join("project");
    std::fs::create_dir_all(&state_dir).expect("mkdir state");
    std::fs::create_dir_all(&project_root).expect("mkdir project");

    let resolver = SourceResolver::new(state_dir, project_root, HashMap::new());

    let manifest_json = r#"{
        "marketplace": {"name": "test"},
        "plugins": [{
            "name": "skill-only",
            "description": "Skill only plugin",
            "version": "1.0.0",
            "source": "./skill-only",
            "skills": ["./SKILL.md"]
        }]
    }"#;

    let manifest = MarketplaceAdapter::parse(manifest_json).expect("Parse should succeed");
    let plugin =
        MarketplaceAdapter::find_plugin(&manifest, "skill-only").expect("Plugin should exist");

    let resolutions = resolver
        .resolve_mcp_from_plugin(plugin, "test-registry")
        .expect("Should succeed");

    assert!(
        resolutions.is_empty(),
        "Skill-only plugin should have no MCP resolutions"
    );
}
