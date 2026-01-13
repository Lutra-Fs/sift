//! Gemini CLI client implementation.

use serde_json::{Map, Value, json};

use crate::client::{
    ClientAdapter, ClientCapabilities, ClientContext, ManagedJsonPlan, McpConfigFormat, PathRoot,
    ScopeSupport, SkillDeliveryMode, SkillDeliveryPlan,
};
use crate::mcp::spec::{McpResolvedServer, McpTransport};
use crate::types::ConfigScope;

#[derive(Debug, Default)]
pub struct GeminiCliClient;

impl GeminiCliClient {
    pub fn new() -> Self {
        Self
    }
}

impl ClientAdapter for GeminiCliClient {
    fn id(&self) -> &'static str {
        "gemini-cli"
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
                global_path: "~/.gemini/skills".to_string(),
                project_path: Some(".gemini/skills".to_string()),
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
            ConfigScope::Global => Ok(ManagedJsonPlan {
                root: PathRoot::User,
                relative_path: ".gemini/settings.json".into(),
                json_path: vec!["mcpServers".to_string()],
                entries,
            }),
            ConfigScope::PerProjectShared => Ok(ManagedJsonPlan {
                root: PathRoot::Project,
                relative_path: ".gemini/settings.json".into(),
                json_path: vec!["mcpServers".to_string()],
                entries,
            }),
            ConfigScope::PerProjectLocal => {
                anyhow::bail!(
                    "Gemini CLI does not support local (per-project private) MCP configuration"
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
            ConfigScope::Global => ".gemini/skills".into(),
            ConfigScope::PerProjectLocal | ConfigScope::PerProjectShared => ".gemini/skills".into(),
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
    // Gemini CLI format:
    // - stdio: { "command": "...", "args": [...], "env": {...} }
    // - http: { "httpUrl": "...", "headers": {...} }
    match server.transport {
        McpTransport::Stdio => Ok(json!({
            "command": server.command.clone().unwrap_or_default(),
            "args": server.args.clone(),
            "env": server.env.clone(),
        })),
        McpTransport::Http => {
            let mut obj = serde_json::Map::new();
            // Gemini CLI uses "httpUrl" for streamable HTTP transport
            obj.insert(
                "httpUrl".to_string(),
                json!(server.url.clone().unwrap_or_default()),
            );
            if !server.headers.is_empty() {
                obj.insert("headers".to_string(), json!(server.headers.clone()));
            }
            Ok(Value::Object(obj))
        }
    }
}
