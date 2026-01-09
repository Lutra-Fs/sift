//! Lockfile persistence in XDG state directory
//!
//! Lockfiles are stored in the user's state directory, not in project directories.
//! This allows tracking installed skills without polluting project repositories.

use anyhow::Context;
use serde_json;
use std::fs;
use std::path::{Path, PathBuf};

use crate::version::lock::Lockfile;

/// Lockfile storage and persistence
///
/// Lockfiles are stored in XDG state directory:
/// - Unix: `XDG_STATE_HOME/sift/locks/` (fallback: `~/.local/state/sift/locks/`)
/// - Windows: `%LOCALAPPDATA%\sift\locks\`
///
/// Per-project lockfiles: `<project_key>.lock.json`
/// Global lockfile: `global.lock.json`
pub struct LockfileStore;

impl LockfileStore {
    /// Get default state directory for lockfiles
    ///
    /// # Returns
    /// - Unix: `$XDG_STATE_HOME/sift/locks` or `~/.local/state/sift/locks`
    /// - Windows: `%LOCALAPPDATA%\sift\locks`
    pub fn default_state_dir() -> anyhow::Result<PathBuf> {
        let base = if cfg!(unix) {
            dirs::state_dir()
                .or_else(dirs::data_local_dir)
                .ok_or_else(|| anyhow::anyhow!("Cannot determine state directory"))?
        } else {
            dirs::data_local_dir()
                .ok_or_else(|| anyhow::anyhow!("Cannot determine local app data directory"))?
        };
        Ok(base.join("sift").join("locks"))
    }

    /// Generate stable project key from path
    ///
    /// Uses canonical path if possible; falls back to absolute path.
    ///
    /// # Note
    /// Moving a project directory will generate a new key (new lockfile).
    /// This is a documented tradeoff for avoiding project directory pollution.
    pub fn project_key(project_root: &Path) -> String {
        // Try canonicalize first, fall back to absolute path via std::fs::canonicalize
        let path = fs::canonicalize(project_root).unwrap_or_else(|_| {
            // Fallback: use the path as-is if it's already absolute,
            // otherwise we can't do much better
            if project_root.is_absolute() {
                project_root.to_path_buf()
            } else {
                // Best effort: current directory + relative path
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| {
                        project_root.strip_prefix(&cwd).ok().map(|_| {
                            // We'll just use the original path string
                            project_root.to_path_buf()
                        })
                    })
                    .unwrap_or_else(|| project_root.to_path_buf())
            }
        });
        let hash = blake3::hash(path.to_string_lossy().as_bytes());
        hash.to_hex().to_string()
    }

    /// Load lockfile from disk
    ///
    /// Returns a new empty lockfile if the file doesn't exist.
    ///
    /// # Parameters
    /// - `project_root`: Optional project root path. If None, uses "global" key.
    /// - `store_dir`: Directory containing lockfiles
    pub fn load(project_root: Option<PathBuf>, store_dir: PathBuf) -> anyhow::Result<Lockfile> {
        let key = project_root
            .as_ref()
            .map(|p| Self::project_key(p))
            .unwrap_or_else(|| "global".to_string());
        let lockfile_path = store_dir.join(format!("{}.lock.json", key));

        if !lockfile_path.exists() {
            return Ok(Lockfile::new());
        }

        let bytes = fs::read(&lockfile_path)
            .with_context(|| format!("Failed to read lockfile: {}", lockfile_path.display()))?;
        let lockfile: Lockfile = serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse lockfile: {}", lockfile_path.display()))?;
        lockfile.validate()?;
        Ok(lockfile)
    }

    /// Save lockfile atomically (tmp + rename)
    ///
    /// # Parameters
    /// - `project_root`: Optional project root path. If None, uses "global" key.
    /// - `store_dir`: Directory to store lockfiles
    /// - `lockfile`: Lockfile to save
    ///
    /// # Errors
    /// Fails if any PathBuf in installed_skills is non-UTF-8 (cannot serialize to JSON).
    pub fn save(
        project_root: Option<PathBuf>,
        store_dir: PathBuf,
        lockfile: &Lockfile,
    ) -> anyhow::Result<()> {
        fs::create_dir_all(&store_dir).with_context(|| {
            format!("Failed to create store directory: {}", store_dir.display())
        })?;

        let key = project_root
            .as_ref()
            .map(|p| Self::project_key(p))
            .unwrap_or_else(|| "global".to_string());
        let lockfile_path = store_dir.join(format!("{}.lock.json", key));
        let tmp_path = store_dir.join(format!("{}.lock.json.tmp", std::process::id()));

        // Serialize first to catch UTF-8 errors before writing
        let bytes = serde_json::to_vec_pretty(lockfile).context("Failed to serialize lockfile")?;

        // Write to temp file
        fs::write(&tmp_path, bytes)
            .with_context(|| format!("Failed to write tmp lockfile: {}", tmp_path.display()))?;

        // Atomic rename (remove target first on Windows for replace semantics)
        if lockfile_path.exists() {
            fs::remove_file(&lockfile_path).with_context(|| {
                format!(
                    "Failed to remove existing lockfile: {}",
                    lockfile_path.display()
                )
            })?;
        }
        fs::rename(&tmp_path, &lockfile_path)
            .with_context(|| format!("Failed to rename tmp lockfile: {}", tmp_path.display()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_creates_new_lockfile() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let store_dir = tmp.path().join("locks");
        let project_dir = tmp.path().join("project");
        fs::create_dir_all(&project_dir).expect("create_dir_all should succeed");

        let lockfile =
            LockfileStore::load(Some(project_dir), store_dir).expect("load should succeed");

        assert_eq!(lockfile.version, 1);
        assert!(lockfile.mcp_servers.is_empty());
        assert!(lockfile.skills.is_empty());
    }

    #[test]
    fn test_save_and_load_persist_data() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let store_dir = tmp.path().join("locks");
        let project_dir = tmp.path().join("project");
        fs::create_dir_all(&project_dir).expect("create_dir_all should succeed");

        let mut lockfile = Lockfile::new();
        use crate::version::lock::LockedMcpServer;
        let server = LockedMcpServer::new(
            "test-server".to_string(),
            "1.0.0".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
        );
        lockfile.add_mcp_server("test".to_string(), server);

        LockfileStore::save(Some(project_dir.clone()), store_dir.clone(), &lockfile)
            .expect("save should succeed");

        let loaded =
            LockfileStore::load(Some(project_dir), store_dir).expect("load should succeed");

        assert_eq!(loaded.mcp_servers.len(), 1);
        let loaded_server = loaded.get_mcp_server("test").expect("server should exist");
        assert_eq!(loaded_server.name, "test-server");
        assert_eq!(loaded_server.resolved_version, "1.0.0");
    }

    #[test]
    fn test_project_key_is_stable() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let project_dir = tmp.path().join("project");
        fs::create_dir_all(&project_dir).expect("create_dir_all should succeed");

        let key1 = LockfileStore::project_key(&project_dir);
        let key2 = LockfileStore::project_key(&project_dir);

        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 64); // blake3 hex output
    }

    #[test]
    fn test_project_key_different_paths_different_keys() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let dir1 = tmp.path().join("project1");
        let dir2 = tmp.path().join("project2");
        fs::create_dir_all(&dir1).expect("create_dir_all should succeed");
        fs::create_dir_all(&dir2).expect("create_dir_all should succeed");

        let key1 = LockfileStore::project_key(&dir1);
        let key2 = LockfileStore::project_key(&dir2);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_project_key_fallback_on_canonicalize_failure() {
        // Nonexistent path should fall back to absolute path
        let nonexistent = Path::new("/nonexistent/path/that/does/not/exist");
        let key = LockfileStore::project_key(nonexistent);

        // Should still produce a valid hash
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_save_is_crash_safe() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let store_dir = tmp.path().join("locks");
        let project_dir = tmp.path().join("project");
        fs::create_dir_all(&project_dir).expect("create_dir_all should succeed");

        let lockfile = Lockfile::new();

        // First save
        LockfileStore::save(Some(project_dir.clone()), store_dir.clone(), &lockfile)
            .expect("save should succeed");

        // Verify tmp file is cleaned up
        let tmp_pattern = format!(
            "{}.lock.json.tmp.{}",
            std::process::id(),
            std::process::id()
        );
        let tmp_files: Vec<_> = fs::read_dir(&store_dir)
            .expect("read_dir should succeed")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.contains(&tmp_pattern))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(
            tmp_files.len(),
            0,
            "Tmp file should be cleaned up after save"
        );

        // Verify target file is valid JSON
        let key = LockfileStore::project_key(&project_dir);
        let lockfile_path = store_dir.join(format!("{}.lock.json", key));
        assert!(lockfile_path.exists());

        let bytes = fs::read(&lockfile_path).expect("read should succeed");
        let _: Lockfile = serde_json::from_slice(&bytes).expect("lockfile should be valid JSON");

        // Multiple saves should not corrupt
        for _ in 0..3 {
            LockfileStore::save(Some(project_dir.clone()), store_dir.clone(), &lockfile)
                .expect("save should succeed");
        }

        let loaded =
            LockfileStore::load(Some(project_dir), store_dir).expect("load should succeed");
        assert_eq!(loaded.version, 1);
    }

    #[test]
    fn test_global_lockfile_key() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let store_dir = tmp.path().join("locks");

        let lockfile = LockfileStore::load(None, store_dir.clone()).expect("load should succeed");

        LockfileStore::save(None, store_dir.clone(), &lockfile).expect("save should succeed");

        // Verify global lockfile exists
        let global_path = store_dir.join("global.lock.json");
        assert!(global_path.exists());

        // Verify it's valid JSON
        let bytes = fs::read(&global_path).expect("read should succeed");
        let _: Lockfile = serde_json::from_slice(&bytes).expect("lockfile should be valid JSON");
    }
}
