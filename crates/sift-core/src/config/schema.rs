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

    /// Global install link mode
    #[serde(default)]
    pub link_mode: Option<crate::fs::LinkMode>,

    /// Client configurations (valid in all scopes)
    #[serde(default)]
    pub clients: HashMap<String, ClientConfigEntry>,

    /// Registry configurations
    #[serde(default)]
    pub registry: HashMap<String, RegistryConfigEntry>,

    /// Project-local configuration (ONLY valid in global config)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub projects: HashMap<String, ProjectConfig>,
}

/// MCP server configuration entry (inline config for TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfigEntry {
    /// Transport type: stdio or http (defaults to stdio)
    #[serde(default)]
    pub transport: Option<String>,

    /// STDIO: Source: "registry:name" or "local:/path/to/server"
    #[serde(default)]
    pub source: String,

    /// STDIO: Runtime: docker, node, python, bun, shell
    #[serde(default)]
    pub runtime: Option<String>,

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

    // RESET FLAGS - for clearing inherited values
    /// Reset targets to None (clear inherited whitelist)
    #[serde(default)]
    pub reset_targets: bool,

    /// Reset ignore_targets to None (clear inherited blacklist)
    #[serde(default)]
    pub reset_ignore_targets: bool,

    /// Reset specific environment variables by key
    #[serde(default)]
    pub reset_env: Option<Vec<String>>,

    /// Reset all environment variables to empty
    #[serde(default)]
    pub reset_env_all: bool,
}

impl McpConfigEntry {
    /// Check if this MCP server should be deployed to a client
    pub fn should_deploy_to(&self, client_id: &str) -> bool {
        if let Some(ref targets) = self.targets {
            return targets.contains(&client_id.to_string());
        }
        if let Some(ref ignore) = self.ignore_targets {
            return !ignore.contains(&client_id.to_string());
        }
        true
    }
}

impl TryFrom<McpConfigEntry> for crate::mcp::McpConfig {
    type Error = anyhow::Error;

    fn try_from(entry: McpConfigEntry) -> Result<Self, Self::Error> {
        // After Phase 2: runtime and transport are Option<String>
        let runtime_str = entry.runtime.as_deref().unwrap_or("node");
        let runtime = crate::mcp::RuntimeType::try_from(runtime_str)?;

        let transport_str = entry.transport.as_deref().unwrap_or("stdio");
        let transport = crate::mcp::TransportType::try_from(transport_str)?;

        Ok(crate::mcp::McpConfig {
            transport,
            source: entry.source,
            runtime,
            args: entry.args,
            url: entry.url,
            headers: entry.headers,
            targets: entry.targets,
            ignore_targets: entry.ignore_targets,
            env: entry.env,
        })
    }
}

/// Skill configuration entry (inline config for TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfigEntry {
    /// Source: "registry:author/skill" or "local:/path/to/skill"
    pub source: String,

    /// Version constraint: semver, git SHA, or "latest"
    #[serde(default)]
    pub version: Option<String>,

    /// Target control (whitelist)
    #[serde(default)]
    pub targets: Option<Vec<String>>,

    /// Ignore targets (blacklist)
    #[serde(default)]
    pub ignore_targets: Option<Vec<String>>,

    // RESET FLAG
    /// Reset version to None (clear inherited version)
    #[serde(default)]
    pub reset_version: bool,
}

impl SkillConfigEntry {
    /// Check if this skill should be deployed to a client
    pub fn should_deploy_to(&self, client_id: &str) -> bool {
        if let Some(ref targets) = self.targets {
            return targets.contains(&client_id.to_string());
        }
        if let Some(ref ignore) = self.ignore_targets {
            return !ignore.contains(&client_id.to_string());
        }
        true
    }
}

impl TryFrom<SkillConfigEntry> for crate::skills::SkillConfig {
    type Error = anyhow::Error;

    fn try_from(entry: SkillConfigEntry) -> Result<Self, Self::Error> {
        let version_str = entry.version.as_deref().unwrap_or("latest");
        Ok(crate::skills::SkillConfig {
            source: entry.source,
            version: version_str.to_string(),
            targets: entry.targets,
            ignore_targets: entry.ignore_targets,
        })
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

    /// Capabilities (optional, auto-detected for built-in clients)
    #[serde(default)]
    pub capabilities: Option<serde_json::Value>,
}

fn default_client_enabled() -> bool {
    true
}

impl TryFrom<ClientConfigEntry> for crate::client::ClientConfig {
    type Error = anyhow::Error;

    fn try_from(entry: ClientConfigEntry) -> Result<Self, Self::Error> {
        Ok(crate::client::ClientConfig {
            enabled: entry.enabled,
            source: entry.source,
            capabilities: None, // Would need JSON deserialization for full support
        })
    }
}

/// Registry configuration entry (inline config for TOML)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Project-specific configuration
///
/// Key is absolute path to project root
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    /// Path to project (absolute) - inferred from the key
    #[serde(default)]
    pub path: std::path::PathBuf,

    /// Project-local MCP entries for this project
    #[serde(default)]
    pub mcp: HashMap<String, McpConfigEntry>,

    /// Project-local skill entries for this project
    #[serde(default)]
    pub skill: HashMap<String, SkillConfigEntry>,

    /// MCP server overrides for this project
    #[serde(default)]
    pub mcp_overrides: HashMap<String, McpOverrideEntry>,

    /// Skill overrides for this project
    #[serde(default)]
    pub skill_overrides: HashMap<String, SkillOverrideEntry>,
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
    pub fn to_override(&self) -> anyhow::Result<crate::mcp::McpConfigOverride> {
        let runtime = if let Some(runtime_str) = self.runtime.as_deref() {
            Some(
                crate::mcp::RuntimeType::try_from(runtime_str)
                    .with_context(|| format!("Invalid runtime override: '{runtime_str}'"))?,
            )
        } else {
            None
        };

        Ok(crate::mcp::McpConfigOverride {
            runtime,
            env: self.env.clone(),
        })
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
        // Validate project-local entries and overrides in the global config.
        for (project_key, project_config) in &self.projects {
            for (mcp_name, entry) in &project_config.mcp {
                let config: crate::mcp::McpConfig =
                    entry.clone().try_into().with_context(|| {
                        format!("Invalid MCP entry for project '{project_key}', MCP '{mcp_name}'")
                    })?;
                config.validate().with_context(|| {
                    format!("Invalid MCP entry for project '{project_key}', MCP '{mcp_name}'")
                })?;
            }
            for (mcp_name, mcp_override) in &project_config.mcp_overrides {
                if let Some(runtime_str) = mcp_override.runtime.as_deref() {
                    crate::mcp::RuntimeType::try_from(runtime_str).with_context(|| {
                        format!(
                            "Invalid runtime override for project '{project_key}', MCP '{mcp_name}': '{runtime_str}'"
                        )
                    })?;
                }
            }
            for (skill_name, entry) in &project_config.skill {
                let config: crate::skills::SkillConfig =
                    entry.clone().try_into().with_context(|| {
                        format!(
                            "Invalid skill entry for project '{project_key}', skill '{skill_name}'"
                        )
                    })?;
                config.validate().with_context(|| {
                    format!("Invalid skill entry for project '{project_key}', skill '{skill_name}'")
                })?;
            }
        }

        // Validate each MCP server config
        for (name, entry) in &self.mcp {
            // Reset flag mutual exclusion (entry-level validation)
            if entry.reset_targets && entry.targets.is_some() {
                anyhow::bail!(
                    "MCP server '{}': Cannot specify both reset_targets=true and targets=[...]. \
                     Use reset_targets=true to clear, or targets=[...] to set specific values.",
                    name
                );
            }
            if entry.reset_ignore_targets && entry.ignore_targets.is_some() {
                anyhow::bail!(
                    "MCP server '{}': Cannot specify both reset_ignore_targets=true and ignore_targets=[...].",
                    name
                );
            }
            if entry.reset_env_all && entry.reset_env.is_some() {
                anyhow::bail!(
                    "MCP server '{}': Cannot specify both reset_env_all=true and reset_env=[...]. \
                     Use reset_env_all=true to clear all, or reset_env=[...] to clear specific keys.",
                    name
                );
            }
            if entry.reset_env_all && !entry.env.is_empty() {
                anyhow::bail!(
                    "MCP server '{}': Cannot specify both reset_env_all=true and env={{...}} in same config layer.",
                    name
                );
            }

            let config: crate::mcp::McpConfig = entry
                .clone()
                .try_into()
                .with_context(|| format!("Invalid MCP server configuration: '{}'", name))?;
            config
                .validate()
                .with_context(|| format!("Invalid MCP server configuration: '{}'", name))?;
        }

        // Validate each skill config
        for (name, entry) in &self.skill {
            // Reset flag mutual exclusion
            if entry.reset_version && entry.version.is_some() {
                anyhow::bail!(
                    "Skill '{}': Cannot specify both reset_version=true and version=\"...\".",
                    name
                );
            }

            let config: crate::skills::SkillConfig = entry
                .clone()
                .try_into()
                .with_context(|| format!("Invalid skill configuration: '{}'", name))?;
            config
                .validate()
                .with_context(|| format!("Invalid skill configuration: '{}'", name))?;
        }

        // Validate each client config
        for (name, entry) in &self.clients {
            // Warn about unimplemented capabilities override
            if entry.capabilities.is_some() {
                eprintln!(
                    "Warning: Client '{}' has capabilities override, but this is currently ignored. \
                     The capabilities field is not yet supported and will be removed from the final config.",
                    name
                );
            }

            let config: crate::client::ClientConfig = entry
                .clone()
                .try_into()
                .with_context(|| format!("Invalid client configuration: '{}'", name))?;
            config
                .validate()
                .with_context(|| format!("Invalid client configuration: '{}'", name))?;
        }

        // Validate each registry config
        for (name, entry) in &self.registry {
            let config: crate::registry::RegistryConfig = entry
                .clone()
                .try_into()
                .with_context(|| format!("Invalid registry configuration: '{}'", name))?;
            config
                .validate()
                .with_context(|| format!("Invalid registry configuration: '{}'", name))?;
        }

        Ok(())
    }

    /// Check if this is a global config (has projects section)
    pub fn is_global(&self) -> bool {
        !self.projects.is_empty()
    }

    /// Get the project configuration for a given path
    ///
    /// Returns `(project_key, project_config)` where `project_key` is the
    /// matching path from the projects map.
    ///
    /// Path matching is deterministic: tries exact match first, then longest
    /// prefix match (sorted by path length descending).
    ///
    /// Note: Paths should be normalized before calling this method.
    /// Use `Path::canonicalize()` for symlinks, or normalize with
    /// `Path::components()` for relative path resolution.
    pub fn get_project_config(&self, path: &Path) -> Option<(String, &ProjectConfig)> {
        // Try exact match first - use get_key_value to get the actual key reference
        let path_str = path.to_string_lossy().to_string();
        if let Some((key, project_config)) = self.projects.get_key_value(&path_str) {
            return Some((key.clone(), project_config));
        }

        // Sort by length (descending) for longest prefix match
        let mut sorted: Vec<_> = self.projects.iter().collect();
        sorted.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));

        for (project_path, project_config) in sorted {
            if path.starts_with(project_path) {
                return Some((project_path.to_string(), project_config));
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
        config
            .projects
            .insert("/Users/test/project".to_string(), ProjectConfig::default());
        assert!(config.is_global());
    }

    #[test]
    fn test_mcp_config_entry_to_mcp_config() {
        let entry = McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: "registry:postgres-mcp".to_string(),
            runtime: Some("docker".to_string()),
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
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        let config: crate::mcp::McpConfig = entry.try_into().unwrap();
        assert_eq!(config.source, "registry:postgres-mcp");
        assert_eq!(config.runtime, crate::mcp::RuntimeType::Docker);
        assert_eq!(config.args, vec!["--readonly".to_string()]);
        assert_eq!(config.env.len(), 1);
    }

    #[test]
    fn test_skill_config_entry_to_skill_config() {
        let entry = SkillConfigEntry {
            source: "registry:anthropic/pdf".to_string(),
            version: Some("^1.0".to_string()),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
            reset_version: false,
        };

        let config: crate::skills::SkillConfig = entry.try_into().unwrap();
        assert_eq!(config.source, "registry:anthropic/pdf");
        assert_eq!(config.version, "^1.0");
        assert!(config.targets.is_some());
    }

    #[test]
    fn test_get_project_config() {
        let mut config = SiftConfig::new();
        let project_path = "/Users/test/project".to_string();
        let project_config = ProjectConfig {
            path: std::path::PathBuf::from(&project_path),
            ..Default::default()
        };
        config.projects.insert(project_path.clone(), project_config);

        let found = config.get_project_config(&std::path::PathBuf::from("/Users/test/project"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().0, "/Users/test/project");

        let not_found = config.get_project_config(&std::path::PathBuf::from("/other/path"));
        assert!(not_found.is_none());
    }

    #[test]
    fn test_project_config_deterministic_longest_match() {
        let mut config = SiftConfig::new();
        config
            .projects
            .insert("/Users/me".to_string(), ProjectConfig::default());
        config
            .projects
            .insert("/Users/me/repos".to_string(), ProjectConfig::default());

        let result = config.get_project_config(std::path::Path::new("/Users/me/repos/project"));
        // Should ALWAYS return the longer match key, regardless of HashMap order
        assert_eq!(result.unwrap().0, "/Users/me/repos");
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

        let override_config = entry.to_override().unwrap();
        assert_eq!(
            override_config.runtime,
            Some(crate::mcp::RuntimeType::Docker)
        );
        assert_eq!(override_config.env.len(), 1);
    }

    #[test]
    fn test_validate_valid_config() {
        let mut config = SiftConfig::new();

        config.mcp.insert(
            "test-mcp".to_string(),
            McpConfigEntry {
                transport: Some("stdio".to_string()),
                source: "registry:test".to_string(),
                runtime: Some("node".to_string()),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: HashMap::new(),
                reset_targets: false,
                reset_ignore_targets: false,
                reset_env: None,
                reset_env_all: false,
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
                transport: Some("stdio".to_string()),
                source: "invalid:source".to_string(),
                runtime: Some("node".to_string()),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: Some(vec!["target".to_string()]),
                ignore_targets: Some(vec!["ignore".to_string()]), // Both set - should fail
                env: HashMap::new(),
                reset_targets: false,
                reset_ignore_targets: false,
                reset_env: None,
                reset_env_all: false,
            },
        );

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_project_override_runtime() {
        let mut config = SiftConfig::new();

        let mut project_config = ProjectConfig::default();
        project_config.mcp_overrides.insert(
            "test-mcp".to_string(),
            McpOverrideEntry {
                runtime: Some("doker".to_string()),
                env: HashMap::new(),
            },
        );

        config
            .projects
            .insert("/Users/test/project".to_string(), project_config);

        let err = config
            .validate()
            .expect_err("validate() must fail for invalid project override runtime")
            .to_string();
        assert!(
            err.contains("Invalid runtime") || err.contains("doker"),
            "unexpected error: {err}"
        );
    }
}
