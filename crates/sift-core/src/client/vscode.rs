//! VS Code (GitHub Copilot) client implementation.

use serde_json::{Map, Value, json};

use crate::client::{
    ClientAdapter, ClientCapabilities, ClientContext, ManagedJsonPlan, McpConfigFormat, PathRoot,
    ScopeSupport, SkillDeliveryMode, SkillDeliveryPlan,
};
use crate::mcp::spec::{McpResolvedServer, McpTransport};
use crate::types::ConfigScope;

#[derive(Debug, Default)]
pub struct VsCodeClient;

impl VsCodeClient {
    pub fn new() -> Self {
        Self
    }
}

impl ClientAdapter for VsCodeClient {
    fn id(&self) -> &'static str {
        "vscode"
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            mcp: ScopeSupport {
                global: true,
                project: true,
                local: false,
            },
            skills: ScopeSupport {
                global: true,
                project: true,
                local: false,
            },
            supports_symlinked_skills: false,
            skill_delivery: SkillDeliveryMode::Filesystem {
                // VS Code recommends ~/.copilot/skills/ but also supports ~/.claude/skills/
                global_path: "~/.copilot/skills".to_string(),
                // VS Code recommends .github/skills/ but also supports .claude/skills/
                project_path: Some(".github/skills".to_string()),
            },
            mcp_config_format: McpConfigFormat::Generic,
            supported_transports: ["stdio", "http"]
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn plan_mcp(
        &self,
        _ctx: &ClientContext,
        scope: ConfigScope,
        servers: &[McpResolvedServer],
    ) -> anyhow::Result<ManagedJsonPlan> {
        let entries = build_mcp_entries(servers)?;
        match scope {
            ConfigScope::Global => {
                // VS Code user-level MCP config is in user profile settings
                // This is typically managed through VS Code UI, not direct file editing
                anyhow::bail!(
                    "VS Code global MCP configuration is managed through VS Code profile settings"
                )
            }
            ConfigScope::PerProjectShared => Ok(ManagedJsonPlan {
                root: PathRoot::Project,
                relative_path: ".vscode/mcp.json".into(),
                // VS Code uses "servers" instead of "mcpServers"
                config_path: vec!["servers".to_string()],
                entries,
                format: McpConfigFormat::Generic,
            }),
            ConfigScope::PerProjectLocal => {
                anyhow::bail!(
                    "VS Code does not support local (per-project private) MCP configuration"
                )
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

        let relative_path = match scope {
            ConfigScope::Global => ".copilot/skills".into(),
            ConfigScope::PerProjectLocal | ConfigScope::PerProjectShared => ".github/skills".into(),
        };

        Ok(SkillDeliveryPlan {
            root,
            relative_path,
            use_git_exclude: false,
        })
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
    // VS Code format:
    // - stdio: { "type": "stdio", "command": "...", "args": [...], "env": {...} }
    // - http: { "type": "http", "url": "...", "headers": {...} }
    match server.transport {
        McpTransport::Stdio => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), json!("stdio"));
            obj.insert(
                "command".to_string(),
                json!(server.command.clone().unwrap_or_default()),
            );
            if !server.args.is_empty() {
                obj.insert("args".to_string(), json!(server.args.clone()));
            }
            if !server.env.is_empty() {
                obj.insert("env".to_string(), json!(server.env.clone()));
            }
            Ok(Value::Object(obj))
        }
        McpTransport::Http => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), json!("http"));
            obj.insert(
                "url".to_string(),
                json!(server.url.clone().unwrap_or_default()),
            );
            if !server.headers.is_empty() {
                obj.insert("headers".to_string(), json!(server.headers.clone()));
            }
            Ok(Value::Object(obj))
        }
    }
}
