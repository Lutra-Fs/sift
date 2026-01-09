//! Resolved MCP server specification for config rendering.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    Stdio,
    Http,
}

#[derive(Debug, Clone)]
pub struct McpResolvedServer {
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: Option<String>,
    pub headers: HashMap<String, String>,
}

impl McpResolvedServer {
    pub fn stdio(
        name: String,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> Self {
        Self {
            name,
            transport: McpTransport::Stdio,
            command: Some(command),
            args,
            env,
            url: None,
            headers: HashMap::new(),
        }
    }

    pub fn http(name: String, url: String, headers: HashMap<String, String>) -> Self {
        Self {
            name,
            transport: McpTransport::Http,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            url: Some(url),
            headers,
        }
    }
}
