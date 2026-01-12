//! Integration tests for the uninstall command

use std::path::PathBuf;

use tempfile::TempDir;

use sift_core::commands::{InstallCommand, InstallOptions, UninstallCommand, UninstallOptions};
use sift_core::fs::LinkMode;
use sift_core::lockfile::LockfileStore;
use sift_core::types::ConfigScope;

fn setup_isolated_commands() -> (TempDir, InstallCommand, UninstallCommand) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let state = temp.path().join("state");
    let global_config = temp.path().join("config");

    std::fs::create_dir_all(&home).expect("Failed to create home dir");
    std::fs::create_dir_all(&project).expect("Failed to create project dir");
    std::fs::create_dir_all(&state).expect("Failed to create state dir");
    std::fs::create_dir_all(&global_config).expect("Failed to create global config dir");

    let install = InstallCommand::with_global_config_dir(
        home.clone(),
        project.clone(),
        state.clone(),
        global_config.clone(),
        LinkMode::Copy,
    );
    let uninstall = UninstallCommand::with_global_config_dir(
        home,
        project,
        state,
        global_config,
        LinkMode::Copy,
    );

    (temp, install, uninstall)
}

fn write_skill_dir(root: &std::path::Path, relative: &str, name: &str) {
    let skill_dir = root.join(relative);
    std::fs::create_dir_all(&skill_dir).expect("Failed to create skill dir");
    let content =
        format!("---\nname: {name}\ndescription: Test skill\n---\n\nTest instructions.\n");
    std::fs::write(skill_dir.join("SKILL.md"), content).expect("Failed to write SKILL.md");
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("Failed to invoke git");
    assert!(status.success(), "git command failed: {:?}", args);
}

fn init_git_repo(repo: &std::path::Path) {
    std::fs::create_dir_all(repo).expect("Failed to create repo dir");
    run_git(repo, &["init"]);
    run_git(repo, &["checkout", "-b", "main"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("README.md"), "root file").expect("Failed to write README.md");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "init"]);
}

#[test]
fn uninstall_mcp_scope_all_removes_from_all_scopes() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let global_opts = InstallOptions::mcp("shared")
        .with_source("registry:shared")
        .with_scope(ConfigScope::Global);
    install.execute(&global_opts).expect("global install");

    let project_opts = InstallOptions::mcp("shared")
        .with_source("registry:shared")
        .with_scope(ConfigScope::PerProjectShared)
        .with_force(true);
    install.execute(&project_opts).expect("project install");

    let uninstall_opts = UninstallOptions::mcp("shared").with_scope_all();
    let report = uninstall.execute(&uninstall_opts).expect("uninstall");
    assert!(report.changed);

    let project_root = temp.path().join("project");
    let state_dir = temp.path().join("state").join("locks");
    let lockfile =
        LockfileStore::load(Some(project_root.clone()), state_dir).expect("Lockfile should load");
    assert!(!lockfile.mcp_servers.contains_key("shared"));

    let global_config = temp.path().join("config").join("sift.toml");
    if global_config.exists() {
        let content = std::fs::read_to_string(&global_config).expect("read global config");
        assert!(!content.contains("shared"), "global config cleaned");
    }

    let project_config = project_root.join("sift.toml");
    if project_config.exists() {
        let content = std::fs::read_to_string(&project_config).expect("read project config");
        assert!(!content.contains("shared"), "project config cleaned");
    }
}

#[test]
fn uninstall_mcp_defaults_to_lockfile_scope() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let project_opts = InstallOptions::mcp("lock-scope")
        .with_source("registry:lock-scope")
        .with_scope(ConfigScope::PerProjectShared);
    install.execute(&project_opts).expect("project install");

    let uninstall_opts = UninstallOptions::mcp("lock-scope");
    let report = uninstall.execute(&uninstall_opts).expect("uninstall");
    assert!(report.changed);

    let project_root = temp.path().join("project");
    let project_config = project_root.join("sift.toml");
    let content = std::fs::read_to_string(&project_config).expect("read project config");
    assert!(!content.contains("lock-scope"));

    let global_config = temp.path().join("config").join("sift.toml");
    if global_config.exists() {
        let content = std::fs::read_to_string(&global_config).expect("read global config");
        assert!(!content.contains("lock-scope"));
    }
}

#[test]
fn uninstall_skill_removes_config_lockfile_and_dst_dir() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let skill_root = temp.path().join("skills");
    let skill_name = "demo-skill";
    write_skill_dir(&skill_root, skill_name, skill_name);

    let project_opts = InstallOptions::skill(skill_name)
        .with_source(format!("local:{}", skill_root.join(skill_name).display()))
        .with_scope(ConfigScope::PerProjectShared);
    install.execute(&project_opts).expect("install skill");

    let project_root = temp.path().join("project");
    let dst_dir = project_root.join(".claude/skills").join(skill_name);
    assert!(dst_dir.exists(), "skill should be materialized");

    let uninstall_opts = UninstallOptions::skill(skill_name);
    let report = uninstall.execute(&uninstall_opts).expect("uninstall");
    assert!(report.changed);

    assert!(!dst_dir.exists(), "skill directory removed");

    let state_dir = temp.path().join("state").join("locks");
    let lockfile =
        LockfileStore::load(Some(project_root), state_dir).expect("Lockfile should load");
    assert!(!lockfile.skills.contains_key(skill_name));
}

#[test]
fn uninstall_explicit_scope_mismatch_errors() {
    let (_temp, _install, uninstall) = setup_isolated_commands();

    let uninstall_opts = UninstallOptions::mcp("missing").with_scope(ConfigScope::PerProjectShared);
    let result = uninstall.execute(&uninstall_opts);
    assert!(result.is_err());
}

#[test]
fn uninstall_scope_all_removes_local_project_global() {
    let (temp, install, uninstall) = setup_isolated_commands();
    let project_root = temp.path().join("project");
    init_git_repo(&project_root);
    let skill_root = temp.path().join("skills");
    write_skill_dir(&skill_root, "all-scope", "all-scope");
    let skill_source = format!("local:{}", skill_root.join("all-scope").display());

    let global_opts = InstallOptions::skill("all-scope")
        .with_source(&skill_source)
        .with_scope(ConfigScope::Global);
    install.execute(&global_opts).expect("global install");

    let project_opts = InstallOptions::skill("all-scope")
        .with_source(&skill_source)
        .with_scope(ConfigScope::PerProjectShared)
        .with_force(true);
    install.execute(&project_opts).expect("project install");

    let local_opts = InstallOptions::skill("all-scope")
        .with_source(&skill_source)
        .with_scope(ConfigScope::PerProjectLocal)
        .with_force(true);
    install.execute(&local_opts).expect("local install");

    let uninstall_opts = UninstallOptions::skill("all-scope").with_scope_all();
    let report = uninstall.execute(&uninstall_opts).expect("uninstall");
    assert!(report.changed);

    let state_dir = temp.path().join("state").join("locks");
    let lockfile =
        LockfileStore::load(Some(project_root.clone()), state_dir).expect("Lockfile should load");
    assert!(!lockfile.skills.contains_key("all-scope"));

    let global_config = temp.path().join("config").join("sift.toml");
    if global_config.exists() {
        let content = std::fs::read_to_string(&global_config).expect("read global config");
        assert!(!content.contains("all-scope"));
    }

    let project_config = project_root.join("sift.toml");
    if project_config.exists() {
        let content = std::fs::read_to_string(&project_config).expect("read project config");
        assert!(!content.contains("all-scope"));
    }

    let global_dst = temp
        .path()
        .join("home")
        .join(".claude/skills")
        .join("all-scope");
    let project_dst = project_root.join(".claude/skills").join("all-scope");
    assert!(!global_dst.exists());
    assert!(!project_dst.exists());
}

#[test]
fn uninstall_managed_json_removes_mcp_entry() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let opts = InstallOptions::mcp("managed-entry")
        .with_source("registry:managed-entry")
        .with_scope(ConfigScope::PerProjectShared);
    install.execute(&opts).expect("install");

    let project_root = temp.path().join("project");
    let config_path = project_root.join(".mcp.json");
    let content = std::fs::read_to_string(&config_path).expect("read config");
    assert!(content.contains("managed-entry"));

    let uninstall_opts = UninstallOptions::mcp("managed-entry");
    uninstall.execute(&uninstall_opts).expect("uninstall");

    let content = std::fs::read_to_string(&config_path).expect("read config");
    assert!(!content.contains("managed-entry"));
}

#[test]
fn uninstall_uses_lockfile_scope_for_skill_dir() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let skill_root = temp.path().join("skills");
    let skill_name = "lock-skill";
    write_skill_dir(&skill_root, skill_name, skill_name);

    let project_opts = InstallOptions::skill(skill_name)
        .with_source(format!("local:{}", skill_root.join(skill_name).display()))
        .with_scope(ConfigScope::PerProjectShared);
    install.execute(&project_opts).expect("install skill");

    let dst_dir = temp
        .path()
        .join("project")
        .join(".claude/skills")
        .join(skill_name);
    assert!(dst_dir.exists());

    let uninstall_opts = UninstallOptions::skill(skill_name);
    uninstall.execute(&uninstall_opts).expect("uninstall");

    assert!(!dst_dir.exists());
}

#[test]
fn uninstall_errors_when_missing_everywhere() {
    let (_temp, _install, uninstall) = setup_isolated_commands();

    let uninstall_opts = UninstallOptions::skill("missing");
    let result = uninstall.execute(&uninstall_opts);
    assert!(result.is_err());
}

#[test]
fn uninstall_mcp_all_scopes_clears_lockfile_entry() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let project_opts = InstallOptions::mcp("clear-lock")
        .with_source("registry:clear-lock")
        .with_scope(ConfigScope::PerProjectShared);
    install.execute(&project_opts).expect("install");

    let uninstall_opts = UninstallOptions::mcp("clear-lock").with_scope_all();
    uninstall.execute(&uninstall_opts).expect("uninstall");

    let project_root = temp.path().join("project");
    let state_dir = temp.path().join("state").join("locks");
    let lockfile =
        LockfileStore::load(Some(project_root), state_dir).expect("Lockfile should load");
    assert!(!lockfile.mcp_servers.contains_key("clear-lock"));
}

fn owned_config_path(root: &TempDir, path: &str) -> PathBuf {
    root.path().join(path)
}

#[test]
fn uninstall_managed_json_respects_ownership() {
    let (temp, install, uninstall) = setup_isolated_commands();

    let opts = InstallOptions::mcp("owned")
        .with_source("registry:owned")
        .with_scope(ConfigScope::PerProjectShared)
        .with_force(true);
    install.execute(&opts).expect("install");

    let config_path = owned_config_path(&temp, "project/.mcp.json");
    let content = std::fs::read_to_string(&config_path).expect("read config");
    assert!(content.contains("owned"));

    let uninstall_opts = UninstallOptions::mcp("owned");
    uninstall.execute(&uninstall_opts).expect("uninstall");

    let content = std::fs::read_to_string(&config_path).expect("read config");
    assert!(!content.contains("owned"));
}
