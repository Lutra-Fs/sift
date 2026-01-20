//! Source resolver implementation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::git::{GitFetcher, GitSpec};
use crate::mcpb::{derive_name_from_mcpb_url, is_mcpb_url, normalize_mcpb_source};
use crate::registry::marketplace::MarketplaceAdapter;
use crate::registry::{RegistryConfig, RegistryType};

use super::spec::{LocalSpec, McpbSpec, ResolvedSource};

/// Result of resolving user input to a name and source.
///
/// Used during install command processing to determine the canonical
/// name and normalized source string from user-provided input.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
    /// Canonical name for the package (derived from input or path)
    pub name: String,
    /// Normalized source string with appropriate prefix
    pub source: String,
    /// Whether the source refers to a registry
    pub source_is_registry: bool,
    /// Whether the source was explicitly provided (vs inferred)
    pub source_explicit: bool,
    /// Any warnings generated during resolution
    pub warnings: Vec<String>,
}

/// Metadata about a registry resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryMetadata {
    /// Original registry source (e.g., "registry:anthropic-skills/pdf")
    pub original_source: String,
    /// Registry key (e.g., "anthropic-skills")
    pub registry_key: String,
    /// Canonical name (e.g., "document-skills/xlsx" for nested plugins)
    pub skill_name: String,
    /// Version declared in marketplace.json
    pub marketplace_version: String,
    /// All names that can be used to reference this plugin
    /// (e.g., ["xlsx", "document-skills/xlsx"])
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Parent plugin name if this is a nested plugin
    #[serde(default)]
    pub parent_plugin: Option<String>,
    /// True if this is a group alias that expands to all nested plugins
    #[serde(default)]
    pub is_group: bool,
}

/// Result of resolving a registry source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryResolution {
    /// The resolved git specification
    pub git_spec: GitSpec,
    /// Registry metadata for lockfile
    pub metadata: RegistryMetadata,
}

/// Result of resolving an MCP server from a registry.
///
/// Used when marketplace plugins define `mcpServers` with MCPB bundle URLs
/// or STDIO/HTTP configurations.
#[derive(Debug, Clone)]
pub struct McpRegistryResolution {
    /// The resolved MCP configuration
    pub mcp_config: crate::mcp::McpConfig,
    /// Registry key (e.g., "anthropic-skills")
    pub registry_key: String,
    /// Plugin name from marketplace
    pub plugin_name: String,
    /// Plugin version from marketplace
    pub plugin_version: String,
}

/// Resolves source strings into fetchable specifications.
#[derive(Debug)]
pub struct SourceResolver {
    /// State directory for git operations
    state_dir: PathBuf,
    /// Project root for resolving relative paths
    project_root: PathBuf,
    /// Configured registries
    registries: HashMap<String, RegistryConfig>,
}

impl SourceResolver {
    /// Create a new SourceResolver.
    pub fn new(
        state_dir: PathBuf,
        project_root: PathBuf,
        registries: HashMap<String, RegistryConfig>,
    ) -> Self {
        Self {
            state_dir,
            project_root,
            registries,
        }
    }

    /// Resolve a source string into a fetchable specification.
    ///
    /// Handles:
    /// - `local:./path` or `local:/absolute/path` -> LocalSpec
    /// - `git:url` or `github:org/repo` -> GitSpec
    /// - `registry:name/skill` -> GitSpec (via marketplace resolution)
    /// - `mcpb:url` -> McpbSpec (MCPB bundle)
    pub fn resolve(&self, source: &str) -> anyhow::Result<ResolvedSource> {
        if let Some(path) = source.strip_prefix("local:") {
            return self.resolve_local(path);
        }

        if let Some(url) = source.strip_prefix("mcpb:") {
            return self.resolve_mcpb(url);
        }

        if source.starts_with("git:") || source.starts_with("github:") {
            return self.resolve_git(source);
        }

        if let Some(registry_part) = source.strip_prefix("registry:") {
            let resolution = self.resolve_registry(registry_part)?;
            return Ok(ResolvedSource::Git(resolution.git_spec));
        }

        // Try auto-detection
        self.auto_detect(source)
    }

    /// Resolve a source string and return registry metadata if applicable.
    ///
    /// Use this when you need both the resolved source and registry information
    /// for lockfile updates.
    pub fn resolve_with_metadata(
        &self,
        source: &str,
    ) -> anyhow::Result<(ResolvedSource, Option<RegistryMetadata>)> {
        if let Some(path) = source.strip_prefix("local:") {
            return Ok((self.resolve_local(path)?, None));
        }

        if let Some(url) = source.strip_prefix("mcpb:") {
            return Ok((self.resolve_mcpb(url)?, None));
        }

        if source.starts_with("git:") || source.starts_with("github:") {
            return Ok((self.resolve_git(source)?, None));
        }

        if let Some(registry_part) = source.strip_prefix("registry:") {
            let resolution = self.resolve_registry(registry_part)?;
            return Ok((
                ResolvedSource::Git(resolution.git_spec),
                Some(resolution.metadata),
            ));
        }

        // Auto-detection never produces registry metadata
        Ok((self.auto_detect(source)?, None))
    }

    /// Resolve a local path source.
    fn resolve_local(&self, path: &str) -> anyhow::Result<ResolvedSource> {
        let resolved_path = if path.starts_with('/') {
            PathBuf::from(path)
        } else if path.starts_with("~/") {
            dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
                .join(path.strip_prefix("~/").unwrap_or(path))
        } else {
            // Relative path - resolve from project root
            let stripped = path.strip_prefix("./").unwrap_or(path);
            self.project_root.join(stripped)
        };

        Ok(ResolvedSource::Local(LocalSpec::new(resolved_path)))
    }

    /// Resolve a git source string.
    fn resolve_git(&self, source: &str) -> anyhow::Result<ResolvedSource> {
        let spec = GitSpec::parse(source)?;
        Ok(ResolvedSource::Git(spec))
    }

    /// Resolve an MCPB bundle source string.
    fn resolve_mcpb(&self, url: &str) -> anyhow::Result<ResolvedSource> {
        Ok(ResolvedSource::Mcpb(McpbSpec::new(url)))
    }

    /// Resolve a registry source by fetching and parsing marketplace.json.
    fn resolve_registry(&self, registry_part: &str) -> anyhow::Result<RegistryResolution> {
        let (registry_key, skill_name) = self.parse_registry_source(registry_part)?;

        let config = self
            .registries
            .get(&registry_key)
            .ok_or_else(|| anyhow::anyhow!("Unknown registry: {}", registry_key))?;

        match config.r#type {
            RegistryType::ClaudeMarketplace => {
                if let Some((parent, child)) = skill_name.split_once('/') {
                    self.resolve_claude_marketplace_nested(config, &registry_key, parent, child)
                } else {
                    self.resolve_claude_marketplace(config, &registry_key, skill_name)
                }
            }
            RegistryType::Sift => {
                anyhow::bail!("Sift registry resolution not yet implemented")
            }
        }
    }

    fn resolve_claude_marketplace_nested(
        &self,
        config: &RegistryConfig,
        registry_key: &str,
        parent: &str,
        child: &str,
    ) -> anyhow::Result<RegistryResolution> {
        let marketplace_source = config
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Marketplace registry missing source field"))?;

        let marketplace_spec = GitSpec::parse(marketplace_source)?;

        let fetcher = GitFetcher::new(self.state_dir.clone());
        let manifest_content = fetcher
            .read_root_file(&marketplace_spec, ".claude-plugin/marketplace.json")
            .with_context(|| {
                format!(
                    "Failed to read .claude-plugin/marketplace.json from {}",
                    marketplace_source
                )
            })?;

        let manifest = MarketplaceAdapter::parse(&manifest_content)?;
        let plugin = MarketplaceAdapter::find_plugin(&manifest, parent)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found in registry: {}", parent))?;

        let base_source = MarketplaceAdapter::get_source_string(plugin)?;
        let skill_paths = match &plugin.skills {
            Some(crate::registry::marketplace::adapter::SkillsOrPaths::Single(path)) => {
                vec![path.as_str()]
            }
            Some(crate::registry::marketplace::adapter::SkillsOrPaths::Multiple(paths)) => {
                paths.iter().map(|p| p.as_str()).collect()
            }
            None => {
                anyhow::bail!(
                    "Plugin '{}' does not define nested skills for registry source '{}'",
                    parent,
                    child
                );
            }
        };

        let matched_path = skill_paths
            .iter()
            .find(|path| {
                let trimmed = path.trim_start_matches("./");
                let leaf = trimmed.split('/').next_back().unwrap_or(trimmed);
                leaf == child
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Plugin '{}' does not contain skill path '{}'",
                    parent,
                    child
                )
            })?;

        let actual_source = if matched_path.starts_with("./") {
            format!(
                "{}/{}",
                base_source.trim_end_matches('/'),
                matched_path.trim_start_matches("./")
            )
        } else if matched_path.starts_with('/') {
            matched_path.to_string()
        } else {
            format!("{}/{}", base_source, matched_path)
        };

        let git_spec =
            self.plugin_source_to_git_spec(&actual_source, &marketplace_spec, marketplace_source)?;

        let canonical_name = format!("{}/{}", parent, child);
        Ok(RegistryResolution {
            git_spec,
            metadata: RegistryMetadata {
                original_source: format!("registry:{}/{}", registry_key, canonical_name),
                registry_key: registry_key.to_string(),
                skill_name: canonical_name.clone(),
                marketplace_version: plugin.version.clone(),
                aliases: vec![child.to_string(), canonical_name],
                parent_plugin: Some(parent.to_string()),
                is_group: false,
            },
        })
    }

    /// Resolve a Claude Marketplace registry source.
    fn resolve_claude_marketplace(
        &self,
        config: &RegistryConfig,
        registry_key: &str,
        skill_name: &str,
    ) -> anyhow::Result<RegistryResolution> {
        let marketplace_source = config
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Marketplace registry missing source field"))?;

        // Parse marketplace source to get repo info
        let marketplace_spec = GitSpec::parse(marketplace_source)?;

        // Fetch marketplace.json from .claude-plugin directory per Claude Code Marketplace spec
        let fetcher = GitFetcher::new(self.state_dir.clone());
        let manifest_content = fetcher
            .read_root_file(&marketplace_spec, ".claude-plugin/marketplace.json")
            .with_context(|| {
                format!(
                    "Failed to read .claude-plugin/marketplace.json from {}",
                    marketplace_source
                )
            })?;

        // Parse manifest and find plugin
        let manifest = MarketplaceAdapter::parse(&manifest_content)?;
        let plugin = MarketplaceAdapter::find_plugin(&manifest, skill_name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found in registry: {}", skill_name))?;

        // Get the base plugin source
        let base_source = MarketplaceAdapter::get_source_string(plugin)?;

        // If plugin has skills array, use the first skill path as the subdir
        // This handles the case where a plugin defines multiple skills in a single marketplace entry
        let actual_source = if let Some(skills) = &plugin.skills {
            // Use the first skill path as the installation target
            match skills {
                crate::registry::marketplace::adapter::SkillsOrPaths::Single(path) => {
                    format!(
                        "{}/{}",
                        base_source.trim_end_matches('/'),
                        path.trim_start_matches("./")
                    )
                }
                crate::registry::marketplace::adapter::SkillsOrPaths::Multiple(paths) => {
                    if let Some(first_path) = paths.first() {
                        format!(
                            "{}/{}",
                            base_source.trim_end_matches('/'),
                            first_path.trim_start_matches("./")
                        )
                    } else {
                        base_source.clone()
                    }
                }
            }
        } else {
            base_source.clone()
        };

        // Convert the actual source to git spec
        let git_spec =
            self.plugin_source_to_git_spec(&actual_source, &marketplace_spec, marketplace_source)?;

        Ok(RegistryResolution {
            git_spec,
            metadata: RegistryMetadata {
                original_source: format!("registry:{}/{}", registry_key, skill_name),
                registry_key: registry_key.to_string(),
                skill_name: skill_name.to_string(),
                marketplace_version: plugin.version.clone(),
                aliases: vec![skill_name.to_string()],
                parent_plugin: None,
                is_group: false,
            },
        })
    }

    /// Convert a plugin source string to a GitSpec.
    ///
    /// Handles:
    /// - `./relative/path` -> combines with marketplace repo
    /// - `github:org/repo` -> direct conversion
    /// - `local:path` -> error (not supported for registry plugins)
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn plugin_source_to_git_spec(
        &self,
        plugin_source: &str,
        marketplace_spec: &GitSpec,
        marketplace_source: &str,
    ) -> anyhow::Result<GitSpec> {
        if let Some(relative_path) = plugin_source.strip_prefix("local:") {
            // Relative path within marketplace repo
            let path = relative_path.strip_prefix("./").unwrap_or(relative_path);

            // Combine marketplace repo with plugin path
            let reference = marketplace_spec
                .reference
                .clone()
                .unwrap_or_else(|| "main".to_string());

            // If marketplace already has a subdir, combine them
            let subdir = if let Some(ref base_subdir) = marketplace_spec.subdir {
                format!("{}/{}", base_subdir, path)
            } else {
                path.to_string()
            };

            Ok(GitSpec {
                repo_url: marketplace_spec.repo_url.clone(),
                reference: Some(reference),
                subdir: Some(subdir),
            })
        } else if plugin_source.starts_with("github:") || plugin_source.starts_with("git:") {
            // Direct git source
            GitSpec::parse(plugin_source)
        } else {
            anyhow::bail!(
                "Unsupported plugin source format in {}: {}",
                marketplace_source,
                plugin_source
            )
        }
    }

    /// Parse "registry:name/skill" or "registry:skill" format.
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn parse_registry_source<'a>(
        &self,
        source: &'a str,
    ) -> anyhow::Result<(String, &'a str)> {
        if let Some((key, name)) = source.split_once('/') {
            Ok((key.to_string(), name))
        } else {
            // No explicit registry key - use default if only one registry exists
            if self.registries.len() == 1 {
                let key = self.registries.keys().next().unwrap().clone();
                Ok((key, source))
            } else if self.registries.is_empty() {
                anyhow::bail!("No registries configured")
            } else {
                anyhow::bail!(
                    "Multiple registries configured; specify registry explicitly: registry:NAME/{}",
                    source
                )
            }
        }
    }

    /// Auto-detect source type from input.
    fn auto_detect(&self, input: &str) -> anyhow::Result<ResolvedSource> {
        // Check if it looks like a path
        if input.starts_with('/')
            || input.starts_with("./")
            || input.starts_with("../")
            || input.starts_with("~/")
        {
            return self.resolve_local(input);
        }

        // Check if path exists relative to project root
        if self.project_root.join(input).exists() {
            return self.resolve_local(&format!("./{}", input));
        }

        // Check if it looks like a git URL
        if input.starts_with("http://")
            || input.starts_with("https://")
            || input.starts_with("git@")
            || input.contains("/tree/")
        {
            let spec = GitSpec::parse(&format!("git:{}", input))?;
            return Ok(ResolvedSource::Git(spec));
        }

        // Default: treat as registry source
        let resolution = self.resolve_registry(input)?;
        Ok(ResolvedSource::Git(resolution.git_spec))
    }

    /// Resolve a registry source with potential nested marketplace expansion.
    ///
    /// Returns multiple resolutions if a nested marketplace is detected:
    /// - First entry: Parent plugin with `is_group: true` (group alias)
    /// - Subsequent entries: Each nested plugin with fully qualified names
    pub fn resolve_registry_with_expansion(
        &self,
        registry_part: &str,
    ) -> anyhow::Result<Vec<RegistryResolution>> {
        let (registry_key, skill_name) = self.parse_registry_source(registry_part)?;

        let config = self
            .registries
            .get(&registry_key)
            .ok_or_else(|| anyhow::anyhow!("Unknown registry: {}", registry_key))?;

        match config.r#type {
            RegistryType::ClaudeMarketplace => {
                self.expand_nested_marketplace(config, &registry_key, skill_name)
            }
            RegistryType::Sift => {
                anyhow::bail!("Sift registry resolution not yet implemented")
            }
        }
    }

    /// Expand a potentially nested marketplace into multiple resolutions.
    ///
    /// This method:
    /// 1. Resolves the plugin normally
    /// 2. Checks if the plugin contains a nested marketplace.json
    /// 3. If yes, expands into parent + all nested plugins
    /// 4. If no, returns single resolution for the plugin itself
    fn expand_nested_marketplace(
        &self,
        config: &RegistryConfig,
        registry_key: &str,
        skill_name: &str,
    ) -> anyhow::Result<Vec<RegistryResolution>> {
        let marketplace_source = config
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Marketplace registry missing source field"))?;

        let marketplace_spec = GitSpec::parse(marketplace_source)?;

        // Fetch marketplace.json from .claude-plugin directory
        let fetcher = GitFetcher::new(self.state_dir.clone());
        let manifest_content = fetcher
            .read_root_file(&marketplace_spec, ".claude-plugin/marketplace.json")
            .with_context(|| {
                format!(
                    "Failed to read .claude-plugin/marketplace.json from {}",
                    marketplace_source
                )
            })?;

        // Parse manifest and find plugin
        let manifest = MarketplaceAdapter::parse(&manifest_content)?;
        let plugin = MarketplaceAdapter::find_plugin(&manifest, skill_name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found in registry: {}", skill_name))?;

        // Get the base plugin source
        let base_source = MarketplaceAdapter::get_source_string(plugin)?;

        // Check if plugin has a skills array - this means we need to expand
        let skill_paths: Vec<String> = if let Some(skills) = &plugin.skills {
            match skills {
                crate::registry::marketplace::adapter::SkillsOrPaths::Single(path) => {
                    vec![path.trim_start_matches("./").to_string()]
                }
                crate::registry::marketplace::adapter::SkillsOrPaths::Multiple(paths) => paths
                    .iter()
                    .map(|p| p.trim_start_matches("./").to_string())
                    .collect(),
            }
        } else {
            vec![]
        };

        // If plugin has multiple skill paths, expand into multiple resolutions
        if skill_paths.len() > 1 {
            // Create parent resolution (group alias)
            let parent_resolution = RegistryResolution {
                git_spec: GitSpec {
                    repo_url: marketplace_spec.repo_url.clone(),
                    reference: marketplace_spec.reference.clone(),
                    subdir: Some("".to_string()), // Root of marketplace
                },
                metadata: RegistryMetadata {
                    original_source: format!("registry:{}/{}", registry_key, skill_name),
                    registry_key: registry_key.to_string(),
                    skill_name: skill_name.to_string(),
                    marketplace_version: plugin.version.clone(),
                    aliases: vec![skill_name.to_string()],
                    parent_plugin: None,
                    is_group: true,
                },
            };

            // Create resolutions for each skill path
            let mut nested_resolutions = Vec::new();
            for skill_path in &skill_paths {
                let full_path = format!("{}/{}", base_source.trim_end_matches('/'), skill_path);
                let git_spec = self.plugin_source_to_git_spec(
                    &full_path,
                    &marketplace_spec,
                    marketplace_source,
                )?;

                // Extract short name from skill path (e.g., "skills/xlsx" -> "xlsx")
                let short_name = skill_path
                    .split('/')
                    .next_back()
                    .unwrap_or(skill_path)
                    .to_string();
                let canonical_name = format!("{}/{}", skill_name, short_name);

                nested_resolutions.push(RegistryResolution {
                    git_spec,
                    metadata: RegistryMetadata {
                        original_source: format!("registry:{}/{}", registry_key, canonical_name),
                        registry_key: registry_key.to_string(),
                        skill_name: canonical_name.clone(),
                        marketplace_version: plugin.version.clone(),
                        aliases: vec![short_name, canonical_name],
                        parent_plugin: Some(skill_name.to_string()),
                        is_group: false,
                    },
                });
            }

            // Return parent + all nested resolutions
            let mut all_resolutions = vec![parent_resolution];
            all_resolutions.extend(nested_resolutions);
            return Ok(all_resolutions);
        }

        // Single skill or no skills array - use the first/only path
        let actual_source = if let Some(first_path) = skill_paths.first() {
            format!("{}/{}", base_source.trim_end_matches('/'), first_path)
        } else {
            base_source.clone()
        };

        // Convert the actual source to git spec
        let git_spec =
            self.plugin_source_to_git_spec(&actual_source, &marketplace_spec, marketplace_source)?;

        // Check for nested marketplace
        let has_nested = fetcher.has_marketplace_manifest(&git_spec)?;

        if !has_nested {
            // No nested marketplace - return single resolution
            return Ok(vec![RegistryResolution {
                git_spec,
                metadata: RegistryMetadata {
                    original_source: format!("registry:{}/{}", registry_key, skill_name),
                    registry_key: registry_key.to_string(),
                    skill_name: skill_name.to_string(),
                    marketplace_version: plugin.version.clone(),
                    aliases: vec![skill_name.to_string()],
                    parent_plugin: None,
                    is_group: false,
                },
            }]);
        }

        // Nested marketplace detected - expand it
        let nested_content = fetcher.read_nested_marketplace(&git_spec)?;
        let nested_manifest = MarketplaceAdapter::parse(&nested_content)?;

        // Create parent resolution (group alias)
        let parent_resolution = RegistryResolution {
            git_spec: git_spec.clone(),
            metadata: RegistryMetadata {
                original_source: format!("registry:{}/{}", registry_key, skill_name),
                registry_key: registry_key.to_string(),
                skill_name: skill_name.to_string(),
                marketplace_version: plugin.version.clone(),
                aliases: vec![skill_name.to_string()],
                parent_plugin: None,
                is_group: true,
            },
        };

        // Create resolutions for each nested plugin
        let mut nested_resolutions = Vec::new();
        for nested_plugin in &nested_manifest.plugins {
            let nested_base_source = MarketplaceAdapter::get_source_string(nested_plugin)?;
            let nested_source = if let Some(skills) = &nested_plugin.skills {
                match skills {
                    crate::registry::marketplace::adapter::SkillsOrPaths::Single(path) => {
                        format!(
                            "{}/{}",
                            nested_base_source.trim_end_matches('/'),
                            path.trim_start_matches("./")
                        )
                    }
                    crate::registry::marketplace::adapter::SkillsOrPaths::Multiple(paths) => {
                        if let Some(first_path) = paths.first() {
                            format!(
                                "{}/{}",
                                nested_base_source.trim_end_matches('/'),
                                first_path.trim_start_matches("./")
                            )
                        } else {
                            nested_base_source.clone()
                        }
                    }
                }
            } else {
                nested_base_source.clone()
            };

            // Combine parent's git_spec with nested plugin's path
            let nested_git_spec = self.plugin_source_to_git_spec(
                &nested_source,
                &marketplace_spec,
                marketplace_source,
            )?;

            let canonical_name = format!("{}/{}", skill_name, nested_plugin.name);
            let short_name = nested_plugin.name.clone();

            nested_resolutions.push(RegistryResolution {
                git_spec: nested_git_spec,
                metadata: RegistryMetadata {
                    original_source: format!("registry:{}/{}", registry_key, canonical_name),
                    registry_key: registry_key.to_string(),
                    skill_name: canonical_name.clone(),
                    marketplace_version: nested_plugin.version.clone(),
                    aliases: vec![short_name, canonical_name],
                    parent_plugin: Some(skill_name.to_string()),
                    is_group: false,
                },
            });
        }

        // Return parent (group) + all nested resolutions
        let mut all_resolutions = vec![parent_resolution];
        all_resolutions.extend(nested_resolutions);
        Ok(all_resolutions)
    }

    /// Resolve MCP server configurations from a marketplace plugin.
    ///
    /// This method extracts MCP server definitions from the `mcpServers` field
    /// of a marketplace plugin and converts them to `McpRegistryResolution`.
    ///
    /// Supports:
    /// - MCPB bundle URLs (e.g., `"https://example.com/server.mcpb"`)
    /// - HTTP URLs for remote MCP servers
    /// - STDIO command configurations
    /// - Named server objects with multiple servers
    ///
    /// Returns an empty vec if the plugin has no `mcpServers` field.
    pub fn resolve_mcp_from_plugin(
        &self,
        plugin: &crate::registry::marketplace::MarketplacePlugin,
        registry_key: &str,
    ) -> anyhow::Result<Vec<McpRegistryResolution>> {
        let configs = MarketplaceAdapter::plugin_to_mcp_configs(plugin, None)?;

        if configs.is_empty() {
            return Ok(Vec::new());
        }

        let mut resolutions = Vec::new();
        for (name, mcp_config) in configs {
            resolutions.push(McpRegistryResolution {
                mcp_config,
                registry_key: registry_key.to_string(),
                plugin_name: name,
                plugin_version: plugin.version.clone(),
            });
        }

        Ok(resolutions)
    }

    /// Resolve an MCP server from a registry source string.
    ///
    /// Handles `registry:name` or `registry:key/name` format.
    /// Fetches marketplace.json, finds the plugin, and extracts mcpServers.
    ///
    /// Returns `None` if the plugin exists but has no mcpServers defined
    /// (it's a skill-only plugin).
    pub fn resolve_mcp_registry(
        &self,
        registry_part: &str,
    ) -> anyhow::Result<Option<Vec<McpRegistryResolution>>> {
        let (registry_key, plugin_name) = self.parse_registry_source(registry_part)?;

        let config = self
            .registries
            .get(&registry_key)
            .ok_or_else(|| anyhow::anyhow!("Unknown registry: {}", registry_key))?;

        match config.r#type {
            RegistryType::ClaudeMarketplace => {
                let marketplace_source = config
                    .source
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Marketplace registry missing source field"))?;

                let marketplace_spec = GitSpec::parse(marketplace_source)?;

                let fetcher = GitFetcher::new(self.state_dir.clone());
                let manifest_content = fetcher
                    .read_root_file(&marketplace_spec, ".claude-plugin/marketplace.json")
                    .with_context(|| {
                        format!(
                            "Failed to read .claude-plugin/marketplace.json from {}",
                            marketplace_source
                        )
                    })?;

                let manifest = MarketplaceAdapter::parse(&manifest_content)?;
                let plugin =
                    MarketplaceAdapter::find_plugin(&manifest, plugin_name).ok_or_else(|| {
                        anyhow::anyhow!("Plugin not found in registry: {}", plugin_name)
                    })?;

                let resolutions = self.resolve_mcp_from_plugin(plugin, &registry_key)?;
                if resolutions.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(resolutions))
                }
            }
            RegistryType::Sift => {
                anyhow::bail!("Sift registry MCP resolution not yet implemented")
            }
        }
    }

    /// Detect name collisions across plugin aliases.
    ///
    /// Returns an error if the same alias maps to different plugins.
    /// This helps prevent ambiguity when installing with short names.
    pub fn detect_collisions(resolutions: &[RegistryResolution]) -> anyhow::Result<()> {
        use std::collections::HashMap;
        let mut alias_to_plugins: HashMap<&str, Vec<&str>> = HashMap::new();

        for resolution in resolutions {
            for alias in &resolution.metadata.aliases {
                alias_to_plugins
                    .entry(alias.as_str())
                    .or_default()
                    .push(resolution.metadata.skill_name.as_str());
            }
        }

        // Find aliases that map to multiple plugins
        let collisions: Vec<_> = alias_to_plugins
            .iter()
            .filter(|(_, plugins)| plugins.len() > 1)
            .collect();

        if !collisions.is_empty() {
            let mut collision_msg =
                String::from("Name collision detected. The following names are ambiguous:\n");
            for (alias, plugins) in collisions {
                collision_msg.push_str(&format!(
                    "  - '{}' maps to multiple plugins: {}\n",
                    alias,
                    plugins.join(", ")
                ));
            }
            collision_msg.push_str("\nUse fully qualified names to disambiguate.");
            anyhow::bail!("{}", collision_msg);
        }

        Ok(())
    }

    // ========================================================================
    // Input Resolution Methods
    // ========================================================================
    // These methods handle user input resolution for install commands,
    // determining the canonical name and normalized source from raw input.

    /// Resolve user input to a name and source.
    ///
    /// This is the main entry point for resolving install command input.
    /// It handles:
    /// - Explicit source: normalizes and uses user-provided source
    /// - Inferred source: detects source type from input pattern
    /// - Registry fallback: treats unknown input as registry package name
    ///
    /// # Arguments
    /// * `input` - User-provided input (name or path)
    /// * `source` - Optional explicit source specification
    /// * `registry` - Optional registry name for disambiguation
    pub fn resolve_input(
        &self,
        input: &str,
        source: Option<&str>,
        registry: Option<&str>,
    ) -> anyhow::Result<ResolvedInput> {
        let mut warnings = Vec::new();

        if let Some(explicit) = source {
            let (normalized, normalized_warning) = self.normalize_source(explicit)?;
            if let Some(warning) = normalized_warning {
                warnings.push(warning);
            }
            if registry.is_some() {
                warnings.push("Ignoring --registry because --source was provided.".to_string());
            }
            let is_registry = normalized.starts_with("registry:");
            return Ok(ResolvedInput {
                name: input.to_string(),
                source: normalized,
                source_is_registry: is_registry,
                source_explicit: true,
                warnings,
            });
        }

        // No explicit source - infer from input
        let mut result = self.infer_input_with_registry(input, registry)?;
        result.warnings.extend(warnings);
        Ok(result)
    }

    /// Infer source type from input string.
    ///
    /// Attempts to detect the source type from the input pattern:
    /// - Local path patterns (./path, ../path, /path, ~/path)
    /// - Existing directories in project root
    /// - MCPB URLs (*.mcpb)
    /// - Git URLs (https://, git://, git@, github:, etc.)
    /// - Falls back to registry source for plain names
    pub fn infer_input(&self, input: &str) -> anyhow::Result<ResolvedInput> {
        self.infer_input_with_registry(input, None)
    }

    /// Infer source type from input string with optional registry.
    pub fn infer_input_with_registry(
        &self,
        input: &str,
        registry: Option<&str>,
    ) -> anyhow::Result<ResolvedInput> {
        // Try local path detection
        if let Some(result) = self.try_infer_local(input) {
            return Ok(result);
        }

        // Try MCPB URL detection (before git, as MCPB URLs are also HTTPS)
        if let Some(result) = self.try_infer_mcpb(input) {
            return Ok(result);
        }

        // Try git URL detection
        if let Some(result) = self.try_infer_git(input) {
            return Ok(result);
        }

        // Fall back to registry source
        let source = if let Some(selected) = registry {
            format!("registry:{}/{}", selected, input)
        } else {
            format!("registry:{}", input)
        };

        Ok(ResolvedInput {
            name: input.to_string(),
            source,
            source_is_registry: true,
            source_explicit: registry.is_some(),
            warnings: Vec::new(),
        })
    }

    /// Normalize an explicit source string.
    ///
    /// If the source already has a recognized prefix, returns it unchanged.
    /// Otherwise, attempts to detect and normalize:
    /// - Local paths → `local:` prefix
    /// - MCPB URLs → `mcpb:` prefix
    /// - Git URLs → `git:` prefix
    ///
    /// Returns the normalized source and an optional warning message.
    pub fn normalize_source(&self, source: &str) -> anyhow::Result<(String, Option<String>)> {
        // Already prefixed sources pass through unchanged
        if source.starts_with("registry:")
            || source.starts_with("local:")
            || source.starts_with("github:")
            || source.starts_with("git:")
            || source.starts_with("mcpb:")
        {
            return Ok((source.to_string(), None));
        }

        // Try local path detection
        if is_local_path(source, &self.project_root) {
            let normalized = format!("local:{}", source);
            return Ok((
                normalized.clone(),
                Some(format!(
                    "Normalized source '{}' to '{}'.",
                    source, normalized
                )),
            ));
        }

        // Try MCPB URL detection (before git)
        if is_mcpb_url(source)
            && let Some(normalized) = normalize_mcpb_source(source)
        {
            return Ok((
                normalized.clone(),
                Some(format!(
                    "Normalized source '{}' to '{}'.",
                    source, normalized
                )),
            ));
        }

        // Try git URL detection
        if is_git_like(source) {
            let normalized = normalize_git_source(source);
            return Ok((
                normalized.clone(),
                Some(format!(
                    "Normalized source '{}' to '{}'.",
                    source, normalized
                )),
            ));
        }

        anyhow::bail!(
            "Invalid source format: must be 'registry:', 'local:', 'github:', 'git:', 'mcpb:', a path, or a git URL"
        )
    }

    /// Get project root path (for testing and external access).
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    // --- Private inference helpers ---

    fn try_infer_local(&self, input: &str) -> Option<ResolvedInput> {
        if !is_local_path(input, &self.project_root) {
            return None;
        }
        let name = derive_name_from_path(input).ok()?;
        Some(ResolvedInput {
            name,
            source: format!("local:{}", input),
            source_is_registry: false,
            source_explicit: false,
            warnings: Vec::new(),
        })
    }

    fn try_infer_git(&self, input: &str) -> Option<ResolvedInput> {
        if !is_git_like(input) {
            return None;
        }
        let source = normalize_git_source(input);
        let name = derive_name_from_git_source(&source).ok()?;
        Some(ResolvedInput {
            name,
            source,
            source_is_registry: false,
            source_explicit: false,
            warnings: Vec::new(),
        })
    }

    fn try_infer_mcpb(&self, input: &str) -> Option<ResolvedInput> {
        if !is_mcpb_url(input) {
            return None;
        }
        let source = normalize_mcpb_source(input)?;
        let name = derive_name_from_mcpb_url(input).ok()?;
        Some(ResolvedInput {
            name,
            source,
            source_is_registry: false,
            source_explicit: false,
            warnings: Vec::new(),
        })
    }
}

// ============================================================================
// Public Helper Functions
// ============================================================================
// These functions are used by the resolver and exported for testing.

/// Check if input looks like a local path.
///
/// Returns true if:
/// - Starts with `./`, `../`, `/`, or `~/`
/// - Exists as a directory in the project root
pub fn is_local_path(input: &str, project_root: &Path) -> bool {
    if input.starts_with("./")
        || input.starts_with("../")
        || input.starts_with('/')
        || input.starts_with("~/")
    {
        return true;
    }
    project_root.join(input).exists()
}

/// Check if input looks like a git URL or reference.
pub fn is_git_like(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git://")
        || input.starts_with("git+")
        || input.starts_with("github:")
        || input.starts_with("git:")
        || input.starts_with("git@")
}

/// Normalize a git-like input to a git: prefixed source.
pub fn normalize_git_source(input: &str) -> String {
    // Convert git+ prefix to git:
    if let Some(stripped) = input.strip_prefix("git+") {
        return format!("git:{}", stripped);
    }
    // Add git: prefix to raw URLs
    if input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git://")
        || input.starts_with("git@")
    {
        return format!("git:{}", input);
    }
    // Already prefixed or shorthand (github:, git:)
    input.to_string()
}

/// Derive a package name from a file path.
pub fn derive_name_from_path(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim_end_matches('/');
    let file_name = Path::new(trimmed)
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid path for skill name: {}", input))?;
    Ok(file_name.to_string_lossy().to_string())
}

/// Derive a package name from a git source string.
pub fn derive_name_from_git_source(source: &str) -> anyhow::Result<String> {
    let raw = source
        .strip_prefix("git:")
        .or_else(|| source.strip_prefix("github:"))
        .unwrap_or(source)
        .trim_end_matches('/');
    let segment = raw
        .rsplit('/')
        .next()
        .unwrap_or(raw)
        .rsplit(':')
        .next()
        .unwrap_or(raw);
    let name = segment.strip_suffix(".git").unwrap_or(segment);
    if name.is_empty() {
        anyhow::bail!("Invalid git source for skill name: {}", source);
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_collisions_no_collisions() {
        let resolutions = vec![
            RegistryResolution {
                git_spec: GitSpec::new("https://github.com/test/repo1"),
                metadata: RegistryMetadata {
                    original_source: "registry:test/plugin1".to_string(),
                    registry_key: "test".to_string(),
                    skill_name: "plugin1".to_string(),
                    marketplace_version: "1.0.0".to_string(),
                    aliases: vec!["plugin1".to_string(), "test/plugin1".to_string()],
                    parent_plugin: None,
                    is_group: false,
                },
            },
            RegistryResolution {
                git_spec: GitSpec::new("https://github.com/test/repo2"),
                metadata: RegistryMetadata {
                    original_source: "registry:test/plugin2".to_string(),
                    registry_key: "test".to_string(),
                    skill_name: "plugin2".to_string(),
                    marketplace_version: "1.0.0".to_string(),
                    aliases: vec!["plugin2".to_string(), "test/plugin2".to_string()],
                    parent_plugin: None,
                    is_group: false,
                },
            },
        ];

        assert!(SourceResolver::detect_collisions(&resolutions).is_ok());
    }

    #[test]
    fn test_detect_collisions_with_collisions() {
        let resolutions = vec![
            RegistryResolution {
                git_spec: GitSpec::new("https://github.com/test/repo1"),
                metadata: RegistryMetadata {
                    original_source: "registry:test/plugin1".to_string(),
                    registry_key: "test".to_string(),
                    skill_name: "plugin1".to_string(),
                    marketplace_version: "1.0.0".to_string(),
                    aliases: vec!["common".to_string()],
                    parent_plugin: None,
                    is_group: false,
                },
            },
            RegistryResolution {
                git_spec: GitSpec::new("https://github.com/test/repo2"),
                metadata: RegistryMetadata {
                    original_source: "registry:test/plugin2".to_string(),
                    registry_key: "test".to_string(),
                    skill_name: "plugin2".to_string(),
                    marketplace_version: "1.0.0".to_string(),
                    aliases: vec!["common".to_string()],
                    parent_plugin: None,
                    is_group: false,
                },
            },
        ];

        let result = SourceResolver::detect_collisions(&resolutions);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("common"));
        assert!(err_msg.contains("plugin1"));
        assert!(err_msg.contains("plugin2"));
    }

    #[test]
    fn test_detect_collisions_with_nested_plugins() {
        let resolutions = vec![
            RegistryResolution {
                git_spec: GitSpec::new("https://github.com/test/repo1"),
                metadata: RegistryMetadata {
                    original_source: "registry:test/document-skills".to_string(),
                    registry_key: "test".to_string(),
                    skill_name: "document-skills".to_string(),
                    marketplace_version: "1.0.0".to_string(),
                    aliases: vec!["document-skills".to_string()],
                    parent_plugin: None,
                    is_group: true,
                },
            },
            RegistryResolution {
                git_spec: GitSpec::new("https://github.com/test/repo2"),
                metadata: RegistryMetadata {
                    original_source: "registry:test/document-skills/xlsx".to_string(),
                    registry_key: "test".to_string(),
                    skill_name: "document-skills/xlsx".to_string(),
                    marketplace_version: "1.0.0".to_string(),
                    aliases: vec!["xlsx".to_string(), "document-skills/xlsx".to_string()],
                    parent_plugin: Some("document-skills".to_string()),
                    is_group: false,
                },
            },
        ];

        // No collision - parent and child have different aliases
        assert!(SourceResolver::detect_collisions(&resolutions).is_ok());
    }
}
