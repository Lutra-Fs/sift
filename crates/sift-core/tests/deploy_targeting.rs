//! Tests for client targeting policy.

use sift_core::deploy::targeting::TargetingPolicy;

#[test]
fn targeting_empty_allows_all() {
    let policy = TargetingPolicy::new(None, None);
    assert!(policy.should_deploy_to("claude-code"));
    assert!(policy.should_deploy_to("amp"));
    assert!(policy.should_deploy_to("any-client"));
}

#[test]
fn targeting_whitelist_only_allows_listed() {
    let policy = TargetingPolicy::new(Some(vec!["claude-code".into(), "amp".into()]), None);
    assert!(policy.should_deploy_to("claude-code"));
    assert!(policy.should_deploy_to("amp"));
    assert!(!policy.should_deploy_to("vscode"));
}

#[test]
fn targeting_blacklist_excludes_listed() {
    let policy = TargetingPolicy::new(None, Some(vec!["codex".into()]));
    assert!(policy.should_deploy_to("claude-code"));
    assert!(!policy.should_deploy_to("codex"));
}

#[test]
fn targeting_whitelist_takes_precedence() {
    let policy = TargetingPolicy::new(
        Some(vec!["amp".into()]),
        Some(vec!["amp".into()]), // conflict
    );
    // Whitelist takes precedence: only amp allowed
    assert!(policy.should_deploy_to("amp"));
    assert!(!policy.should_deploy_to("claude-code"));
}
