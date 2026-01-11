//! Configuration management for different scopes
//!
//! Supports three configuration scopes:
//! - Global: System-wide configuration
//! - PerProjectLocal: Project-specific, not shared
//! - PerProjectShared: Project-specific, shared across team

pub mod managed_json;
pub mod merge;
pub mod ownership;
pub mod ownership_store;
pub mod parser;
pub mod paths;
pub mod schema;
pub mod store;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export the new module types
pub use managed_json::ManagedJsonResult;
pub use merge::merge_configs;
pub use ownership_store::OwnershipStore;
pub use parser::{parse_sift_toml, parse_sift_toml_str, to_toml};
pub use paths::config_path_for_scope;
pub use schema::{
    ClientConfigEntry, McpConfigEntry, McpOverrideEntry, ProjectConfig, SiftConfig,
    SkillConfigEntry, SkillOverrideEntry,
};
pub use store::ConfigStore;

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
    config_path: PathBuf,
}

impl ConfigManager {
    /// Create a new config manager for the given scope
    pub fn new(scope: ConfigScope) -> anyhow::Result<Self> {
        let config_path = Self::resolve_config_path(scope)?;
        Ok(Self { scope, config_path })
    }

    /// Resolve the configuration path for the given scope
    fn resolve_config_path(scope: ConfigScope) -> anyhow::Result<PathBuf> {
        let global_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("sift");
        let project_root = std::env::current_dir()?;

        Ok(paths::config_path_for_scope(
            scope,
            &global_dir,
            &project_root,
        ))
    }

    /// Get the current scope
    pub fn scope(&self) -> ConfigScope {
        self.scope
    }

    /// Get the configuration directory
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }
}
