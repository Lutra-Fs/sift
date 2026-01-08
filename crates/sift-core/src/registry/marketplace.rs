//! Claude Marketplace compatibility layer
//!
//! Adapts Claude marketplace.json to Sift skills

use serde::{Deserialize, Serialize};
use url::Url;

/// Claude marketplace.json structure (based on the documentation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceManifest {
    /// Marketplace metadata
    #[serde(default)]
    pub marketplace: MarketplaceInfo,

    /// List of plugins
    pub plugins: Vec<MarketplacePlugin>,
}

/// Marketplace metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketplaceInfo {
    /// Marketplace name (identifier)
    pub name: Option<String>,

    /// Marketplace owner/maintainer
    pub owner: Option<MarketplaceOwner>,

    /// Marketplace description
    pub description: Option<String>,

    /// Marketplace version
    pub version: Option<String>,

    /// Base directory for plugin sources
    #[serde(default)]
    pub plugin_root: Option<String>,
}

/// Marketplace owner information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceOwner {
    /// Owner name
    pub name: String,

    /// Owner email
    pub email: Option<String>,
}

/// A plugin from the marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePlugin {
    /// Plugin name (identifier)
    pub name: String,

    /// Plugin description
    pub description: String,

    /// Plugin version
    #[serde(default = "default_plugin_version")]
    pub version: String,

    /// Source information
    pub source: MarketplaceSource,

    /// Skills provided by this plugin
    #[serde(default)]
    pub commands: Option<CommandsOrPaths>,

    /// Hooks configuration
    #[serde(default)]
    pub hooks: Option<serde_json::Value>,

    /// MCP servers configuration
    #[serde(default)]
    pub mcp_servers: Option<serde_json::Value>,

    /// Author information
    pub author: Option<MarketplaceOwner>,

    /// Homepage URL
    pub homepage: Option<String>,

    /// Repository URL
    pub repository: Option<String>,

    /// License
    pub license: Option<String>,

    /// Keywords for discovery
    #[serde(default)]
    pub keywords: Vec<String>,

    /// Category
    pub category: Option<String>,

    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_plugin_version() -> String {
    "0.1.0".to_string()
}

/// Source specification for a plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MarketplaceSource {
    /// String path (relative or local)
    String(String),
    /// Object with type and repo/url
    Object(MarketplaceSourceObject),
}

/// Detailed source object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSourceObject {
    /// Source type
    pub source: SourceType,

    /// Repository (for github type)
    pub repo: Option<String>,

    /// URL (for url type)
    pub url: Option<Url>,

    /// Git reference (branch, tag, or commit)
    #[serde(rename = "ref")]
    pub ref_: Option<String>,

    /// Subdirectory within the repository
    pub path: Option<String>,
}

/// Source type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Github,
    Url,
    Local,
}

/// Commands can be a string, array, or object
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CommandsOrPaths {
    Single(String),
    Multiple(Vec<String>),
}

/// Adapter to convert marketplace plugins to Sift configurations
pub struct MarketplaceAdapter;

impl MarketplaceAdapter {
    /// Parse marketplace.json from a string
    pub fn parse(content: &str) -> anyhow::Result<MarketplaceManifest> {
        let manifest: MarketplaceManifest = serde_json::from_str(content)
            .map_err(|e| anyhow::anyhow!("Failed to parse marketplace.json: {}", e))?;
        Ok(manifest)
    }

    /// Get the source string from a marketplace plugin
    pub fn get_source_string(plugin: &MarketplacePlugin) -> String {
        match &plugin.source {
            MarketplaceSource::String(s) => {
                if s.starts_with("./") || s.starts_with("../") {
                    format!("local:{}", s)
                } else {
                    s.clone()
                }
            }
            MarketplaceSource::Object(obj) => match obj.source {
                SourceType::Github => {
                    let repo = obj.repo.as_ref().expect("GitHub source requires 'repo'");
                    let ref_part = obj
                        .ref_
                        .as_ref()
                        .map(|r| format!("@{}", r))
                        .unwrap_or_default();
                    let path_part = obj
                        .path
                        .as_ref()
                        .map(|p| format!("/{}", p))
                        .unwrap_or_default();
                    format!("github:{}{}{}", repo, ref_part, path_part)
                }
                SourceType::Url => {
                    let url = obj.url.as_ref().expect("URL source requires 'url'");
                    url.to_string()
                }
                SourceType::Local => {
                    let path = obj.path.as_deref().unwrap_or(".");
                    format!("local:{}", path)
                }
            },
        }
    }

    /// List all available plugins from a marketplace
    pub fn list_plugins(manifest: &MarketplaceManifest) -> Vec<PluginSummary> {
        manifest
            .plugins
            .iter()
            .map(|plugin| PluginSummary {
                name: plugin.name.clone(),
                description: plugin.description.clone(),
                version: plugin.version.clone(),
                source: Self::get_source_string(plugin),
            })
            .collect()
    }

    /// Find a plugin by name in the marketplace
    pub fn find_plugin<'a>(
        manifest: &'a MarketplaceManifest,
        name: &str,
    ) -> Option<&'a MarketplacePlugin> {
        manifest.plugins.iter().find(|p| p.name == name)
    }

    /// Convert a marketplace plugin to a Sift skill config
    pub fn plugin_to_skill_config(
        plugin: &MarketplacePlugin,
    ) -> anyhow::Result<crate::skills::SkillConfig> {
        let source = Self::get_source_string(plugin);

        Ok(crate::skills::SkillConfig {
            source,
            version: plugin.version.clone(),
            targets: None,
            ignore_targets: None,
        })
    }

    /// Infer runtime type from a command string
    fn infer_runtime_from_command(command: &str) -> crate::mcp::RuntimeType {
        // Normalize command - detect package managers
        let first_word = command.split_whitespace().next().unwrap_or("");

        match first_word {
            // Python package managers
            "uvx" | "uv" | "python" | "python3" => crate::mcp::RuntimeType::Python,

            // Node.js package managers
            "npx" | "npm" | "node" => crate::mcp::RuntimeType::Node,

            // Bun package managers
            "bunx" | "bun" => crate::mcp::RuntimeType::Bun,

            // Docker
            cmd if cmd.starts_with("docker") => crate::mcp::RuntimeType::Docker,

            // Fallback: check file extensions in full command
            _ => {
                if command.contains(".py") {
                    crate::mcp::RuntimeType::Python
                } else if command.contains(".ts") {
                    crate::mcp::RuntimeType::Bun
                } else if command.contains(".js") || command.contains(".mjs") {
                    crate::mcp::RuntimeType::Node
                } else {
                    // Unrecognized command - use Shell runtime (direct execution)
                    crate::mcp::RuntimeType::Shell
                }
            }
        }
    }

    /// Convert a marketplace plugin with MCP servers to Sift MCP configs
    pub fn plugin_to_mcp_configs(
        plugin: &MarketplacePlugin,
    ) -> anyhow::Result<Vec<(String, crate::mcp::McpConfig)>> {
        let mut configs = Vec::new();

        if let Some(mcp_servers) = &plugin.mcp_servers
            && let Some(obj) = mcp_servers.as_object()
        {
            for (name, server_config) in obj {
                // Extract command and args from the server config
                let command = server_config
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        anyhow::anyhow!("MCP server '{}' missing 'command' field", name)
                    })?;

                let args = server_config
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let source = Self::get_source_string(plugin);

                // Determine runtime from command
                let runtime = Self::infer_runtime_from_command(command);

                configs.push((
                    name.clone(),
                    crate::mcp::McpConfig {
                        transport: crate::mcp::TransportType::Stdio,
                        source: format!("{}:{}", source, command),
                        runtime,
                        args,
                        url: None,
                        headers: std::collections::HashMap::new(),
                        targets: None,
                        ignore_targets: None,
                        env: std::collections::HashMap::new(),
                    },
                ));
            }
        }

        Ok(configs)
    }
}

/// Summary of a plugin in the marketplace
#[derive(Debug, Clone)]
pub struct PluginSummary {
    pub name: String,
    pub description: String,
    pub version: String,
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_marketplace() {
        let json = r#"{
            "marketplace": {
                "name": "test-marketplace",
                "owner": {
                    "name": "Test Owner"
                }
            },
            "plugins": [
                {
                    "name": "test-plugin",
                    "description": "A test plugin",
                    "version": "1.0.0",
                    "source": "./plugins/test-plugin"
                }
            ]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        assert_eq!(manifest.marketplace.name, Some("test-marketplace".to_string()));
        assert_eq!(manifest.plugins.len(), 1);
        assert_eq!(manifest.plugins[0].name, "test-plugin");
    }

    #[test]
    fn test_parse_invalid_marketplace() {
        let json = r#"{"invalid": "json"}"#;
        let result = MarketplaceAdapter::parse(json);
        // Should fail because required fields are missing
        assert!(result.is_err());
    }

    #[test]
    fn test_get_source_string_local() {
        let plugin = MarketplacePlugin {
            name: "test".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            source: MarketplaceSource::String("./plugins/test".to_string()),
            commands: None,
            hooks: None,
            mcp_servers: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
        };

        let source = MarketplaceAdapter::get_source_string(&plugin);
        assert_eq!(source, "local:./plugins/test");
    }

    #[test]
    fn test_get_source_string_github() {
        let plugin = MarketplacePlugin {
            name: "test".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            source: MarketplaceSource::Object(MarketplaceSourceObject {
                source: SourceType::Github,
                repo: Some("owner/repo".to_string()),
                url: None,
                ref_: Some("v1.0.0".to_string()),
                path: None,
            }),
            commands: None,
            hooks: None,
            mcp_servers: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
        };

        let source = MarketplaceAdapter::get_source_string(&plugin);
        assert_eq!(source, "github:owner/repo@v1.0.0");
    }

    #[test]
    fn test_list_plugins() {
        let json = r#"{
            "marketplace": {
                "name": "test-marketplace",
                "owner": {"name": "Test"}
            },
            "plugins": [
                {
                    "name": "plugin1",
                    "description": "First plugin",
                    "version": "1.0.0",
                    "source": "./plugins/plugin1"
                },
                {
                    "name": "plugin2",
                    "description": "Second plugin",
                    "version": "2.0.0",
                    "source": "./plugins/plugin2"
                }
            ]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        let plugins = MarketplaceAdapter::list_plugins(&manifest);

        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0].name, "plugin1");
        assert_eq!(plugins[1].name, "plugin2");
    }

    #[test]
    fn test_find_plugin() {
        let json = r#"{
            "marketplace": {
                "name": "test-marketplace",
                "owner": {"name": "Test"}
            },
            "plugins": [
                {
                    "name": "target-plugin",
                    "description": "Target plugin",
                    "version": "1.0.0",
                    "source": "./plugins/target"
                }
            ]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();

        let found = MarketplaceAdapter::find_plugin(&manifest, "target-plugin");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "target-plugin");

        let not_found = MarketplaceAdapter::find_plugin(&manifest, "nonexistent");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_plugin_to_skill_config() {
        let plugin = MarketplacePlugin {
            name: "test-plugin".to_string(),
            description: "Test plugin".to_string(),
            version: "2.1.0".to_string(),
            source: MarketplaceSource::String("registry:anthropic/pdf".to_string()),
            commands: None,
            hooks: None,
            mcp_servers: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
        };

        let config = MarketplaceAdapter::plugin_to_skill_config(&plugin).unwrap();
        assert_eq!(config.source, "registry:anthropic/pdf");
        assert_eq!(config.version, "2.1.0");
    }

    /// Test with actual claude-code marketplace.json
    /// This validates that the parser can handle the real marketplace format
    #[test]
    fn test_parse_claude_code_marketplace() {
        let json = r#"{
  "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
  "name": "claude-code-plugins",
  "version": "1.0.0",
  "description": "Bundled plugins for Claude Code including Agent SDK development tools, PR review toolkit, and commit workflows",
  "owner": {
    "name": "Anthropic",
    "email": "support@anthropic.com"
  },
  "plugins": [
    {
      "name": "agent-sdk-dev",
      "description": "Development kit for working with the Claude Agent SDK",
      "source": "./plugins/agent-sdk-dev",
      "category": "development"
    },
    {
      "name": "commit-commands",
      "description": "Commands for git commit workflows including commit, push, and PR creation",
      "version": "1.0.0",
      "author": {
        "name": "Anthropic",
        "email": "support@anthropic.com"
      },
      "source": "./plugins/commit-commands",
      "category": "productivity"
    }
  ]
}"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        assert_eq!(manifest.plugins.len(), 2);

        // First plugin without version (should default to 0.1.0)
        let plugin1 = &manifest.plugins[0];
        assert_eq!(plugin1.name, "agent-sdk-dev");
        assert_eq!(plugin1.version, "0.1.0"); // default version
        assert_eq!(plugin1.category.as_ref().unwrap(), "development");

        // Second plugin with explicit version
        let plugin2 = &manifest.plugins[1];
        assert_eq!(plugin2.name, "commit-commands");
        assert_eq!(plugin2.version, "1.0.0");
        assert_eq!(plugin2.category.as_ref().unwrap(), "productivity");

        // Test source string conversion
        let source1 = MarketplaceAdapter::get_source_string(plugin1);
        assert_eq!(source1, "local:./plugins/agent-sdk-dev");

        // Test plugin listing
        let plugins = MarketplaceAdapter::list_plugins(&manifest);
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0].name, "agent-sdk-dev");
        assert_eq!(plugins[1].name, "commit-commands");
    }

    /// Test with full claude-code marketplace.json
    /// This is a comprehensive test with all plugins from the actual marketplace
    #[test]
    fn test_parse_full_claude_code_marketplace() {
        let json = r#"{
  "$schema": "https://anthropic.com/claude-code/marketplace.schema.json",
  "name": "claude-code-plugins",
  "version": "1.0.0",
  "description": "Bundled plugins for Claude Code including Agent SDK development tools, PR review toolkit, and commit workflows",
  "owner": {
    "name": "Anthropic",
    "email": "support@anthropic.com"
  },
  "plugins": [
    {
      "name": "agent-sdk-dev",
      "description": "Development kit for working with the Claude Agent SDK",
      "source": "./plugins/agent-sdk-dev",
      "category": "development"
    },
    {
      "name": "claude-opus-4-5-migration",
      "description": "Migrate your code and prompts from Sonnet 4.x and Opus 4.1 to Opus 4.5.",
      "version": "1.0.0",
      "author": {
        "name": "William Hu",
        "email": "whu@anthropic.com"
      },
      "source": "./plugins/claude-opus-4-5-migration",
      "category": "development"
    },
    {
      "name": "code-review",
      "description": "Automated code review for pull requests using multiple specialized agents with confidence-based scoring to filter false positives",
      "version": "1.0.0",
      "author": {
        "name": "Boris Cherny",
        "email": "boris@anthropic.com"
      },
      "source": "./plugins/code-review",
      "category": "productivity"
    },
    {
      "name": "commit-commands",
      "description": "Commands for git commit workflows including commit, push, and PR creation",
      "version": "1.0.0",
      "author": {
        "name": "Anthropic",
        "email": "support@anthropic.com"
      },
      "source": "./plugins/commit-commands",
      "category": "productivity"
    }
  ]
}"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        assert_eq!(manifest.plugins.len(), 4);

        // Find a specific plugin
        let plugin = MarketplaceAdapter::find_plugin(&manifest, "code-review");
        assert!(plugin.is_some());
        assert_eq!(plugin.unwrap().author.as_ref().unwrap().name, "Boris Cherny");

        // Test that non-existent plugin returns None
        let not_found = MarketplaceAdapter::find_plugin(&manifest, "nonexistent");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_marketplace_npx_runtime_detection() {
        let json = r#"{
            "name": "test-marketplace",
            "plugins": [{
                "name": "test",
                "description": "Test",
                "version": "1.0.0",
                "source": "./test",
                "mcp_servers": {
                    "postgres": {
                        "command": "npx",
                        "args": ["-y", "@modelcontextprotocol/server-postgres"]
                    }
                }
            }]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        let plugin = &manifest.plugins[0];
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin).unwrap();
        assert_eq!(configs[0].1.runtime, crate::mcp::RuntimeType::Node);
    }

    #[test]
    fn test_marketplace_uvx_runtime_detection() {
        let json = r#"{
            "name": "test-marketplace",
            "plugins": [{
                "name": "test",
                "description": "Test",
                "version": "1.0.0",
                "source": "./test",
                "mcp_servers": {
                    "everything": {
                        "command": "uvx",
                        "args": ["@modelcontextprotocol/server-everything"]
                    }
                }
            }]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        let plugin = &manifest.plugins[0];
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin).unwrap();
        assert_eq!(configs[0].1.runtime, crate::mcp::RuntimeType::Python);
    }

    #[test]
    fn test_marketplace_shell_runtime_fallback() {
        let json = r#"{
            "name": "test-marketplace",
            "plugins": [{
                "name": "test",
                "description": "Test",
                "version": "1.0.0",
                "source": "./test",
                "mcp_servers": {
                    "custom-binary": {
                        "command": "/usr/local/bin/my-mcp-server",
                        "args": ["--port", "8080"]
                    }
                }
            }]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        let plugin = &manifest.plugins[0];
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin).unwrap();
        assert_eq!(configs[0].1.runtime, crate::mcp::RuntimeType::Shell);
    }
}
