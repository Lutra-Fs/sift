//! Execute deployment plans to client config files.

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::client::{ClientAdapter, ClientContext, PathRoot};
use crate::config::client_config::{self, ConfigFormat};
use crate::lockfile::LockfileService;
use crate::mcp::spec::McpResolvedServer;
use crate::types::ConfigScope;

#[derive(Debug)]
pub struct DeployReport {
    pub applied: bool,
    pub warnings: Vec<String>,
}

/// Deploy MCP servers to a single client's config file.
pub fn deploy_mcp_to_client(
    client: &dyn ClientAdapter,
    ctx: &ClientContext,
    scope: ConfigScope,
    servers: &[McpResolvedServer],
    lockfile: &LockfileService,
    force: bool,
) -> anyhow::Result<DeployReport> {
    let plan = client.plan_mcp(ctx, scope, servers)?;
    let config_path = resolve_plan_path(ctx, plan.root, &plan.relative_path)?;
    let path: Vec<&str> = plan.config_path.iter().map(|s| s.as_str()).collect();
    let format: ConfigFormat = plan.format.into();

    client_config::apply_managed_entries_in_path(
        &config_path,
        &path,
        &plan.entries,
        lockfile,
        force,
        format,
    )
    .with_context(|| format!("Failed to apply config to {}", config_path.display()))?;

    Ok(DeployReport {
        applied: true,
        warnings: Vec::new(),
    })
}

fn resolve_plan_path(
    ctx: &ClientContext,
    root: PathRoot,
    relative: &Path,
) -> anyhow::Result<PathBuf> {
    ensure_relative_path(relative)?;
    let base = match root {
        PathRoot::User => &ctx.home_dir,
        PathRoot::Project => &ctx.project_root,
    };
    Ok(base.join(relative))
}

fn ensure_relative_path(path: &Path) -> anyhow::Result<()> {
    if path.is_absolute() {
        anyhow::bail!("Absolute paths not allowed in deploy plans");
    }
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            anyhow::bail!("Path traversal not allowed in deploy plans");
        }
    }
    Ok(())
}
