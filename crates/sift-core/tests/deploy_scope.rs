use sift_core::deploy::scope::{
    RepoStatus, ResourceKind, ScopeRequest, ScopeResolution, ScopeSupport, resolve_scope,
};
use sift_core::types::ConfigScope;

#[test]
fn deploy_scope_skills_local_in_git_uses_project_with_exclude() {
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
