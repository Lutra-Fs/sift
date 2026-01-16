//! Runtime resolution for MCP servers and skills.
//!
//! This module handles the resolution of runtime configurations into concrete
//! `RunnerSpec` instances that can be used to spawn MCP server processes.
//!
//! ## Design
//!
//! - `RuntimeKind` represents the high-level runtime type (Node, Python, Shell, Docker)
//! - Executors are the specific tools used within a runtime (e.g., npx/bunx for Node, python/uv for Python)
//! - `RuntimeRequest` captures all information needed to resolve a runtime
//! - `RunnerSpec` is the final executable specification with command, args, and env
//!
//! ## MCPB Integration
//!
//! MCPB bundles provide their own execution configuration in `manifest.json`.
//! The `resolve_mcpb` function converts MCPB server config into a `RunnerSpec`:
//!
//! | MCPB server.type | Sift RuntimeType | Executor |
//! |------------------|------------------|----------|
//! | `node`           | Node             | node     |
//! | `python`         | Python           | python   |
//! | `uv`             | Python           | uv       |
//! | `binary`         | Shell            | direct   |

mod mcpb_resolver;

use std::collections::HashMap;
use std::path::PathBuf;

pub use mcpb_resolver::{McpbRuntimeRequest, resolve_mcpb};

/// High-level runtime kind for MCP servers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Bunx,
    Npx,
    Docker,
    Python,
    Shell,
}

/// Executor for Python runtime - determines how Python scripts are launched
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PythonExecutor {
    /// Standard Python interpreter
    #[default]
    Python,
    /// UV package manager (faster, handles dependencies)
    Uv,
}

/// Request to resolve a runtime into a RunnerSpec
#[derive(Debug, Clone)]
pub struct RuntimeRequest {
    pub kind: RuntimeKind,
    pub package: String,
    pub version: String,
    pub cache_dir: PathBuf,
    pub extra_args: Vec<String>,
}

/// Python-specific runtime request with executor choice
#[derive(Debug, Clone)]
pub struct PythonRuntimeRequest {
    /// The executor to use (python or uv)
    pub executor: PythonExecutor,
    /// Entry point script path
    pub entry_point: PathBuf,
    /// Additional arguments to pass to the script
    pub extra_args: Vec<String>,
    /// Additional environment variables
    pub env: HashMap<String, String>,
}

/// Shell runtime request for direct command execution
#[derive(Debug, Clone)]
pub struct ShellRuntimeRequest {
    /// The command to execute
    pub command: String,
    /// Arguments to pass to the command
    pub args: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
}

/// Executable specification - the final output of runtime resolution
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

pub fn resolve_runtime(request: &RuntimeRequest) -> anyhow::Result<RunnerSpec> {
    match request.kind {
        RuntimeKind::Bunx => resolve_bunx(request),
        RuntimeKind::Npx => resolve_npx(request),
        RuntimeKind::Docker => anyhow::bail!("Docker runtime resolution is not implemented yet"),
        RuntimeKind::Python => anyhow::bail!("Python runtime resolution is not implemented yet"),
        RuntimeKind::Shell => anyhow::bail!("Shell runtime resolution is not implemented yet"),
    }
}

/// Resolve a Python runtime request into a RunnerSpec
pub fn resolve_python(request: &PythonRuntimeRequest) -> anyhow::Result<RunnerSpec> {
    let entry_point = request.entry_point.to_string_lossy().to_string();

    let (command, mut args) = match request.executor {
        PythonExecutor::Python => {
            // Standard: python <entry_point> [extra_args...]
            ("python".to_string(), vec![entry_point])
        }
        PythonExecutor::Uv => {
            // UV: uv run <entry_point> [extra_args...]
            ("uv".to_string(), vec!["run".to_string(), entry_point])
        }
    };

    args.extend(request.extra_args.clone());

    Ok(RunnerSpec {
        command,
        args,
        env: request.env.clone(),
    })
}

/// Resolve a shell runtime request into a RunnerSpec
///
/// Used for pre-compiled binaries or shell scripts that are executed directly.
pub fn resolve_shell(request: &ShellRuntimeRequest) -> RunnerSpec {
    RunnerSpec {
        command: request.command.clone(),
        args: request.args.clone(),
        env: request.env.clone(),
    }
}

fn resolve_bunx(request: &RuntimeRequest) -> anyhow::Result<RunnerSpec> {
    let cache_dir = request.cache_dir.to_string_lossy().to_string();
    let mut args = vec![
        "--cache-dir".to_string(),
        cache_dir.clone(),
        format!("{}@{}", request.package, request.version),
    ];
    args.extend(request.extra_args.clone());

    let mut env = HashMap::new();
    env.insert("BUN_INSTALL_CACHE_DIR".to_string(), cache_dir);

    Ok(RunnerSpec {
        command: "bunx".to_string(),
        args,
        env,
    })
}

fn resolve_npx(request: &RuntimeRequest) -> anyhow::Result<RunnerSpec> {
    let cache_dir = request.cache_dir.to_string_lossy().to_string();
    let mut args = vec![format!("{}@{}", request.package, request.version)];
    args.extend(request.extra_args.clone());

    let mut env = HashMap::new();
    env.insert("npm_config_cache".to_string(), cache_dir);

    Ok(RunnerSpec {
        command: "npx".to_string(),
        args,
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Python Runtime Tests
    // =========================================================================

    #[test]
    fn test_resolve_python_with_python_executor() {
        let request = PythonRuntimeRequest {
            executor: PythonExecutor::Python,
            entry_point: PathBuf::from("/path/to/server.py"),
            extra_args: vec!["--verbose".to_string()],
            env: HashMap::new(),
        };

        let spec = resolve_python(&request).unwrap();

        assert_eq!(spec.command, "python");
        assert_eq!(
            spec.args,
            vec!["/path/to/server.py".to_string(), "--verbose".to_string()]
        );
    }

    #[test]
    fn test_resolve_python_with_uv_executor() {
        let request = PythonRuntimeRequest {
            executor: PythonExecutor::Uv,
            entry_point: PathBuf::from("/bundle/src/main.py"),
            extra_args: vec![],
            env: HashMap::new(),
        };

        let spec = resolve_python(&request).unwrap();

        assert_eq!(spec.command, "uv");
        assert_eq!(
            spec.args,
            vec!["run".to_string(), "/bundle/src/main.py".to_string()]
        );
    }

    #[test]
    fn test_resolve_python_preserves_env() {
        let mut env = HashMap::new();
        env.insert("PYTHONPATH".to_string(), "/custom/path".to_string());
        env.insert("API_KEY".to_string(), "secret".to_string());

        let request = PythonRuntimeRequest {
            executor: PythonExecutor::Python,
            entry_point: PathBuf::from("server.py"),
            extra_args: vec![],
            env,
        };

        let spec = resolve_python(&request).unwrap();

        assert_eq!(
            spec.env.get("PYTHONPATH"),
            Some(&"/custom/path".to_string())
        );
        assert_eq!(spec.env.get("API_KEY"), Some(&"secret".to_string()));
    }

    #[test]
    fn test_python_executor_default_is_python() {
        let executor = PythonExecutor::default();
        assert_eq!(executor, PythonExecutor::Python);
    }

    // =========================================================================
    // Shell Runtime Tests
    // =========================================================================

    #[test]
    fn test_resolve_shell_direct_binary() {
        let request = ShellRuntimeRequest {
            command: "/usr/local/bin/my-mcp-server".to_string(),
            args: vec!["--port".to_string(), "8080".to_string()],
            env: HashMap::new(),
        };

        let spec = resolve_shell(&request);

        assert_eq!(spec.command, "/usr/local/bin/my-mcp-server");
        assert_eq!(spec.args, vec!["--port".to_string(), "8080".to_string()]);
    }

    #[test]
    fn test_resolve_shell_preserves_env() {
        let mut env = HashMap::new();
        env.insert("HOME".to_string(), "/home/user".to_string());

        let request = ShellRuntimeRequest {
            command: "./server".to_string(),
            args: vec![],
            env,
        };

        let spec = resolve_shell(&request);

        assert_eq!(spec.env.get("HOME"), Some(&"/home/user".to_string()));
    }

    // =========================================================================
    // Existing Runtime Tests (bunx, npx)
    // =========================================================================

    #[test]
    fn test_resolve_bunx() {
        let request = RuntimeRequest {
            kind: RuntimeKind::Bunx,
            package: "my-mcp-server".to_string(),
            version: "1.0.0".to_string(),
            cache_dir: PathBuf::from("/tmp/sift-cache"),
            extra_args: vec!["--readonly".to_string()],
        };

        let spec = resolve_runtime(&request).unwrap();

        assert_eq!(spec.command, "bunx");
        assert!(spec.args.contains(&"--cache-dir".to_string()));
        assert!(spec.args.contains(&"/tmp/sift-cache".to_string()));
        assert!(spec.args.contains(&"my-mcp-server@1.0.0".to_string()));
        assert!(spec.args.contains(&"--readonly".to_string()));
        assert_eq!(
            spec.env.get("BUN_INSTALL_CACHE_DIR"),
            Some(&"/tmp/sift-cache".to_string())
        );
    }

    #[test]
    fn test_resolve_npx() {
        let request = RuntimeRequest {
            kind: RuntimeKind::Npx,
            package: "my-mcp-server".to_string(),
            version: "2.0.0".to_string(),
            cache_dir: PathBuf::from("/tmp/npm-cache"),
            extra_args: vec![],
        };

        let spec = resolve_runtime(&request).unwrap();

        assert_eq!(spec.command, "npx");
        assert!(spec.args.contains(&"my-mcp-server@2.0.0".to_string()));
        assert_eq!(
            spec.env.get("npm_config_cache"),
            Some(&"/tmp/npm-cache".to_string())
        );
    }
}
