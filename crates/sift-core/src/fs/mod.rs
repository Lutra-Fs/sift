//! Filesystem primitives shared across features.

use std::path::Path;

use anyhow::Context;

pub mod link_mode;
pub mod tree_hash;

pub use link_mode::LinkMode;
pub use tree_hash::hash_tree;

/// Remove a path (file or directory) if it exists.
///
/// Returns `Ok(true)` if something was removed, `Ok(false)` if path didn't exist.
pub fn remove_path_if_exists(path: &Path) -> anyhow::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("Failed to read metadata: {}", path.display()))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove directory: {}", path.display()))?;
    } else {
        std::fs::remove_file(path)
            .with_context(|| format!("Failed to remove file: {}", path.display()))?;
    }
    Ok(true)
}
