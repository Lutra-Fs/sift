//! Config store for loading and saving sift.toml.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::types::ConfigScope;

use super::{SiftConfig, parser, paths::config_path_for_scope};

#[derive(Debug, Clone)]
pub struct ConfigStore {
    scope: ConfigScope,
    config_path: PathBuf,
    project_root: PathBuf,
}

impl ConfigStore {
    pub fn from_scope(scope: ConfigScope) -> anyhow::Result<Self> {
        let global_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("sift");
        let project_root = std::env::current_dir()?;

        Ok(Self::from_paths(scope, global_dir, project_root))
    }

    pub fn from_paths(scope: ConfigScope, global_dir: PathBuf, project_root: PathBuf) -> Self {
        let config_path = config_path_for_scope(scope, &global_dir, &project_root);
        Self {
            scope,
            config_path,
            project_root,
        }
    }

    pub fn scope(&self) -> ConfigScope {
        self.scope
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn load(&self) -> anyhow::Result<SiftConfig> {
        if !self.config_path.exists() {
            return Ok(SiftConfig::new());
        }
        parser::parse_sift_toml(&self.config_path)
    }

    pub fn save(&self, config: &SiftConfig) -> anyhow::Result<()> {
        let content = parser::to_toml(config).context("Failed to serialize config to TOML")?;
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }
        std::fs::write(&self.config_path, content).with_context(|| {
            format!(
                "Failed to write config file: {}",
                self.config_path.display()
            )
        })?;
        Ok(())
    }
}
