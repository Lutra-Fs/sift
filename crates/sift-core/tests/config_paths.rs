use std::path::PathBuf;

use sift_core::config::ConfigScope;
use sift_core::config::paths::config_path_for_scope;

#[test]
fn global_scope_uses_config_dir() {
    let global_dir = PathBuf::from("/tmp/sift-config");
    let project_root = PathBuf::from("/workspace/project");

    let path = config_path_for_scope(ConfigScope::Global, &global_dir, &project_root);

    assert_eq!(path, PathBuf::from("/tmp/sift-config/sift.toml"));
}

#[test]
fn project_shared_scope_uses_project_root() {
    let global_dir = PathBuf::from("/tmp/sift-config");
    let project_root = PathBuf::from("/workspace/project");

    let path = config_path_for_scope(ConfigScope::PerProjectShared, &global_dir, &project_root);

    assert_eq!(path, PathBuf::from("/workspace/project/sift.toml"));
}

#[test]
fn project_local_scope_uses_global_config() {
    let global_dir = PathBuf::from("/tmp/sift-config");
    let project_root = PathBuf::from("/workspace/project");

    let path = config_path_for_scope(ConfigScope::PerProjectLocal, &global_dir, &project_root);

    assert_eq!(path, PathBuf::from("/tmp/sift-config/sift.toml"));
}
