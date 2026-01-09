use std::path::PathBuf;

use sift_core::runtime::{RuntimeKind, RuntimeRequest, resolve_runtime};

#[test]
fn bunx_runtime_includes_cache_dir() {
    let request = RuntimeRequest {
        kind: RuntimeKind::Bunx,
        package: "pkg".to_string(),
        version: "1.2.3".to_string(),
        cache_dir: PathBuf::from("/tmp/sift-cache"),
        extra_args: vec![],
    };

    let spec = resolve_runtime(&request).unwrap();

    assert_eq!(spec.command, "bunx");
    assert_eq!(
        spec.args,
        vec![
            "--cache-dir".to_string(),
            "/tmp/sift-cache".to_string(),
            "pkg@1.2.3".to_string()
        ]
    );
    assert_eq!(
        spec.env.get("BUN_INSTALL_CACHE_DIR").map(|v| v.as_str()),
        Some("/tmp/sift-cache")
    );
}

#[test]
fn npx_runtime_includes_cache_dir_env() {
    let request = RuntimeRequest {
        kind: RuntimeKind::Npx,
        package: "pkg".to_string(),
        version: "1.2.3".to_string(),
        cache_dir: PathBuf::from("/tmp/sift-cache"),
        extra_args: vec![],
    };

    let spec = resolve_runtime(&request).unwrap();

    assert_eq!(spec.command, "npx");
    assert_eq!(spec.args, vec!["pkg@1.2.3".to_string()]);
    assert_eq!(
        spec.env.get("npm_config_cache").map(|v| v.as_str()),
        Some("/tmp/sift-cache")
    );
}
