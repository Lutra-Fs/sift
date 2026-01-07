//! Integration tests for Sift

#[test]
fn test_workspace_builds() {
    // Basic smoke test to ensure the workspace compiles
    assert!(true);
}

#[test]
fn test_config_scopes() {
    use sift_core::config::ConfigScope;

    // Test that all config scopes can be instantiated
    let _ = ConfigScope::Global;
    let _ = ConfigScope::PerProjectLocal;
    let _ = ConfigScope::PerProjectShared;
}
