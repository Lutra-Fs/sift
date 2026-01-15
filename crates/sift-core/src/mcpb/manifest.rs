//! MCPB Manifest schema
//!
//! Defines the structure of `manifest.json` files inside `.mcpb` bundles.
//! Based on the MCPB specification v0.3.
//!
//! See: https://github.com/modelcontextprotocol/mcpb/blob/main/MANIFEST.md

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// MCPB Manifest - the root structure of manifest.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbManifest {
    /// Specification version this bundle conforms to (e.g., "0.3")
    pub manifest_version: String,

    /// Machine-readable name (used for CLI, APIs)
    pub name: String,

    /// Semantic version
    pub version: String,

    /// Brief description
    pub description: String,

    /// Author information
    pub author: McpbAuthor,

    /// Server configuration
    pub server: McpbServer,

    /// Human-friendly display name (optional)
    #[serde(default)]
    pub display_name: Option<String>,

    /// Detailed description for extension stores (optional, markdown)
    #[serde(default)]
    pub long_description: Option<String>,

    /// Path to icon file (relative or https URL)
    #[serde(default)]
    pub icon: Option<String>,

    /// Multiple icon variants for different sizes/themes
    #[serde(default)]
    pub icons: Vec<McpbIcon>,

    /// Source code repository
    #[serde(default)]
    pub repository: Option<McpbRepository>,

    /// Homepage URL
    #[serde(default)]
    pub homepage: Option<String>,

    /// Documentation URL
    #[serde(default)]
    pub documentation: Option<String>,

    /// Support/issues URL
    #[serde(default)]
    pub support: Option<String>,

    /// Screenshot paths
    #[serde(default)]
    pub screenshots: Vec<String>,

    /// Tools the extension provides
    #[serde(default)]
    pub tools: Vec<McpbTool>,

    /// Whether the server generates additional tools at runtime
    #[serde(default)]
    pub tools_generated: bool,

    /// Prompts the extension provides
    #[serde(default)]
    pub prompts: Vec<McpbPrompt>,

    /// Whether the server generates additional prompts at runtime
    #[serde(default)]
    pub prompts_generated: bool,

    /// Search keywords
    #[serde(default)]
    pub keywords: Vec<String>,

    /// License identifier (e.g., "MIT")
    #[serde(default)]
    pub license: Option<String>,

    /// Privacy policy URLs for external services
    #[serde(default)]
    pub privacy_policies: Vec<String>,

    /// Compatibility requirements
    #[serde(default)]
    pub compatibility: Option<McpbCompatibility>,

    /// User-configurable options
    #[serde(default)]
    pub user_config: HashMap<String, McpbUserConfig>,

    /// Localization settings
    #[serde(default)]
    pub localization: Option<McpbLocalization>,

    /// Platform-specific client integration metadata
    #[serde(default, rename = "_meta")]
    pub meta: HashMap<String, serde_json::Value>,
}

/// Author information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbAuthor {
    /// Author name (required)
    pub name: String,

    /// Author email (optional)
    #[serde(default)]
    pub email: Option<String>,

    /// Author URL (optional)
    #[serde(default)]
    pub url: Option<String>,
}

/// Icon descriptor for multiple variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbIcon {
    /// Path to icon file
    pub src: String,

    /// Size in WIDTHxHEIGHT format (e.g., "128x128")
    pub size: String,

    /// Theme variant (e.g., "light", "dark")
    #[serde(default)]
    pub theme: Option<String>,
}

/// Repository information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbRepository {
    /// Repository type (e.g., "git")
    #[serde(rename = "type")]
    pub repo_type: String,

    /// Repository URL
    pub url: String,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbServer {
    /// Server type: "node", "python", "binary", or "uv"
    #[serde(rename = "type")]
    pub server_type: McpbServerType,

    /// Entry point file path (relative to bundle root)
    #[serde(default)]
    pub entry_point: Option<String>,

    /// MCP configuration for running the server
    #[serde(default)]
    pub mcp_config: Option<McpbMcpConfig>,
}

/// Server type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpbServerType {
    /// Node.js server with bundled dependencies
    Node,
    /// Python server with bundled dependencies
    Python,
    /// Pre-compiled executable
    Binary,
    /// Python server using UV runtime (experimental)
    Uv,
}

impl std::fmt::Display for McpbServerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpbServerType::Node => write!(f, "node"),
            McpbServerType::Python => write!(f, "python"),
            McpbServerType::Binary => write!(f, "binary"),
            McpbServerType::Uv => write!(f, "uv"),
        }
    }
}

/// MCP configuration for running the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbMcpConfig {
    /// Command to run
    pub command: String,

    /// Command arguments (supports variable substitution)
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Platform-specific overrides
    #[serde(default)]
    pub platforms: HashMap<String, McpbPlatformConfig>,
}

/// Platform-specific MCP configuration override
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbPlatformConfig {
    /// Command override
    #[serde(default)]
    pub command: Option<String>,

    /// Args override
    #[serde(default)]
    pub args: Option<Vec<String>>,

    /// Env override/additions
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbTool {
    /// Tool name/identifier
    pub name: String,

    /// Tool description
    #[serde(default)]
    pub description: Option<String>,
}

/// Prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbPrompt {
    /// Prompt name/identifier
    pub name: String,

    /// Prompt description
    #[serde(default)]
    pub description: Option<String>,

    /// Argument names
    #[serde(default)]
    pub arguments: Vec<String>,

    /// Prompt text with variable placeholders
    #[serde(default)]
    pub text: Option<String>,
}

/// Compatibility requirements
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpbCompatibility {
    /// Minimum Claude Desktop version (semver)
    #[serde(default)]
    pub claude_desktop: Option<String>,

    /// Supported platforms: "darwin", "win32", "linux"
    #[serde(default)]
    pub platforms: Vec<String>,

    /// Runtime version requirements
    #[serde(default)]
    pub runtimes: McpbRuntimes,
}

/// Runtime version requirements
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpbRuntimes {
    /// Node.js version requirement (semver)
    #[serde(default)]
    pub node: Option<String>,

    /// Python version requirement (semver)
    #[serde(default)]
    pub python: Option<String>,
}

/// User configuration option
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbUserConfig {
    /// Data type
    #[serde(rename = "type")]
    pub config_type: McpbUserConfigType,

    /// Display name for UI
    pub title: String,

    /// Help text
    #[serde(default)]
    pub description: Option<String>,

    /// Whether this field is required
    #[serde(default)]
    pub required: bool,

    /// Default value (supports variable substitution)
    #[serde(default)]
    pub default: Option<serde_json::Value>,

    /// For directory/file types, allow multiple selections
    #[serde(default)]
    pub multiple: bool,

    /// For string types, mask input and store securely
    #[serde(default)]
    pub sensitive: bool,

    /// For number types, minimum value
    #[serde(default)]
    pub min: Option<f64>,

    /// For number types, maximum value
    #[serde(default)]
    pub max: Option<f64>,
}

/// User configuration data types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpbUserConfigType {
    /// Text input
    String,
    /// Numeric input
    Number,
    /// Checkbox/toggle
    Boolean,
    /// Directory picker
    Directory,
    /// File picker
    File,
}

/// Localization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbLocalization {
    /// Path to resources with ${locale} placeholder
    #[serde(default = "default_resources_path")]
    pub resources: String,

    /// Default locale (BCP 47 identifier)
    #[serde(default = "default_locale")]
    pub default_locale: String,
}

fn default_resources_path() -> String {
    "mcpb-resources/${locale}.json".to_string()
}

fn default_locale() -> String {
    "en-US".to_string()
}

impl McpbManifest {
    /// Parse a manifest from JSON string
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("Failed to parse manifest: {}", e))
    }

    /// Parse a manifest from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|e| anyhow::anyhow!("Failed to parse manifest: {}", e))
    }

    /// Get the display name, falling back to the machine name
    pub fn display_name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }

    /// Get all required user config keys
    pub fn required_user_config_keys(&self) -> Vec<&str> {
        self.user_config
            .iter()
            .filter(|(_, v)| v.required)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Check if this manifest is compatible with the current platform
    pub fn is_compatible_with_platform(&self, platform: &str) -> bool {
        match &self.compatibility {
            Some(compat) if !compat.platforms.is_empty() => {
                compat.platforms.iter().any(|p| p == platform)
            }
            _ => true, // No platform restrictions
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Manifest Parsing Tests
    // =========================================================================

    #[test]
    fn test_parse_minimal_manifest() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "my-server",
            "version": "1.0.0",
            "description": "A simple MCP server",
            "author": {
                "name": "Test Author"
            },
            "server": {
                "type": "node",
                "entry_point": "server/index.js"
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.manifest_version, "0.3");
        assert_eq!(manifest.name, "my-server");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description, "A simple MCP server");
        assert_eq!(manifest.author.name, "Test Author");
        assert_eq!(manifest.server.server_type, McpbServerType::Node);
        assert_eq!(
            manifest.server.entry_point,
            Some("server/index.js".to_string())
        );
    }

    #[test]
    fn test_parse_full_manifest() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "hello-world-node",
            "display_name": "Hello World MCP Server",
            "version": "0.1.0",
            "description": "A reference MCP extension",
            "long_description": "Detailed description here",
            "author": {
                "name": "Acme Inc",
                "email": "support@acme.void",
                "url": "https://www.acme.void"
            },
            "repository": {
                "type": "git",
                "url": "https://github.com/acme/my-repo"
            },
            "homepage": "https://docs.acme.void",
            "documentation": "https://docs.acme.void/hello-world",
            "support": "https://github.com/acme/my-repo/issues",
            "icon": "icon.png",
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": {
                    "command": "node",
                    "args": ["${__dirname}/server/index.js", "--verbose=${user_config.verbose}"],
                    "env": {
                        "API_KEY": "${user_config.api_key}"
                    }
                }
            },
            "tools": [
                {
                    "name": "get_current_time",
                    "description": "Get the current time"
                }
            ],
            "keywords": ["reference", "example"],
            "license": "MIT",
            "user_config": {
                "api_key": {
                    "type": "string",
                    "title": "API Key",
                    "description": "Your API key",
                    "sensitive": true,
                    "required": true
                },
                "verbose": {
                    "type": "boolean",
                    "title": "Verbose Logging",
                    "default": false,
                    "required": false
                },
                "max_results": {
                    "type": "number",
                    "title": "Max Results",
                    "default": 10,
                    "min": 1,
                    "max": 100,
                    "required": false
                }
            },
            "compatibility": {
                "claude_desktop": ">=0.10.0",
                "platforms": ["darwin", "win32", "linux"],
                "runtimes": {
                    "node": ">=16.0.0"
                }
            },
            "privacy_policies": []
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();

        // Basic fields
        assert_eq!(manifest.name, "hello-world-node");
        assert_eq!(manifest.display_name(), "Hello World MCP Server");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.license, Some("MIT".to_string()));

        // Author
        assert_eq!(manifest.author.name, "Acme Inc");
        assert_eq!(manifest.author.email, Some("support@acme.void".to_string()));
        assert_eq!(
            manifest.author.url,
            Some("https://www.acme.void".to_string())
        );

        // Repository
        let repo = manifest.repository.as_ref().unwrap();
        assert_eq!(repo.repo_type, "git");
        assert_eq!(repo.url, "https://github.com/acme/my-repo");

        // Server
        assert_eq!(manifest.server.server_type, McpbServerType::Node);
        let mcp_config = manifest.server.mcp_config.as_ref().unwrap();
        assert_eq!(mcp_config.command, "node");
        assert_eq!(mcp_config.args.len(), 2);
        assert!(mcp_config.args[0].contains("${__dirname}"));
        assert!(mcp_config.env.contains_key("API_KEY"));

        // Tools
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "get_current_time");

        // User config
        assert_eq!(manifest.user_config.len(), 3);
        let api_key_config = &manifest.user_config["api_key"];
        assert_eq!(api_key_config.config_type, McpbUserConfigType::String);
        assert!(api_key_config.sensitive);
        assert!(api_key_config.required);

        let max_results = &manifest.user_config["max_results"];
        assert_eq!(max_results.config_type, McpbUserConfigType::Number);
        assert_eq!(max_results.min, Some(1.0));
        assert_eq!(max_results.max, Some(100.0));

        // Compatibility
        let compat = manifest.compatibility.as_ref().unwrap();
        assert_eq!(compat.claude_desktop, Some(">=0.10.0".to_string()));
        assert_eq!(compat.platforms, vec!["darwin", "win32", "linux"]);
        assert_eq!(compat.runtimes.node, Some(">=16.0.0".to_string()));
    }

    #[test]
    fn test_parse_python_server() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "python-server",
            "version": "1.0.0",
            "description": "Python MCP server",
            "author": { "name": "Test" },
            "server": {
                "type": "python",
                "entry_point": "server/main.py",
                "mcp_config": {
                    "command": "python3",
                    "args": ["${__dirname}/server/main.py"]
                }
            },
            "compatibility": {
                "runtimes": {
                    "python": ">=3.10"
                }
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.server.server_type, McpbServerType::Python);
        assert_eq!(
            manifest.compatibility.as_ref().unwrap().runtimes.python,
            Some(">=3.10".to_string())
        );
    }

    #[test]
    fn test_parse_uv_server() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "uv-server",
            "version": "1.0.0",
            "description": "UV-managed Python server",
            "author": { "name": "Test" },
            "server": {
                "type": "uv",
                "entry_point": "server/main.py"
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.server.server_type, McpbServerType::Uv);
    }

    #[test]
    fn test_parse_binary_server() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "binary-server",
            "version": "1.0.0",
            "description": "Binary MCP server",
            "author": { "name": "Test" },
            "server": {
                "type": "binary",
                "mcp_config": {
                    "command": "${__dirname}/server/my-tool",
                    "platforms": {
                        "win32": {
                            "command": "${__dirname}/server/my-tool.exe"
                        }
                    }
                }
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.server.server_type, McpbServerType::Binary);
        let mcp_config = manifest.server.mcp_config.as_ref().unwrap();
        assert!(mcp_config.platforms.contains_key("win32"));
    }

    #[test]
    fn test_parse_manifest_with_icons_array() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test-server",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" },
            "icons": [
                { "src": "icons/icon-128.png", "size": "128x128" },
                { "src": "icons/icon-dark.png", "size": "64x64", "theme": "dark" }
            ]
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.icons.len(), 2);
        assert_eq!(manifest.icons[0].size, "128x128");
        assert_eq!(manifest.icons[1].theme, Some("dark".to_string()));
    }

    #[test]
    fn test_parse_manifest_with_prompts() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test-server",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" },
            "prompts": [
                {
                    "name": "explain_topic",
                    "description": "Explain a topic",
                    "arguments": ["topic", "depth"],
                    "text": "Explain ${arguments.topic} at ${arguments.depth} level"
                }
            ],
            "prompts_generated": true
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.prompts.len(), 1);
        assert_eq!(manifest.prompts[0].name, "explain_topic");
        assert_eq!(manifest.prompts[0].arguments, vec!["topic", "depth"]);
        assert!(manifest.prompts_generated);
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "{ invalid json }";
        assert!(McpbManifest::from_json(json).is_err());
    }

    #[test]
    fn test_parse_missing_required_fields() {
        let json = r#"{ "name": "test" }"#;
        assert!(McpbManifest::from_json(json).is_err());
    }

    // =========================================================================
    // Helper Method Tests
    // =========================================================================

    #[test]
    fn test_display_name_fallback() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "machine-name",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert_eq!(manifest.display_name(), "machine-name");
    }

    #[test]
    fn test_required_user_config_keys() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" },
            "user_config": {
                "api_key": { "type": "string", "title": "API Key", "required": true },
                "optional": { "type": "string", "title": "Optional", "required": false },
                "database": { "type": "string", "title": "Database", "required": true }
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        let required = manifest.required_user_config_keys();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&"api_key"));
        assert!(required.contains(&"database"));
        assert!(!required.contains(&"optional"));
    }

    #[test]
    fn test_platform_compatibility() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" },
            "compatibility": {
                "platforms": ["darwin", "linux"]
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        assert!(manifest.is_compatible_with_platform("darwin"));
        assert!(manifest.is_compatible_with_platform("linux"));
        assert!(!manifest.is_compatible_with_platform("win32"));
    }

    #[test]
    fn test_platform_compatibility_no_restrictions() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        // No platform restrictions means compatible with all
        assert!(manifest.is_compatible_with_platform("darwin"));
        assert!(manifest.is_compatible_with_platform("win32"));
        assert!(manifest.is_compatible_with_platform("linux"));
    }

    // =========================================================================
    // User Config Type Tests
    // =========================================================================

    #[test]
    fn test_user_config_directory_type() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" },
            "user_config": {
                "workspace": {
                    "type": "directory",
                    "title": "Workspace",
                    "description": "Select workspace directory",
                    "default": "${HOME}/Documents",
                    "multiple": true
                }
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        let workspace = &manifest.user_config["workspace"];
        assert_eq!(workspace.config_type, McpbUserConfigType::Directory);
        assert!(workspace.multiple);
    }

    #[test]
    fn test_user_config_file_type() {
        let json = r#"{
            "manifest_version": "0.3",
            "name": "test",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": { "type": "node" },
            "user_config": {
                "config_file": {
                    "type": "file",
                    "title": "Config File",
                    "required": false
                }
            }
        }"#;

        let manifest = McpbManifest::from_json(json).unwrap();
        let config_file = &manifest.user_config["config_file"];
        assert_eq!(config_file.config_type, McpbUserConfigType::File);
    }

    // =========================================================================
    // Server Type Display Tests
    // =========================================================================

    #[test]
    fn test_server_type_display() {
        assert_eq!(McpbServerType::Node.to_string(), "node");
        assert_eq!(McpbServerType::Python.to_string(), "python");
        assert_eq!(McpbServerType::Binary.to_string(), "binary");
        assert_eq!(McpbServerType::Uv.to_string(), "uv");
    }
}
