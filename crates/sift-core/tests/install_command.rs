//! Integration tests for the install command

use std::path::Path;
use std::process::Command;

use sift_core::version::store::LockfileStore;
use tempfile::TempDir;
use url::Url;

use sift_core::commands::{InstallCommand, InstallOptions};
use sift_core::config::ConfigScope;
use sift_core::fs::LinkMode;
use toml::Value;

fn setup_isolated_install_command() -> (TempDir, InstallCommand) {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let state = temp.path().join("state");
    let global_config = temp.path().join("config");

    std::fs::create_dir_all(&home).expect("Failed to create home dir");
    std::fs::create_dir_all(&project).expect("Failed to create project dir");
    std::fs::create_dir_all(&state).expect("Failed to create state dir");
    std::fs::create_dir_all(&global_config).expect("Failed to create global config dir");

    let cmd =
        InstallCommand::with_global_config_dir(home, project, state, global_config, LinkMode::Copy);

    (temp, cmd)
}

fn write_skill_dir(root: &std::path::Path, relative: &str, name: &str) {
    let skill_dir = root.join(relative);
    std::fs::create_dir_all(&skill_dir).expect("Failed to create skill dir");
    let content =
        format!("---\nname: {name}\ndescription: Test skill\n---\n\nTest instructions.\n");
    std::fs::write(skill_dir.join("SKILL.md"), content).expect("Failed to write SKILL.md");
}

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .expect("Failed to invoke git");
    assert!(status.success(), "git command failed: {:?}", args);
}

fn git_rev_parse(repo: &Path, rev: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()
        .expect("Failed to run git rev-parse");
    assert!(output.status.success(), "git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init_git_repo_with_skill(repo: &std::path::Path, skill_rel: &str, name: &str) {
    std::fs::create_dir_all(repo).expect("Failed to create repo dir");
    run_git(repo, &["init"]);
    run_git(repo, &["checkout", "-b", "main"]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test User"]);
    run_git(repo, &["config", "commit.gpgsign", "false"]);

    add_skill_to_repo(repo, skill_rel, name);
    std::fs::write(repo.join("README.md"), "root file").expect("Failed to write README.md");

    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-m", "init"]);
}

fn add_skill_to_repo(repo: &std::path::Path, skill_rel: &str, name: &str) {
    let skill_dir = repo.join(skill_rel);
    std::fs::create_dir_all(&skill_dir).expect("Failed to create skill dir");
    let content =
        format!("---\nname: {name}\ndescription: Test skill\n---\n\nTest instructions.\n");
    std::fs::write(skill_dir.join("SKILL.md"), content).expect("Failed to write SKILL.md");
}

#[test]
fn install_mcp_server_creates_config_and_applies() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("postgres")
        .with_source("registry:postgres-mcp")
        .with_version("1.2.3")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");

    // Verify report
    assert_eq!(report.name, "postgres");
    assert!(report.changed);
    assert!(report.applied);

    // Verify config file was created
    let config_path = temp.path().join("project").join("sift.toml");
    assert!(config_path.exists(), "sift.toml should be created");

    // Verify .mcp.json was created for Claude Code client
    let mcp_json_path = temp.path().join("project").join(".mcp.json");
    assert!(mcp_json_path.exists(), ".mcp.json should be created");

    // Verify content
    let content = std::fs::read_to_string(&mcp_json_path).expect("Should read .mcp.json");
    assert!(
        content.contains("postgres"),
        "Should contain postgres server"
    );
}

#[test]
fn install_mcp_writes_lockfile() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("postgres")
        .with_source("registry:postgres-mcp")
        .with_version("1.2.3")
        .with_scope(ConfigScope::PerProjectShared);

    cmd.execute(&opts).expect("Install should succeed");

    let project_root = temp.path().join("project");
    let state_dir = temp.path().join("state").join("locks");
    let lockfile = sift_core::version::store::LockfileStore::load(Some(project_root), state_dir)
        .expect("Lockfile should load");

    let locked = lockfile
        .mcp_servers
        .get("postgres")
        .expect("MCP server should be locked");
    assert_eq!(locked.constraint, "1.2.3");
    assert_eq!(locked.resolved_version, "todo");
}

#[test]
fn install_mcp_auto_scope_writes_project_override_and_local_config() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("auto-local").with_source("registry:auto-local");

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let project_root = temp.path().join("project");
    let global_config_path = temp.path().join("config").join("sift.toml");
    assert!(
        global_config_path.exists(),
        "Global sift.toml should be created for local scope"
    );

    let content =
        std::fs::read_to_string(&global_config_path).expect("Should read global sift.toml");
    let toml_value: Value = content.parse().expect("Should parse global sift.toml");
    let projects = toml_value
        .get("projects")
        .and_then(|value| value.as_table())
        .expect("Expected [projects] table in global config");

    let project_key = project_root.to_string_lossy().to_string();
    let project_entry = projects
        .get(&project_key)
        .and_then(|value| value.as_table())
        .expect("Expected project entry under [projects]");
    let mcp_entries = project_entry
        .get("mcp")
        .and_then(|value| value.as_table())
        .expect("Expected [projects.<path>.mcp] table");
    let mcp_entry = mcp_entries
        .get("auto-local")
        .and_then(|value| value.as_table())
        .expect("Expected local MCP entry under [projects.<path>.mcp]");
    assert_eq!(
        mcp_entry.get("source").and_then(|value| value.as_str()),
        Some("registry:auto-local")
    );

    let project_config_path = project_root.join("sift.toml");
    if project_config_path.exists() {
        let project_content =
            std::fs::read_to_string(&project_config_path).expect("Should read project sift.toml");
        assert!(
            !project_content.contains("auto-local"),
            "Local MCP should not be written to project sift.toml"
        );
    }

    let claude_config_path = temp.path().join("home").join(".claude.json");
    assert!(
        claude_config_path.exists(),
        "Local MCP should be written to ~/.claude.json"
    );
    let claude_content =
        std::fs::read_to_string(&claude_config_path).expect("Should read .claude.json");
    assert!(
        claude_content.contains("auto-local"),
        "Claude local MCP config should include auto-local"
    );
}

#[test]
fn install_skill_creates_config_and_directory() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::skill("commit")
        .with_source("registry:official/commit")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");

    // Verify report
    assert_eq!(report.name, "commit");
    assert!(report.changed);

    // Verify config file was created
    let config_path = temp.path().join("project").join("sift.toml");
    assert!(config_path.exists(), "sift.toml should be created");

    // Parse and verify config content
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("[skill.commit]"),
        "Should contain skill entry"
    );
}

#[test]
fn install_is_idempotent() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("idempotent-test")
        .with_source("registry:test")
        .with_scope(ConfigScope::PerProjectShared);

    // First install
    let report1 = cmd.execute(&opts).expect("First install should succeed");
    assert!(report1.changed, "First install should change config");

    // Second install (identical)
    let report2 = cmd.execute(&opts).expect("Second install should succeed");
    assert!(!report2.changed, "Second install should be a no-op");
}

#[test]
fn install_with_force_overwrites_existing() {
    let (_temp, cmd) = setup_isolated_install_command();

    // First install
    let opts1 = InstallOptions::mcp("force-test")
        .with_source("registry:source1")
        .with_scope(ConfigScope::PerProjectShared);
    cmd.execute(&opts1).expect("First install should succeed");

    // Second install with different source (without force - should fail)
    let opts2 = InstallOptions::mcp("force-test")
        .with_source("registry:source2")
        .with_scope(ConfigScope::PerProjectShared);
    let result = cmd.execute(&opts2);
    assert!(result.is_err(), "Should fail without force flag");

    // Third install with force flag
    let opts3 = InstallOptions::mcp("force-test")
        .with_source("registry:source2")
        .with_scope(ConfigScope::PerProjectShared)
        .with_force(true);
    let report = cmd.execute(&opts3).expect("Force install should succeed");
    assert!(report.changed, "Force install should change config");
}

#[test]
fn install_global_scope_writes_to_global_config() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("global-test")
        .with_source("registry:global-test")
        .with_scope(ConfigScope::Global);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    // Verify global config was created
    let global_config_path = temp.path().join("config").join("sift.toml");
    assert!(
        global_config_path.exists(),
        "Global sift.toml should be created"
    );

    let content = std::fs::read_to_string(&global_config_path).expect("Should read global config");
    assert!(
        content.contains("global-test"),
        "Should contain global-test entry"
    );
}

#[test]
fn install_with_version_constraint() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::skill("versioned-skill")
        .with_source("registry:test/versioned")
        .with_version("^1.0.0")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    // Verify config contains version
    let config_path = temp.path().join("project").join("sift.toml");
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("version = \"^1.0.0\""),
        "Should contain version constraint"
    );
}

#[test]
fn install_mcp_with_runtime() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("docker-mcp")
        .with_source("registry:docker-mcp")
        .with_runtime("docker")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    // Verify config contains runtime
    let config_path = temp.path().join("project").join("sift.toml");
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("runtime = \"docker\""),
        "Should contain runtime"
    );
}

#[test]
fn install_mcp_uses_version_in_runtime_args() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("versioned-mcp")
        .with_source("registry:versioned-mcp")
        .with_version("1.2.3")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let mcp_json_path = temp.path().join("project").join(".mcp.json");
    let content = std::fs::read_to_string(&mcp_json_path).expect("Should read .mcp.json");
    assert!(
        content.contains("versioned-mcp@1.2.3"),
        "Expected resolved args to include versioned-mcp@1.2.3"
    );
}

#[test]
fn install_mcp_explicit_stdio_command_ignores_source() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("custom-mcp")
        .with_transport("stdio")
        .with_command(vec!["npx", "-y", "@acme/custom"])
        .with_source("registry:ignored")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("Ignoring --source"))
    );

    let config_store = sift_core::config::ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let config = config_store.load().expect("Should load config");
    let entry = config
        .mcp
        .get("custom-mcp")
        .expect("MCP entry should exist");
    assert_eq!(entry.transport.as_deref(), Some("stdio"));
    assert_eq!(entry.runtime.as_deref(), Some("shell"));
    assert_eq!(entry.source, "local:npx");
    assert_eq!(
        entry.args,
        vec!["-y".to_string(), "@acme/custom".to_string()]
    );
}

#[test]
fn install_mcp_explicit_stdio_command_ignores_version() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("custom-mcp")
        .with_transport("stdio")
        .with_command(vec!["npx", "-y", "@acme/custom"])
        .with_version("2.0.0")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(
        report.warnings.iter().any(|warning| warning
            .contains("Ignoring version because an explicit command or URL was provided")),
        "Expected warning about ignoring explicit version input"
    );
}

#[test]
fn install_mcp_explicit_http_url_ignores_source() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("http-mcp")
        .with_transport("http")
        .with_url("https://mcp.example.com")
        .with_source("registry:ignored")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("Ignoring --source"))
    );

    let config_store = sift_core::config::ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let config = config_store.load().expect("Should load config");
    let entry = config.mcp.get("http-mcp").expect("MCP entry should exist");
    assert_eq!(entry.transport.as_deref(), Some("http"));
    assert_eq!(entry.url.as_deref(), Some("https://mcp.example.com"));
    assert_eq!(entry.source, "local:http-mcp");
}

#[test]
fn install_mcp_registry_with_env_and_headers() {
    let (temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("registry-mcp")
        .with_env("API_KEY=secret")
        .with_header("X-Trace=1")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let config_store = sift_core::config::ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        temp.path().join("project"),
    );
    let config = config_store.load().expect("Should load config");
    let entry = config
        .mcp
        .get("registry-mcp")
        .expect("MCP entry should exist");
    assert_eq!(entry.source, "registry:registry-mcp");
    assert_eq!(entry.env.get("API_KEY"), Some(&"secret".to_string()));
    assert_eq!(entry.headers.get("X-Trace"), Some(&"1".to_string()));
}

#[test]
fn install_mcp_registry_without_version_support_ignores_version() {
    let (temp, cmd) = setup_isolated_install_command();
    let global_config_path = temp.path().join("config").join("sift.toml");
    let global_config = r#"
[registry.company]
type = "claude-marketplace"
source = "github:company/plugins"
"#;
    std::fs::write(&global_config_path, global_config).expect("Failed to write global config");

    let opts = InstallOptions::mcp("demo-mcp")
        .with_source("registry:company/demo-mcp")
        .with_version("1.2.3")
        .with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("Registry does not support version pinning")),
        "Expected warning about registry not supporting version pinning"
    );

    let mcp_json_path = temp.path().join("project").join(".mcp.json");
    let content = std::fs::read_to_string(&mcp_json_path).expect("Should read .mcp.json");
    assert!(
        !content.contains("demo-mcp@1.2.3"),
        "Expected args to ignore versioned request"
    );
}

#[test]
fn install_skill_from_local_path_infers_name_and_source() {
    let (temp, cmd) = setup_isolated_install_command();
    let project_root = temp.path().join("project");
    write_skill_dir(&project_root, "skills/commit", "commit");

    let opts = InstallOptions::skill("./skills/commit").with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let config_store = sift_core::config::ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        project_root.clone(),
    );
    let config = config_store.load().expect("Should load config");
    let entry = config
        .skill
        .get("commit")
        .expect("Skill entry should be keyed by directory name");
    assert_eq!(entry.source, "local:./skills/commit");
}

#[test]
fn install_skill_from_git_url_infers_name_and_source() {
    let (temp, cmd) = setup_isolated_install_command();
    let project_root = temp.path().join("project");
    let repo_root = temp.path().join("repo");
    let skill_rel = "skills/gh-fix-ci";
    init_git_repo_with_skill(&repo_root, skill_rel, "gh-fix-ci");

    let file_url = Url::from_directory_path(&repo_root)
        .expect("repo root should convert to file URL")
        .to_string();
    let file_url = file_url.trim_end_matches('/');
    let url = format!("git+{}/tree/main/{}", file_url, skill_rel);
    let opts = InstallOptions::skill(url.clone()).with_scope(ConfigScope::PerProjectShared);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let config_store = sift_core::config::ConfigStore::from_paths(
        ConfigScope::PerProjectShared,
        temp.path().join("config"),
        project_root,
    );
    let config = config_store.load().expect("Should load config");
    let entry = config
        .skill
        .get("gh-fix-ci")
        .expect("Skill entry should be keyed by URL directory name");
    let normalized = url.trim_start_matches("git+");
    assert_eq!(entry.source, format!("git:{normalized}"));
}

#[test]
fn install_skill_from_git_url_materializes_skill_dir() {
    let (temp, cmd) = setup_isolated_install_command();
    let project_root = temp.path().join("project");
    let repo_root = temp.path().join("repo");
    let skill_rel = "skills/json-canvas";

    init_git_repo_with_skill(&repo_root, skill_rel, "json-canvas");

    let file_url = Url::from_directory_path(&repo_root)
        .expect("repo root should convert to file URL")
        .to_string();
    let file_url = file_url.trim_end_matches('/');
    let url = format!("git+{}/tree/main/{}", file_url, skill_rel);

    let opts = InstallOptions::skill(url);
    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let installed = project_root.join(".claude/skills/json-canvas/SKILL.md");
    assert!(installed.exists(), "Expected installed SKILL.md to exist");
}

#[test]
fn install_skill_from_git_url_records_git_metadata() {
    let (temp, cmd) = setup_isolated_install_command();
    let project_root = temp.path().join("project");
    let repo_root = temp.path().join("repo");
    let skill_rel = "skills/json-canvas";

    init_git_repo_with_skill(&repo_root, skill_rel, "json-canvas");

    let file_url = Url::from_directory_path(&repo_root)
        .expect("repo root should convert to file URL")
        .to_string();
    let file_url = file_url.trim_end_matches('/');
    let url = format!("git+{}/tree/main/{}", file_url, skill_rel);

    let opts = InstallOptions::skill(url);
    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(report.changed);

    let lockfile = LockfileStore::load(
        Some(project_root.clone()),
        temp.path().join("state").join("locks"),
    )
    .expect("Lockfile should load");
    let locked = lockfile
        .skills
        .get("json-canvas")
        .expect("Skill should be locked");
    let commit = git_rev_parse(&repo_root, "HEAD");
    assert_eq!(locked.resolved_version, commit);
    assert_eq!(locked.git_ref.as_deref(), Some("main"));
    assert_eq!(locked.git_subdir.as_deref(), Some(skill_rel));
    assert!(
        locked
            .git_repo
            .as_deref()
            .unwrap_or_default()
            .contains("file://")
    );
}

#[test]
fn install_skills_from_same_repo_reuses_bare_repo() {
    let (temp, cmd) = setup_isolated_install_command();
    let repo_root = temp.path().join("repo");
    let skill_a = "skills/a-skill";
    let skill_b = "skills/b-skill";

    init_git_repo_with_skill(&repo_root, skill_a, "a-skill");
    add_skill_to_repo(&repo_root, skill_b, "b-skill");
    run_git(&repo_root, &["add", "."]);
    run_git(&repo_root, &["commit", "-m", "add b-skill"]);

    let file_url = Url::from_directory_path(&repo_root)
        .expect("repo root should convert to file URL")
        .to_string();
    let file_url = file_url.trim_end_matches('/');
    let url_a = format!("git+{}/tree/main/{}", file_url, skill_a);
    let url_b = format!("git+{}/tree/main/{}", file_url, skill_b);

    cmd.execute(&InstallOptions::skill(url_a))
        .expect("First install should succeed");
    cmd.execute(&InstallOptions::skill(url_b))
        .expect("Second install should succeed");

    let git_cache_dir = temp.path().join("state").join("git");
    let count = std::fs::read_dir(&git_cache_dir)
        .expect("git cache dir should exist")
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with(".git"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(count, 1, "Expected a single bare repo cache");
}

#[test]
fn install_warns_when_multiple_registries_and_no_source() {
    let (temp, cmd) = setup_isolated_install_command();
    let global_config_path = temp.path().join("config").join("sift.toml");
    let global_config = r#"
[registry.official]
type = "sift"
url = "https://registry.sift.sh/v1"

[registry.company]
type = "claude-marketplace"
source = "github:company/plugins"
"#;
    std::fs::write(&global_config_path, global_config).expect("Failed to write global config");

    let opts = InstallOptions::skill("demo").with_scope(ConfigScope::Global);

    let report = cmd.execute(&opts).expect("Install should succeed");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("Multiple registries")),
        "Expected warning about multiple registries when --source is omitted"
    );
}

// =============================================================================
// Error path tests
// =============================================================================

#[test]
fn install_mcp_errors_when_both_command_and_url() {
    let (_temp, cmd) = setup_isolated_install_command();

    // Cannot specify both --command and --url
    let opts = InstallOptions::mcp("conflict-mcp")
        .with_transport("stdio")
        .with_command(vec!["npx", "-y", "@acme/custom"])
        .with_url("https://mcp.example.com")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(
        result.is_err(),
        "Should fail when both command and URL are provided"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Cannot specify both stdio command and HTTP URL"),
        "Expected error about conflicting command and URL"
    );
}

#[test]
fn install_mcp_errors_when_stdio_transport_with_url() {
    let (_temp, cmd) = setup_isolated_install_command();

    // --transport stdio conflicts with --url
    let opts = InstallOptions::mcp("stdio-url-conflict")
        .with_transport("stdio")
        .with_url("https://mcp.example.com")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(
        result.is_err(),
        "Should fail when stdio transport conflicts with URL"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("HTTP URL requires transport 'http'"),
        "Expected error about HTTP URL requiring http transport"
    );
}

#[test]
fn install_mcp_errors_when_http_transport_with_command() {
    let (_temp, cmd) = setup_isolated_install_command();

    // --transport http conflicts with --command
    let opts = InstallOptions::mcp("http-cmd-conflict")
        .with_transport("http")
        .with_command(vec!["npx", "-y", "@acme/custom"])
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(
        result.is_err(),
        "Should fail when http transport conflicts with command"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Stdio command requires transport 'stdio'"),
        "Expected error about stdio command requiring stdio transport"
    );
}

#[test]
fn install_mcp_errors_when_http_transport_without_url() {
    let (_temp, cmd) = setup_isolated_install_command();

    // --transport http requires --url
    let opts = InstallOptions::mcp("no-url-mcp")
        .with_transport("http")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(result.is_err(), "Should fail when http transport lacks URL");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("HTTP transport requires a URL"),
        "Expected error about http transport requiring URL"
    );
}

#[test]
fn install_mcp_errors_on_invalid_transport() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("invalid-transport-mcp")
        .with_transport("foo")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(result.is_err(), "Should fail with invalid transport value");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Invalid transport: foo"),
        "Expected error about invalid transport value"
    );
}

#[test]
fn install_mcp_errors_on_invalid_env_format() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("invalid-env-mcp")
        .with_env("NOEQUALS")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(result.is_err(), "Should fail with invalid env format");
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Invalid env entry (expected KEY=VALUE)"),
        "Expected error about invalid env format"
    );
}

#[test]
fn install_mcp_errors_on_invalid_header_format() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("invalid-header-mcp")
        .with_header("NOEQUALS")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(result.is_err(), "Should fail with invalid header format");
    let err = result.unwrap_err();
    assert!(
        err.to_string()
            .contains("Invalid header entry (expected KEY=VALUE)"),
        "Expected error about invalid header format"
    );
}

#[test]
fn install_mcp_errors_on_empty_env_key() {
    let (_temp, cmd) = setup_isolated_install_command();

    let opts = InstallOptions::mcp("empty-key-mcp")
        .with_env("=value")
        .with_scope(ConfigScope::PerProjectShared);

    let result = cmd.execute(&opts);
    assert!(result.is_err(), "Should fail with empty env key");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Invalid env entry (empty key)"),
        "Expected error about empty env key"
    );
}
