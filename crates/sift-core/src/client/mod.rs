//! Client abstraction layer.
//!
//! Client implementations live in submodules under this directory.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::mcp::spec::McpResolvedServer;
use crate::types::ConfigScope;

pub mod amp;
pub mod claude_code;
pub mod codex;
pub mod droid;
pub mod gemini_cli;
pub mod opencode;
pub mod vscode;

/// Client configuration from sift.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// Whether this client is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Source for external providers: "registry:some-provider"
    #[serde(default)]
    pub source: Option<String>,

    /// Capabilities (optional, provided by client implementations)
    #[serde(default)]
    pub capabilities: Option<ClientCapabilities>,
}

#[derive(Debug, Clone)]
pub struct ClientContext {
    pub home_dir: PathBuf,
    pub project_root: PathBuf,
}

impl ClientContext {
    pub fn new(home_dir: PathBuf, project_root: PathBuf) -> Self {
        Self {
            home_dir,
            project_root,
        }
    }
}

fn default_enabled() -> bool {
    true
}

impl ClientConfig {
    /// Merge another ClientConfig into this one
    pub fn merge(&mut self, other: ClientConfig) {
        self.enabled = other.enabled;
        if other.source.is_some() {
            self.source = other.source;
        }
        if other.capabilities.is_some() {
            self.capabilities = other.capabilities;
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(ref source) = self.source
            && !source.starts_with("registry:")
            && !source.starts_with("local:")
        {
            anyhow::bail!("Invalid client source format: must be 'registry:name' or 'local:/path'");
        }
        Ok(())
    }
}

/// Client capabilities interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Scope support for MCP servers
    #[serde(default)]
    pub mcp: ScopeSupport,

    /// Scope support for skills
    #[serde(default)]
    pub skills: ScopeSupport,

    /// Whether the client will recognize skills delivered as symlinked directories
    #[serde(default)]
    pub supports_symlinked_skills: bool,

    /// Skill delivery mode
    pub skill_delivery: SkillDeliveryMode,

    /// MCP configuration format
    #[serde(default = "default_mcp_config_format")]
    pub mcp_config_format: McpConfigFormat,

    /// Supported transport types
    #[serde(default)]
    pub supported_transports: HashSet<String>,
}

fn default_mcp_config_format() -> McpConfigFormat {
    McpConfigFormat::Generic
}

/// How skills are delivered to the client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SkillDeliveryMode {
    /// Client scans a directory
    Filesystem {
        global_path: String,
        #[serde(default)]
        project_path: Option<String>,
    },
    /// Client reads paths from config
    ConfigReference,
    /// Client doesn't support skills
    None,
}

/// MCP configuration format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpConfigFormat {
    /// Claude Desktop format: { "mcpServers": {...} }
    ClaudeDesktop,
    /// Claude Code format: { "mcp": {...} }
    ClaudeCode,
    /// Generic format
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRoot {
    User,
    Project,
}

#[derive(Debug, Clone)]
pub struct ManagedJsonPlan {
    pub root: PathRoot,
    pub relative_path: PathBuf,
    pub json_path: Vec<String>,
    pub entries: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct SkillDeliveryPlan {
    pub root: PathRoot,
    pub relative_path: PathBuf,
    pub use_git_exclude: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScopeSupport {
    #[serde(default)]
    pub global: bool,
    #[serde(default)]
    pub project: bool,
    #[serde(default)]
    pub local: bool,
}

pub trait Client: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> ClientCapabilities;
}

pub trait ClientAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> ClientCapabilities;

    fn plan_mcp(
        &self,
        ctx: &ClientContext,
        scope: ConfigScope,
        servers: &[McpResolvedServer],
    ) -> anyhow::Result<ManagedJsonPlan>;

    fn plan_skill(
        &self,
        ctx: &ClientContext,
        scope: ConfigScope,
    ) -> anyhow::Result<SkillDeliveryPlan>;
}
