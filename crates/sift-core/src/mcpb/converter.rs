//! MCPB Manifest to MCP Server conversion
//!
//! Converts MCPB bundle manifest into McpResolvedServer specs
//! suitable for client configuration generation.

use std::collections::HashMap;
use std::path::Path;

use crate::mcp::spec::McpResolvedServer;

use super::{McpbManifest, McpbMcpConfig, McpbServerType};

/// Convert an MCPB manifest into an McpResolvedServer.
///
/// The `extract_dir` is where the bundle was extracted, used for
/// substituting `${__dirname}` in command/args.
///
/// Platform-specific overrides are applied based on the current OS.
pub fn manifest_to_server(
    name: &str,
    manifest: &McpbManifest,
    extract_dir: &Path,
) -> anyhow::Result<McpResolvedServer> {
    let server = &manifest.server;

    // Get the mcp_config, or derive one from server type and entry_point
    let mcp_config = match &server.mcp_config {
        Some(config) => config.clone(),
        None => derive_mcp_config(
            server.server_type,
            server.entry_point.as_deref(),
            extract_dir,
        )?,
    };

    // Apply platform-specific overrides
    let effective_config = apply_platform_overrides(&mcp_config);

    // Substitute ${__dirname} placeholders
    let command = substitute_dirname(&effective_config.command, extract_dir);
    let args: Vec<String> = effective_config
        .args
        .iter()
        .map(|arg| substitute_dirname(arg, extract_dir))
        .collect();

    // Merge environment variables
    let mut env = effective_config.env.clone();

    // Add any user_config defaults that should be in env
    for (key, user_config) in &manifest.user_config {
        if let Some(default) = &user_config.default {
            // Only include string defaults in env (other types need special handling)
            if let Some(s) = default.as_str() {
                let env_key = key.to_uppercase();
                env.entry(env_key).or_insert_with(|| s.to_string());
            }
        }
    }

    Ok(McpResolvedServer::stdio(
        name.to_string(),
        command,
        args,
        env,
    ))
}

/// Derive mcp_config from server type and entry point when not explicitly provided.
fn derive_mcp_config(
    server_type: McpbServerType,
    entry_point: Option<&str>,
    extract_dir: &Path,
) -> anyhow::Result<McpbMcpConfig> {
    let entry = entry_point.ok_or_else(|| {
        anyhow::anyhow!(
            "MCPB manifest must specify either mcp_config or entry_point for {} server",
            server_type
        )
    })?;

    let (command, args) = match server_type {
        McpbServerType::Node => {
            let full_path = extract_dir.join(entry);
            ("node".to_string(), vec![full_path.display().to_string()])
        }
        McpbServerType::Python => {
            let full_path = extract_dir.join(entry);
            ("python".to_string(), vec![full_path.display().to_string()])
        }
        McpbServerType::Uv => {
            let full_path = extract_dir.join(entry);
            (
                "uv".to_string(),
                vec!["run".to_string(), full_path.display().to_string()],
            )
        }
        McpbServerType::Binary => {
            let full_path = extract_dir.join(entry);
            (full_path.display().to_string(), Vec::new())
        }
    };

    Ok(McpbMcpConfig {
        command,
        args,
        env: HashMap::new(),
        platforms: HashMap::new(),
    })
}

/// Apply platform-specific overrides based on current OS.
fn apply_platform_overrides(config: &McpbMcpConfig) -> McpbMcpConfig {
    let platform_key = current_platform();

    let Some(platform_override) = config.platforms.get(platform_key) else {
        return config.clone();
    };

    let mut result = config.clone();

    if let Some(cmd) = &platform_override.command {
        result.command = cmd.clone();
    }

    if let Some(args) = &platform_override.args {
        result.args = args.clone();
    }

    if let Some(env) = &platform_override.env {
        result.env.extend(env.clone());
    }

    result
}

/// Get the current platform key for mcp_config.platforms lookup.
fn current_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "win32"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "unknown"
    }
}

/// Substitute ${__dirname} placeholder with actual extract directory.
fn substitute_dirname(value: &str, extract_dir: &Path) -> String {
    value.replace("${__dirname}", &extract_dir.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcpb::McpbManifest;
    use std::path::PathBuf;

    fn parse_manifest(json: &str) -> McpbManifest {
        McpbManifest::from_json(json).expect("Valid manifest JSON")
    }

    // =========================================================================
    // Basic Conversion Tests
    // =========================================================================

    #[test]
    fn convert_node_server_with_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "test-server",
            "version": "1.0.0",
            "description": "Test",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "dist/index.js"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let server =
            manifest_to_server("test-server", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(server.name, "test-server");
        assert_eq!(server.command, Some("node".to_string()));
        assert_eq!(server.args, vec!["/cache/bundles/abc123/dist/index.js"]);
    }

    #[test]
    fn convert_python_server_with_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "py-server",
            "version": "1.0.0",
            "description": "Python MCP",
            "author": { "name": "Test" },
            "server": {
                "type": "python",
                "entry_point": "src/main.py"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/tmp/extracted");
        let server =
            manifest_to_server("py-server", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(server.command, Some("python".to_string()));
        assert_eq!(server.args, vec!["/tmp/extracted/src/main.py"]);
    }

    #[test]
    fn convert_binary_server_with_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "bin-server",
            "version": "1.0.0",
            "description": "Binary MCP",
            "author": { "name": "Test" },
            "server": {
                "type": "binary",
                "entry_point": "bin/server"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/opt/mcpb");
        let server =
            manifest_to_server("bin-server", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(server.command, Some("/opt/mcpb/bin/server".to_string()));
        assert!(server.args.is_empty());
    }

    #[test]
    fn convert_uv_server_with_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "uv-server",
            "version": "1.0.0",
            "description": "UV-managed Python",
            "author": { "name": "Test" },
            "server": {
                "type": "uv",
                "entry_point": "server/main.py"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache/uv-bundle");
        let server =
            manifest_to_server("uv-server", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(server.command, Some("uv".to_string()));
        assert_eq!(server.args, vec!["run", "/cache/uv-bundle/server/main.py"]);
    }

    // =========================================================================
    // Explicit mcp_config Tests
    // =========================================================================

    #[test]
    fn convert_with_explicit_mcp_config() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "custom-server",
            "version": "1.0.0",
            "description": "Custom config",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "mcp_config": {
                    "command": "${__dirname}/node_modules/.bin/server",
                    "args": ["--port", "3000"],
                    "env": { "NODE_ENV": "production" }
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/bundle");
        let server =
            manifest_to_server("custom-server", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(
            server.command,
            Some("/bundle/node_modules/.bin/server".to_string())
        );
        assert_eq!(server.args, vec!["--port", "3000"]);
        assert_eq!(server.env.get("NODE_ENV"), Some(&"production".to_string()));
    }

    #[test]
    fn convert_with_dirname_in_args() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "dirname-test",
            "version": "1.0.0",
            "description": "Dirname substitution",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "mcp_config": {
                    "command": "node",
                    "args": ["${__dirname}/dist/index.js", "--config", "${__dirname}/config.json"]
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/opt/server");
        let server =
            manifest_to_server("dirname-test", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(
            server.args,
            vec![
                "/opt/server/dist/index.js",
                "--config",
                "/opt/server/config.json"
            ]
        );
    }

    // =========================================================================
    // User Config Defaults Tests
    // =========================================================================

    #[test]
    fn convert_includes_user_config_defaults_in_env() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "config-test",
            "version": "1.0.0",
            "description": "User config defaults",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "index.js"
            },
            "user_config": {
                "api_key": { "type": "string", "title": "API Key", "required": true },
                "workspace": {
                    "type": "directory",
                    "title": "Workspace",
                    "default": "${HOME}/Documents"
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache");
        let server =
            manifest_to_server("config-test", &manifest, &extract_dir).expect("Should convert");

        // Only the one with a default should appear in env
        assert_eq!(
            server.env.get("WORKSPACE"),
            Some(&"${HOME}/Documents".to_string())
        );
        assert!(!server.env.contains_key("API_KEY"));
    }

    // =========================================================================
    // Error Cases
    // =========================================================================

    #[test]
    fn convert_fails_without_entry_point_or_mcp_config() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "missing-entry",
            "version": "1.0.0",
            "description": "No entry point",
            "author": { "name": "Test" },
            "server": {
                "type": "node"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache");
        let result = manifest_to_server("missing-entry", &manifest, &extract_dir);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("entry_point") || err.contains("mcp_config"));
    }

    // =========================================================================
    // Platform Override Tests (conditional on current platform)
    // =========================================================================

    #[test]
    #[cfg(target_os = "macos")]
    fn convert_applies_darwin_platform_override() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "platform-test",
            "version": "1.0.0",
            "description": "Platform overrides",
            "author": { "name": "Test" },
            "server": {
                "type": "binary",
                "mcp_config": {
                    "command": "${__dirname}/bin/server",
                    "platforms": {
                        "darwin": {
                            "command": "${__dirname}/bin/server-macos"
                        },
                        "win32": {
                            "command": "${__dirname}/bin/server.exe"
                        }
                    }
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/bundle");
        let server =
            manifest_to_server("platform-test", &manifest, &extract_dir).expect("Should convert");

        assert_eq!(server.command, Some("/bundle/bin/server-macos".to_string()));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn convert_uses_default_on_linux() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "platform-test",
            "version": "1.0.0",
            "description": "Platform overrides",
            "author": { "name": "Test" },
            "server": {
                "type": "binary",
                "mcp_config": {
                    "command": "${__dirname}/bin/server-linux",
                    "platforms": {
                        "darwin": {
                            "command": "${__dirname}/bin/server-macos"
                        },
                        "win32": {
                            "command": "${__dirname}/bin/server.exe"
                        }
                    }
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/bundle");
        let server =
            manifest_to_server("platform-test", &manifest, &extract_dir).expect("Should convert");

        // Linux has no override, so uses default command
        assert_eq!(server.command, Some("/bundle/bin/server-linux".to_string()));
    }
}
