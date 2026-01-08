//! Client adapter layer for cross-client compatibility
//!
//! Provides abstraction for different AI client configurations with support for:
//! - Built-in clients (Claude Code, Claude Desktop, VS Code, etc.)
//! - Dynamic registration through external providers
//! - Extensible capabilities interface

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::fs::LinkMode;

/// Built-in client types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientType {
    /// Claude Code CLI
    ClaudeCode,
    /// Claude Desktop
    ClaudeDesktop,
    /// Visual Studio Code
    VSCode,
    /// Gemini CLI
    GeminiCli,
    /// Codex
    Codex,
}

/// Client configuration from sift.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Whether this client is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Source for external providers: "registry:some-provider"
    #[serde(default)]
    pub source: Option<String>,

    /// How Sift should materialize skills into the client's expected location
    #[serde(default)]
    pub link_mode: LinkMode,

    /// Capabilities (auto-detected for built-in clients)
    #[serde(default)]
    pub capabilities: Option<ClientCapabilities>,
}

fn default_enabled() -> bool {
    true
}

impl ClientConfig {
    /// Merge another ClientConfig into this one
    pub fn merge(&mut self, other: ClientConfig) {
        self.enabled = other.enabled;
        if other.source.is_some() {
            self.source = other.source;
        }
        self.link_mode = other.link_mode;
        if other.capabilities.is_some() {
            self.capabilities = other.capabilities;
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(ref source) = self.source
            && !source.starts_with("registry:") && !source.starts_with("local:")
        {
            anyhow::bail!(
                "Invalid client source format: must be 'registry:name' or 'local:/path'"
            );
        }
        Ok(())
    }

    /// Get the default configuration for a built-in client
    pub fn for_client_type(client_type: ClientType) -> Self {
        match client_type {
            ClientType::ClaudeCode => ClientConfig::claude_code(),
            ClientType::ClaudeDesktop => ClientConfig::claude_desktop(),
            ClientType::VSCode => ClientConfig::vscode(),
            ClientType::GeminiCli => ClientConfig {
                enabled: true,
                source: None,
                link_mode: LinkMode::Auto,
                capabilities: Some(ClientCapabilities {
                    supports_global: true,
                    supports_project: false,
                    supports_symlinked_skills: false,
                    skill_delivery: SkillDeliveryMode::ConfigReference,
                    mcp_config_format: McpConfigFormat::Generic,
                    supported_transports: {
                        let mut set = HashSet::new();
                        set.insert("stdio".to_string());
                        set
                    },
                }),
            },
            ClientType::Codex => ClientConfig {
                enabled: true,
                source: None,
                link_mode: LinkMode::Auto,
                capabilities: Some(ClientCapabilities {
                    supports_global: true,
                    supports_project: false,
                    supports_symlinked_skills: false,
                    skill_delivery: SkillDeliveryMode::ConfigReference,
                    mcp_config_format: McpConfigFormat::Generic,
                    supported_transports: {
                        let mut set = HashSet::new();
                        set.insert("stdio".to_string());
                        set
                    },
                }),
            },
        }
    }

    /// Get the default configuration for Claude Desktop
    pub fn claude_desktop() -> Self {
        ClientConfig {
            enabled: true,
            source: None,
            link_mode: LinkMode::Auto,
            capabilities: Some(ClientCapabilities {
                supports_global: true,
                supports_project: false,
                supports_symlinked_skills: true,
                skill_delivery: SkillDeliveryMode::Filesystem {
                    global_path: "~/.claude/skills".to_string(),
                    project_path: None,
                },
                mcp_config_format: McpConfigFormat::ClaudeDesktop,
                supported_transports: {
                    let mut set = HashSet::new();
                    set.insert("stdio".to_string());
                    set
                },
            }),
        }
    }

    /// Get the default configuration for Claude Code
    pub fn claude_code() -> Self {
        ClientConfig {
            enabled: true,
            source: None,
            link_mode: LinkMode::Auto,
            capabilities: Some(ClientCapabilities {
                supports_global: true,
                supports_project: true,
                supports_symlinked_skills: true,
                skill_delivery: SkillDeliveryMode::Filesystem {
                    global_path: "~/.claude/skills".to_string(),
                    project_path: Some("./.claude/skills".to_string()),
                },
                mcp_config_format: McpConfigFormat::ClaudeCode,
                supported_transports: {
                    let mut set = HashSet::new();
                    set.insert("stdio".to_string());
                    set.insert("sse".to_string());
                    set
                },
            }),
        }
    }

    /// Get the default configuration for VS Code
    pub fn vscode() -> Self {
        ClientConfig {
            enabled: true,
            source: None,
            link_mode: LinkMode::Auto,
            capabilities: Some(ClientCapabilities {
                supports_global: true,
                supports_project: true,
                supports_symlinked_skills: false,
                skill_delivery: SkillDeliveryMode::ConfigReference,
                mcp_config_format: McpConfigFormat::Generic,
                supported_transports: {
                    let mut set = HashSet::new();
                    set.insert("stdio".to_string());
                    set
                },
            }),
        }
    }
}

/// Client capabilities interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Scope support
    #[serde(default)]
    pub supports_global: bool,

    #[serde(default)]
    pub supports_project: bool,

    /// Whether the client will recognize skills delivered as symlinked directories
    #[serde(default)]
    pub supports_symlinked_skills: bool,

    /// Skill delivery mode
    pub skill_delivery: SkillDeliveryMode,

    /// MCP configuration format
    #[serde(default = "default_mcp_config_format")]
    pub mcp_config_format: McpConfigFormat,

    /// Supported transport types
    #[serde(default)]
    pub supported_transports: HashSet<String>,
}

fn default_mcp_config_format() -> McpConfigFormat {
    McpConfigFormat::ClaudeDesktop
}

impl ClientCapabilities {
    /// Validate capabilities for consistency
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.supports_global && !self.supports_project {
            anyhow::bail!("Client must support at least one scope (global or project)");
        }
        Ok(())
    }

    /// Check if the client supports a given scope
    pub fn supports_scope(&self, scope: crate::config::ConfigScope) -> bool {
        match scope {
            crate::config::ConfigScope::Global => self.supports_global,
            crate::config::ConfigScope::PerProjectLocal | crate::config::ConfigScope::PerProjectShared => {
                self.supports_project
            }
        }
    }
}

/// How skills are delivered to the client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SkillDeliveryMode {
    /// Client scans a directory
    Filesystem {
        global_path: String,
        #[serde(default)]
        project_path: Option<String>,
    },
    /// Client reads paths from config
    ConfigReference,
    /// Client doesn't support skills
    None,
}

/// MCP configuration format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpConfigFormat {
    /// Claude Desktop format: { "mcpServers": {...} }
    ClaudeDesktop,
    /// Claude Code format: { "mcp": {...} }
    ClaudeCode,
    /// Generic format
    Generic,
}

/// Trait for client-specific configuration adapters
pub trait ClientAdapter: Send + Sync {
    /// Get the client type identifier
    fn client_type(&self) -> ClientType;

    /// Get the configuration path for this client
    fn config_path(&self) -> anyhow::Result<std::path::PathBuf>;

    /// Read configuration for this client
    fn read_config(&self) -> anyhow::Result<serde_json::Value>;

    /// Write configuration for this client
    fn write_config(&self, config: &serde_json::Value) -> anyhow::Result<()>;
}

/// Claude Code client adapter
#[derive(Debug)]
pub struct ClaudeCodeAdapter;

impl ClientAdapter for ClaudeCodeAdapter {
    fn client_type(&self) -> ClientType {
        ClientType::ClaudeCode
    }

    fn config_path(&self) -> anyhow::Result<std::path::PathBuf> {
        let base = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        Ok(base.join("claude-code"))
    }

    fn read_config(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    fn write_config(&self, _config: &serde_json::Value) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Claude Desktop client adapter
#[derive(Debug)]
pub struct ClaudeDesktopAdapter;

impl ClientAdapter for ClaudeDesktopAdapter {
    fn client_type(&self) -> ClientType {
        ClientType::ClaudeDesktop
    }

    fn config_path(&self) -> anyhow::Result<std::path::PathBuf> {
        let base = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        Ok(base.join("Claude"))
    }

    fn read_config(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    fn write_config(&self, _config: &serde_json::Value) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Get the adapter for a given client type
pub fn get_adapter(client_type: ClientType) -> Box<dyn ClientAdapter> {
    match client_type {
        ClientType::ClaudeCode => Box::new(ClaudeCodeAdapter),
        ClientType::ClaudeDesktop => Box::new(ClaudeDesktopAdapter),
        ClientType::VSCode => unimplemented!("VS Code adapter not yet implemented"),
        ClientType::GeminiCli => unimplemented!("Gemini CLI adapter not yet implemented"),
        ClientType::Codex => unimplemented!("Codex adapter not yet implemented"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_claude_code() {
        let config = ClientConfig::claude_code();
        assert!(config.enabled);
        assert!(config.capabilities.is_some());
    }

    #[test]
    fn test_client_config_claude_desktop() {
        let config = ClientConfig::claude_desktop();
        assert!(config.enabled);
        assert!(config.capabilities.is_some());
    }

    #[test]
    fn test_client_config_vscode() {
        let config = ClientConfig::vscode();
        assert!(config.enabled);
        assert!(config.capabilities.is_some());
    }

    #[test]
    fn test_client_config_merge() {
        let mut base = ClientConfig {
            enabled: true,
            source: Some("registry:base".to_string()),
            link_mode: LinkMode::Auto,
            capabilities: None,
        };

        let overlay = ClientConfig {
            enabled: false,
            source: Some("registry:overlay".to_string()),
            link_mode: LinkMode::Symlink,
            capabilities: None,
        };

        base.merge(overlay);

        assert!(!base.enabled);
        assert_eq!(base.source, Some("registry:overlay".to_string()));
        assert_eq!(base.link_mode, LinkMode::Symlink);
    }

    #[test]
    fn test_client_config_validate_valid() {
        let config = ClientConfig {
            enabled: true,
            source: Some("registry:custom-provider".to_string()),
            link_mode: LinkMode::Auto,
            capabilities: None,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_client_config_validate_invalid() {
        let config = ClientConfig {
            enabled: true,
            source: Some("invalid:source".to_string()),
            link_mode: LinkMode::Auto,
            capabilities: None,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_client_capabilities_validate_no_scope() {
        let caps = ClientCapabilities {
            supports_global: false,
            supports_project: false,
            supports_symlinked_skills: false,
            skill_delivery: SkillDeliveryMode::None,
            mcp_config_format: McpConfigFormat::Generic,
            supported_transports: HashSet::new(),
        };
        assert!(caps.validate().is_err());
    }

    #[test]
    fn test_client_capabilities_validate_with_scope() {
        let caps = ClientCapabilities {
            supports_global: true,
            supports_project: false,
            supports_symlinked_skills: false,
            skill_delivery: SkillDeliveryMode::None,
            mcp_config_format: McpConfigFormat::Generic,
            supported_transports: HashSet::new(),
        };
        assert!(caps.validate().is_ok());
    }

    #[test]
    fn test_client_type_serialization() {
        // Test that ClientType serializes as kebab-case
        let code = ClientType::ClaudeCode;
        let json = serde_json::to_string(&code).expect("ClientType serialization should succeed");
        assert_eq!(json, "\"claude-code\"");

        let desktop = ClientType::ClaudeDesktop;
        let json = serde_json::to_string(&desktop).expect("ClientType serialization should succeed");
        assert_eq!(json, "\"claude-desktop\"");
    }
}
