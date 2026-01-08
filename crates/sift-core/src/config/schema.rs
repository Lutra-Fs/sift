//! Configuration schema for sift.toml
//!
//! Defines the structure for all three configuration layers:
//! - Global: ~/.config/sift/sift.toml
//! - Project: ./sift.toml
//! - Project-Local: [projects."/path"] in global config

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Root configuration structure for sift.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiftConfig {
    /// MCP server configurations
    #[serde(default)]
    pub mcp: HashMap<String, McpConfigEntry>,

    /// Skill configurations
    #[serde(default)]
    pub skill: HashMap<String, SkillConfigEntry>,

    /// Client configurations (valid in all scopes)
    #[serde(default)]
    pub clients: HashMap<String, ClientConfigEntry>,

    /// Registry configurations
    #[serde(default)]
    pub registry: HashMap<String, RegistryConfigEntry>,

    /// Project-local overrides (ONLY valid in global config)
    #[serde(default)]
    pub projects: HashMap<String, ProjectOverride>,
}

/// MCP server configuration entry (inline config for TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfigEntry {
    /// Transport type: stdio or http (defaults to stdio)
    #[serde(default = "default_transport")]
    pub transport: String,

    /// STDIO: Source: "registry:name" or "local:/path/to/server"
    #[serde(default)]
    pub source: String,

    /// STDIO: Runtime: docker, node, python, bun
    #[serde(default = "default_runtime")]
    pub runtime: String,

    /// STDIO: Command arguments
    #[serde(default)]
    pub args: Vec<String>,

    /// HTTP: Server URL
    #[serde(default)]
    pub url: Option<String>,

    /// HTTP: Static HTTP headers
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Target control (whitelist)
    #[serde(default)]
    pub targets: Option<Vec<String>>,

    /// Ignore targets (blacklist)
    #[serde(default)]
    pub ignore_targets: Option<Vec<String>>,

    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_runtime() -> String {
    "node".to_string()
}

fn default_transport() -> String {
    "stdio".to_string()
}

impl From<McpConfigEntry> for crate::mcp::McpConfig {
    fn from(entry: McpConfigEntry) -> Self {
        let runtime = match entry.runtime.as_str() {
            "docker" => crate::mcp::RuntimeType::Docker,
            "python" => crate::mcp::RuntimeType::Python,
            "bun" => crate::mcp::RuntimeType::Bun,
            _ => crate::mcp::RuntimeType::Node,
        };

        let transport = match entry.transport.as_str() {
            "http" => crate::mcp::TransportType::Http,
            _ => crate::mcp::TransportType::Stdio,
        };

        crate::mcp::McpConfig {
            transport,
            source: entry.source,
            runtime,
            args: entry.args,
            url: entry.url,
            headers: entry.headers,
            targets: entry.targets,
            ignore_targets: entry.ignore_targets,
            env: entry.env,
        }
    }
}

/// Skill configuration entry (inline config for TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfigEntry {
    /// Source: "registry:author/skill" or "local:/path/to/skill"
    pub source: String,

    /// Version constraint: semver, git SHA, or "latest"
    #[serde(default = "default_skill_version")]
    pub version: String,

    /// Target control (whitelist)
    #[serde(default)]
    pub targets: Option<Vec<String>>,

    /// Ignore targets (blacklist)
    #[serde(default)]
    pub ignore_targets: Option<Vec<String>>,
}

fn default_skill_version() -> String {
    "latest".to_string()
}

impl From<SkillConfigEntry> for crate::skills::SkillConfig {
    fn from(entry: SkillConfigEntry) -> Self {
        crate::skills::SkillConfig {
            source: entry.source,
            version: entry.version,
            targets: entry.targets,
            ignore_targets: entry.ignore_targets,
        }
    }
}

/// Client configuration entry (inline config for TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfigEntry {
    /// Whether this client is enabled
    #[serde(default = "default_client_enabled")]
    pub enabled: bool,

    /// Source for external providers: "registry:provider-name"
    #[serde(default)]
    pub source: Option<String>,

    /// Filesystem strategy for skill delivery
    #[serde(default)]
    pub fs_strategy: Option<String>,

    /// Capabilities (optional, auto-detected for built-in clients)
    #[serde(default)]
    pub capabilities: Option<serde_json::Value>,
}

fn default_client_enabled() -> bool {
    true
}

impl From<ClientConfigEntry> for crate::client::ClientConfig {
    fn from(entry: ClientConfigEntry) -> Self {
        let fs_strategy = match entry.fs_strategy.as_deref() {
            Some("symlink") => crate::client::FsStrategy::Symlink,
            Some("copy") => crate::client::FsStrategy::Copy,
            _ => crate::client::FsStrategy::Auto,
        };

        crate::client::ClientConfig {
            enabled: entry.enabled,
            source: entry.source,
            fs_strategy,
            capabilities: None, // Would need JSON deserialization for full support
        }
    }
}

/// Registry configuration entry (inline config for TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfigEntry {
    /// Registry type: "sift" or "claude-marketplace"
    #[serde(default = "default_registry_type")]
    pub r#type: String,

    /// URL for sift-type registries
    pub url: Option<String>,

    /// Source for claude-marketplace type: "github:org/repo"
    #[serde(default)]
    pub source: Option<String>,
}

fn default_registry_type() -> String {
    "sift".to_string()
}

impl TryFrom<RegistryConfigEntry> for crate::registry::RegistryConfig {
    type Error = anyhow::Error;

    fn try_from(entry: RegistryConfigEntry) -> Result<Self, Self::Error> {
        let registry_type = match entry.r#type.as_str() {
            "claude-marketplace" => crate::registry::RegistryType::ClaudeMarketplace,
            _ => crate::registry::RegistryType::Sift,
        };

        let url = if let Some(url_str) = entry.url {
            Some(url::Url::parse(&url_str)?)
        } else {
            None
        };

        Ok(crate::registry::RegistryConfig {
            r#type: registry_type,
            url,
            source: entry.source,
        })
    }
}

/// Project-specific configuration override
///
/// Key is absolute path to project root
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectOverride {
    /// Path to project (absolute) - inferred from the key
    #[serde(default)]
    pub path: std::path::PathBuf,

    /// MCP server overrides for this project
    #[serde(default)]
    pub mcp: HashMap<String, McpOverrideEntry>,

    /// Skill overrides for this project
    #[serde(default)]
    pub skill: HashMap<String, SkillOverrideEntry>,
}

/// MCP server override for project-local configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpOverrideEntry {
    /// Override runtime
    pub runtime: Option<String>,

    /// Override environment variables (merged with base)
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl McpOverrideEntry {
    /// Convert to full McpConfigOverride
    pub fn to_override(&self) -> crate::mcp::McpConfigOverride {
        crate::mcp::McpConfigOverride {
            runtime: self.runtime.as_ref().map(|r| match r.as_str() {
                "docker" => crate::mcp::RuntimeType::Docker,
                "python" => crate::mcp::RuntimeType::Python,
                "bun" => crate::mcp::RuntimeType::Bun,
                _ => crate::mcp::RuntimeType::Node,
            }),
            env: self.env.clone(),
        }
    }
}

/// Skill override for project-local configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillOverrideEntry {
    /// Override version
    pub version: Option<String>,
}

impl SkillOverrideEntry {
    /// Convert to full SkillConfigOverride
    pub fn to_override(&self) -> crate::skills::SkillConfigOverride {
        crate::skills::SkillConfigOverride {
            version: self.version.clone(),
        }
    }
}

impl SiftConfig {
    /// Create a new empty configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        // Validate each MCP server config
        for (name, entry) in &self.mcp {
            let config: crate::mcp::McpConfig = entry.clone().into();
            config.validate()
                .with_context(|| format!("Invalid MCP server configuration: '{}'", name))?;
        }

        // Validate each skill config
        for (name, entry) in &self.skill {
            let config: crate::skills::SkillConfig = entry.clone().into();
            config.validate()
                .with_context(|| format!("Invalid skill configuration: '{}'", name))?;
        }

        // Validate each client config
        for (name, entry) in &self.clients {
            let config: crate::client::ClientConfig = entry.clone().into();
            config.validate()
                .with_context(|| format!("Invalid client configuration: '{}'", name))?;
        }

        // Validate each registry config
        for (name, entry) in &self.registry {
            let config: crate::registry::RegistryConfig = entry.clone().try_into()
                .with_context(|| format!("Invalid registry configuration: '{}'", name))?;
            config.validate()
                .with_context(|| format!("Invalid registry configuration: '{}'", name))?;
        }

        Ok(())
    }

    /// Check if this is a global config (has projects section)
    pub fn is_global(&self) -> bool {
        !self.projects.is_empty()
    }

    /// Get the project override for a given path
    pub fn get_project_override(&self, path: &Path) -> Option<&ProjectOverride> {
        // Try exact match first
        if let Some(override_config) = self.projects.get(&path.to_string_lossy().to_string()) {
            return Some(override_config);
        }

        // Try to find by checking if path starts with any project key
        for (project_path, override_config) in &self.projects {
            if path.starts_with(project_path) {
                return Some(override_config);
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config() {
        let config = SiftConfig::new();
        assert!(config.mcp.is_empty());
        assert!(config.skill.is_empty());
        assert!(config.clients.is_empty());
        assert!(config.registry.is_empty());
        assert!(config.projects.is_empty());
        assert!(!config.is_global());
    }

    #[test]
    fn test_config_with_projects_is_global() {
        let mut config = SiftConfig::new();
        config.projects.insert(
            "/Users/test/project".to_string(),
            ProjectOverride::default(),
        );
        assert!(config.is_global());
    }

    #[test]
    fn test_mcp_config_entry_to_mcp_config() {
        let entry = McpConfigEntry {
            transport: "stdio".to_string(),
            source: "registry:postgres-mcp".to_string(),
            runtime: "docker".to_string(),
            args: vec!["--readonly".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("DB_URL".to_string(), "postgres://localhost".to_string());
                map
            },
        };

        let config: crate::mcp::McpConfig = entry.into();
        assert_eq!(config.source, "registry:postgres-mcp");
        assert_eq!(config.runtime, crate::mcp::RuntimeType::Docker);
        assert_eq!(config.args, vec!["--readonly".to_string()]);
        assert_eq!(config.env.len(), 1);
    }

    #[test]
    fn test_skill_config_entry_to_skill_config() {
        let entry = SkillConfigEntry {
            source: "registry:anthropic/pdf".to_string(),
            version: "^1.0".to_string(),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
        };

        let config: crate::skills::SkillConfig = entry.into();
        assert_eq!(config.source, "registry:anthropic/pdf");
        assert_eq!(config.version, "^1.0");
        assert!(config.targets.is_some());
    }

    #[test]
    fn test_get_project_override() {
        let mut config = SiftConfig::new();
        let project_path = "/Users/test/project".to_string();
        let override_config = ProjectOverride {
            path: std::path::PathBuf::from(&project_path),
            ..Default::default()
        };
        config.projects.insert(project_path.clone(), override_config);

        let found = config.get_project_override(&std::path::PathBuf::from("/Users/test/project"));
        assert!(found.is_some());

        let not_found = config.get_project_override(&std::path::PathBuf::from("/other/path"));
        assert!(not_found.is_none());
    }

    #[test]
    fn test_mcp_override_to_override() {
        let entry = McpOverrideEntry {
            runtime: Some("docker".to_string()),
            env: {
                let mut map = HashMap::new();
                map.insert("OVERRIDE_VAR".to_string(), "value".to_string());
                map
            },
        };

        let override_config = entry.to_override();
        assert_eq!(override_config.runtime, Some(crate::mcp::RuntimeType::Docker));
        assert_eq!(override_config.env.len(), 1);
    }

    #[test]
    fn test_validate_valid_config() {
        let mut config = SiftConfig::new();

        config.mcp.insert(
            "test-mcp".to_string(),
            McpConfigEntry {
                transport: "stdio".to_string(),
                source: "registry:test".to_string(),
                runtime: "node".to_string(),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: HashMap::new(),
            },
        );

        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_config() {
        let mut config = SiftConfig::new();

        config.mcp.insert(
            "test-mcp".to_string(),
            McpConfigEntry {
                transport: "stdio".to_string(),
                source: "invalid:source".to_string(),
                runtime: "node".to_string(),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: Some(vec!["target".to_string()]),
                ignore_targets: Some(vec!["ignore".to_string()]), // Both set - should fail
                env: HashMap::new(),
            },
        );

        assert!(config.validate().is_err());
    }
}
