use std::collections::HashMap;

use tempfile::TempDir;

use sift_core::client::ClientAdapter;
use sift_core::client::ClientContext;
use sift_core::client::claude_code::ClaudeCodeClient;
use sift_core::config::{ConfigScope, ConfigStore, McpConfigEntry};
use sift_core::fs::LinkMode;
use sift_core::mcp::spec::McpResolvedServer;
use sift_core::orchestration::orchestrator::InstallMcpRequest;
use sift_core::orchestration::orchestrator::InstallOrchestrator;
use sift_core::orchestration::scope::{RepoStatus, ResourceKind, ScopeRequest, resolve_scope};

#[test]
fn install_mcp_updates_config_and_writes_project_file() {
    let temp = TempDir::new().unwrap();
    let config_store = ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        temp.path().join("project"),
    );

    let home = temp.path().join("home");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let ownership_store =
        sift_core::config::OwnershipStore::new(temp.path().join("state"), Some(project.clone()));
    let skill_installer = sift_core::skills::installer::SkillInstaller::new(
        temp.path().join("locks"),
        Some(project.clone()),
    );
    let orchestrator = InstallOrchestrator::new(
        config_store,
        ownership_store,
        skill_installer,
        LinkMode::Auto,
    );
    let adapter = ClaudeCodeClient::new();
    let ctx = ClientContext::new(home.clone(), project.clone());

    let entry = McpConfigEntry {
        transport: Some("stdio".to_string()),
        source: "registry:demo".to_string(),
        runtime: Some("node".to_string()),
        args: vec!["--flag".to_string()],
        url: None,
        headers: HashMap::new(),
        targets: None,
        ignore_targets: None,
        env: HashMap::new(),
        reset_targets: false,
        reset_ignore_targets: false,
        reset_env: None,
        reset_env_all: false,
    };

    let servers = vec![McpResolvedServer::stdio(
        "demo".to_string(),
        "npx".to_string(),
        vec!["pkg@1.2.3".to_string()],
        HashMap::new(),
    )];

    let resolution = resolve_scope(
        ResourceKind::Mcp,
        ScopeRequest::Explicit(ConfigScope::PerProjectShared),
        adapter.capabilities().mcp,
        RepoStatus::from_project_root(&ctx.project_root),
    )
    .unwrap();

    let report = orchestrator
        .install_mcp(
            &adapter,
            &ctx,
            InstallMcpRequest {
                name: "demo",
                entry,
                servers: &servers,
                resolution,
                force: false,
                declared_version: None,
            },
        )
        .unwrap();

    assert!(report.applied);
    let loaded = orchestrator.config_store().load().unwrap();
    assert!(loaded.mcp.contains_key("demo"));
    assert!(project.join(".mcp.json").exists());
}
