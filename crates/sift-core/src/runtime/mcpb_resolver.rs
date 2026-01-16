//! MCPB runtime resolution
//!
//! Converts MCPB bundle manifest configuration into RunnerSpec.
//! This bridges MCPB's execution format to Sift's runtime system.

use std::collections::HashMap;
use std::path::Path;

use crate::mcpb::security::validate_entry_point;
use crate::mcpb::{McpbManifest, McpbMcpConfig, McpbServerType};

use super::{
    PythonExecutor, PythonRuntimeRequest, RunnerSpec, ShellRuntimeRequest, resolve_python,
    resolve_shell,
};

/// Request to resolve an MCPB bundle into a RunnerSpec
#[derive(Debug, Clone)]
pub struct McpbRuntimeRequest<'a> {
    /// The MCPB manifest
    pub manifest: &'a McpbManifest,
    /// Directory where the bundle was extracted
    pub extract_dir: &'a Path,
    /// Additional environment variables to include
    pub extra_env: HashMap<String, String>,
}

/// Resolve an MCPB bundle into a RunnerSpec
///
/// This function reads the MCPB manifest and generates the appropriate
/// RunnerSpec based on the server type and configuration.
///
/// # Mapping
///
/// | MCPB server.type | Resolution |
/// |------------------|------------|
/// | `node`           | Uses `mcp_config.command/args` or derives `node <entry_point>` |
/// | `python`         | Uses Python executor with `python <entry_point>` |
/// | `uv`             | Uses Python executor with `uv run <entry_point>` |
/// | `binary`         | Uses Shell runtime with direct execution |
pub fn resolve_mcpb(request: &McpbRuntimeRequest) -> anyhow::Result<RunnerSpec> {
    let manifest = request.manifest;
    let server = &manifest.server;
    let extract_dir = request.extract_dir;

    // If mcp_config is provided, use it directly with variable substitution
    if let Some(mcp_config) = &server.mcp_config {
        return resolve_from_mcp_config(mcp_config, extract_dir, &request.extra_env);
    }

    // Otherwise, derive from server type and entry point
    let entry_point = server.entry_point.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "MCPB manifest for '{}' must specify either mcp_config or entry_point",
            manifest.name
        )
    })?;

    let full_entry_path = validate_entry_point(entry_point, extract_dir, &manifest.name)?;

    match server.server_type {
        McpbServerType::Node => Ok(RunnerSpec {
            command: "node".to_string(),
            args: vec![full_entry_path.to_string_lossy().to_string()],
            env: request.extra_env.clone(),
        }),
        McpbServerType::Python => {
            let python_request = PythonRuntimeRequest {
                executor: PythonExecutor::Python,
                entry_point: full_entry_path,
                extra_args: vec![],
                env: request.extra_env.clone(),
            };
            resolve_python(&python_request)
        }
        McpbServerType::Uv => {
            let python_request = PythonRuntimeRequest {
                executor: PythonExecutor::Uv,
                entry_point: full_entry_path,
                extra_args: vec![],
                env: request.extra_env.clone(),
            };
            resolve_python(&python_request)
        }
        McpbServerType::Binary => {
            let shell_request = ShellRuntimeRequest {
                command: full_entry_path.to_string_lossy().to_string(),
                args: vec![],
                env: request.extra_env.clone(),
            };
            Ok(resolve_shell(&shell_request))
        }
    }
}

/// Resolve from explicit mcp_config with ${__dirname} substitution
fn resolve_from_mcp_config(
    mcp_config: &McpbMcpConfig,
    extract_dir: &Path,
    extra_env: &HashMap<String, String>,
) -> anyhow::Result<RunnerSpec> {
    let dirname = extract_dir.to_string_lossy();

    // Apply platform-specific overrides first
    let effective_config = apply_platform_overrides(mcp_config);

    // Substitute ${__dirname} in command and args
    let command = substitute_dirname(&effective_config.command, &dirname);
    let args: Vec<String> = effective_config
        .args
        .iter()
        .map(|arg| substitute_dirname(arg, &dirname))
        .collect();

    // Merge environment variables
    let mut env = effective_config.env.clone();
    for (key, value) in extra_env {
        env.entry(key.clone()).or_insert_with(|| value.clone());
    }

    // Also substitute ${__dirname} in env values
    for value in env.values_mut() {
        *value = substitute_dirname(value, &dirname);
    }

    Ok(RunnerSpec { command, args, env })
}

/// Apply platform-specific overrides based on current OS
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

/// Get the current platform key for MCPB platforms lookup
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

/// Substitute ${__dirname} placeholder with actual directory path
fn substitute_dirname(value: &str, dirname: &str) -> String {
    value.replace("${__dirname}", dirname)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_manifest(json: &str) -> McpbManifest {
        McpbManifest::from_json(json).expect("Valid manifest JSON")
    }

    // =========================================================================
    // Node Server Resolution
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_node_with_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "test-node-server",
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.command, "node");
        assert_eq!(spec.args, vec!["/cache/bundles/abc123/dist/index.js"]);
    }

    #[test]
    fn test_resolve_mcpb_node_with_mcp_config() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "custom-node",
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.command, "/bundle/node_modules/.bin/server");
        assert_eq!(spec.args, vec!["--port", "3000"]);
        assert_eq!(spec.env.get("NODE_ENV"), Some(&"production".to_string()));
    }

    // =========================================================================
    // Python Server Resolution
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_python_with_entry_point() {
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.command, "python");
        assert_eq!(spec.args, vec!["/tmp/extracted/src/main.py"]);
    }

    // =========================================================================
    // UV Server Resolution (Python with uv executor)
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_uv_with_entry_point() {
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.command, "uv");
        assert_eq!(spec.args, vec!["run", "/cache/uv-bundle/server/main.py"]);
    }

    // =========================================================================
    // Binary Server Resolution
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_binary_with_entry_point() {
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.command, "/opt/mcpb/bin/server");
        assert!(spec.args.is_empty());
    }

    // =========================================================================
    // ${__dirname} Substitution Tests
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_substitutes_dirname_in_args() {
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(
            spec.args,
            vec![
                "/opt/server/dist/index.js",
                "--config",
                "/opt/server/config.json"
            ]
        );
    }

    #[test]
    fn test_resolve_mcpb_substitutes_dirname_in_env() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "env-test",
            "version": "1.0.0",
            "description": "Env substitution",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "mcp_config": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {
                        "CONFIG_DIR": "${__dirname}/config",
                        "STATIC_VALUE": "unchanged"
                    }
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/bundle");
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(
            spec.env.get("CONFIG_DIR"),
            Some(&"/bundle/config".to_string())
        );
        assert_eq!(spec.env.get("STATIC_VALUE"), Some(&"unchanged".to_string()));
    }

    // =========================================================================
    // Extra Environment Variables
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_merges_extra_env() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "env-merge",
            "version": "1.0.0",
            "description": "Env merge test",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "mcp_config": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": { "FROM_MANIFEST": "manifest_value" }
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/bundle");
        let mut extra_env = HashMap::new();
        extra_env.insert("USER_API_KEY".to_string(), "secret123".to_string());
        extra_env.insert(
            "FROM_MANIFEST".to_string(),
            "should_not_override".to_string(),
        );

        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env,
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.env.get("USER_API_KEY"), Some(&"secret123".to_string()));
        // Manifest value takes precedence
        assert_eq!(
            spec.env.get("FROM_MANIFEST"),
            Some(&"manifest_value".to_string())
        );
    }

    // =========================================================================
    // Platform Override Tests
    // =========================================================================

    #[test]
    #[cfg(target_os = "macos")]
    fn test_resolve_mcpb_applies_darwin_platform_override() {
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        assert_eq!(spec.command, "/bundle/bin/server-macos");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_resolve_mcpb_uses_default_on_linux() {
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
                        }
                    }
                }
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/bundle");
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Should resolve");

        // Linux has no override, uses default
        assert_eq!(spec.command, "/bundle/bin/server-linux");
    }

    // =========================================================================
    // Path Traversal Security Tests
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_rejects_absolute_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "malicious-absolute",
            "version": "1.0.0",
            "description": "Absolute path attack",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "/bin/sh"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let result = resolve_mcpb(&request);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("absolute") || err.contains("outside"),
            "Error should mention path issue: {}",
            err
        );
    }

    #[test]
    fn test_resolve_mcpb_rejects_path_traversal_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "malicious-traversal",
            "version": "1.0.0",
            "description": "Path traversal attack",
            "author": { "name": "Test" },
            "server": {
                "type": "python",
                "entry_point": "../../../usr/bin/python"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let result = resolve_mcpb(&request);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("outside") || err.contains("traversal"),
            "Error should mention path escape: {}",
            err
        );
    }

    #[test]
    fn test_resolve_mcpb_rejects_hidden_traversal_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "hidden-traversal",
            "version": "1.0.0",
            "description": "Hidden path traversal",
            "author": { "name": "Test" },
            "server": {
                "type": "binary",
                "entry_point": "dist/../../../etc/passwd"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let result = resolve_mcpb(&request);

        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_mcpb_allows_valid_nested_entry_point() {
        let manifest = parse_manifest(
            r#"{
            "manifest_version": "0.3",
            "name": "valid-nested",
            "version": "1.0.0",
            "description": "Valid nested path",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "dist/src/index.js"
            }
        }"#,
        );

        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let spec = resolve_mcpb(&request).expect("Valid nested path should resolve");

        assert_eq!(spec.command, "node");
        assert_eq!(spec.args, vec!["/cache/bundles/abc123/dist/src/index.js"]);
    }

    // =========================================================================
    // Error Cases
    // =========================================================================

    #[test]
    fn test_resolve_mcpb_fails_without_entry_point_or_mcp_config() {
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
        let request = McpbRuntimeRequest {
            manifest: &manifest,
            extract_dir: &extract_dir,
            extra_env: HashMap::new(),
        };

        let result = resolve_mcpb(&request);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("entry_point") || err.contains("mcp_config"));
    }
}
