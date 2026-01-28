//! Application context for unified dependency injection.

use std::path::{Path, PathBuf};

use crate::client::ClientContext;
use crate::config::ConfigStore;
use crate::fs::LinkMode;
use crate::git::GitFetcher;
use crate::lockfile::LockfileService;
use crate::registry::RegistryConfig;
use crate::skills::installer::SkillInstaller;
use crate::source::SourceResolver;
use crate::types::ConfigScope;

/// Unified application context for dependency injection.
///
/// Provides access to all shared services and configuration paths.
/// Frontends (CLI/TUI/GUI) create this once and pass it to commands.
#[derive(Debug, Clone)]
pub struct AppContext {
    home_dir: PathBuf,
    project_root: PathBuf,
    state_dir: PathBuf,
    global_config_dir: PathBuf,
    link_mode: LinkMode,
}

impl AppContext {
    /// Create a new context with explicit paths.
    pub fn new(
        home_dir: PathBuf,
        project_root: PathBuf,
        state_dir: PathBuf,
        link_mode: LinkMode,
    ) -> Self {
        let global_config_dir = dirs::config_dir()
            .map(|p| p.join("sift"))
            .unwrap_or_else(|| home_dir.join(".config").join("sift"));

        Self {
            home_dir,
            project_root,
            state_dir,
            global_config_dir,
            link_mode,
        }
    }

    /// Create context with custom global config directory (for testing).
    pub fn with_global_config_dir(
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
        }
    }

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

    /// Get a ClientContext for adapter calls.
    pub fn client_context(&self) -> ClientContext {
        ClientContext::new(self.home_dir.clone(), self.project_root.clone())
    }

    /// Get a LockfileService for the current project.
    pub fn lockfile_service(&self) -> LockfileService {
        LockfileService::new(self.state_dir.clone(), Some(self.project_root.clone()))
    }

    /// Get a ConfigStore for the given scope.
    pub fn config_store(&self, scope: ConfigScope) -> ConfigStore {
        ConfigStore::from_paths(
            scope,
            self.global_config_dir.clone(),
            self.project_root.clone(),
        )
    }

    /// Get a GitFetcher.
    pub fn git_fetcher(&self) -> GitFetcher {
        GitFetcher::new(self.state_dir.clone())
    }

    /// Get a SkillInstaller.
    pub fn skill_installer(&self) -> SkillInstaller {
        SkillInstaller::from_service(self.lockfile_service())
    }

    /// Get a SourceResolver (requires registry configs).
    pub fn source_resolver(
        &self,
        registries: std::collections::HashMap<String, RegistryConfig>,
    ) -> SourceResolver {
        SourceResolver::new(
            self.state_dir.clone(),
            self.project_root.clone(),
            registries,
        )
    }
}
