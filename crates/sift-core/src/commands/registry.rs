//! Registry command implementation.
//!
//! Manages registry configurations in sift.toml across scopes.

use std::path::PathBuf;

use anyhow::Context;

use crate::config::{ConfigScope, ConfigStore, RegistryConfigEntry};
use crate::registry::{RegistryConfig, RegistryType};

/// Options for listing registries
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    /// Filter by scope (None = all scopes)
    pub scope_filter: Option<ConfigScope>,
}

impl ListOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_scope(mut self, scope: ConfigScope) -> Self {
        self.scope_filter = Some(scope);
        self
    }
}

/// Options for adding a registry
#[derive(Debug, Clone)]
pub struct AddOptions {
    /// Registry name (identifier used in sift.toml)
    pub name: String,
    /// Registry URL or source
    pub source: String,
    /// Explicit registry type (None = auto-detect)
    pub registry_type: Option<RegistryType>,
    /// Configuration scope
    pub scope: ConfigScope,
    /// Force overwrite existing
    pub force: bool,
}

impl AddOptions {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            registry_type: None,
            scope: ConfigScope::Global,
            force: false,
        }
    }

    pub fn with_type(mut self, registry_type: RegistryType) -> Self {
        self.registry_type = Some(registry_type);
        self
    }

    pub fn with_scope(mut self, scope: ConfigScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
}

/// Options for removing a registry
#[derive(Debug, Clone)]
pub struct RemoveOptions {
    /// Registry name to remove
    pub name: String,
    /// Scope to remove from (None = try global first, then shared)
    pub scope: Option<ConfigScope>,
    /// Remove from all scopes
    pub all_scopes: bool,
}

impl RemoveOptions {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            scope: None,
            all_scopes: false,
        }
    }

    pub fn with_scope(mut self, scope: ConfigScope) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn with_all_scopes(mut self, all: bool) -> Self {
        self.all_scopes = all;
        self
    }
}

/// Result of a registry operation
#[derive(Debug, Clone)]
pub struct RegistryReport {
    /// Registry name
    pub name: String,
    /// Whether the operation changed anything
    pub changed: bool,
    /// Scope that was modified
    pub scope: ConfigScope,
    /// Any warnings generated
    pub warnings: Vec<String>,
}

/// Listed registry entry
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Registry name (key in sift.toml)
    pub name: String,
    /// Registry type
    pub registry_type: RegistryType,
    /// URL for sift-type registries
    pub url: Option<String>,
    /// Source for claude-marketplace type
    pub source: Option<String>,
    /// Config scope where this registry is defined
    pub scope: ConfigScope,
}

/// Registry command orchestrator
#[derive(Debug)]
pub struct RegistryCommand {
    global_config_dir: PathBuf,
    project_root: PathBuf,
}

impl RegistryCommand {
    /// Create a new registry command with explicit paths
    pub fn new(global_config_dir: PathBuf, project_root: PathBuf) -> Self {
        Self {
            global_config_dir,
            project_root,
        }
    }

    /// Create a registry command with default paths
    pub fn with_defaults() -> anyhow::Result<Self> {
        let global_config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("sift");
        let project_root = std::env::current_dir()?;
        Ok(Self::new(global_config_dir, project_root))
    }

    /// List configured registries
    pub fn list(&self, options: &ListOptions) -> anyhow::Result<Vec<RegistryEntry>> {
        let mut entries = Vec::new();

        // Determine which scopes to check
        let scopes = match options.scope_filter {
            Some(scope) => vec![scope],
            None => vec![ConfigScope::Global, ConfigScope::PerProjectShared],
        };

        for scope in scopes {
            let store = self.create_config_store(scope);
            let config = store.load()?;

            for (name, entry) in &config.registry {
                let registry_config: RegistryConfig = entry
                    .clone()
                    .try_into()
                    .with_context(|| format!("Invalid registry config: {}", name))?;

                entries.push(RegistryEntry {
                    name: name.clone(),
                    registry_type: registry_config.r#type,
                    url: registry_config.url.map(|u| u.to_string()),
                    source: registry_config.source,
                    scope,
                });
            }
        }

        Ok(entries)
    }

    /// Add a new registry
    pub fn add(&self, options: &AddOptions) -> anyhow::Result<RegistryReport> {
        let warnings = Vec::new();

        // Resolve source and type
        let (registry_type, url, source) =
            resolve_source_and_type(&options.source, options.registry_type)?;

        // Build the config entry
        let entry = RegistryConfigEntry {
            r#type: match registry_type {
                RegistryType::Sift => "sift".to_string(),
                RegistryType::ClaudeMarketplace => "claude-marketplace".to_string(),
            },
            url,
            source,
        };

        // Validate before writing
        let registry_config: RegistryConfig = entry
            .clone()
            .try_into()
            .context("Failed to convert registry entry")?;
        registry_config
            .validate()
            .context("Invalid registry configuration")?;

        // Load and modify config
        let store = self.create_config_store(options.scope);
        let mut config = store.load()?;

        // Check for existing entry
        let existing_entry = config.registry.get(&options.name);
        if let Some(existing_entry) = existing_entry
            && existing_entry != &entry
            && !options.force
        {
            anyhow::bail!(
                "Registry '{}' already exists. Use --force to overwrite.",
                options.name
            );
        }

        let changed = existing_entry != Some(&entry);

        if changed {
            config.registry.insert(options.name.clone(), entry);
            store.save(&config)?;
        }

        Ok(RegistryReport {
            name: options.name.clone(),
            changed,
            scope: options.scope,
            warnings,
        })
    }

    /// Remove a registry
    pub fn remove(&self, options: &RemoveOptions) -> anyhow::Result<RegistryReport> {
        let scopes = if options.all_scopes {
            vec![ConfigScope::Global, ConfigScope::PerProjectShared]
        } else if let Some(scope) = options.scope {
            vec![scope]
        } else {
            // Try both scopes, starting with global
            vec![ConfigScope::Global, ConfigScope::PerProjectShared]
        };

        let mut removed_from = None;

        for scope in scopes {
            let store = self.create_config_store(scope);
            let mut config = store.load()?;

            if config.registry.remove(&options.name).is_some() {
                store.save(&config)?;
                removed_from = Some(scope);
                // If not removing from all scopes, stop after first removal
                if !options.all_scopes {
                    break;
                }
            }
        }

        match removed_from {
            Some(scope) => Ok(RegistryReport {
                name: options.name.clone(),
                changed: true,
                scope,
                warnings: Vec::new(),
            }),
            None => anyhow::bail!("Registry '{}' not found", options.name),
        }
    }

    fn create_config_store(&self, scope: ConfigScope) -> ConfigStore {
        ConfigStore::from_paths(
            scope,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        )
    }
}

/// Resolve source string and optional explicit type to registry components.
///
/// Auto-detection rules:
/// - `github:` or `git:` prefix -> ClaudeMarketplace
/// - `https://` or `http://` URL -> Sift
fn resolve_source_and_type(
    source: &str,
    explicit_type: Option<RegistryType>,
) -> anyhow::Result<(RegistryType, Option<String>, Option<String>)> {
    // Explicit type takes precedence
    if let Some(registry_type) = explicit_type {
        return match registry_type {
            RegistryType::Sift => {
                let url = url::Url::parse(source).map_err(|e| {
                    anyhow::anyhow!("Invalid URL for sift registry '{}': {}", source, e)
                })?;
                Ok((RegistryType::Sift, Some(url.to_string()), None))
            }
            RegistryType::ClaudeMarketplace => Ok((
                RegistryType::ClaudeMarketplace,
                None,
                Some(source.to_string()),
            )),
        };
    }

    // Auto-detect type from source format
    if source.starts_with("github:") || source.starts_with("git:") {
        return Ok((
            RegistryType::ClaudeMarketplace,
            None,
            Some(source.to_string()),
        ));
    }

    // Try parsing as URL (sift registry)
    if let Ok(url) = url::Url::parse(source)
        && (url.scheme() == "http" || url.scheme() == "https")
    {
        return Ok((RegistryType::Sift, Some(url.to_string()), None));
    }

    anyhow::bail!(
        "Could not determine registry type from source '{}'. \
         Use --type to specify, or use a recognized format:\n\
         - Sift registry: https://registry.example.com/v1\n\
         - Claude Marketplace: github:org/repo",
        source
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, RegistryCommand) {
        let temp = TempDir::new().unwrap();
        let global_config = temp.path().join("config");
        let project = temp.path().join("project");

        std::fs::create_dir_all(&global_config).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        let cmd = RegistryCommand::new(global_config, project);
        (temp, cmd)
    }

    // ==========================================================================
    // List tests
    // ==========================================================================

    #[test]
    fn test_list_empty_returns_empty_vec() {
        let (_temp, cmd) = setup_test_env();
        let options = ListOptions::new();

        let entries = cmd.list(&options).unwrap();

        assert!(entries.is_empty());
    }

    #[test]
    fn test_list_returns_configured_registries() {
        let (temp, cmd) = setup_test_env();

        // Create a config with registries
        let config_file = temp.path().join("config").join("sift.toml");
        std::fs::write(
            &config_file,
            r#"
[registry.official]
type = "sift"
url = "https://registry.sift.sh/v1"

[registry.anthropic]
type = "claude-marketplace"
source = "github:anthropics/skills"
"#,
        )
        .unwrap();

        let options = ListOptions::new().with_scope(ConfigScope::Global);
        let entries = cmd.list(&options).unwrap();

        assert_eq!(entries.len(), 2);

        let official = entries.iter().find(|e| e.name == "official").unwrap();
        assert_eq!(official.registry_type, RegistryType::Sift);
        assert_eq!(
            official.url,
            Some("https://registry.sift.sh/v1".to_string())
        );
        assert_eq!(official.scope, ConfigScope::Global);

        let anthropic = entries.iter().find(|e| e.name == "anthropic").unwrap();
        assert_eq!(anthropic.registry_type, RegistryType::ClaudeMarketplace);
        assert_eq!(
            anthropic.source,
            Some("github:anthropics/skills".to_string())
        );
    }

    #[test]
    fn test_list_filters_by_scope() {
        let (temp, cmd) = setup_test_env();

        // Create global config
        let global_config = temp.path().join("config").join("sift.toml");
        std::fs::write(
            &global_config,
            r#"
[registry.global-reg]
type = "sift"
url = "https://global.example.com/v1"
"#,
        )
        .unwrap();

        // Create project config
        let project_config = temp.path().join("project").join("sift.toml");
        std::fs::write(
            &project_config,
            r#"
[registry.project-reg]
type = "sift"
url = "https://project.example.com/v1"
"#,
        )
        .unwrap();

        // List only global
        let global_entries = cmd
            .list(&ListOptions::new().with_scope(ConfigScope::Global))
            .unwrap();
        assert_eq!(global_entries.len(), 1);
        assert_eq!(global_entries[0].name, "global-reg");

        // List only shared
        let shared_entries = cmd
            .list(&ListOptions::new().with_scope(ConfigScope::PerProjectShared))
            .unwrap();
        assert_eq!(shared_entries.len(), 1);
        assert_eq!(shared_entries[0].name, "project-reg");

        // List all (no filter)
        let all_entries = cmd.list(&ListOptions::new()).unwrap();
        assert_eq!(all_entries.len(), 2);
    }

    // ==========================================================================
    // Add tests
    // ==========================================================================

    #[test]
    fn test_add_sift_registry_from_url() {
        let (_temp, cmd) = setup_test_env();

        let options = AddOptions::new("official", "https://registry.sift.sh/v1");
        let report = cmd.add(&options).unwrap();

        assert_eq!(report.name, "official");
        assert!(report.changed);
        assert_eq!(report.scope, ConfigScope::Global);

        // Verify it was saved
        let entries = cmd
            .list(&ListOptions::new().with_scope(ConfigScope::Global))
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "official");
        assert_eq!(entries[0].registry_type, RegistryType::Sift);
    }

    #[test]
    fn test_add_marketplace_from_github() {
        let (_temp, cmd) = setup_test_env();

        let options = AddOptions::new("anthropic", "github:anthropics/claude-skills");
        let report = cmd.add(&options).unwrap();

        assert_eq!(report.name, "anthropic");
        assert!(report.changed);

        let entries = cmd.list(&ListOptions::new()).unwrap();
        let entry = entries.iter().find(|e| e.name == "anthropic").unwrap();
        assert_eq!(entry.registry_type, RegistryType::ClaudeMarketplace);
        assert_eq!(
            entry.source,
            Some("github:anthropics/claude-skills".to_string())
        );
    }

    #[test]
    fn test_add_with_explicit_type() {
        let (_temp, cmd) = setup_test_env();

        let options = AddOptions::new("custom", "github:myorg/custom-plugins")
            .with_type(RegistryType::ClaudeMarketplace);

        let report = cmd.add(&options).unwrap();

        assert!(report.changed);
        let entries = cmd.list(&ListOptions::new()).unwrap();
        assert_eq!(entries[0].registry_type, RegistryType::ClaudeMarketplace);
    }

    #[test]
    fn test_add_to_shared_scope() {
        let (_temp, cmd) = setup_test_env();

        let options = AddOptions::new("project-reg", "https://internal.example.com/v1")
            .with_scope(ConfigScope::PerProjectShared);

        let report = cmd.add(&options).unwrap();

        assert_eq!(report.scope, ConfigScope::PerProjectShared);

        // Should not be in global
        let global_entries = cmd
            .list(&ListOptions::new().with_scope(ConfigScope::Global))
            .unwrap();
        assert!(global_entries.is_empty());

        // Should be in shared
        let shared_entries = cmd
            .list(&ListOptions::new().with_scope(ConfigScope::PerProjectShared))
            .unwrap();
        assert_eq!(shared_entries.len(), 1);
    }

    #[test]
    fn test_add_fails_without_force_if_exists() {
        let (_temp, cmd) = setup_test_env();

        let options = AddOptions::new("duplicate", "https://first.example.com/v1");
        cmd.add(&options).unwrap();

        let options2 = AddOptions::new("duplicate", "https://second.example.com/v1");
        let result = cmd.add(&options2);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("already exists"));
        assert!(err.contains("--force"));
    }

    #[test]
    fn test_add_with_force_overwrites() {
        let (_temp, cmd) = setup_test_env();

        let options1 = AddOptions::new("overwrite", "https://first.example.com/v1");
        cmd.add(&options1).unwrap();

        let options2 =
            AddOptions::new("overwrite", "https://second.example.com/v1").with_force(true);
        let report = cmd.add(&options2).unwrap();

        assert!(report.changed);

        let entries = cmd.list(&ListOptions::new()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].url,
            Some("https://second.example.com/v1".to_string())
        );
    }

    #[test]
    fn test_add_idempotent_no_change() {
        let (_temp, cmd) = setup_test_env();

        let options = AddOptions::new("idempotent", "https://example.com/v1");

        let report1 = cmd.add(&options).unwrap();
        assert!(report1.changed);

        let report2 = cmd.add(&options).unwrap();
        assert!(!report2.changed);
    }

    #[test]
    fn test_add_validates_before_writing() {
        let (_temp, cmd) = setup_test_env();

        // Try to add a sift registry without a valid URL
        let options = AddOptions::new("invalid", "not-a-url");
        let result = cmd.add(&options);

        assert!(result.is_err());
    }

    // ==========================================================================
    // Remove tests
    // ==========================================================================

    #[test]
    fn test_remove_from_global() {
        let (_temp, cmd) = setup_test_env();

        // Add a registry first
        let add_opts = AddOptions::new("to-remove", "https://example.com/v1");
        cmd.add(&add_opts).unwrap();

        // Remove it
        let remove_opts = RemoveOptions::new("to-remove");
        let report = cmd.remove(&remove_opts).unwrap();

        assert_eq!(report.name, "to-remove");
        assert!(report.changed);
        assert_eq!(report.scope, ConfigScope::Global);

        // Verify it's gone
        let entries = cmd.list(&ListOptions::new()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_remove_from_shared() {
        let (_temp, cmd) = setup_test_env();

        // Add to shared scope
        let add_opts = AddOptions::new("shared-reg", "https://example.com/v1")
            .with_scope(ConfigScope::PerProjectShared);
        cmd.add(&add_opts).unwrap();

        // Remove with explicit scope
        let remove_opts =
            RemoveOptions::new("shared-reg").with_scope(ConfigScope::PerProjectShared);
        let report = cmd.remove(&remove_opts).unwrap();

        assert!(report.changed);
        assert_eq!(report.scope, ConfigScope::PerProjectShared);
    }

    #[test]
    fn test_remove_nonexistent_fails() {
        let (_temp, cmd) = setup_test_env();

        let remove_opts = RemoveOptions::new("nonexistent");
        let result = cmd.remove(&remove_opts);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"));
    }

    #[test]
    fn test_remove_all_scopes() {
        let (temp, cmd) = setup_test_env();

        // Add same name to both scopes
        let global_config = temp.path().join("config").join("sift.toml");
        std::fs::write(
            &global_config,
            r#"
[registry.both]
type = "sift"
url = "https://global.example.com/v1"
"#,
        )
        .unwrap();

        let project_config = temp.path().join("project").join("sift.toml");
        std::fs::write(
            &project_config,
            r#"
[registry.both]
type = "sift"
url = "https://project.example.com/v1"
"#,
        )
        .unwrap();

        // Verify both exist
        let entries = cmd.list(&ListOptions::new()).unwrap();
        assert_eq!(entries.len(), 2);

        // Remove from all scopes
        let remove_opts = RemoveOptions::new("both").with_all_scopes(true);
        cmd.remove(&remove_opts).unwrap();

        // Verify both are gone
        let entries = cmd.list(&ListOptions::new()).unwrap();
        assert!(entries.is_empty());
    }

    // ==========================================================================
    // Source resolution tests
    // ==========================================================================

    #[test]
    fn test_resolve_github_source() {
        let (registry_type, url, source) =
            resolve_source_and_type("github:anthropics/skills", None).unwrap();

        assert_eq!(registry_type, RegistryType::ClaudeMarketplace);
        assert!(url.is_none());
        assert_eq!(source, Some("github:anthropics/skills".to_string()));
    }

    #[test]
    fn test_resolve_git_source() {
        let (registry_type, url, source) =
            resolve_source_and_type("git:https://github.com/org/repo.git", None).unwrap();

        assert_eq!(registry_type, RegistryType::ClaudeMarketplace);
        assert!(url.is_none());
        assert_eq!(
            source,
            Some("git:https://github.com/org/repo.git".to_string())
        );
    }

    #[test]
    fn test_resolve_https_url() {
        let (registry_type, url, source) =
            resolve_source_and_type("https://registry.example.com/v1", None).unwrap();

        assert_eq!(registry_type, RegistryType::Sift);
        assert_eq!(url, Some("https://registry.example.com/v1".to_string()));
        assert!(source.is_none());
    }

    #[test]
    fn test_resolve_explicit_type_overrides() {
        // Even though it looks like a URL, explicit type makes it marketplace
        let (registry_type, url, source) = resolve_source_and_type(
            "github:custom/source",
            Some(RegistryType::ClaudeMarketplace),
        )
        .unwrap();

        assert_eq!(registry_type, RegistryType::ClaudeMarketplace);
        assert!(url.is_none());
        assert_eq!(source, Some("github:custom/source".to_string()));
    }

    #[test]
    fn test_resolve_ambiguous_fails() {
        let result = resolve_source_and_type("just-a-name", None);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Could not determine registry type"));
    }
}
