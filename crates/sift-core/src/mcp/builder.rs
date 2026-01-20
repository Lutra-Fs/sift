//! MCP server builder for constructing resolved server specifications.
//!
//! Extracted from InstallCommand to handle the complex logic of building
//! McpResolvedServer instances from various sources (HTTP, MCPB, registry, npm-style).

use std::path::Path;

use crate::config::McpConfigEntry;
use crate::source::SourceResolver;

use super::spec::McpResolvedServer;

/// Default runtime for MCP servers when not specified
pub const DEFAULT_RUNTIME: &str = "shell";

/// Default version constraint when not specified
pub const DEFAULT_VERSION: &str = "latest";

/// Builds resolved MCP server specifications from various sources.
///
/// This builder handles:
/// - HTTP transport servers (direct URL configuration)
/// - MCPB bundle sources (downloaded and extracted archives)
/// - Registry sources (resolved via marketplace adapters)
/// - Shell runtime servers (local commands)
/// - npm-style fallback (name@version pattern)
pub struct McpServerBuilder<'a> {
    /// State directory for MCPB cache
    state_dir: &'a Path,
    /// Source resolver for registry lookups
    source_resolver: Option<SourceResolver>,
}

impl<'a> McpServerBuilder<'a> {
    /// Create a new builder with the given state directory.
    pub fn new(state_dir: &'a Path) -> Self {
        Self {
            state_dir,
            source_resolver: None,
        }
    }

    /// Set the source resolver for registry lookups.
    pub fn with_source_resolver(mut self, resolver: SourceResolver) -> Self {
        self.source_resolver = Some(resolver);
        self
    }

    /// Build resolved server specifications from a config entry.
    ///
    /// Dispatches to the appropriate handler based on transport type and source prefix.
    pub fn build(
        &self,
        name: &str,
        source: &str,
        entry: &McpConfigEntry,
        version: Option<&str>,
        force: bool,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        // HTTP transport - direct URL configuration
        if entry.transport.as_deref() == Some("http") {
            return self.build_http(name, entry);
        }

        // MCPB bundle source
        if let Some(mcpb_url) = source.strip_prefix("mcpb:") {
            return self.build_from_mcpb(name, mcpb_url, entry, force);
        }

        // Registry source - resolve from marketplace
        if let Some(registry_part) = source.strip_prefix("registry:") {
            return self.build_from_registry(name, registry_part, entry, version, force);
        }

        // Shell runtime - local command
        if entry.runtime.as_deref() == Some("shell") {
            return self.build_shell(name, entry);
        }

        // Fallback: npm-style name@version
        self.build_npm_fallback(name, version, entry)
    }

    /// Build an HTTP transport server.
    fn build_http(
        &self,
        name: &str,
        entry: &McpConfigEntry,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        let url = entry
            .url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("HTTP transport requires a URL"))?;
        Ok(vec![McpResolvedServer::http(
            name.to_string(),
            url,
            entry.headers.clone(),
        )])
    }

    /// Build a shell runtime server.
    fn build_shell(
        &self,
        name: &str,
        entry: &McpConfigEntry,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        let command = entry
            .source
            .strip_prefix("local:")
            .unwrap_or(&entry.source)
            .to_string();
        Ok(vec![McpResolvedServer::stdio(
            name.to_string(),
            command,
            entry.args.clone(),
            entry.env.clone(),
        )])
    }

    /// Build servers from an MCPB bundle.
    ///
    /// Downloads the bundle, extracts it, parses manifest.json, and converts
    /// to McpResolvedServer with platform-specific overrides applied.
    fn build_from_mcpb(
        &self,
        name: &str,
        url: &str,
        entry: &McpConfigEntry,
        force: bool,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        use crate::mcpb::{McpbFetcher, manifest_to_server};

        let fetcher = McpbFetcher::new(self.state_dir.join("cache"));

        // Block on async fetch using tokio runtime
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| anyhow::anyhow!("Failed to create tokio runtime: {}", e))?;

        let bundle = runtime.block_on(fetcher.fetch(url, force))?;

        // Convert manifest to resolved server
        let mut server = manifest_to_server(name, &bundle.manifest, &bundle.extract_dir)?;

        // Merge user-provided environment variables
        for (key, value) in &entry.env {
            server.env.insert(key.clone(), value.clone());
        }

        // Append additional args
        server.args.extend(entry.args.clone());

        Ok(vec![server])
    }

    /// Build servers from a registry source.
    ///
    /// Resolves the plugin from the marketplace registry, extracts mcpServers,
    /// and handles MCPB bundles or other transport types.
    ///
    /// Falls back to npm-style resolution if registry resolution fails.
    fn build_from_registry(
        &self,
        name: &str,
        registry_part: &str,
        entry: &McpConfigEntry,
        version: Option<&str>,
        force: bool,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        let resolver = match &self.source_resolver {
            Some(r) => r,
            None => return self.build_npm_fallback(name, version, entry),
        };

        // Try to resolve the MCP config from the registry
        let resolutions = match resolver.resolve_mcp_registry(registry_part) {
            Ok(Some(resolutions)) if !resolutions.is_empty() => resolutions,
            Ok(Some(_)) | Ok(None) => {
                // Plugin found but has no mcpServers - fall back to npm-style
                return self.build_npm_fallback(name, version, entry);
            }
            Err(_) => {
                // Registry resolution failed - fall back to npm-style
                return self.build_npm_fallback(name, version, entry);
            }
        };

        if resolutions.is_empty() {
            anyhow::bail!("Plugin '{}' has no MCP servers defined", name);
        }

        let mut servers = Vec::new();
        for resolution in resolutions {
            let mcp_source = &resolution.mcp_config.source;

            if let Some(mcpb_url) = mcp_source.strip_prefix("mcpb:") {
                // MCPB bundle from registry
                let mcpb_servers =
                    self.build_from_mcpb(&resolution.plugin_name, mcpb_url, entry, force)?;
                servers.extend(mcpb_servers);
            } else if resolution.mcp_config.url.is_some() {
                // HTTP transport
                let url = resolution.mcp_config.url.clone().unwrap_or_default();
                let mut headers = resolution.mcp_config.headers.clone();
                headers.extend(entry.headers.clone());
                servers.push(McpResolvedServer::http(
                    resolution.plugin_name.clone(),
                    url,
                    headers,
                ));
            } else {
                // STDIO transport with command
                let command = Self::command_from_registry_source(&resolution.mcp_config.source)
                    .unwrap_or_else(|| {
                        entry
                            .runtime
                            .as_deref()
                            .unwrap_or(DEFAULT_RUNTIME)
                            .to_string()
                    });

                let mut args = resolution.mcp_config.args.clone();
                args.extend(entry.args.clone());

                let mut env = resolution.mcp_config.env.clone();
                env.extend(entry.env.clone());

                servers.push(McpResolvedServer::stdio(
                    resolution.plugin_name.clone(),
                    command,
                    args,
                    env,
                ));
            }
        }

        Ok(servers)
    }

    /// Build servers using npm-style name@version resolution.
    ///
    /// Used as fallback when registry resolution fails or is unavailable.
    fn build_npm_fallback(
        &self,
        name: &str,
        version: Option<&str>,
        entry: &McpConfigEntry,
    ) -> anyhow::Result<Vec<McpResolvedServer>> {
        let runtime = entry.runtime.as_deref().unwrap_or(DEFAULT_RUNTIME);
        let command = runtime.to_string();

        let resolved_version = version.unwrap_or(DEFAULT_VERSION);
        let mut args = vec![format!("{}@{}", name, resolved_version)];
        args.extend(entry.args.clone());

        Ok(vec![McpResolvedServer::stdio(
            name.to_string(),
            command,
            args,
            entry.env.clone(),
        )])
    }

    /// Extract command from a registry source string.
    ///
    /// Registry sources can include a command suffix after a second colon:
    /// `registry:name:command` -> Some("command")
    /// `registry:name` -> None
    pub fn command_from_registry_source(source: &str) -> Option<String> {
        let colon_count = source.chars().filter(|c| *c == ':').count();
        if colon_count < 2 {
            return None;
        }
        let (_base, command) = source.rsplit_once(':')?;
        if command.is_empty() {
            return None;
        }
        Some(command.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::McpTransport;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_entry() -> McpConfigEntry {
        McpConfigEntry {
            transport: None,
            source: String::new(),
            runtime: None,
            args: Vec::new(),
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        }
    }

    // =========================================================================
    // HTTP Transport Tests
    // =========================================================================

    #[test]
    fn test_build_http_transport_with_url() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry.transport = Some("http".to_string());
        entry.url = Some("https://example.com/api".to_string());
        entry
            .headers
            .insert("Authorization".to_string(), "Bearer token".to_string());

        let servers = builder
            .build("my-server", "registry:test", &entry, None, false)
            .unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "my-server");
        assert_eq!(servers[0].transport, McpTransport::Http);
        assert_eq!(servers[0].url, Some("https://example.com/api".to_string()));
        assert_eq!(
            servers[0].headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
    }

    #[test]
    fn test_build_http_transport_requires_url() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry.transport = Some("http".to_string());
        // No URL set

        let result = builder.build("my-server", "registry:test", &entry, None, false);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("HTTP transport requires a URL"));
    }

    // =========================================================================
    // Shell Runtime Tests
    // =========================================================================

    #[test]
    fn test_build_shell_runtime() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry.runtime = Some("shell".to_string());
        entry.source = "/usr/bin/my-server".to_string();
        entry.args = vec!["--port".to_string(), "8080".to_string()];

        let servers = builder
            .build("my-server", "local:/usr/bin/my-server", &entry, None, false)
            .unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "my-server");
        assert_eq!(servers[0].transport, McpTransport::Stdio);
        assert_eq!(servers[0].command, Some("/usr/bin/my-server".to_string()));
        assert_eq!(servers[0].args, vec!["--port", "8080"]);
    }

    #[test]
    fn test_build_shell_runtime_strips_local_prefix() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry.runtime = Some("shell".to_string());
        entry.source = "local:/path/to/server".to_string();

        let servers = builder
            .build("test", "local:/path/to/server", &entry, None, false)
            .unwrap();

        assert_eq!(servers[0].command, Some("/path/to/server".to_string()));
    }

    // =========================================================================
    // npm Fallback Tests
    // =========================================================================

    #[test]
    fn test_build_npm_fallback_default_runtime() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let entry = create_test_entry();

        let servers = builder
            .build(
                "@modelcontextprotocol/server-filesystem",
                "registry:fs",
                &entry,
                None,
                false,
            )
            .unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "@modelcontextprotocol/server-filesystem");
        assert_eq!(servers[0].transport, McpTransport::Stdio);
        assert_eq!(servers[0].command, Some("shell".to_string()));
        assert_eq!(
            servers[0].args,
            vec!["@modelcontextprotocol/server-filesystem@latest"]
        );
    }

    #[test]
    fn test_build_npm_fallback_with_version() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let entry = create_test_entry();

        let servers = builder
            .build("my-package", "registry:test", &entry, Some("1.2.3"), false)
            .unwrap();

        assert_eq!(servers[0].args, vec!["my-package@1.2.3"]);
    }

    #[test]
    fn test_build_npm_fallback_with_custom_runtime() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry.runtime = Some("bunx".to_string());

        let servers = builder
            .build("my-package", "registry:test", &entry, None, false)
            .unwrap();

        assert_eq!(servers[0].command, Some("bunx".to_string()));
    }

    #[test]
    fn test_build_npm_fallback_merges_args() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry.args = vec![
            "--verbose".to_string(),
            "--config".to_string(),
            "/path".to_string(),
        ];

        let servers = builder
            .build("pkg", "registry:test", &entry, Some("2.0"), false)
            .unwrap();

        assert_eq!(
            servers[0].args,
            vec!["pkg@2.0", "--verbose", "--config", "/path"]
        );
    }

    #[test]
    fn test_build_npm_fallback_includes_env() {
        let temp = TempDir::new().unwrap();
        let builder = McpServerBuilder::new(temp.path());

        let mut entry = create_test_entry();
        entry
            .env
            .insert("API_KEY".to_string(), "secret123".to_string());
        entry.env.insert("DEBUG".to_string(), "true".to_string());

        let servers = builder
            .build("pkg", "registry:test", &entry, None, false)
            .unwrap();

        assert_eq!(
            servers[0].env.get("API_KEY"),
            Some(&"secret123".to_string())
        );
        assert_eq!(servers[0].env.get("DEBUG"), Some(&"true".to_string()));
    }

    // =========================================================================
    // command_from_registry_source Tests
    // =========================================================================

    #[test]
    fn test_command_from_registry_source_with_command() {
        assert_eq!(
            McpServerBuilder::command_from_registry_source("registry:package:uvx"),
            Some("uvx".to_string())
        );
    }

    #[test]
    fn test_command_from_registry_source_no_command() {
        assert_eq!(
            McpServerBuilder::command_from_registry_source("registry:package"),
            None
        );
    }

    #[test]
    fn test_command_from_registry_source_empty_command() {
        assert_eq!(
            McpServerBuilder::command_from_registry_source("registry:package:"),
            None
        );
    }

    #[test]
    fn test_command_from_registry_source_complex_path() {
        // Three colons: registry:org/package:command
        assert_eq!(
            McpServerBuilder::command_from_registry_source("registry:org/package:npx"),
            Some("npx".to_string())
        );
    }

    // =========================================================================
    // Constants Tests
    // =========================================================================

    #[test]
    fn test_default_runtime_is_shell() {
        assert_eq!(DEFAULT_RUNTIME, "shell");
    }

    #[test]
    fn test_default_version_is_latest() {
        assert_eq!(DEFAULT_VERSION, "latest");
    }
}
