//! Integration tests for SkillInstallPipeline.

use sift_core::client::claude_code::ClaudeCodeClient;
use sift_core::config::SkillConfigEntry;
use sift_core::context::AppContext;
use sift_core::fs::LinkMode;
use sift_core::skills::installer::{SkillInstallPipeline, SkillPipelineRequest};
use sift_core::types::ConfigScope;
use tempfile::TempDir;

#[test]
fn skill_pipeline_writes_toml_and_deploys() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let skill_src = temp.path().join("skill-source");

    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(
        skill_src.join("SKILL.md"),
        "# Test Skill\nInstructions here.",
    )
    .unwrap();

    let ctx = AppContext::with_global_config_dir(
        home.clone(),
        project.clone(),
        temp.path().join("state"),
        temp.path().join("config"),
        LinkMode::Copy, // Use copy for testing
    );

    let pipeline = SkillInstallPipeline::new(&ctx, ConfigScope::PerProjectShared);
    let client = ClaudeCodeClient::new();

    let entry = SkillConfigEntry {
        source: format!("local:{}", skill_src.display()),
        version: None,
        targets: None,
        ignore_targets: None,
        reset_version: false,
    };

    let request = SkillPipelineRequest {
        name: "test-skill".to_string(),
        entry,
        force: false,
    };

    let report = pipeline.install(&client, request).unwrap();

    assert!(report.changed);
    assert!(report.applied);

    // Verify sift.toml
    let toml_path = project.join("sift.toml");
    assert!(toml_path.exists());
    let content = std::fs::read_to_string(&toml_path).unwrap();
    assert!(content.contains("test-skill"));

    // Verify skill was copied
    let skill_dir = project.join(".claude").join("skills").join("test-skill");
    assert!(skill_dir.exists());
    assert!(skill_dir.join("SKILL.md").exists());
}

#[test]
fn skill_pipeline_skips_non_targeted_client() {
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let project = temp.path().join("project");
    let skill_src = temp.path().join("skill-source");

    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&skill_src).unwrap();
    std::fs::write(skill_src.join("SKILL.md"), "# Targeted Skill").unwrap();

    let ctx = AppContext::with_global_config_dir(
        home.clone(),
        project.clone(),
        temp.path().join("state"),
        temp.path().join("config"),
        LinkMode::Copy,
    );

    let pipeline = SkillInstallPipeline::new(&ctx, ConfigScope::PerProjectShared);
    let client = ClaudeCodeClient::new();

    let entry = SkillConfigEntry {
        source: format!("local:{}", skill_src.display()),
        version: None,
        targets: Some(vec!["amp".to_string()]), // Only target amp
        ignore_targets: None,
        reset_version: false,
    };

    let request = SkillPipelineRequest {
        name: "targeted-skill".to_string(),
        entry,
        force: false,
    };

    let report = pipeline.install(&client, request).unwrap();

    assert!(report.changed); // sift.toml written
    assert!(!report.applied); // skill NOT deployed
    assert!(!report.warnings.is_empty());

    // sift.toml exists
    assert!(project.join("sift.toml").exists());
    // skill dir should NOT exist
    assert!(
        !project
            .join(".claude")
            .join("skills")
            .join("targeted-skill")
            .exists()
    );
}
