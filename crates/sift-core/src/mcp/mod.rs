//! MCP (Model Context Protocol) server management

pub mod builder;
pub mod installer;
pub mod schema;
pub mod spec;

use serde::{Deserialize, Serialize};

// Re-export the new schema types
pub use builder::{DEFAULT_RUNTIME, DEFAULT_VERSION, McpServerBuilder};
pub use schema::{McpConfig, McpConfigOverride, RuntimeType, TransportType};
pub use spec::{McpResolvedServer, McpTransport};

/// Represents an MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    /// Unique identifier for the server
    pub id: String,
    /// Display name
    pub name: String,
    /// Command to run the server
    pub command: String,
    /// Arguments for the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: Vec<String>,
}

// Legacy manager removed: operations should go through install/update services.
