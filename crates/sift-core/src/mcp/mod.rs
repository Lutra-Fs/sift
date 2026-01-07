//! MCP (Model Context Protocol) server management

use serde::{Deserialize, Serialize};

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

/// Manager for MCP servers
#[derive(Debug)]
pub struct McpManager;

impl McpManager {
    /// Create a new MCP manager
    pub fn new() -> Self {
        Self
    }

    /// List all configured MCP servers
    pub fn list_servers(&self) -> Vec<McpServer> {
        Vec::new()
    }

    /// Add a new MCP server
    pub fn add_server(&self, _server: McpServer) -> anyhow::Result<()> {
        Ok(())
    }

    /// Remove an MCP server
    pub fn remove_server(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
