//! Skill configuration schema
//!
//! Extends existing Skill with source-based configuration

use serde::{Deserialize, Serialize};

/// Complete skill configuration from sift.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillConfig {
    /// Source: "registry:author/skill" or "local:/path/to/skill"
    pub source: String,

    /// Version constraint: semver, git SHA, or "latest"
    #[serde(default = "default_version")]
    pub version: String,

    /// Target control (whitelist)
    #[serde(default)]
    pub targets: Option<Vec<String>>,

    /// Ignore targets (blacklist)
    #[serde(default)]
    pub ignore_targets: Option<Vec<String>>,
}

fn default_version() -> String {
    "latest".to_string()
}

/// Override configuration for project-local skill settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillConfigOverride {
    /// Override version
    pub version: Option<String>,
}

impl SkillConfig {
    /// Merge another SkillConfig into this one
    pub fn merge(&mut self, other: SkillConfig) {
        if other.version != "latest" {
            self.version = other.version;
        }
        if other.targets.is_some() {
            self.targets = other.targets;
        }
        if other.ignore_targets.is_some() {
            self.ignore_targets = other.ignore_targets;
        }
    }

    /// Apply project-local override
    pub fn apply_override(&mut self, override_config: &SkillConfigOverride) {
        if let Some(version) = &override_config.version {
            self.version = version.clone();
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.targets.is_some() && self.ignore_targets.is_some() {
            anyhow::bail!("Cannot specify both 'targets' and 'ignore_targets'");
        }

        // Validate source format
        if !self.source.starts_with("registry:")
            && !self.source.starts_with("local:")
            && !self.source.starts_with("github:")
            && !self.source.starts_with("git:")
        {
            anyhow::bail!(
                "Invalid source format: must be 'registry:author/skill', 'local:/path', 'github:org/repo', or 'git:url'"
            );
        }

        Ok(())
    }

    /// Check if this skill should be deployed to a provider
    pub fn should_deploy_to(&self, provider_id: &str) -> bool {
        if let Some(ref targets) = self.targets {
            return targets.contains(&provider_id.to_string());
        }
        if let Some(ref ignore) = self.ignore_targets {
            return !ignore.contains(&provider_id.to_string());
        }
        true
    }

    /// Convert to legacy Skill for backward compatibility
    pub fn to_legacy(&self, id: String) -> super::Skill {
        super::Skill {
            id,
            name: self.source.clone(),
            description: format!("{}@{}", self.source, self.version),
            source: self.source.clone(),
        }
    }

    /// Get the registry name from the source (e.g., "anthropic/pdf" from "registry:anthropic/pdf")
    pub fn registry_name(&self) -> Option<&str> {
        self.source.strip_prefix("registry:")
    }

    /// Check if this is a registry-based source
    pub fn is_registry(&self) -> bool {
        self.source.starts_with("registry:")
    }

    /// Check if this is a local source
    pub fn is_local(&self) -> bool {
        self.source.starts_with("local:")
    }

    /// Check if this is a git-based source (github or git)
    pub fn is_git(&self) -> bool {
        self.source.starts_with("github:") || self.source.starts_with("git:")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let config = SkillConfig {
            source: "registry:anthropic/pdf".to_string(),
            version: "^1.0".to_string(),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_both_targets_and_ignore() {
        let config = SkillConfig {
            source: "registry:test".to_string(),
            version: "latest".to_string(),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: Some(vec!["vscode".to_string()]),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_should_deploy_with_targets() {
        let config = SkillConfig {
            source: "registry:test".to_string(),
            version: "latest".to_string(),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
        };
        assert!(config.should_deploy_to("claude-code"));
        assert!(!config.should_deploy_to("vscode"));
    }

    #[test]
    fn test_registry_name() {
        let config = SkillConfig {
            source: "registry:anthropic/pdf".to_string(),
            version: "latest".to_string(),
            targets: None,
            ignore_targets: None,
        };
        assert_eq!(config.registry_name(), Some("anthropic/pdf"));
    }

    #[test]
    fn test_source_type_checks() {
        let registry = SkillConfig {
            source: "registry:test/skill".to_string(),
            version: "latest".to_string(),
            targets: None,
            ignore_targets: None,
        };
        assert!(registry.is_registry());
        assert!(!registry.is_local());
        assert!(!registry.is_git());

        let local = SkillConfig {
            source: "local:/path/to/skill".to_string(),
            version: "latest".to_string(),
            targets: None,
            ignore_targets: None,
        };
        assert!(!local.is_registry());
        assert!(local.is_local());
        assert!(!local.is_git());

        let github = SkillConfig {
            source: "github:org/repo".to_string(),
            version: "latest".to_string(),
            targets: None,
            ignore_targets: None,
        };
        assert!(!github.is_registry());
        assert!(!github.is_local());
        assert!(github.is_git());
    }

    #[test]
    fn test_merge_configs() {
        let mut base = SkillConfig {
            source: "registry:test".to_string(),
            version: "^1.0".to_string(),
            targets: None,
            ignore_targets: None,
        };

        let overlay = SkillConfig {
            source: "registry:test".to_string(),
            version: "^2.0".to_string(),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
        };

        base.merge(overlay);

        assert_eq!(base.version, "^2.0");
        assert!(base.targets.is_some());
    }

    #[test]
    fn test_apply_override() {
        let mut config = SkillConfig {
            source: "registry:test".to_string(),
            version: "^1.0".to_string(),
            targets: None,
            ignore_targets: None,
        };

        let override_config = SkillConfigOverride {
            version: Some("main".to_string()),
        };

        config.apply_override(&override_config);

        assert_eq!(config.version, "main");
    }
}
