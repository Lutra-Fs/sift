//! Claude Code client implementation.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::client::{
    ClientAdapter, ClientCapabilities, ClientContext, ManagedJsonPlan, McpConfigFormat, PathRoot,
    ScopeSupport, SkillDeliveryMode, SkillDeliveryPlan,
};
use crate::config::ConfigScope;
use crate::config::managed_json::{apply_managed_entries_in_field, apply_managed_entries_in_path};
use crate::mcp::spec::{McpResolvedServer, McpTransport};
use crate::version::store::LockfileService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeCodeScope {
    User,
    Project,
}

#[derive(Debug, Default)]
pub struct ClaudeCodeClient;

impl ClaudeCodeClient {
    pub fn new() -> Self {
        Self
    }

    pub fn skill_path(&self, scope: ClaudeCodeScope) -> anyhow::Result<String> {
        match scope {
            ClaudeCodeScope::User => {
                let home = dirs::home_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
                Ok(home.join(".claude/skills").to_string_lossy().to_string())
            }
            ClaudeCodeScope::Project => Ok("./.claude/skills".to_string()),
        }
    }

    pub fn render_project_mcp_config(
        &self,
        servers: &[McpResolvedServer],
    ) -> anyhow::Result<Value> {
        let map = build_mcp_entries(servers)?;
        Ok(json!({ "mcpServers": map }))
    }
}

impl ClientAdapter for ClaudeCodeClient {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            mcp: ScopeSupport {
                global: true,
                project: true,
                local: true,
            },
            skills: ScopeSupport {
                global: true,
                project: true,
                local: false,
            },
            supports_symlinked_skills: false,
            skill_delivery: SkillDeliveryMode::Filesystem {
                global_path: "~/.claude/skills".to_string(),
                project_path: Some(".claude/skills".to_string()),
            },
            mcp_config_format: McpConfigFormat::ClaudeCode,
            supported_transports: ["stdio", "http"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn plan_mcp(
        &self,
        ctx: &ClientContext,
        scope: ConfigScope,
        servers: &[McpResolvedServer],
    ) -> anyhow::Result<ManagedJsonPlan> {
        let entries = build_mcp_entries(servers)?;
        match scope {
            ConfigScope::Global => Ok(ManagedJsonPlan {
                root: PathRoot::User,
                relative_path: ".claude.json".into(),
                json_path: vec!["mcpServers".to_string()],
                entries,
            }),
            ConfigScope::PerProjectShared => Ok(ManagedJsonPlan {
                root: PathRoot::Project,
                relative_path: ".mcp.json".into(),
                json_path: vec!["mcpServers".to_string()],
                entries,
            }),
            ConfigScope::PerProjectLocal => {
                let project_key = ctx.project_root.to_string_lossy().to_string();
                Ok(ManagedJsonPlan {
                    root: PathRoot::User,
                    relative_path: ".claude.json".into(),
                    json_path: vec![
                        "projects".to_string(),
                        project_key,
                        "mcpServers".to_string(),
                    ],
                    entries,
                })
            }
        }
    }

    fn plan_skill(
        &self,
        _ctx: &ClientContext,
        scope: ConfigScope,
    ) -> anyhow::Result<SkillDeliveryPlan> {
        let root = match scope {
            ConfigScope::Global => PathRoot::User,
            ConfigScope::PerProjectLocal | ConfigScope::PerProjectShared => PathRoot::Project,
        };

        Ok(SkillDeliveryPlan {
            root,
            relative_path: ".claude/skills".into(),
            use_git_exclude: false,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeCodePaths {
    home_dir: PathBuf,
    project_root: PathBuf,
}

impl ClaudeCodePaths {
    pub fn new(home_dir: PathBuf, project_root: PathBuf) -> Self {
        Self {
            home_dir,
            project_root,
        }
    }

    pub fn user_config_path(&self) -> PathBuf {
        self.home_dir.join(".claude.json")
    }

    pub fn project_mcp_path(&self) -> PathBuf {
        self.project_root.join(".mcp.json")
    }

    pub fn skill_root_for_scope(&self, scope: ConfigScope) -> PathBuf {
        match scope {
            ConfigScope::Global => self.home_dir.join(".claude/skills"),
            ConfigScope::PerProjectShared | ConfigScope::PerProjectLocal => {
                self.project_root.join(".claude/skills")
            }
        }
    }
}

#[derive(Debug)]
pub struct ClaudeCodeMcpWriter {
    paths: ClaudeCodePaths,
    lockfile_service: LockfileService,
}

impl ClaudeCodeMcpWriter {
    pub fn new(paths: ClaudeCodePaths, lockfile_service: LockfileService) -> Self {
        Self {
            paths,
            lockfile_service,
        }
    }

    pub fn paths(&self) -> &ClaudeCodePaths {
        &self.paths
    }

    pub fn apply_project_servers(
        &self,
        servers: &[McpResolvedServer],
        force: bool,
    ) -> anyhow::Result<crate::config::ManagedJsonResult> {
        let map = build_mcp_entries(servers)?;
        apply_managed_entries_in_field(
            &self.paths.project_mcp_path(),
            "mcpServers",
            &map,
            &self.lockfile_service,
            force,
        )
    }

    pub fn apply_user_servers(
        &self,
        servers: &[McpResolvedServer],
        force: bool,
    ) -> anyhow::Result<crate::config::ManagedJsonResult> {
        let map = build_mcp_entries(servers)?;
        apply_managed_entries_in_field(
            &self.paths.user_config_path(),
            "mcpServers",
            &map,
            &self.lockfile_service,
            force,
        )
    }

    pub fn apply_local_servers(
        &self,
        servers: &[McpResolvedServer],
        force: bool,
    ) -> anyhow::Result<crate::config::ManagedJsonResult> {
        let map = build_mcp_entries(servers)?;
        let project_key = self.paths.project_root.to_string_lossy().to_string();
        apply_managed_entries_in_path(
            &self.paths.user_config_path(),
            &["projects", project_key.as_str(), "mcpServers"],
            &map,
            &self.lockfile_service,
            force,
        )
    }

    pub fn apply_servers_for_scope(
        &self,
        scope: ConfigScope,
        servers: &[McpResolvedServer],
        force: bool,
    ) -> anyhow::Result<crate::config::ManagedJsonResult> {
        match scope {
            ConfigScope::Global => self.apply_user_servers(servers, force),
            ConfigScope::PerProjectShared => self.apply_project_servers(servers, force),
            ConfigScope::PerProjectLocal => self.apply_local_servers(servers, force),
        }
    }
}

fn build_mcp_entries(servers: &[McpResolvedServer]) -> anyhow::Result<Map<String, Value>> {
    let mut map = Map::new();
    for server in servers {
        map.insert(server.name.clone(), render_server(server)?);
    }
    Ok(map)
}

fn render_server(server: &McpResolvedServer) -> anyhow::Result<Value> {
    match server.transport {
        McpTransport::Stdio => Ok(json!({
            "command": server.command.clone().unwrap_or_default(),
            "args": server.args.clone(),
            "env": server.env.clone(),
        })),
        McpTransport::Http => Ok(json!({
            "type": "http",
            "url": server.url.clone().unwrap_or_default(),
            "headers": server.headers.clone(),
        })),
    }
}
