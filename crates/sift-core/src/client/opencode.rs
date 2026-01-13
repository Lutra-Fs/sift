//! OpenCode client implementation.

use serde_json::{Map, Value, json};

use crate::client::{
    ClientAdapter, ClientCapabilities, ClientContext, ManagedJsonPlan, McpConfigFormat, PathRoot,
    ScopeSupport, SkillDeliveryMode, SkillDeliveryPlan,
};
use crate::mcp::spec::{McpResolvedServer, McpTransport};
use crate::types::ConfigScope;

#[derive(Debug, Default)]
pub struct OpenCodeClient;

impl OpenCodeClient {
    pub fn new() -> Self {
        Self
    }
}

impl ClientAdapter for OpenCodeClient {
    fn id(&self) -> &'static str {
        "opencode"
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
                global_path: "~/.config/opencode/skill".to_string(),
                project_path: Some(".opencode/skill".to_string()),
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
                // OpenCode doesn't have a standard global config location
                // Users typically use project-level config
                anyhow::bail!("OpenCode primarily uses project-level configuration (opencode.json)")
            }
            ConfigScope::PerProjectShared => Ok(ManagedJsonPlan {
                root: PathRoot::Project,
                relative_path: "opencode.json".into(),
                json_path: vec!["mcp".to_string()],
                entries,
            }),
            ConfigScope::PerProjectLocal => {
                anyhow::bail!(
                    "OpenCode does not support local (per-project private) MCP configuration"
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
            ConfigScope::Global => ".config/opencode/skill".into(),
            ConfigScope::PerProjectLocal | ConfigScope::PerProjectShared => {
                ".opencode/skill".into()
            }
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
    // OpenCode format:
    // - local (stdio): { "type": "local", "command": [...], "environment": {...} }
    // - remote (http): { "type": "remote", "url": "...", "headers": {...} }
    match server.transport {
        McpTransport::Stdio => {
            // OpenCode expects command as an array including the executable
            let mut command = vec![server.command.clone().unwrap_or_default()];
            command.extend(server.args.clone());

            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), json!("local"));
            obj.insert("command".to_string(), json!(command));
            if !server.env.is_empty() {
                obj.insert("environment".to_string(), json!(server.env.clone()));
            }
            Ok(Value::Object(obj))
        }
        McpTransport::Http => {
            let mut obj = serde_json::Map::new();
            obj.insert("type".to_string(), json!("remote"));
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
