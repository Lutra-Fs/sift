//! Client configuration file management with ownership tracking.
//!
//! This module handles reading and writing third-party client configuration files
//! (e.g., `~/.codex/config.toml`, `claude_desktop_config.json`) with support for
//! multiple formats (JSON, TOML) and ownership-based merge semantics.

mod json;
mod toml;

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::config::ownership::{hash_json, merge_owned_map};
use crate::lockfile::LockfileService;

pub use json::JsonSerializer;
pub use toml::TomlSerializer;

/// Configuration file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Json,
    Toml,
}

impl From<crate::client::McpConfigFormat> for ConfigFormat {
    fn from(format: crate::client::McpConfigFormat) -> Self {
        match format {
            crate::client::McpConfigFormat::Toml => ConfigFormat::Toml,
            // All other formats are JSON-based
            crate::client::McpConfigFormat::ClaudeDesktop
            | crate::client::McpConfigFormat::ClaudeCode
            | crate::client::McpConfigFormat::Generic => ConfigFormat::Json,
        }
    }
}

/// Trait for serializing/deserializing client configuration files.
///
/// All implementations normalize to `serde_json::Map<String, Value>` as the
/// intermediate representation, allowing format-agnostic ownership tracking.
pub trait ConfigSerializer: Send + Sync {
    /// Load a configuration file and return its contents as a JSON-compatible map.
    ///
    /// Returns an empty map if the file does not exist.
    fn load(&self, path: &Path) -> Result<Map<String, Value>>;

    /// Save a JSON-compatible map to the configuration file.
    ///
    /// Creates parent directories if they don't exist.
    fn save(&self, path: &Path, map: &Map<String, Value>) -> Result<()>;

    /// Get the format this serializer handles.
    fn format(&self) -> ConfigFormat;
}

/// Result of applying managed entries to a client config file.
#[derive(Debug)]
pub struct ManagedConfigResult {
    /// The merged configuration map.
    pub merged: Map<String, Value>,
    /// Updated ownership hashes for the managed entries.
    pub ownership: HashMap<String, String>,
}

/// Create a serializer for the given format.
pub fn serializer_for_format(format: ConfigFormat) -> Box<dyn ConfigSerializer> {
    match format {
        ConfigFormat::Json => Box::new(JsonSerializer),
        ConfigFormat::Toml => Box::new(TomlSerializer),
    }
}

/// Read a map at a nested path from a config file, using the specified format.
pub fn read_map_at_path(
    config_path: &Path,
    path: &[&str],
    format: ConfigFormat,
) -> Result<Map<String, Value>> {
    if path.is_empty() {
        anyhow::bail!("Path for reading entries cannot be empty");
    }
    if !config_path.exists() {
        return Ok(Map::new());
    }
    let serializer = serializer_for_format(format);
    let root = serializer.load(config_path)?;
    extract_map_at_path(&root, path)
}

/// Apply managed entries to a config file at a nested path, using the specified format.
///
/// This is the format-aware version of `managed_json::apply_managed_entries_in_path`.
pub fn apply_managed_entries_in_path(
    config_path: &Path,
    path: &[&str],
    desired: &Map<String, Value>,
    lockfile_service: &LockfileService,
    force: bool,
    format: ConfigFormat,
) -> Result<ManagedConfigResult> {
    if path.is_empty() {
        anyhow::bail!("Path for managed entries cannot be empty");
    }

    let serializer = serializer_for_format(format);

    let mut root = serializer
        .load(config_path)
        .with_context(|| format!("Failed to load config: {}", config_path.display()))?;
    let existing = extract_map_at_path(&root, path)?;

    let field_key = path.join(".");
    let ownership = lockfile_service.load_ownership(config_path, Some(&field_key))?;
    let merged_field = merge_owned_map(&existing, desired, &ownership, force)?;

    set_map_at_path(&mut root, path, merged_field.clone())?;

    let mut updated_ownership = HashMap::new();
    for (key, value) in desired {
        updated_ownership.insert(key.clone(), hash_json(value));
    }

    serializer
        .save(config_path, &root)
        .with_context(|| format!("Failed to save config: {}", config_path.display()))?;
    lockfile_service.save_ownership(config_path, Some(&field_key), &updated_ownership)?;

    Ok(ManagedConfigResult {
        merged: root,
        ownership: updated_ownership,
    })
}

/// Extract a nested map from a root map at the given path.
fn extract_map_at_path(root: &Map<String, Value>, path: &[&str]) -> Result<Map<String, Value>> {
    let mut current = root;
    for (idx, segment) in path.iter().enumerate() {
        let value = match current.get(*segment) {
            Some(value) => value,
            None => return Ok(Map::new()),
        };
        if idx == path.len() - 1 {
            return match value {
                Value::Object(map) => Ok(map.clone()),
                _ => anyhow::bail!("Expected '{}' to be an object", segment),
            };
        }
        match value {
            Value::Object(map) => current = map,
            _ => anyhow::bail!("Expected '{}' to be an object", segment),
        }
    }
    Ok(Map::new())
}

/// Set a map at a nested path within a root map.
fn set_map_at_path(
    root: &mut Map<String, Value>,
    path: &[&str],
    map: Map<String, Value>,
) -> Result<()> {
    let mut current = root;
    for (idx, segment) in path.iter().enumerate() {
        if idx == path.len() - 1 {
            current.insert(segment.to_string(), Value::Object(map));
            return Ok(());
        }
        if !current.contains_key(*segment) {
            current.insert(segment.to_string(), Value::Object(Map::new()));
        }
        // Safe to unwrap: we just inserted if missing
        let next = current.get_mut(*segment).unwrap();
        match next {
            Value::Object(m) => current = m,
            _ => anyhow::bail!("Expected '{}' to be an object", segment),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    // ===========================================
    // JSON Serializer Tests
    // ===========================================

    #[test]
    fn json_load_nonexistent_file_returns_empty_map() {
        let serializer = JsonSerializer;
        let path = Path::new("/nonexistent/path/config.json");

        let result = serializer.load(path).expect("load should succeed");

        assert!(result.is_empty());
    }

    #[test]
    fn json_load_existing_file_parses_correctly() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("config.json");
        std::fs::write(&path, r#"{"mcpServers": {"test": {"command": "echo"}}}"#)
            .expect("write file");

        let serializer = JsonSerializer;
        let result = serializer.load(&path).expect("load should succeed");

        assert!(result.contains_key("mcpServers"));
        let servers = result.get("mcpServers").unwrap().as_object().unwrap();
        assert!(servers.contains_key("test"));
    }

    #[test]
    fn json_save_creates_parent_directories() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("nested/deep/config.json");

        let serializer = JsonSerializer;
        let mut map = Map::new();
        map.insert("key".to_string(), json!("value"));

        serializer.save(&path, &map).expect("save should succeed");

        assert!(path.exists());
        let content = std::fs::read_to_string(&path).expect("read file");
        assert!(content.contains("key"));
    }

    #[test]
    fn json_roundtrip_preserves_data() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("config.json");

        let serializer = JsonSerializer;
        let mut original = Map::new();
        original.insert("string".to_string(), json!("value"));
        original.insert("number".to_string(), json!(42));
        original.insert("array".to_string(), json!(["a", "b", "c"]));
        original.insert("nested".to_string(), json!({"inner": {"deep": true}}));

        serializer.save(&path, &original).expect("save");
        let loaded = serializer.load(&path).expect("load");

        assert_eq!(original, loaded);
    }

    #[test]
    fn json_format_returns_json() {
        let serializer = JsonSerializer;
        assert_eq!(serializer.format(), ConfigFormat::Json);
    }

    // ===========================================
    // TOML Serializer Tests
    // ===========================================

    #[test]
    fn toml_load_nonexistent_file_returns_empty_map() {
        let serializer = TomlSerializer;
        let path = Path::new("/nonexistent/path/config.toml");

        let result = serializer.load(path).expect("load should succeed");

        assert!(result.is_empty());
    }

    #[test]
    fn toml_load_existing_file_parses_correctly() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[mcp_servers.test]
command = "echo"
args = ["hello"]
"#,
        )
        .expect("write file");

        let serializer = TomlSerializer;
        let result = serializer.load(&path).expect("load should succeed");

        assert!(result.contains_key("mcp_servers"));
        let servers = result.get("mcp_servers").unwrap().as_object().unwrap();
        assert!(servers.contains_key("test"));
    }

    #[test]
    fn toml_save_creates_parent_directories() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("nested/deep/config.toml");

        let serializer = TomlSerializer;
        let mut map = Map::new();
        map.insert("key".to_string(), json!("value"));

        serializer.save(&path, &map).expect("save should succeed");

        assert!(path.exists());
        let content = std::fs::read_to_string(&path).expect("read file");
        assert!(content.contains("key"));
    }

    #[test]
    fn toml_roundtrip_preserves_data() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("config.toml");

        let serializer = TomlSerializer;
        let mut original = Map::new();
        original.insert("string".to_string(), json!("value"));
        original.insert("number".to_string(), json!(42));
        original.insert("array".to_string(), json!(["a", "b", "c"]));
        original.insert("nested".to_string(), json!({"inner": {"deep": true}}));

        serializer.save(&path, &original).expect("save");
        let loaded = serializer.load(&path).expect("load");

        assert_eq!(original, loaded);
    }

    #[test]
    fn toml_format_returns_toml() {
        let serializer = TomlSerializer;
        assert_eq!(serializer.format(), ConfigFormat::Toml);
    }

    #[test]
    fn toml_handles_codex_mcp_structure() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let path = temp_dir.path().join("config.toml");

        let serializer = TomlSerializer;

        // Build Codex-style MCP structure
        let mut map = Map::new();
        let mut mcp_servers = Map::new();

        // STDIO server
        let mut stdio_server = Map::new();
        stdio_server.insert("command".to_string(), json!("npx"));
        stdio_server.insert("args".to_string(), json!(["-y", "@upstash/context7-mcp"]));
        let mut env = Map::new();
        env.insert("MY_VAR".to_string(), json!("MY_VALUE"));
        stdio_server.insert("env".to_string(), Value::Object(env));
        mcp_servers.insert("context7".to_string(), Value::Object(stdio_server));

        // HTTP server
        let mut http_server = Map::new();
        http_server.insert("url".to_string(), json!("https://mcp.figma.com/mcp"));
        let mut headers = Map::new();
        headers.insert("X-Figma-Region".to_string(), json!("us-east-1"));
        http_server.insert("http_headers".to_string(), Value::Object(headers));
        mcp_servers.insert("figma".to_string(), Value::Object(http_server));

        map.insert("mcp_servers".to_string(), Value::Object(mcp_servers));

        serializer.save(&path, &map).expect("save");

        // Verify TOML structure
        let content = std::fs::read_to_string(&path).expect("read file");
        assert!(content.contains("[mcp_servers.context7]") || content.contains("[mcp_servers]"));
        assert!(content.contains("command"));
        assert!(content.contains("npx"));

        // Verify roundtrip
        let loaded = serializer.load(&path).expect("load");
        assert_eq!(map, loaded);
    }

    // ===========================================
    // Factory Tests
    // ===========================================

    #[test]
    fn serializer_for_format_returns_correct_type() {
        let json_serializer = serializer_for_format(ConfigFormat::Json);
        assert_eq!(json_serializer.format(), ConfigFormat::Json);

        let toml_serializer = serializer_for_format(ConfigFormat::Toml);
        assert_eq!(toml_serializer.format(), ConfigFormat::Toml);
    }
}
