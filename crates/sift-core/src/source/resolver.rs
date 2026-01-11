//! Source resolver implementation.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;

use crate::git::{GitFetcher, GitSpec};
use crate::registry::marketplace::MarketplaceAdapter;
use crate::registry::{RegistryConfig, RegistryType};

use super::spec::{LocalSpec, ResolvedSource};

/// Metadata about a registry resolution.
#[derive(Debug, Clone)]
pub struct RegistryMetadata {
    /// Original registry source (e.g., "registry:anthropic-skills/pdf")
    pub original_source: String,
    /// Registry key (e.g., "anthropic-skills")
    pub registry_key: String,
    /// Plugin/skill name within the registry
    pub skill_name: String,
    /// Version declared in marketplace.json
    pub marketplace_version: String,
}

/// Result of resolving a registry source.
#[derive(Debug, Clone)]
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

        // Fetch marketplace.json via git
        let fetcher = GitFetcher::new(self.state_dir.clone());
        let manifest_content = fetcher
            .read_root_file(&marketplace_spec, "marketplace.json")
            .with_context(|| {
                format!(
                    "Failed to read marketplace.json from {}",
                    marketplace_source
                )
            })?;

        // Parse manifest and find plugin
        let manifest = MarketplaceAdapter::parse(&manifest_content)?;
        let plugin = MarketplaceAdapter::find_plugin(&manifest, skill_name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found in registry: {}", skill_name))?;

        // Convert plugin source to git spec
        let plugin_source = MarketplaceAdapter::get_source_string(plugin)?;
        let git_spec =
            self.plugin_source_to_git_spec(&plugin_source, &marketplace_spec, marketplace_source)?;

        Ok(RegistryResolution {
            git_spec,
            metadata: RegistryMetadata {
                original_source: format!("registry:{}/{}", registry_key, skill_name),
                registry_key: registry_key.to_string(),
                skill_name: skill_name.to_string(),
                marketplace_version: plugin.version.clone(),
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
}
