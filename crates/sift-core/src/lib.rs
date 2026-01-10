//! Sift Core Library
//!
//! Provides the domain logic for MCP server and skills management
//! with support for multiple configuration scopes and clients.

pub mod client;
pub mod config;
pub mod fs;
pub mod install;
pub mod mcp;
pub mod registry;
pub mod runtime;
pub mod skills;
pub mod status;
pub mod version;

/// Re-exports of commonly used types
pub mod prelude {
    // Configuration
    pub use crate::config::{
        ClientConfigEntry, ConfigManager, ConfigScope, McpConfigEntry, ProjectOverride, SiftConfig,
        SkillConfigEntry,
    };

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
    pub use crate::version::{
        LockedMcpServer, LockedSkill, Lockfile, VersionConstraint, VersionResolver,
    };
}
