//! Configuration management for different scopes
//!
//! Supports three configuration scopes:
//! - Global: System-wide configuration
//! - PerProjectLocal: Project-specific, not shared
//! - PerProjectShared: Project-specific, shared across team

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration scope levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfigScope {
    /// Global/system-wide configuration
    Global,
    /// Per-project, local (not shared)
    PerProjectLocal,
    /// Per-project, shared (e.g., checked into version control)
    PerProjectShared,
}

/// Configuration manager
#[derive(Debug, Clone)]
pub struct ConfigManager {
    scope: ConfigScope,
    config_dir: PathBuf,
}

impl ConfigManager {
    /// Create a new config manager for the given scope
    pub fn new(scope: ConfigScope) -> anyhow::Result<Self> {
        let config_dir = Self::resolve_config_dir(scope)?;
        Ok(Self { scope, config_dir })
    }

    /// Resolve the configuration directory for the given scope
    fn resolve_config_dir(scope: ConfigScope) -> anyhow::Result<PathBuf> {
        match scope {
            ConfigScope::Global => {
                let base = dirs::config_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
                Ok(base.join("sift"))
            }
            ConfigScope::PerProjectLocal => {
                // Detect project root via .git
                let cwd = std::env::current_dir()?;
                Ok(cwd.join(".sift"))
            }
            ConfigScope::PerProjectShared => {
                // Detect project root via .git, use shared location
                let cwd = std::env::current_dir()?;
                Ok(cwd.join("sift.shared"))
            }
        }
    }

    /// Get the current scope
    pub fn scope(&self) -> ConfigScope {
        self.scope
    }

    /// Get the configuration directory
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }
}
