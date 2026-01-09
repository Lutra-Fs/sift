//! Ownership state persistence for managed config entries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::version::store::LockfileStore;

#[derive(Debug, Clone)]
pub struct OwnershipStore {
    store_dir: PathBuf,
    project_root: Option<PathBuf>,
}

impl OwnershipStore {
    pub fn new(store_dir: PathBuf, project_root: Option<PathBuf>) -> Self {
        Self {
            store_dir,
            project_root,
        }
    }

    pub fn load(&self, config_path: &Path) -> anyhow::Result<HashMap<String, String>> {
        self.load_for_field_key(config_path, None)
    }

    pub fn save(
        &self,
        config_path: &Path,
        ownership: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        self.save_for_field_key(config_path, None, ownership)
    }

    pub fn load_for_field(
        &self,
        config_path: &Path,
        field: &str,
    ) -> anyhow::Result<HashMap<String, String>> {
        self.load_for_field_key(config_path, Some(field))
    }

    pub fn save_for_field(
        &self,
        config_path: &Path,
        field: &str,
        ownership: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        self.save_for_field_key(config_path, Some(field), ownership)
    }

    fn load_for_field_key(
        &self,
        config_path: &Path,
        field: Option<&str>,
    ) -> anyhow::Result<HashMap<String, String>> {
        let key = ownership_key(config_path, field);
        let lockfile = LockfileStore::load(self.project_root.clone(), self.store_dir.clone())
            .context("Failed to load lockfile for ownership state")?;
        Ok(lockfile
            .managed_configs
            .get(&key)
            .cloned()
            .unwrap_or_default())
    }

    fn save_for_field_key(
        &self,
        config_path: &Path,
        field: Option<&str>,
        ownership: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let key = ownership_key(config_path, field);
        let mut lockfile = LockfileStore::load(self.project_root.clone(), self.store_dir.clone())
            .context("Failed to load lockfile for ownership state")?;
        lockfile.managed_configs.insert(key, ownership.clone());
        LockfileStore::save(self.project_root.clone(), self.store_dir.clone(), &lockfile)
            .context("Failed to save lockfile ownership state")?;
        Ok(())
    }
}

fn ownership_key(config_path: &Path, field: Option<&str>) -> String {
    let mut path_string = config_path.to_string_lossy().to_string();
    if let Some(field) = field {
        path_string.push('#');
        path_string.push_str(field);
    }
    blake3::hash(path_string.as_bytes()).to_hex().to_string()
}
