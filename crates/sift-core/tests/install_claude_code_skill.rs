use std::fs;

use tempfile::TempDir;

use sift_core::client::ClientAdapter;
use sift_core::client::ClientContext;
use sift_core::client::claude_code::ClaudeCodeClient;
use sift_core::config::{ConfigScope, ConfigStore, SkillConfigEntry};
use sift_core::fs::LinkMode;
use sift_core::install::orchestrator::InstallOrchestrator;
use sift_core::install::scope::{RepoStatus, ResourceKind, ScopeRequest, resolve_scope};
use sift_core::skills::installer::SkillInstaller;

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Test skill for {name}.\n---\n# {name}\n")
}

#[test]
fn install_skill_links_to_project_scope() {
    let temp = TempDir::new().unwrap();
    let config_store = ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        temp.path().join("project"),
    );

    let cache_dir = temp.path().join("demo");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("SKILL.md"), skill_md("demo")).unwrap();

    let home = temp.path().join("home");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).unwrap();

    let ownership_store =
        sift_core::config::OwnershipStore::new(temp.path().join("state"), Some(project.clone()));
    let skill_installer = SkillInstaller::new(temp.path().join("locks"), Some(project.clone()));
    let orchestrator = InstallOrchestrator::new(
        config_store,
        ownership_store,
        skill_installer,
        LinkMode::Copy,
    );
    let adapter = ClaudeCodeClient::new();
    let ctx = ClientContext::new(home.clone(), project.clone());

    let entry = SkillConfigEntry {
        source: "registry:demo".to_string(),
        version: Some("latest".to_string()),
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    let resolution = resolve_scope(
        ResourceKind::Skill,
        ScopeRequest::Explicit(ConfigScope::PerProjectShared),
        adapter.capabilities().skills,
        RepoStatus::from_project_root(&ctx.project_root),
    )
    .unwrap();

    let report = orchestrator
        .install_skill(
            &adapter,
            &ctx,
            "demo",
            entry,
            &cache_dir,
            resolution,
            false,
            "resolved-1",
            "latest",
            "registry:test",
        )
        .unwrap();

    assert!(report.applied);

    let dst = project.join(".claude/skills/demo/SKILL.md");
    assert!(dst.exists());
}
