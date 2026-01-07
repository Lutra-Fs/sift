//! Sift Core Library
//!
//! Provides the domain logic for MCP server and skills management
//! with support for multiple configuration scopes and clients.

pub mod config;
pub mod mcp;
pub mod skills;
pub mod client;

/// Re-exports of commonly used types
pub mod prelude {
    pub use crate::config::{ConfigScope, ConfigManager};
    pub use crate::client::{ClientAdapter, ClientType};
}
