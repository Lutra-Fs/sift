//! MCP server installation pipeline.

use anyhow::Context as _;

use crate::client::ClientAdapter;
use crate::config::McpConfigEntry;
use crate::context::AppContext;
use crate::deploy::executor::deploy_mcp_to_client;
use crate::deploy::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, resolve_scope,
};
use crate::deploy::targeting::TargetingPolicy;
use crate::lockfile::LockedMcpServer;
use crate::mcp::McpServerBuilder;
use crate::types::ConfigScope;

/// Request to install an MCP server.
#[derive(Debug)]
pub struct McpInstallRequest {
    pub name: String,
    pub entry: McpConfigEntry,
    pub version: Option<String>,
    pub force: bool,
}

/// Report from MCP installation.
#[derive(Debug)]
pub struct McpInstallReport {
    pub changed: bool,
    pub applied: bool,
    pub warnings: Vec<String>,
}

/// MCP server installer.
pub struct McpInstaller<'a> {
    ctx: &'a AppContext,
    scope: ConfigScope,
}

impl<'a> McpInstaller<'a> {
    pub fn new(ctx: &'a AppContext, scope: ConfigScope) -> Self {
        Self { ctx, scope }
    }

    /// Install an MCP server to the given client.
    pub fn install(
        &self,
        client: &dyn ClientAdapter,
        request: McpInstallRequest,
    ) -> anyhow::Result<McpInstallReport> {
        let mut warnings = Vec::new();

        // 1. Resolve scope
        let capabilities = client.capabilities();
        let repo_status = RepoStatus::from_project_root(self.ctx.project_root());
        let resolution = resolve_scope(
            ResourceKind::Mcp,
            ScopeRequest::Explicit(self.scope),
            capabilities.mcp,
            repo_status,
        )?;

        let deploy_scope = match resolution {
            ScopeResolution::Skip { warning } => {
                warnings.push(warning);
                return Ok(McpInstallReport {
                    changed: false,
                    applied: false,
                    warnings,
                });
            }
            ScopeResolution::Apply(decision) => decision.scope,
        };

        // 2. Check targeting
        let targeting = TargetingPolicy::new(
            request.entry.targets.clone(),
            request.entry.ignore_targets.clone(),
        );
        if !targeting.should_deploy_to(client.id()) {
            warnings.push(format!(
                "Skipping deployment to '{}': not in target clients",
                client.id()
            ));
            // Still write to sift.toml, just don't deploy
            self.write_config(&request)?;
            self.update_lockfile(&request)?;
            return Ok(McpInstallReport {
                changed: true,
                applied: false,
                warnings,
            });
        }

        // 3. Build server specs
        let builder = McpServerBuilder::new(self.ctx.state_dir());
        let servers = builder.build(
            &request.name,
            &request.entry.source,
            &request.entry,
            request.version.as_deref(),
            request.force,
        )?;

        // 4. Write sift.toml
        self.write_config(&request)?;

        // 5. Deploy to client
        let client_ctx = self.ctx.client_context();
        let lockfile = self.ctx.lockfile_service();
        let deploy_report = deploy_mcp_to_client(
            client,
            &client_ctx,
            deploy_scope,
            &servers,
            &lockfile,
            request.force,
        )
        .with_context(|| format!("Failed to deploy MCP '{}' to client", request.name))?;

        // 6. Update lockfile
        self.update_lockfile(&request)?;

        Ok(McpInstallReport {
            changed: true,
            applied: deploy_report.applied,
            warnings,
        })
    }

    fn write_config(&self, request: &McpInstallRequest) -> anyhow::Result<()> {
        let store = self.ctx.config_store(self.scope);
        let mut config = store.load()?;
        config
            .mcp
            .insert(request.name.clone(), request.entry.clone());
        store.save(&config)?;
        Ok(())
    }

    fn update_lockfile(&self, request: &McpInstallRequest) -> anyhow::Result<()> {
        let lockfile = self.ctx.lockfile_service();
        let locked = LockedMcpServer::new(
            request.name.clone(),
            "todo".to_string(), // TODO: proper version resolution
            request
                .version
                .clone()
                .unwrap_or_else(|| "latest".to_string()),
            request.entry.source.clone(),
            self.scope,
        );
        lockfile.add_mcp(&request.name, locked)?;
        Ok(())
    }
}
