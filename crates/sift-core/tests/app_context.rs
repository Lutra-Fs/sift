use sift_core::context::AppContext;
use sift_core::fs::LinkMode;
use sift_core::types::ConfigScope;
use tempfile::TempDir;

#[test]
fn app_context_creates_from_paths() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let state = temp.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let ctx = AppContext::new(home.clone(), project.clone(), state.clone(), LinkMode::Auto);

    assert_eq!(ctx.home_dir(), &home);
    assert_eq!(ctx.project_root(), &project);
    assert_eq!(ctx.state_dir(), &state);
    assert_eq!(ctx.link_mode(), LinkMode::Auto);
}

#[test]
fn app_context_provides_client_context() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    let ctx = AppContext::new(
        home.clone(),
        project.clone(),
        temp.path().join("state"),
        LinkMode::Auto,
    );

    let client_ctx = ctx.client_context();
    assert_eq!(client_ctx.home_dir, home);
    assert_eq!(client_ctx.project_root, project);
}

#[test]
fn app_context_provides_lockfile_service() {
    let temp = TempDir::new().unwrap();
    let ctx = AppContext::new(
        temp.path().join("home"),
        temp.path().join("project"),
        temp.path().join("state"),
        LinkMode::Auto,
    );

    let lockfile = ctx.lockfile_service();
    // Should not panic, service is usable
    let _ = lockfile.load();
}

#[test]
fn app_context_provides_config_store() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let ctx = AppContext::with_global_config_dir(
        temp.path().join("home"),
        project.clone(),
        temp.path().join("state"),
        temp.path().join("config"),
        LinkMode::Auto,
    );

    let store = ctx.config_store(ConfigScope::PerProjectShared);
    // Should load without error (empty config is fine)
    let config = store.load().unwrap();
    assert!(config.mcp.is_empty());
}
