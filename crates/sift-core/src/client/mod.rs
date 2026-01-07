//! Client adapter layer for cross-client compatibility
//!
//! Provides abstraction for different AI client configurations:
//! - Claude Code
//! - VS Code
//! - Gemini CLI
//! - Codex

use serde::{Deserialize, Serialize};

/// Supported client types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClientType {
    /// Claude Code CLI
    ClaudeCode,
    /// Visual Studio Code
    VSCode,
    /// Gemini CLI
    GeminiCli,
    /// Codex
    Codex,
}

/// Trait for client-specific configuration adapters
pub trait ClientAdapter: Send + Sync {
    /// Get the client type
    fn client_type(&self) -> ClientType;

    /// Get the configuration path for this client
    fn config_path(&self) -> anyhow::Result<std::path::PathBuf>;

    /// Read configuration for this client
    fn read_config(&self) -> anyhow::Result<serde_json::Value>;

    /// Write configuration for this client
    fn write_config(&self, config: &serde_json::Value) -> anyhow::Result<()>;
}

/// Claude Code client adapter
#[derive(Debug)]
pub struct ClaudeCodeAdapter;

impl ClientAdapter for ClaudeCodeAdapter {
    fn client_type(&self) -> ClientType {
        ClientType::ClaudeCode
    }

    fn config_path(&self) -> anyhow::Result<std::path::PathBuf> {
        let base = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        Ok(base.join("claude-code"))
    }

    fn read_config(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }

    fn write_config(&self, _config: &serde_json::Value) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Get the adapter for a given client type
pub fn get_adapter(client_type: ClientType) -> Box<dyn ClientAdapter> {
    match client_type {
        ClientType::ClaudeCode => Box::new(ClaudeCodeAdapter),
        ClientType::VSCode => unimplemented!("VS Code adapter not yet implemented"),
        ClientType::GeminiCli => unimplemented!("Gemini CLI adapter not yet implemented"),
        ClientType::Codex => unimplemented!("Codex adapter not yet implemented"),
    }
}
