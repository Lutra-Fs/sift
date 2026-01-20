//! Client registry for managing available client adapters.
//!
//! The registry provides a central place to discover and filter clients
//! based on their capabilities and scope support.

use crate::types::ConfigScope;

use super::{
    ClientAdapter, amp::AmpClient, claude_code::ClaudeCodeClient, codex::CodexClient,
    droid::DroidClient, gemini_cli::GeminiCliClient, opencode::OpenCodeClient,
    vscode::VsCodeClient,
};

/// Registry of available client adapters.
///
/// Holds all known clients and provides filtering based on scope support.
#[derive(Debug)]
pub struct ClientRegistry {
    clients: Vec<Box<dyn ClientAdapter>>,
}

impl Default for ClientRegistry {
    fn default() -> Self {
        Self::with_default_clients()
    }
}

impl ClientRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            clients: Vec::new(),
        }
    }

    /// Create a registry with all default clients.
    pub fn with_default_clients() -> Self {
        let clients: Vec<Box<dyn ClientAdapter>> = vec![
            Box::new(ClaudeCodeClient::new()),
            Box::new(VsCodeClient::new()),
            Box::new(AmpClient::new()),
            Box::new(CodexClient::new()),
            Box::new(DroidClient::new()),
            Box::new(GeminiCliClient::new()),
            Box::new(OpenCodeClient::new()),
        ];
        Self { clients }
    }

    /// Register a client adapter.
    pub fn register(&mut self, client: Box<dyn ClientAdapter>) {
        self.clients.push(client);
    }

    /// Get all registered clients.
    pub fn all(&self) -> &[Box<dyn ClientAdapter>] {
        &self.clients
    }

    /// Get a client by ID.
    pub fn get(&self, id: &str) -> Option<&dyn ClientAdapter> {
        self.clients
            .iter()
            .find(|c| c.id() == id)
            .map(|c| c.as_ref())
    }

    /// Get clients that support MCP at the given scope.
    pub fn mcp_clients_for_scope(&self, scope: ConfigScope) -> Vec<&dyn ClientAdapter> {
        self.clients
            .iter()
            .filter(|c| scope_supported(&c.capabilities().mcp, scope))
            .map(|c| c.as_ref())
            .collect()
    }

    /// Get clients that support skills at the given scope.
    pub fn skill_clients_for_scope(&self, scope: ConfigScope) -> Vec<&dyn ClientAdapter> {
        self.clients
            .iter()
            .filter(|c| scope_supported(&c.capabilities().skills, scope))
            .map(|c| c.as_ref())
            .collect()
    }

    /// Filter clients by explicit target list (whitelist).
    pub fn filter_by_targets<'a>(&'a self, targets: &[String]) -> Vec<&'a dyn ClientAdapter> {
        self.clients
            .iter()
            .filter(|c| targets.iter().any(|t| t == c.id()))
            .map(|c| c.as_ref())
            .collect()
    }

    /// Filter clients by ignoring specific targets (blacklist).
    pub fn filter_excluding_targets<'a>(
        &'a self,
        ignore_targets: &[String],
    ) -> Vec<&'a dyn ClientAdapter> {
        self.clients
            .iter()
            .filter(|c| !ignore_targets.iter().any(|t| t == c.id()))
            .map(|c| c.as_ref())
            .collect()
    }

    /// Get applicable clients based on target/ignore configuration.
    ///
    /// - If `targets` is provided, only those clients are returned (whitelist).
    /// - If `ignore_targets` is provided, those clients are excluded (blacklist).
    /// - If neither is provided, all clients are returned.
    pub fn applicable_clients<'a>(
        &'a self,
        targets: Option<&[String]>,
        ignore_targets: Option<&[String]>,
    ) -> Vec<&'a dyn ClientAdapter> {
        match (targets, ignore_targets) {
            (Some(t), _) => self.filter_by_targets(t),
            (None, Some(i)) => self.filter_excluding_targets(i),
            (None, None) => self.clients.iter().map(|c| c.as_ref()).collect(),
        }
    }

    /// List all client IDs.
    pub fn client_ids(&self) -> Vec<&'static str> {
        self.clients.iter().map(|c| c.id()).collect()
    }
}

fn scope_supported(support: &super::ScopeSupport, scope: ConfigScope) -> bool {
    match scope {
        ConfigScope::Global => support.global,
        ConfigScope::PerProjectShared => support.project,
        ConfigScope::PerProjectLocal => support.local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_clients_registered() {
        let registry = ClientRegistry::with_default_clients();
        let ids = registry.client_ids();

        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"vscode"));
        assert!(ids.contains(&"amp"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"droid"));
        assert!(ids.contains(&"gemini-cli"));
        assert!(ids.contains(&"opencode"));
        assert_eq!(ids.len(), 7);
    }

    #[test]
    fn test_get_client_by_id() {
        let registry = ClientRegistry::with_default_clients();

        let claude = registry.get("claude-code");
        assert!(claude.is_some());
        assert_eq!(claude.expect("client exists").id(), "claude-code");

        let missing = registry.get("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_mcp_clients_for_global_scope() {
        let registry = ClientRegistry::with_default_clients();
        let clients = registry.mcp_clients_for_scope(ConfigScope::Global);
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        // These clients support global MCP
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"amp"));
        assert!(ids.contains(&"droid"));
        assert!(ids.contains(&"gemini-cli"));
        // vscode and opencode don't support global MCP well (they bail)
    }

    #[test]
    fn test_mcp_clients_for_project_scope() {
        let registry = ClientRegistry::with_default_clients();
        let clients = registry.mcp_clients_for_scope(ConfigScope::PerProjectShared);
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        // These clients support project MCP
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"vscode"));
        assert!(ids.contains(&"amp"));
        assert!(ids.contains(&"droid"));
        assert!(ids.contains(&"gemini-cli"));
        assert!(ids.contains(&"opencode"));
        // codex only supports global
        assert!(!ids.contains(&"codex"));
    }

    #[test]
    fn test_mcp_clients_for_local_scope() {
        let registry = ClientRegistry::with_default_clients();
        let clients = registry.mcp_clients_for_scope(ConfigScope::PerProjectLocal);
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        // Only claude-code supports local MCP scope
        assert!(ids.contains(&"claude-code"));
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn test_skill_clients_for_global_scope() {
        let registry = ClientRegistry::with_default_clients();
        let clients = registry.skill_clients_for_scope(ConfigScope::Global);
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        // All clients support global skills
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"vscode"));
        assert!(ids.contains(&"amp"));
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"droid"));
        assert!(ids.contains(&"gemini-cli"));
        assert!(ids.contains(&"opencode"));
    }

    #[test]
    fn test_filter_by_targets() {
        let registry = ClientRegistry::with_default_clients();
        let clients =
            registry.filter_by_targets(&["claude-code".to_string(), "vscode".to_string()]);
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"vscode"));
    }

    #[test]
    fn test_filter_excluding_targets() {
        let registry = ClientRegistry::with_default_clients();
        let clients =
            registry.filter_excluding_targets(&["codex".to_string(), "droid".to_string()]);
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        assert!(!ids.contains(&"codex"));
        assert!(!ids.contains(&"droid"));
        assert!(ids.contains(&"claude-code"));
        assert!(ids.contains(&"vscode"));
    }

    #[test]
    fn test_applicable_clients_with_targets() {
        let registry = ClientRegistry::with_default_clients();
        let targets = vec!["claude-code".to_string()];
        let clients = registry.applicable_clients(Some(&targets), None);

        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].id(), "claude-code");
    }

    #[test]
    fn test_applicable_clients_with_ignore() {
        let registry = ClientRegistry::with_default_clients();
        let ignore = vec!["claude-code".to_string()];
        let clients = registry.applicable_clients(None, Some(&ignore));
        let ids: Vec<_> = clients.iter().map(|c| c.id()).collect();

        assert!(!ids.contains(&"claude-code"));
        assert!(ids.len() == 6);
    }

    #[test]
    fn test_applicable_clients_no_filter() {
        let registry = ClientRegistry::with_default_clients();
        let clients = registry.applicable_clients(None, None);

        assert_eq!(clients.len(), 7);
    }

    #[test]
    fn test_register_custom_client() {
        let mut registry = ClientRegistry::new();
        assert!(registry.all().is_empty());

        registry.register(Box::new(ClaudeCodeClient::new()));
        assert_eq!(registry.all().len(), 1);
        assert_eq!(
            registry.get("claude-code").expect("exists").id(),
            "claude-code"
        );
    }

    #[test]
    fn test_empty_registry() {
        let registry = ClientRegistry::new();

        assert!(registry.all().is_empty());
        assert!(registry.get("claude-code").is_none());
        assert!(
            registry
                .mcp_clients_for_scope(ConfigScope::Global)
                .is_empty()
        );
    }
}
