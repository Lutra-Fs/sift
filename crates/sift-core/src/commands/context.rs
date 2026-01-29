//! Install context providing dependency injection for install operations.
//!
//! InstallContext centralizes creation of services (ConfigStore, LockfileService,
//! SkillInstaller, GitFetcher, SourceResolver) and caches merged config to avoid
//! repeated loading during install operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::client::ClientContext;
use crate::client::registry::ClientRegistry;
use crate::config::{ConfigStore, SiftConfig, merge_configs};
use crate::context::AppContext;
use crate::fs::LinkMode;
use crate::git::GitFetcher;
use crate::lockfile::LockfileService;
use crate::registry::RegistryConfig;
use crate::skills::installer::SkillInstaller;
use crate::source::SourceResolver;
use crate::types::ConfigScope;

/// Dependency injection container for install operations.
///
/// Caches merged configuration and lazily creates service instances.
/// All path-dependent services are derived from the stored paths.
pub struct InstallContext {
    /// User's home directory
    home_dir: PathBuf,
    /// Project root directory
    project_root: PathBuf,
    /// State directory for caches and locks
    state_dir: PathBuf,
    /// Global config directory (e.g., ~/.config/sift)
    global_config_dir: PathBuf,
    /// Link mode for skills (global policy)
    link_mode: LinkMode,
    /// Cached merged config (loaded once on first access)
    config_cache: OnceLock<SiftConfig>,
}

impl InstallContext {
    /// Create a new install context with explicit paths.
    pub fn new(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        global_config_dir: PathBuf,
        link_mode: LinkMode,
    ) -> Self {
        Self {
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            link_mode,
            config_cache: OnceLock::new(),
        }
    }

    /// Create an install context with system defaults.
    pub fn with_defaults() -> anyhow::Result<Self> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        let project_root = std::env::current_dir()?;
        let state_dir = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .ok_or_else(|| anyhow::anyhow!("Could not determine state directory"))?
            .join("sift");
        let global_config_dir = dirs::config_dir()
            .map(|p| p.join("sift"))
            .unwrap_or_else(|| home_dir.join(".config").join("sift"));

        let mut ctx = Self::new(
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            LinkMode::Auto,
        );

        // Load config to get link_mode
        let config = ctx.merged_config()?;
        ctx.link_mode = config.link_mode.unwrap_or(LinkMode::Auto);

        Ok(ctx)
    }

    /// Create an install context from explicit paths, loading link_mode from config.
    pub fn from_paths(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        global_config_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        let mut ctx = Self::new(
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            LinkMode::Auto,
        );

        let config = ctx.merged_config()?;
        ctx.link_mode = config.link_mode.unwrap_or(LinkMode::Auto);

        Ok(ctx)
    }

    /// Create an InstallContext from an AppContext.
    ///
    /// This is the preferred way to create InstallContext when using
    /// the unified AppContext pattern.
    pub fn from_app_context(app_ctx: AppContext) -> Self {
        Self {
            home_dir: app_ctx.home_dir().to_path_buf(),
            project_root: app_ctx.project_root().to_path_buf(),
            state_dir: app_ctx.state_dir().to_path_buf(),
            global_config_dir: app_ctx.global_config_dir().to_path_buf(),
            link_mode: app_ctx.link_mode(),
            config_cache: OnceLock::new(),
        }
    }

    // --- Accessors ---

    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn global_config_dir(&self) -> &Path {
        &self.global_config_dir
    }

    pub fn link_mode(&self) -> LinkMode {
        self.link_mode
    }

    // --- Cached Config ---

    /// Get merged config (global + project), cached on first access.
    ///
    /// Uses a manual caching pattern since `OnceLock::get_or_try_init` is unstable.
    /// On first call, loads and merges global + project configs.
    /// Subsequent calls return the cached reference.
    pub fn merged_config(&self) -> anyhow::Result<&SiftConfig> {
        if let Some(config) = self.config_cache.get() {
            return Ok(config);
        }

        let global_store = ConfigStore::from_paths(
            ConfigScope::Global,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let project_store = ConfigStore::from_paths(
            ConfigScope::PerProjectShared,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        );
        let global = global_store.load()?;
        let project = project_store.load()?;
        let merged = merge_configs(Some(global), Some(project), &self.project_root)?;

        // set() returns Err if already initialized, which is fine in concurrent scenarios
        // We ignore the result and use get() to return the actual stored value
        let _ = self.config_cache.set(merged);

        // OnceLock guarantees get() returns Some after successful set()
        // or if another thread already set it (data race safe)
        Ok(self
            .config_cache
            .get()
            .expect("config was just set or was already set by another thread"))
    }

    /// Get registry configurations from merged config.
    pub fn registries(&self) -> anyhow::Result<HashMap<String, RegistryConfig>> {
        let config = self.merged_config()?;
        let mut registries = HashMap::new();
        for (key, entry) in &config.registry {
            let registry_config: RegistryConfig = entry.clone().try_into()?;
            registries.insert(key.clone(), registry_config);
        }
        Ok(registries)
    }

    // --- Service Factories ---

    /// Create a ConfigStore for the given scope.
    pub fn config_store(&self, scope: ConfigScope) -> ConfigStore {
        ConfigStore::from_paths(
            scope,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        )
    }

    /// Create a LockfileService.
    pub fn lockfile_service(&self) -> LockfileService {
        LockfileService::new(
            self.state_dir.join("locks"),
            Some(self.project_root.clone()),
        )
    }

    /// Create a SkillInstaller.
    pub fn skill_installer(&self) -> SkillInstaller {
        SkillInstaller::new(
            self.state_dir.join("locks"),
            Some(self.project_root.clone()),
        )
    }

    /// Create a GitFetcher.
    pub fn git_fetcher(&self) -> GitFetcher {
        GitFetcher::new(self.state_dir.clone())
    }

    /// Create a SourceResolver with registry configurations.
    pub fn source_resolver(&self) -> anyhow::Result<SourceResolver> {
        let registries = self.registries()?;
        Ok(SourceResolver::new(
            self.state_dir.clone(),
            self.project_root.clone(),
            registries,
        ))
    }

    /// Create a ClientRegistry with all default clients.
    pub fn client_registry(&self) -> ClientRegistry {
        ClientRegistry::with_default_clients()
    }

    /// Create a ClientContext for client adapters.
    pub fn client_context(&self) -> ClientContext {
        ClientContext::new(self.home_dir.clone(), self.project_root.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_context() -> (TempDir, InstallContext) {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state = temp.path().join("state");
        let global_config = temp.path().join("config");

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&global_config).unwrap();

        let ctx = InstallContext::new(home, project, state, global_config, LinkMode::Copy);
        (temp, ctx)
    }

    fn setup_test_context_with_config() -> (TempDir, InstallContext) {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state = temp.path().join("state");
        let global_config = temp.path().join("config");

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&global_config).unwrap();

        // Write test config with registries
        let config_content = r#"
link_mode = "symlink"

[registry.demo]
type = "claude-marketplace"
source = "github:anthropics/skills"

[registry.official]
type = "claude-marketplace"
source = "github:official/plugins"
"#;
        std::fs::write(global_config.join("sift.toml"), config_content).unwrap();

        let ctx = InstallContext::new(home, project, state, global_config, LinkMode::Auto);
        (temp, ctx)
    }

    #[test]
    fn test_new_stores_paths() {
        let (_temp, ctx) = setup_test_context();

        assert!(ctx.home_dir().ends_with("home"));
        assert!(ctx.project_root().ends_with("project"));
        assert!(ctx.state_dir().ends_with("state"));
        assert!(ctx.global_config_dir().ends_with("config"));
        assert_eq!(ctx.link_mode(), LinkMode::Copy);
    }

    #[test]
    fn test_from_paths_loads_link_mode() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state = temp.path().join("state");
        let global_config = temp.path().join("config");

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&global_config).unwrap();

        std::fs::write(global_config.join("sift.toml"), "link_mode = \"hardlink\"").unwrap();

        let ctx = InstallContext::from_paths(home, project, state, global_config).unwrap();
        assert_eq!(ctx.link_mode(), LinkMode::Hardlink);
    }

    #[test]
    fn test_merged_config_is_cached() {
        let (_temp, ctx) = setup_test_context_with_config();

        // First call loads config
        let config1 = ctx.merged_config().unwrap();
        assert!(config1.registry.contains_key("demo"));

        // Second call returns same cached reference
        let config2 = ctx.merged_config().unwrap();
        assert!(std::ptr::eq(config1, config2));
    }

    #[test]
    fn test_registries_parses_config() {
        let (_temp, ctx) = setup_test_context_with_config();

        let registries = ctx.registries().unwrap();
        assert!(registries.contains_key("demo"));
        assert!(registries.contains_key("official"));
        assert_eq!(registries.len(), 2);
    }

    #[test]
    fn test_config_store_for_scope() {
        let (_temp, ctx) = setup_test_context();

        let global_store = ctx.config_store(ConfigScope::Global);
        let project_store = ctx.config_store(ConfigScope::PerProjectShared);

        // Stores should be created for different scopes
        assert!(
            global_store
                .config_path()
                .to_string_lossy()
                .contains("config")
        );
        assert!(
            project_store
                .config_path()
                .to_string_lossy()
                .contains("project")
        );
    }

    #[test]
    fn test_lockfile_service_uses_state_dir() {
        let (_temp, ctx) = setup_test_context();
        let lockfile_service = ctx.lockfile_service();

        // LockfileService should be created (we can't easily inspect internals)
        // but we can verify the function doesn't panic
        drop(lockfile_service);
    }

    #[test]
    fn test_skill_installer_created() {
        let (_temp, ctx) = setup_test_context();
        let installer = ctx.skill_installer();

        // Just verify creation succeeds
        drop(installer);
    }

    #[test]
    fn test_git_fetcher_created() {
        let (_temp, ctx) = setup_test_context();
        let fetcher = ctx.git_fetcher();

        // Just verify creation succeeds
        drop(fetcher);
    }

    #[test]
    fn test_source_resolver_with_registries() {
        let (_temp, ctx) = setup_test_context_with_config();

        let resolver = ctx.source_resolver().unwrap();

        // Verify resolver was created with registry config
        drop(resolver);
    }

    #[test]
    fn test_source_resolver_empty_registries() {
        let (_temp, ctx) = setup_test_context();

        // Should succeed even without registries configured
        let resolver = ctx.source_resolver().unwrap();
        drop(resolver);
    }

    #[test]
    fn test_merged_config_merges_global_and_project() {
        let temp = TempDir::new().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let state = temp.path().join("state");
        let global_config = temp.path().join("config");

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&global_config).unwrap();

        // Global config with one registry
        std::fs::write(
            global_config.join("sift.toml"),
            r#"
[registry.global-reg]
type = "claude-marketplace"
source = "github:global/skills"
"#,
        )
        .unwrap();

        // Project config with another registry
        std::fs::write(
            project.join("sift.toml"),
            r#"
[registry.project-reg]
type = "claude-marketplace"
source = "github:project/skills"
"#,
        )
        .unwrap();

        let ctx = InstallContext::new(home, project, state, global_config, LinkMode::Auto);
        let config = ctx.merged_config().unwrap();

        // Both registries should be present in merged config
        assert!(config.registry.contains_key("global-reg"));
        assert!(config.registry.contains_key("project-reg"));
    }
}
