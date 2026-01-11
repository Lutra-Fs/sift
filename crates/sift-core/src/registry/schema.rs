//! Registry configuration schema
//!
//! Defines registry sources for discovering and resolving packages

use serde::{Deserialize, Serialize};
use url::Url;

/// Registry capability flags declared by implementations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryCapabilities {
    pub supports_version_pinning: bool,
}

/// Registry configuration from sift.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Registry type
    #[serde(default = "default_registry_type")]
    pub r#type: RegistryType,

    /// URL for sift-type registries
    pub url: Option<Url>,

    /// Source for claude-marketplace type: "github:org/repo"
    #[serde(default)]
    pub source: Option<String>,
}

fn default_registry_type() -> RegistryType {
    RegistryType::Sift
}

/// Registry types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegistryType {
    /// Native Sift registry
    Sift,
    /// Claude Marketplace adapter
    ClaudeMarketplace,
}

impl RegistryConfig {
    /// Merge another RegistryConfig into this one
    pub fn merge(&mut self, other: RegistryConfig) {
        self.r#type = other.r#type;
        if other.url.is_some() {
            self.url = other.url;
        }
        if other.source.is_some() {
            self.source = other.source;
        }
    }

    /// Validate configuration based on registry type
    pub fn validate(&self) -> anyhow::Result<()> {
        match self.r#type {
            RegistryType::Sift => {
                if self.url.is_none() {
                    anyhow::bail!("Sift registry requires 'url' field");
                }
            }
            RegistryType::ClaudeMarketplace => {
                if self.source.is_none() {
                    anyhow::bail!("Claude Marketplace registry requires 'source' field");
                }
                if let Some(ref source) = self.source
                    && !source.starts_with("github:")
                    && !source.starts_with("git:")
                {
                    anyhow::bail!(
                        "Claude Marketplace source must be 'github:org/repo' or 'git:url'"
                    );
                }
            }
        }
        Ok(())
    }

    /// Get the registry name for display purposes
    pub fn display_name(&self) -> String {
        match self.r#type {
            RegistryType::Sift => self
                .url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_else(|| "Sift Registry".to_string()),
            RegistryType::ClaudeMarketplace => self
                .source
                .as_ref()
                .map(|s| {
                    format!(
                        "Claude Marketplace ({})",
                        s.strip_prefix("github:")
                            .or(s.strip_prefix("git:"))
                            .unwrap_or(s)
                    )
                })
                .unwrap_or_else(|| "Claude Marketplace".to_string()),
        }
    }
}

impl Default for RegistryConfig {
    fn default() -> Self {
        RegistryConfig {
            r#type: RegistryType::Sift,
            url: None,
            source: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_sift_registry_valid() {
        let config = RegistryConfig {
            r#type: RegistryType::Sift,
            url: Some(Url::parse("https://registry.sift.sh/v1").unwrap()),
            source: None,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_sift_registry_missing_url() {
        let config = RegistryConfig {
            r#type: RegistryType::Sift,
            url: None,
            source: None,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_claude_marketplace_valid() {
        let config = RegistryConfig {
            r#type: RegistryType::ClaudeMarketplace,
            url: None,
            source: Some("github:anthropic/claude-plugins".to_string()),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_claude_marketplace_missing_source() {
        let config = RegistryConfig {
            r#type: RegistryType::ClaudeMarketplace,
            url: None,
            source: None,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_claude_marketplace_invalid_source() {
        let config = RegistryConfig {
            r#type: RegistryType::ClaudeMarketplace,
            url: None,
            source: Some("invalid:source".to_string()),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_merge_configs() {
        let mut base = RegistryConfig {
            r#type: RegistryType::Sift,
            url: Some(Url::parse("https://base.example.com").unwrap()),
            source: None,
        };

        let overlay = RegistryConfig {
            r#type: RegistryType::Sift,
            url: Some(Url::parse("https://overlay.example.com").unwrap()),
            source: None,
        };

        base.merge(overlay);

        assert_eq!(
            base.url,
            Some(Url::parse("https://overlay.example.com").unwrap())
        );
    }

    #[test]
    fn test_display_name() {
        let sift_config = RegistryConfig {
            r#type: RegistryType::Sift,
            url: Some(Url::parse("https://registry.example.com").unwrap()),
            source: None,
        };
        assert_eq!(sift_config.display_name(), "https://registry.example.com/");

        let claude_config = RegistryConfig {
            r#type: RegistryType::ClaudeMarketplace,
            url: None,
            source: Some("github:anthropic/plugins".to_string()),
        };
        assert_eq!(
            claude_config.display_name(),
            "Claude Marketplace (anthropic/plugins)"
        );
    }
}
