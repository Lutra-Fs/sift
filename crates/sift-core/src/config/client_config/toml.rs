//! TOML serializer for client configuration files.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use super::{ConfigFormat, ConfigSerializer};

/// TOML configuration file serializer.
#[derive(Debug, Default, Clone, Copy)]
pub struct TomlSerializer;

impl ConfigSerializer for TomlSerializer {
    fn load(&self, path: &Path) -> Result<Map<String, Value>> {
        if !path.exists() {
            return Ok(Map::new());
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let toml_value: toml::Value =
            toml::from_str(&content).with_context(|| "Failed to parse TOML config")?;

        toml_to_json_map(toml_value)
    }

    fn save(&self, path: &Path, map: &Map<String, Value>) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let toml_value = json_map_to_toml(map)?;
        let content =
            toml::to_string_pretty(&toml_value).context("Failed to serialize TOML config")?;

        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }

    fn format(&self) -> ConfigFormat {
        ConfigFormat::Toml
    }
}

/// Convert a TOML value to a JSON-compatible map.
fn toml_to_json_map(toml_value: toml::Value) -> Result<Map<String, Value>> {
    match toml_value {
        toml::Value::Table(table) => {
            let mut map = Map::new();
            for (key, value) in table {
                map.insert(key, toml_to_json_value(value));
            }
            Ok(map)
        }
        _ => anyhow::bail!("Expected TOML table at root"),
    }
}

/// Convert a single TOML value to a JSON value.
fn toml_to_json_value(toml_value: toml::Value) -> Value {
    match toml_value {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::Number(i.into()),
        toml::Value::Float(f) => {
            // serde_json::Number doesn't support NaN/Infinity, fall back to string
            serde_json::Number::from_f64(f)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(f.to_string()))
        }
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::String(dt.to_string()),
        toml::Value::Array(arr) => Value::Array(arr.into_iter().map(toml_to_json_value).collect()),
        toml::Value::Table(table) => {
            let mut map = Map::new();
            for (key, value) in table {
                map.insert(key, toml_to_json_value(value));
            }
            Value::Object(map)
        }
    }
}

/// Convert a JSON-compatible map to a TOML value.
fn json_map_to_toml(map: &Map<String, Value>) -> Result<toml::Value> {
    let mut table = toml::map::Map::new();
    for (key, value) in map {
        table.insert(key.clone(), json_to_toml_value(value)?);
    }
    Ok(toml::Value::Table(table))
}

/// Convert a single JSON value to a TOML value.
fn json_to_toml_value(json_value: &Value) -> Result<toml::Value> {
    match json_value {
        Value::Null => {
            // TOML doesn't have null; we skip or use empty string
            Ok(toml::Value::String(String::new()))
        }
        Value::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(toml::Value::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(toml::Value::Float(f))
            } else {
                anyhow::bail!("Unsupported number type")
            }
        }
        Value::String(s) => Ok(toml::Value::String(s.clone())),
        Value::Array(arr) => {
            let toml_arr: Result<Vec<_>> = arr.iter().map(json_to_toml_value).collect();
            Ok(toml::Value::Array(toml_arr?))
        }
        Value::Object(obj) => {
            let mut table = toml::map::Map::new();
            for (key, value) in obj {
                table.insert(key.clone(), json_to_toml_value(value)?);
            }
            Ok(toml::Value::Table(table))
        }
    }
}
