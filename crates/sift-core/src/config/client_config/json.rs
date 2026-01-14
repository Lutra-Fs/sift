//! JSON serializer for client configuration files.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::{ConfigFormat, ConfigSerializer};

/// JSON configuration file serializer.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonSerializer;

impl ConfigSerializer for JsonSerializer {
    fn load(&self, path: &Path) -> Result<Map<String, Value>> {
        if !path.exists() {
            return Ok(Map::new());
        }
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let value: Value =
            serde_json::from_slice(&bytes).with_context(|| "Failed to parse JSON config")?;
        match value {
            Value::Object(map) => Ok(map),
            _ => anyhow::bail!("Expected JSON object at root: {}", path.display()),
        }
    }

    fn save(&self, path: &Path, map: &Map<String, Value>) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }
        let bytes = serde_json::to_vec_pretty(map).context("Failed to serialize JSON config")?;
        std::fs::write(path, bytes)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }

    fn format(&self) -> ConfigFormat {
        ConfigFormat::Json
    }
}
