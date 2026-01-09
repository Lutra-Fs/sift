//! MCP server configuration schema
//!
//! Extends existing McpServer with source-based configuration

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Transport types for MCP servers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Command-based execution (default)
    #[default]
    Stdio,
    /// HTTP-based MCP server
    Http,
}

impl TryFrom<&str> for TransportType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "stdio" => Ok(TransportType::Stdio),
            "http" => Ok(TransportType::Http),
            _ => anyhow::bail!("Invalid transport: '{}'. Valid values: stdio, http", value),
        }
    }
}

/// Complete MCP server configuration from sift.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Transport type (defaults to stdio for backward compatibility)
    #[serde(default)]
    pub transport: TransportType,

    /// STDIO: Source: "registry:name" or "local:/path/to/server"
    #[serde(default)]
    pub source: String,

    /// STDIO: Runtime: docker, node, python, bun
    #[serde(default = "default_runtime")]
    pub runtime: RuntimeType,

    /// STDIO: Command arguments
    #[serde(default)]
    pub args: Vec<String>,

    /// HTTP: Server URL
    #[serde(default)]
    pub url: Option<String>,

    /// HTTP: Static HTTP headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Target control (whitelist) - applies to all transports
    #[serde(default)]
    pub targets: Option<Vec<String>>,

    /// Ignore targets (blacklist) - applies to all transports
    #[serde(default)]
    pub ignore_targets: Option<Vec<String>>,

    /// Environment variables (all transports)
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_runtime() -> RuntimeType {
    RuntimeType::Node
}

/// Runtime types for MCP servers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeType {
    Docker,
    Node,
    Python,
    Bun,
    Shell, // For compiled binaries, shell scripts, etc.
}

impl RuntimeType {
    /// Check if this runtime can be swapped with another during merge
    pub fn is_compatible_with(&self, other: &RuntimeType) -> bool {
        match (self, other) {
            // Node and Bun are compatible (both JS runtimes)
            (RuntimeType::Node, RuntimeType::Bun) | (RuntimeType::Bun, RuntimeType::Node) => true,
            // Shell is only compatible with itself (direct execution)
            (RuntimeType::Shell, RuntimeType::Shell) => true,
            // Same runtime is always compatible
            (a, b) if a == b => true,
            // All other combinations are incompatible
            _ => false,
        }
    }
}

impl TryFrom<&str> for RuntimeType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().as_str() {
            "docker" => Ok(RuntimeType::Docker),
            "node" => Ok(RuntimeType::Node),
            "python" => Ok(RuntimeType::Python),
            "bun" => Ok(RuntimeType::Bun),
            "shell" => Ok(RuntimeType::Shell),
            _ => anyhow::bail!("Invalid runtime: {}", value),
        }
    }
}

/// Override configuration for project-local MCP settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfigOverride {
    /// Override runtime
    pub runtime: Option<RuntimeType>,

    /// Override environment variables (merged with base)
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl McpConfig {
    /// Merge another McpConfig into this one
    pub fn merge(&mut self, other: McpConfig) {
        // Transport type is always overridden
        self.transport = other.transport;

        // STDIO-specific fields
        if other.runtime != RuntimeType::Node {
            self.runtime = other.runtime;
        }
        if !other.args.is_empty() {
            self.args = other.args;
        }
        if !other.source.is_empty() {
            self.source = other.source;
        }

        // HTTP-specific fields
        if other.url.is_some() {
            self.url = other.url;
        }
        if !other.headers.is_empty() {
            self.headers = other.headers;
        }

        // Common fields
        if other.targets.is_some() {
            self.targets = other.targets;
        }
        if other.ignore_targets.is_some() {
            self.ignore_targets = other.ignore_targets;
        }

        // Deep merge env vars
        for (key, value) in other.env {
            self.env.insert(key, value);
        }
    }

    /// Apply project-local override
    pub fn apply_override(&mut self, override_config: &McpConfigOverride) {
        if let Some(runtime) = override_config.runtime {
            self.runtime = runtime;
        }
        for (key, value) in &override_config.env {
            self.env.insert(key.clone(), value.clone());
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.targets.is_some() && self.ignore_targets.is_some() {
            anyhow::bail!("Cannot specify both 'targets' and 'ignore_targets'");
        }

        match self.transport {
            TransportType::Stdio => {
                // Validate source format for stdio
                if !self.source.starts_with("registry:")
                    && !self.source.starts_with("local:")
                    && !self.source.starts_with("github:")
                    && !self.source.starts_with("git:")
                {
                    anyhow::bail!(
                        "Invalid source format for stdio transport: must be 'registry:name', 'local:/path', 'github:org/repo', or 'git:url'"
                    );
                }
            }
            TransportType::Http => {
                // Validate URL for http
                if self.url.is_none() || self.url.as_ref().is_some_and(|u| u.is_empty()) {
                    anyhow::bail!("URL is required for http transport");
                }
            }
        }

        Ok(())
    }

    /// Check if this MCP server should be deployed to a provider
    pub fn should_deploy_to(&self, provider_id: &str) -> bool {
        if let Some(ref targets) = self.targets {
            return targets.contains(&provider_id.to_string());
        }
        if let Some(ref ignore) = self.ignore_targets {
            return !ignore.contains(&provider_id.to_string());
        }
        true
    }

    /// Convert to legacy McpServer for backward compatibility
    pub fn to_legacy(&self, id: String) -> super::McpServer {
        let (command, args) = match self.runtime {
            RuntimeType::Docker => (
                "docker".to_string(),
                vec![
                    "run".to_string(),
                    "-i".to_string(),
                    "--rm".to_string(),
                    self.source.replace("registry:", ""),
                ],
            ),
            RuntimeType::Node => ("node".to_string(), self.args.clone()),
            RuntimeType::Python => ("python".to_string(), self.args.clone()),
            RuntimeType::Bun => ("bun".to_string(), self.args.clone()),
            RuntimeType::Shell => (self.source.clone(), self.args.clone()),
        };

        super::McpServer {
            id,
            name: self.source.clone(),
            command,
            args,
            env: self.env.keys().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let config = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:postgres-mcp".to_string(),
            runtime: RuntimeType::Docker,
            args: vec!["--readonly".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: None,
            env: HashMap::new(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_both_targets_and_ignore() {
        let config = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:postgres-mcp".to_string(),
            runtime: RuntimeType::Docker,
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: Some(vec!["vscode".to_string()]),
            env: HashMap::new(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_should_deploy_with_targets() {
        let config = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:test".to_string(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: None,
            env: HashMap::new(),
        };
        assert!(config.should_deploy_to("claude-desktop"));
        assert!(!config.should_deploy_to("vscode"));
    }

    #[test]
    fn test_should_deploy_with_ignore() {
        let config = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:test".to_string(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: Some(vec!["vscode".to_string()]),
            env: HashMap::new(),
        };
        assert!(config.should_deploy_to("claude-desktop"));
        assert!(!config.should_deploy_to("vscode"));
    }

    #[test]
    fn test_merge_configs() {
        let mut base = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:test".to_string(),
            runtime: RuntimeType::Node,
            args: vec!["--arg1".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("BASE_VAR".to_string(), "base_value".to_string());
                map
            },
        };

        let overlay = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:test".to_string(),
            runtime: RuntimeType::Bun,
            args: vec!["--arg2".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("OVERLAY_VAR".to_string(), "overlay_value".to_string());
                map
            },
        };

        base.merge(overlay);

        assert_eq!(base.runtime, RuntimeType::Bun);
        assert_eq!(base.args, vec!["--arg2".to_string()]);
        assert!(base.targets.is_some());
        assert_eq!(base.env.len(), 2);
        assert_eq!(base.env.get("BASE_VAR"), Some(&"base_value".to_string()));
        assert_eq!(
            base.env.get("OVERLAY_VAR"),
            Some(&"overlay_value".to_string())
        );
    }

    #[test]
    fn test_apply_override() {
        let mut config = McpConfig {
            transport: TransportType::Stdio,
            source: "registry:test".to_string(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("BASE_VAR".to_string(), "base_value".to_string());
                map
            },
        };

        let override_config = McpConfigOverride {
            runtime: Some(RuntimeType::Docker),
            env: {
                let mut map = HashMap::new();
                map.insert("OVERRIDE_VAR".to_string(), "override_value".to_string());
                map
            },
        };

        config.apply_override(&override_config);

        assert_eq!(config.runtime, RuntimeType::Docker);
        assert_eq!(config.env.len(), 2);
        assert_eq!(config.env.get("BASE_VAR"), Some(&"base_value".to_string()));
        assert_eq!(
            config.env.get("OVERRIDE_VAR"),
            Some(&"override_value".to_string())
        );
    }

    #[test]
    fn test_validate_http_config_valid() {
        let config = McpConfig {
            transport: TransportType::Http,
            source: String::new(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: Some("https://mcp.example.com".to_string()),
            headers: {
                let mut map = HashMap::new();
                map.insert("Authorization".to_string(), "Bearer token".to_string());
                map
            },
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_http_config_missing_url() {
        let config = McpConfig {
            transport: TransportType::Http,
            source: String::new(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_stdio_config_invalid_source() {
        let config = McpConfig {
            transport: TransportType::Stdio,
            source: "invalid:source".to_string(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_merge_http_config() {
        let mut base = McpConfig {
            transport: TransportType::Http,
            source: String::new(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: Some("https://example.com".to_string()),
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        };

        let overlay = McpConfig {
            transport: TransportType::Http,
            source: String::new(),
            runtime: RuntimeType::Node,
            args: vec![],
            url: Some("https://example.com/new".to_string()),
            headers: {
                let mut map = HashMap::new();
                map.insert("X-Header".to_string(), "value".to_string());
                map
            },
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("ENV_VAR".to_string(), "env_value".to_string());
                map
            },
        };

        base.merge(overlay);

        assert_eq!(base.url, Some("https://example.com/new".to_string()));
        assert_eq!(base.headers.len(), 1);
        assert!(base.targets.is_some());
        assert_eq!(base.env.len(), 1);
    }
}
