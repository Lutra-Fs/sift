//! Merge marketplace plugin entries with nested plugin.json
//!
//! For marketplace formats like anthropics/life-sciences, each plugin has
//! a nested `.claude-plugin/plugin.json` with additional configuration.
//! This module handles merging the marketplace entry with the nested plugin.

use super::adapter::MarketplacePlugin;
use serde_json::Value;

/// Merges a marketplace plugin entry with its nested plugin.json
///
/// # Arguments
/// * `marketplace_entry` - The plugin entry from marketplace.json
/// * `nested_plugin` - The plugin.json from the plugin's `.claude-plugin/` directory
///
/// # Merging Strategy
/// - `nested_plugin` takes precedence for most fields (version, mcpServers, etc.)
/// - `marketplace_entry` can override with marketplace-specific metadata (category, tags)
/// - Deep merge for objects (like mcpServers) to preserve both configs
///
/// # Example
/// ```ignore
/// let merged = merge_plugin_with_nested(&marketplace_entry, &nested_plugin)?;
/// assert_eq!(merged.version, "1.0.0"); // From nested
/// assert_eq!(merged.category, Some("life-sciences".to_string())); // From marketplace
/// ```
pub fn merge_plugin_with_nested(
    marketplace_entry: &Value,
    nested_plugin: &Value,
) -> anyhow::Result<MarketplacePlugin> {
    // Deep merge: nested_plugin is base, marketplace_entry overlays specific fields
    let merged = merge_json_deep(nested_plugin, marketplace_entry)?;

    // Deserialize merged JSON to MarketplacePlugin
    serde_json::from_value(merged)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize merged plugin: {}", e))
}

/// Deep merge JSON objects
/// - Base (nested_plugin) provides default values
/// - Overlay (marketplace_entry) can override specific fields
/// - Objects are merged recursively, not replaced
fn merge_json_deep(base: &Value, overlay: &Value) -> anyhow::Result<Value> {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut result = base_map.clone();

            for (key, overlay_value) in overlay_map {
                // Special handling for mcpServers - merge configs
                if (key == "mcpServers" || key == "mcp_servers")
                    && let Some(base_servers) = base_map.get(key.as_str())
                {
                    result.insert(
                        key.clone(),
                        merge_mcp_servers(base_servers, overlay_value)?,
                    );
                    continue;
                }

                // Recursively merge nested objects
                if let Some(base_value) = base_map.get(key.as_str())
                    && base_value.is_object() && overlay_value.is_object()
                {
                    result.insert(key.clone(), merge_json_deep(base_value, overlay_value)?);
                    continue;
                }

                // Overlay takes precedence for non-object fields
                result.insert(key.clone(), overlay_value.clone());
            }

            Ok(Value::Object(result))
        }
        // For non-object types, overlay wins
        (_, overlay) => Ok(overlay.clone()),
    }
}

/// Special handling for merging mcpServers configurations
/// Preserves existing servers while allowing marketplace to add/override
fn merge_mcp_servers(base: &Value, overlay: &Value) -> anyhow::Result<Value> {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut result = base_map.clone();

            // Overlay can add new servers or override existing ones
            for (server_name, overlay_config) in overlay_map {
                result.insert(server_name.clone(), overlay_config.clone());
            }

            Ok(Value::Object(result))
        }
        // If either is not an object, overlay wins
        (_, overlay) => Ok(overlay.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_plugin_with_nested() {
        let marketplace_entry = serde_json::json!({
            "name": "10x-genomics",
            "source": "./10x-genomics",
            "description": "Marketplace description",
            "category": "life-sciences",
            "tags": ["genomics", "bioinformatics"]
        });

        let nested_plugin = serde_json::json!({
            "name": "10x-genomics",
            "version": "1.0.0",
            "mcpServers": "https://github.com/10XGenomics/txg-mcp/releases/latest/download/txg-node.mcpb",
            "strict": false
        });

        let merged = merge_plugin_with_nested(&marketplace_entry, &nested_plugin).unwrap();

        assert_eq!(merged.name, "10x-genomics");
        assert_eq!(merged.version, "1.0.0"); // From nested
        assert_eq!(merged.description, "Marketplace description"); // From marketplace
        assert_eq!(merged.category, Some("life-sciences".to_string())); // From marketplace
        assert_eq!(merged.tags, vec!["genomics", "bioinformatics"]); // From marketplace
        assert_eq!(merged.strict, Some(false)); // From nested
        assert!(merged.mcp_servers.is_some()); // From nested
    }

    #[test]
    fn test_merge_json_deep() {
        let base = serde_json::json!({
            "name": "plugin",
            "version": "1.0.0",
            "config": {
                "option1": "value1",
                "option2": "value2"
            }
        });

        let overlay = serde_json::json!({
            "config": {
                "option2": "overridden",
                "option3": "value3"
            },
            "new_field": "new_value",
            "category": "development"
        });

        let merged = merge_json_deep(&base, &overlay).unwrap();

        assert_eq!(merged["name"], "plugin"); // From base
        assert_eq!(merged["version"], "1.0.0"); // From base
        assert_eq!(merged["config"]["option1"], "value1"); // From base
        assert_eq!(merged["config"]["option2"], "overridden"); // Overridden
        assert_eq!(merged["config"]["option3"], "value3"); // Added
        assert_eq!(merged["new_field"], "new_value"); // Added
        assert_eq!(merged["category"], "development"); // Added
    }

    #[test]
    fn test_merge_mcp_servers() {
        let base = serde_json::json!({
            "postgres": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-postgres"]
            }
        });

        let overlay = serde_json::json!({
            "postgres": {
                "args": ["--readonly"]  // Override args
            },
            "sqlite": {  // Add new server
                "command": "uvx",
                "args": ["@modelcontextprotocol/server-sqlite"]
            }
        });

        let merged = merge_mcp_servers(&base, &overlay).unwrap();

        // Postgres args should be overridden
        assert_eq!(merged["postgres"]["args"][0], "--readonly");
        // Both servers should exist
        assert!(merged.get("postgres").is_some());
        assert!(merged.get("sqlite").is_some());
    }

    #[test]
    fn test_merge_preserves_mcp_url() {
        let marketplace_entry = serde_json::json!({
            "name": "test-plugin",
            "source": "./test",
            "description": "Test plugin"
        });

        let nested_plugin = serde_json::json!({
            "name": "test-plugin",
            "version": "1.0.0",
            "mcpServers": "https://example.com/mcp"
        });

        let merged = merge_plugin_with_nested(&marketplace_entry, &nested_plugin).unwrap();

        // MCP servers should be a string URL
        assert!(merged.mcp_servers.is_some());
        let mcp_value = merged.mcp_servers.as_ref().unwrap();
        assert_eq!(mcp_value.as_str(), Some("https://example.com/mcp"));
    }
}
