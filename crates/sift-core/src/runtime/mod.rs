//! Runtime resolution for MCP servers and skills.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Bunx,
    Npx,
    Docker,
    Python,
    Shell,
}

#[derive(Debug, Clone)]
pub struct RuntimeRequest {
    pub kind: RuntimeKind,
    pub package: String,
    pub version: String,
    pub cache_dir: PathBuf,
    pub extra_args: Vec<String>,
}

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
