//! Integration tests for MCPB source resolution in install command
//!
//! Tests that MCPB URLs are correctly detected and normalized to the mcpb: prefix format.
//! These tests focus on source detection and normalization behavior.
//! For actual MCPB bundle fetching and manifest parsing, see unit tests in mcpb module.

use sift_core::commands::{InstallCommand, InstallOptions};
use sift_core::fs::LinkMode;
use sift_core::mcpb::{derive_name_from_mcpb_url, is_mcpb_url, normalize_mcpb_source};
use sift_core::types::ConfigScope;
use tempfile::TempDir;

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

// =========================================================================
// MCPB URL Detection Tests (Unit-level, no network)
// =========================================================================

#[test]
fn mcpb_url_detection_simple() {
    assert!(is_mcpb_url(
        "https://github.com/10XGenomics/txg-mcp/releases/latest/download/txg-node.mcpb"
    ));
}

#[test]
fn mcpb_url_detection_with_query_string() {
    assert!(is_mcpb_url(
        "https://example.com/download/server.mcpb?token=abc123"
    ));
}

#[test]
fn mcpb_url_detection_with_fragment() {
    assert!(is_mcpb_url("https://example.com/server.mcpb#section"));
}

#[test]
fn mcpb_url_detection_negative_cases() {
    assert!(!is_mcpb_url("https://github.com/org/repo"));
    assert!(!is_mcpb_url("https://example.com/server.zip"));
    assert!(!is_mcpb_url("https://example.com/mcpb")); // No dot before mcpb
    assert!(!is_mcpb_url("./local/path/server.js"));
}

#[test]
fn mcpb_url_detection_case_sensitive() {
    // .mcpb extension is case-sensitive
    assert!(!is_mcpb_url("https://example.com/server.MCPB"));
    assert!(!is_mcpb_url("https://example.com/server.Mcpb"));
}

// =========================================================================
// MCPB Source Normalization Tests (Unit-level, no network)
// =========================================================================

#[test]
fn mcpb_source_normalization_already_prefixed() {
    let input = "mcpb:https://example.com/server.mcpb";
    assert_eq!(normalize_mcpb_source(input), Some(input.to_string()));
}

#[test]
fn mcpb_source_normalization_raw_url() {
    let url = "https://github.com/10XGenomics/txg-mcp/releases/latest/download/txg-node.mcpb";
    let expected = format!("mcpb:{}", url);
    assert_eq!(normalize_mcpb_source(url), Some(expected));
}

#[test]
fn mcpb_source_normalization_not_mcpb() {
    assert_eq!(normalize_mcpb_source("https://example.com/repo.git"), None);
    assert_eq!(normalize_mcpb_source("registry:my-server"), None);
    assert_eq!(normalize_mcpb_source("local:/path/to/server"), None);
}

// =========================================================================
// MCPB Name Derivation Tests (Unit-level, no network)
// =========================================================================

#[test]
fn mcpb_name_derivation_simple() {
    let url = "https://example.com/releases/my-server.mcpb";
    assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "my-server");
}

#[test]
fn mcpb_name_derivation_with_prefix() {
    let url = "mcpb:https://github.com/org/repo/txg-node.mcpb";
    assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "txg-node");
}

#[test]
fn mcpb_name_derivation_with_query() {
    let url = "https://example.com/server.mcpb?version=1.0";
    assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "server");
}

#[test]
fn mcpb_name_derivation_github_release() {
    let url = "https://github.com/org/repo/releases/download/v1.0.0/my-mcp-server.mcpb";
    assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "my-mcp-server");
}

// =========================================================================
// Non-MCPB URL Handling Tests (Install command behavior)
// =========================================================================

#[test]
fn install_mcp_non_mcpb_url_is_not_detected_as_mcpb() {
    let (temp, cmd) = setup_isolated_install_command();

    // A regular URL (not .mcpb) should NOT be treated as MCPB
    // Note: https:// URLs are treated as git sources, not MCPB
    let regular_url = "https://example.com/server.zip";
    let opts = InstallOptions::mcp(regular_url).with_scope(ConfigScope::PerProjectShared);

    let report = cmd
        .execute(&opts)
        .expect("Install should succeed as git source");

    // Should be treated as git, not mcpb
    let config_path = temp.path().join("project").join("sift.toml");
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("git:"),
        "Non-MCPB https URL should be treated as git source"
    );
    assert!(
        !content.contains("mcpb:"),
        "Non-MCPB URL should not have mcpb: prefix"
    );
    assert_eq!(report.name, "server.zip");
}

#[test]
fn install_mcp_mcpb_extension_is_case_sensitive() {
    let (temp, cmd) = setup_isolated_install_command();

    // .MCPB (uppercase) should NOT be detected as MCPB, but as git
    let upper_url = "https://example.com/server.MCPB";
    let opts = InstallOptions::mcp(upper_url).with_scope(ConfigScope::PerProjectShared);

    let report = cmd
        .execute(&opts)
        .expect("Install should succeed as git source");

    // Should be treated as git, not mcpb
    let config_path = temp.path().join("project").join("sift.toml");
    let content = std::fs::read_to_string(&config_path).expect("Should read sift.toml");
    assert!(
        content.contains("git:"),
        ".MCPB (uppercase) should be treated as git source"
    );
    assert!(
        !content.contains("mcpb:"),
        ".MCPB (uppercase) should not have mcpb: prefix"
    );
    assert_eq!(report.name, "server.MCPB");
}

// =========================================================================
// MCPB Install Error Handling Tests
// These test that MCPB detection works, but download fails gracefully for invalid URLs
// =========================================================================

#[test]
fn install_mcp_mcpb_url_is_detected_but_download_fails_for_invalid_url() {
    let (_temp, cmd) = setup_isolated_install_command();

    // Valid MCPB URL format, but non-existent server
    let mcpb_url = "https://example.com/nonexistent-server.mcpb";
    let opts = InstallOptions::mcp(mcpb_url).with_scope(ConfigScope::PerProjectShared);

    // Install should fail because the URL doesn't exist
    let result = cmd.execute(&opts);

    assert!(
        result.is_err(),
        "Install should fail for non-existent MCPB URL"
    );
    let err_msg = result.unwrap_err().to_string();
    // The error should be about downloading, not about detection
    assert!(
        err_msg.contains("download") || err_msg.contains("HTTP") || err_msg.contains("404"),
        "Error should be about download failure, got: {}",
        err_msg
    );
}

#[test]
fn install_mcp_mcpb_github_url_is_detected_correctly() {
    let (_temp, cmd) = setup_isolated_install_command();

    // A GitHub releases URL ending with .mcpb should be detected as MCPB
    let github_mcpb = "https://github.com/org/repo/releases/download/v1.0.0/my-mcp-server.mcpb";
    let opts = InstallOptions::mcp(github_mcpb).with_scope(ConfigScope::PerProjectShared);

    // Install will fail because the URL doesn't exist, but we can verify
    // the error is about downloading (not about treating it as git)
    let result = cmd.execute(&opts);

    assert!(
        result.is_err(),
        "Install should fail for non-existent MCPB URL"
    );
    let err_msg = result.unwrap_err().to_string();
    // Error should be about MCPB download, not git clone
    assert!(
        !err_msg.contains("git clone") && !err_msg.contains("repository"),
        "GitHub MCPB URL should NOT be treated as git. Error: {}",
        err_msg
    );
}
