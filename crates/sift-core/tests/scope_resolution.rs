use sift_core::deploy::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, ScopeSupport, resolve_scope,
};
use sift_core::types::ConfigScope;

#[test]
fn skills_local_in_git_repo_uses_project_with_exclude() {
    let support = ScopeSupport {
        global: true,
        project: true,
        local: false,
    };
    let repo = RepoStatus::Git;

    let resolution = resolve_scope(
        ResourceKind::Skill,
        ScopeRequest::Explicit(ConfigScope::PerProjectLocal),
        support,
        repo,
    )
    .expect("resolve_scope should succeed");

    let ScopeResolution::Apply(decision) = resolution else {
        panic!("expected Apply decision");
    };

    assert_eq!(decision.scope, ConfigScope::PerProjectShared);
    assert!(decision.use_git_exclude);
}

#[test]
fn skills_local_non_git_repo_errors() {
    let support = ScopeSupport {
        global: true,
        project: true,
        local: false,
    };
    let repo = RepoStatus::NotGit;

    let err = resolve_scope(
        ResourceKind::Skill,
        ScopeRequest::Explicit(ConfigScope::PerProjectLocal),
        support,
        repo,
    )
    .expect_err("non-git local skills should fail");

    let msg = err.to_string();
    assert!(msg.contains("project") || msg.contains("global"));
}

#[test]
fn mcp_local_supported_applies_local() {
    let support = ScopeSupport {
        global: true,
        project: true,
        local: true,
    };
    let repo = RepoStatus::Git;

    let resolution = resolve_scope(
        ResourceKind::Mcp,
        ScopeRequest::Explicit(ConfigScope::PerProjectLocal),
        support,
        repo,
    )
    .expect("resolve_scope should succeed");

    let ScopeResolution::Apply(decision) = resolution else {
        panic!("expected Apply decision");
    };
    assert_eq!(decision.scope, ConfigScope::PerProjectLocal);
    assert!(!decision.use_git_exclude);
}

#[test]
fn mcp_local_unsupported_explicit_errors() {
    let support = ScopeSupport {
        global: true,
        project: true,
        local: false,
    };
    let repo = RepoStatus::Git;

    let err = resolve_scope(
        ResourceKind::Mcp,
        ScopeRequest::Explicit(ConfigScope::PerProjectLocal),
        support,
        repo,
    )
    .expect_err("explicit local mcp should fail when unsupported");

    assert!(err.to_string().contains("local"));
}

#[test]
fn mcp_local_unsupported_auto_skips() {
    let support = ScopeSupport {
        global: false,
        project: false,
        local: false,
    };
    let repo = RepoStatus::Git;

    let resolution = resolve_scope(ResourceKind::Mcp, ScopeRequest::Auto, support, repo)
        .expect("auto resolution should not error");

    let ScopeResolution::Skip { warning } = resolution else {
        panic!("expected Skip decision");
    };
    assert!(!warning.is_empty());
}

#[test]
fn skills_auto_prefers_project_over_global() {
    let support = ScopeSupport {
        global: true,
        project: true,
        local: false,
    };
    let repo = RepoStatus::Git;

    let resolution = resolve_scope(ResourceKind::Skill, ScopeRequest::Auto, support, repo)
        .expect("resolve_scope should succeed");

    let ScopeResolution::Apply(decision) = resolution else {
        panic!("expected Apply decision");
    };
    assert_eq!(decision.scope, ConfigScope::PerProjectShared);
    assert!(!decision.use_git_exclude);
}
