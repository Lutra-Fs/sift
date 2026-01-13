//! Codex client implementation.
//!
//! Note: Codex uses TOML format for MCP configuration (~/.codex/config.toml).
//! TOML support needs to be implemented separately. For now, this adapter
//! defines the structure but actual TOML writing is not yet implemented.

use serde_json::{Map, Value, json};

use crate::client::{
    ClientAdapter, ClientCapabilities, ClientContext, ManagedJsonPlan, McpConfigFormat, PathRoot,
    ScopeSupport, SkillDeliveryMode, SkillDeliveryPlan,
};
use crate::mcp::spec::{McpResolvedServer, McpTransport};
use crate::types::ConfigScope;

#[derive(Debug, Default)]
pub struct CodexClient;

impl CodexClient {
    pub fn new() -> Self {
        Self
    }
}

impl ClientAdapter for CodexClient {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> ClientCapabilities {
        ClientCapabilities {
            // Codex only supports global MCP configuration
            mcp: ScopeSupport {
                global: true,
                project: false,
                local: false,
            },
            // Codex supports skills at multiple levels
            skills: ScopeSupport {
                global: true,
                project: true,
                local: false,
            },
            supports_symlinked_skills: false,
            skill_delivery: SkillDeliveryMode::Filesystem {
                global_path: "~/.codex/skills".to_string(),
                project_path: Some(".codex/skills".to_string()),
            },
            // Note: Codex actually uses TOML, but we use Generic here as a placeholder
            // TODO: Add McpConfigFormat::Toml variant when TOML support is implemented
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
        match scope {
            ConfigScope::Global => {
                let entries = build_mcp_entries(servers)?;
                // Note: This produces JSON structure that would need to be converted to TOML
                // The actual file is ~/.codex/config.toml with [mcp_servers.<name>] sections
                Ok(ManagedJsonPlan {
                    root: PathRoot::User,
                    relative_path: ".codex/config.toml".into(),
                    json_path: vec!["mcp_servers".to_string()],
                    entries,
                })
            }
            ConfigScope::PerProjectShared | ConfigScope::PerProjectLocal => {
                anyhow::bail!("Codex only supports global MCP configuration")
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
            ConfigScope::Global => ".codex/skills".into(),
            ConfigScope::PerProjectLocal | ConfigScope::PerProjectShared => ".codex/skills".into(),
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
    // Codex TOML format:
    // [mcp_servers.name]
    // command = "..."
    // args = [...]
    // [mcp_servers.name.env]
    // KEY = "VALUE"
    //
    // For HTTP:
    // [mcp_servers.name]
    // url = "..."
    // http_headers = { "Key" = "Value" }
    match server.transport {
        McpTransport::Stdio => Ok(json!({
            "command": server.command.clone().unwrap_or_default(),
            "args": server.args.clone(),
            "env": server.env.clone(),
        })),
        McpTransport::Http => {
            let mut obj = serde_json::Map::new();
            obj.insert(
                "url".to_string(),
                json!(server.url.clone().unwrap_or_default()),
            );
            if !server.headers.is_empty() {
                obj.insert("http_headers".to_string(), json!(server.headers.clone()));
            }
            Ok(Value::Object(obj))
        }
    }
}
