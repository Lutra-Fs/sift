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

    /// Skills array
    #[serde(default)]
    pub skills: Option<SkillsOrPaths>,

    /// Strict mode flag (from anthropics/skills format)
    #[serde(default)]
    pub strict: Option<bool>,

    /// Hooks configuration
    #[serde(default)]
    pub hooks: Option<serde_json::Value>,

    /// MCP servers configuration
    /// Supports both snake_case (mcp_servers) and camelCase (mcpServers)
    #[serde(default, alias = "mcpServers")]
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

/// Skills can be a string, array, or object
/// This is the new standard for defining agent skills in marketplace plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SkillsOrPaths {
    Single(String),
    Multiple(Vec<String>),
}

/// Raw marketplace manifest for format detection
/// Handles both flat (claude-code) and metadata wrapper (skills/life-sciences) formats
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RawMarketplaceManifest {
    /// Format with metadata wrapper (anthropics/skills, anthropics/life-sciences)
    WithMetadata {
        name: String,
        owner: MarketplaceOwner,
        metadata: Option<Metadata>,
        plugins: Vec<serde_json::Value>,
    },
    /// Flat format (claude-code)
    Flat {
        marketplace: MarketplaceInfo,
        plugins: Vec<serde_json::Value>,
    },
}

/// Metadata wrapper for marketplace-level metadata
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub plugin_root: Option<String>,
}

/// Adapter to convert marketplace plugins to Sift configurations
pub struct MarketplaceAdapter;

impl MarketplaceAdapter {
    /// Parse marketplace.json from a string (auto-detects format)
    /// Supports both claude-code (flat) and anthropics/skills/life-sciences (metadata wrapper) formats
    pub fn parse(content: &str) -> anyhow::Result<MarketplaceManifest> {
        // Try to parse as raw manifest to detect format
        let raw: RawMarketplaceManifest = serde_json::from_str(content)
            .map_err(|e| anyhow::anyhow!("Failed to parse marketplace.json: {}", e))?;

        // Normalize to unified MarketplaceManifest
        match raw {
            RawMarketplaceManifest::WithMetadata {
                name,
                owner,
                metadata,
                plugins,
            } => {
                // Parse plugins from JSON values
                let parsed_plugins = plugins
                    .into_iter()
                    .map(|v| {
                        serde_json::from_value(v)
                            .map_err(|e| anyhow::anyhow!("Failed to parse plugin: {}", e))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;

                Ok(MarketplaceManifest {
                    marketplace: MarketplaceInfo {
                        name: Some(name),
                        owner: Some(owner),
                        description: metadata.as_ref().and_then(|m| m.description.clone()),
                        version: metadata.as_ref().and_then(|m| m.version.clone()),
                        plugin_root: metadata.as_ref().and_then(|m| m.plugin_root.clone()),
                    },
                    plugins: parsed_plugins,
                })
            }
            RawMarketplaceManifest::Flat {
                marketplace,
                plugins,
            } => {
                // Parse plugins from JSON values
                let parsed_plugins = plugins
                    .into_iter()
                    .map(|v| {
                        serde_json::from_value(v)
                            .map_err(|e| anyhow::anyhow!("Failed to parse plugin: {}", e))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;

                Ok(MarketplaceManifest {
                    marketplace,
                    plugins: parsed_plugins,
                })
            }
        }
    }

    /// Get the source string from a marketplace plugin
    pub fn get_source_string(plugin: &MarketplacePlugin) -> anyhow::Result<String> {
        match &plugin.source {
            MarketplaceSource::String(s) => {
                if s.starts_with("./") || s.starts_with("../") {
                    Ok(format!("local:{}", s))
                } else {
                    Ok(s.clone())
                }
            }
            MarketplaceSource::Object(obj) => match obj.source {
                SourceType::Github => {
                    let repo = obj
                        .repo
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GitHub source requires 'repo' field"))?;
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
                    Ok(format!("github:{}{}{}", repo, ref_part, path_part))
                }
                SourceType::Url => {
                    let url = obj
                        .url
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("URL source requires 'url' field"))?;
                    Ok(url.to_string())
                }
                SourceType::Local => {
                    let path = obj.path.as_deref().unwrap_or(".");
                    Ok(format!("local:{}", path))
                }
            },
        }
    }

    /// List all available plugins from a marketplace
    pub fn list_plugins(manifest: &MarketplaceManifest) -> anyhow::Result<Vec<PluginSummary>> {
        manifest
            .plugins
            .iter()
            .map(|plugin| {
                Ok(PluginSummary {
                    name: plugin.name.clone(),
                    description: plugin.description.clone(),
                    version: plugin.version.clone(),
                    source: Self::get_source_string(plugin)?,
                })
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
        let source = Self::get_source_string(plugin)?;

        Ok(crate::skills::SkillConfig {
            source,
            version: plugin.version.clone(),
            targets: None,
            ignore_targets: None,
        })
    }

    /// Convert a marketplace plugin to multiple Sift skill configs
    /// Returns one SkillConfig per path in the skills array
    pub fn plugin_to_skill_configs(
        plugin: &MarketplacePlugin,
    ) -> anyhow::Result<Vec<crate::skills::SkillConfig>> {
        let mut configs = Vec::new();
        let base_source = Self::get_source_string(plugin)?;

        // Try skills field first (new format)
        if let Some(skills) = &plugin.skills {
            match skills {
                SkillsOrPaths::Single(path) => {
                    configs.push(Self::create_skill_config(&base_source, plugin, path)?);
                }
                SkillsOrPaths::Multiple(paths) => {
                    for path in paths {
                        configs.push(Self::create_skill_config(&base_source, plugin, path)?);
                    }
                }
            }
            return Ok(configs);
        }

        // No skills - create single config from plugin source
        configs.push(crate::skills::SkillConfig {
            source: base_source,
            version: plugin.version.clone(),
            targets: None,
            ignore_targets: None,
        });
        Ok(configs)
    }

    /// Create a single SkillConfig from a plugin and skill path
    fn create_skill_config(
        base_source: &str,
        plugin: &MarketplacePlugin,
        skill_path: &str,
    ) -> anyhow::Result<crate::skills::SkillConfig> {
        // Append skill path to base source
        // "github:anthropics/skills" + "./skills/xlsx" -> "github:anthropics/skills/skills/xlsx"
        let full_source = if skill_path.starts_with("./") {
            format!(
                "{}/{}",
                base_source.trim_end_matches('/'),
                skill_path.trim_start_matches("./")
            )
        } else if skill_path.starts_with("/") {
            // Absolute path - use as-is
            skill_path.to_string()
        } else {
            format!("{}/{}", base_source, skill_path)
        };

        Ok(crate::skills::SkillConfig {
            source: full_source,
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
    /// Supports both STDIO (command-based) and HTTP (URL-based) transports
    pub fn plugin_to_mcp_configs(
        plugin: &MarketplacePlugin,
        _marketplace_source: Option<&str>,
    ) -> anyhow::Result<Vec<(String, crate::mcp::McpConfig)>> {
        let mut configs = Vec::new();

        if let Some(mcp_servers) = &plugin.mcp_servers
            && let Some(obj) = mcp_servers.as_object()
        {
            for (name, server_config) in obj {
                configs.push((
                    name.clone(),
                    Self::parse_mcp_server_config(name, server_config, plugin)?,
                ));
            }
        }

        Ok(configs)
    }

    /// Parse a single MCP server configuration
    fn parse_mcp_server_config(
        name: &str,
        server_config: &serde_json::Value,
        plugin: &MarketplacePlugin,
    ) -> anyhow::Result<crate::mcp::McpConfig> {
        // Case 1: URL string → HTTP transport
        if let Some(url_str) = server_config.as_str() {
            return Ok(crate::mcp::McpConfig {
                transport: crate::mcp::TransportType::Http,
                source: plugin.name.clone(),
                runtime: crate::mcp::RuntimeType::Node, // Not used for HTTP
                args: vec![],
                url: Some(url_str.to_string()),
                headers: std::collections::HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: std::collections::HashMap::new(),
            });
        }

        // Case 2: Object with url field → HTTP transport
        if let Some(obj) = server_config.as_object() {
            if let Some(url_str) = obj.get("url").and_then(|v| v.as_str()) {
                return Ok(crate::mcp::McpConfig {
                    transport: crate::mcp::TransportType::Http,
                    source: plugin.name.clone(),
                    runtime: crate::mcp::RuntimeType::Node,
                    args: vec![],
                    url: Some(url_str.to_string()),
                    headers: std::collections::HashMap::new(),
                    targets: None,
                    ignore_targets: None,
                    env: std::collections::HashMap::new(),
                });
            }

            // Case 3: Object with command field → STDIO transport
            if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
                let args = obj
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let source = Self::get_source_string(plugin)?;
                let runtime = Self::infer_runtime_from_command(command);

                return Ok(crate::mcp::McpConfig {
                    transport: crate::mcp::TransportType::Stdio,
                    source: format!("{}:{}", source, command),
                    runtime,
                    args,
                    url: None,
                    headers: std::collections::HashMap::new(),
                    targets: None,
                    ignore_targets: None,
                    env: std::collections::HashMap::new(),
                });
            }
        }

        anyhow::bail!(
            "Invalid MCP server config for '{}': must be URL string or object with command/url field",
            name
        )
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
        assert_eq!(
            manifest.marketplace.name,
            Some("test-marketplace".to_string())
        );
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
            hooks: None,
            mcp_servers: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
            skills: None,
            strict: None,
        };

        let source = MarketplaceAdapter::get_source_string(&plugin).unwrap();
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
            hooks: None,
            mcp_servers: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
            skills: None,
            strict: None,
        };

        let source = MarketplaceAdapter::get_source_string(&plugin).unwrap();
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
        let plugins = MarketplaceAdapter::list_plugins(&manifest).unwrap();

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
            hooks: None,
            mcp_servers: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
            skills: None,
            strict: None,
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
        let source1 = MarketplaceAdapter::get_source_string(plugin1).unwrap();
        assert_eq!(source1, "local:./plugins/agent-sdk-dev");

        // Test plugin listing
        let plugins = MarketplaceAdapter::list_plugins(&manifest).unwrap();
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
        assert_eq!(
            plugin.unwrap().author.as_ref().unwrap().name,
            "Boris Cherny"
        );

        // Test that non-existent plugin returns None
        let not_found = MarketplaceAdapter::find_plugin(&manifest, "nonexistent");
        assert!(not_found.is_none());
    }

    #[test]
    fn test_marketplace_npx_runtime_detection() {
        let json = r#"{
            "name": "test-marketplace",
            "owner": {"name": "Test Owner"},
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
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None).unwrap();
        assert_eq!(configs[0].1.runtime, crate::mcp::RuntimeType::Node);
    }

    #[test]
    fn test_marketplace_uvx_runtime_detection() {
        let json = r#"{
            "name": "test-marketplace",
            "owner": {"name": "Test Owner"},
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
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None).unwrap();
        assert_eq!(configs[0].1.runtime, crate::mcp::RuntimeType::Python);
    }

    #[test]
    fn test_marketplace_shell_runtime_fallback() {
        let json = r#"{
            "name": "test-marketplace",
            "owner": {"name": "Test Owner"},
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
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None).unwrap();
        assert_eq!(configs[0].1.runtime, crate::mcp::RuntimeType::Shell);
    }

    // === NEW TESTS FOR MULTI-FORMAT SUPPORT (RED PHASE) ===
    // These tests are expected to FAIL until implementation is complete

    #[test]
    fn test_parse_anthropic_skills_format() {
        // Test metadata wrapper + skills array format
        let json = r#"{
            "name": "anthropic-agent-skills",
            "owner": {"name": "Keith Lazuka"},
            "metadata": {
                "description": "Agent skills",
                "version": "1.0.0"
            },
            "plugins": [{
                "name": "document-skills",
                "description": "Document processing skills",
                "source": "./",
                "strict": false,
                "skills": ["./skills/xlsx", "./skills/docx"]
            }]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        assert_eq!(manifest.plugins.len(), 1);

        let plugin = &manifest.plugins[0];
        assert_eq!(plugin.name, "document-skills");
        assert_eq!(plugin.strict, Some(false));
        assert!(plugin.skills.is_some());
    }

    #[test]
    fn test_skills_to_multiple_skill_configs() {
        // Test skills array expansion to multiple SkillConfig
        let plugin = MarketplacePlugin {
            name: "test".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            source: MarketplaceSource::String("github:anthropics/skills".to_string()),
            skills: Some(SkillsOrPaths::Multiple(vec![
                "./skills/xlsx".to_string(),
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

        let configs = MarketplaceAdapter::plugin_to_skill_configs(&plugin).unwrap();
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].source, "github:anthropics/skills/skills/xlsx");
        assert_eq!(configs[1].source, "github:anthropics/skills/skills/docx");
    }

    #[test]
    fn test_mcp_url_to_http_transport() {
        // Test MCP server URL string → HTTP transport
        let plugin = MarketplacePlugin {
            name: "test".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            source: MarketplaceSource::String("local:./test".to_string()),
            mcp_servers: Some(serde_json::json!({
                "http-server": "https://example.com/mcp"
            })),
            skills: None,
            hooks: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            keywords: vec![],
            category: None,
            tags: vec![],
            strict: None,
        };

        let configs = MarketplaceAdapter::plugin_to_mcp_configs(&plugin, None).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].1.transport, crate::mcp::TransportType::Http);
        assert_eq!(
            configs[0].1.url,
            Some("https://example.com/mcp".to_string())
        );
    }

    #[test]
    fn test_github_url_construction() {
        // Test GitHubFetcher URL construction for nested plugin.json
        let url =
            crate::registry::marketplace::github_fetcher::GitHubFetcher::construct_plugin_json_url(
                "github:anthropics/life-sciences",
                "./10x-genomics",
                None,
            )
            .unwrap();

        assert_eq!(
            url,
            "https://raw.githubusercontent.com/anthropics/life-sciences/main/10x-genomics/.claude-plugin/plugin.json"
        );
    }

    // === PHASE 3: NESTED PLUGIN MERGING TESTS (RED) ===

    #[test]
    fn test_merge_plugin_with_nested_mcp_url() {
        // Test merging marketplace entry with nested plugin.json
        // Marketplace entry has basic info, nested plugin has mcpServers URL
        let marketplace_entry = serde_json::json!({
            "name": "10x-genomics",
            "source": "./10x-genomics",
            "description": "Marketplace description",
            "category": "life-sciences"
        });

        let nested_plugin = serde_json::json!({
            "name": "10x-genomics",
            "version": "1.0.0",
            "mcpServers": "https://github.com/10XGenomics/txg-mcp/releases/latest/download/txg-node.mcpb"
        });

        // This will fail until we implement merge_plugin_with_nested
        let merged = crate::registry::marketplace::merge_nested::merge_plugin_with_nested(
            &marketplace_entry,
            &nested_plugin,
        )
        .unwrap();

        assert_eq!(merged.name, "10x-genomics");
        assert_eq!(merged.version, "1.0.0"); // From nested
        assert_eq!(merged.description, "Marketplace description"); // From marketplace
        assert_eq!(merged.category, Some("life-sciences".to_string())); // From marketplace
        assert!(merged.mcp_servers.is_some());
    }

    // === REAL MARKETPLACE VALIDATION ===

    #[test]
    fn test_parse_real_anthropic_skills_marketplace() {
        // Real marketplace.json from anthropics/skills
        let json = r#"{
            "name": "anthropic-agent-skills",
            "owner": {"name": "Keith Lazuka"},
            "metadata": {
                "description": "Anthropic example skills",
                "version": "1.0.0"
            },
            "plugins": [{
                "name": "document-skills",
                "description": "Document processing skills",
                "source": "./",
                "strict": false,
                "skills": ["./skills/xlsx", "./skills/docx", "./skills/pptx", "./skills/pdf"]
            }]
        }"#;

        let manifest = MarketplaceAdapter::parse(json).unwrap();
        assert_eq!(
            manifest.marketplace.name,
            Some("anthropic-agent-skills".to_string())
        );
        assert_eq!(
            manifest.marketplace.description,
            Some("Anthropic example skills".to_string())
        );
        assert_eq!(manifest.plugins.len(), 1);

        let plugin = &manifest.plugins[0];
        assert_eq!(plugin.name, "document-skills");
        assert_eq!(plugin.strict, Some(false));

        // Test skills expansion
        let configs = MarketplaceAdapter::plugin_to_skill_configs(plugin).unwrap();
        assert_eq!(configs.len(), 4);
        assert_eq!(configs[0].source, "local:./skills/xlsx");
        assert_eq!(configs[1].source, "local:./skills/docx");
    }
}
