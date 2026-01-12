//! Config path resolution helpers.

use std::path::{Path, PathBuf};

use crate::types::ConfigScope;

pub fn config_path_for_scope(
    scope: ConfigScope,
    global_dir: &Path,
    project_root: &Path,
) -> PathBuf {
    match scope {
        ConfigScope::Global => global_dir.join("sift.toml"),
        ConfigScope::PerProjectShared => project_root.join("sift.toml"),
        ConfigScope::PerProjectLocal => global_dir.join("sift.toml"),
    }
}
