//! Sift Core Library
//!
//! Provides the domain logic for MCP server and skills management
//! with support for multiple configuration scopes and clients.

pub mod client;
pub mod commands;
pub mod config;
pub mod context;
pub mod deploy;
pub mod fs;
pub mod git;
pub mod lockfile;
pub mod mcp;
pub mod mcpb;
pub mod orchestration;
pub mod registry;
pub mod runtime;
pub mod skills;
pub mod source;
pub mod status;
pub mod types;
pub mod version;

/// Re-exports of commonly used types
pub mod prelude {
    // Commands
    pub use crate::commands::{
        InstallCommand, InstallOptions, InstallReport, InstallTarget, UninstallCommand,
        UninstallOptions, UninstallReport, UninstallScope, UninstallTarget,
    };

    // Configuration
    pub use crate::config::{
        ClientConfigEntry, ConfigManager, McpConfigEntry, ProjectConfig, SiftConfig,
        SkillConfigEntry,
    };
    pub use crate::types::ConfigScope;

    // MCP
    pub use crate::mcp::{McpConfig, McpConfigOverride, McpServer, RuntimeType};

    // Skills
    pub use crate::skills::{Skill, SkillConfig, SkillConfigOverride};

    // Client
    pub use crate::client::{
        ClientCapabilities, ClientConfig, McpConfigFormat, ScopeSupport, SkillDeliveryMode,
    };

    // Filesystem
    pub use crate::fs::LinkMode;

    // Registry
    pub use crate::registry::marketplace::{MarketplaceAdapter, MarketplaceManifest};
    pub use crate::registry::{RegistryConfig, RegistryType};

    // Version
    pub use crate::lockfile::{LockedMcpServer, LockedSkill, Lockfile};
    pub use crate::version::{VersionConstraint, VersionResolver};

    // Runtime
    pub use crate::runtime::{
        McpbRuntimeRequest, PythonExecutor, PythonRuntimeRequest, RunnerSpec, ShellRuntimeRequest,
    };
}
