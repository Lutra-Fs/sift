//! Source resolver implementation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;

use crate::git::{GitFetcher, GitSpec};
use crate::registry::marketplace::MarketplaceAdapter;
use crate::registry::{RegistryConfig, RegistryType};

use super::spec::{LocalSpec, ResolvedSource};

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
    pub fn resolve(&self, source: &str) -> anyhow::Result<ResolvedSource> {
        if let Some(path) = source.strip_prefix("local:") {
            return self.resolve_local(path);
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

    /// Resolve a registry source by fetching and parsing marketplace.json.
    fn resolve_registry(&self, registry_part: &str) -> anyhow::Result<RegistryResolution> {
        let (registry_key, skill_name) = self.parse_registry_source(registry_part)?;

        let config = self
            .registries
            .get(&registry_key)
            .ok_or_else(|| anyhow::anyhow!("Unknown registry: {}", registry_key))?;

        match config.r#type {
            RegistryType::ClaudeMarketplace => {
                self.resolve_claude_marketplace(config, &registry_key, skill_name)
            }
            RegistryType::Sift => {
                anyhow::bail!("Sift registry resolution not yet implemented")
            }
        }
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
