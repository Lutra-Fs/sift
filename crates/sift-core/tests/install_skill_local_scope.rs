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
fn install_skill_local_git_writes_exclude() {
    let temp = TempDir::new().expect("tempdir should succeed");
    let project = temp.path().join("project");
    fs::create_dir_all(project.join(".git/info")).expect("create_dir_all should succeed");

    let home = temp.path().join("home");
    let cache_dir = temp.path().join("demo");
    fs::create_dir_all(&cache_dir).expect("create_dir_all should succeed");
    fs::write(cache_dir.join("SKILL.md"), skill_md("demo")).expect("write should succeed");

    let config_store = ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        project.clone(),
    );

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
        ScopeRequest::Explicit(ConfigScope::PerProjectLocal),
        adapter.capabilities().skills,
        RepoStatus::from_project_root(&ctx.project_root),
    )
    .expect("local scope resolution should succeed in git repo");

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
        .expect("local install should succeed in git repo");

    assert!(report.applied);
    let exclude_path = project.join(".git/info/exclude");
    let content = fs::read_to_string(&exclude_path).expect("read should succeed");
    assert!(content.lines().any(|line| line == ".claude/skills"));
}

#[test]
fn install_skill_local_non_git_errors() {
    let temp = TempDir::new().expect("tempdir should succeed");
    let project = temp.path().join("project");
    fs::create_dir_all(&project).expect("create_dir_all should succeed");

    let home = temp.path().join("home");

    let adapter = ClaudeCodeClient::new();
    let ctx = ClientContext::new(home.clone(), project.clone());

    let err = resolve_scope(
        ResourceKind::Skill,
        ScopeRequest::Explicit(ConfigScope::PerProjectLocal),
        adapter.capabilities().skills,
        RepoStatus::from_project_root(&ctx.project_root),
    )
    .expect_err("local scope resolution should fail without git repo");

    let msg = err.to_string();
    assert!(msg.contains("project") || msg.contains("global"));
}
