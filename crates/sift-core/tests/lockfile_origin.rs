use sift_core::lockfile::{LockedSkill, Lockfile, ResolvedOrigin};
use sift_core::types::ConfigScope;

#[test]
fn test_lockfile_origin_roundtrip() {
    let origin = ResolvedOrigin {
        original_source: "registry:anthropic/pdf".to_string(),
        registry_key: "anthropic".to_string(),
        registry_version: Some("1.2.3".to_string()),
        aliases: vec!["pdf".to_string(), "doc/pdf".to_string()],
        parent: Some("document-skills".to_string()),
        is_group: false,
    };

    let skill = LockedSkill::new(
        "pdf".to_string(),
        "abc123".to_string(),
        "latest".to_string(),
        "registry:anthropic".to_string(),
        ConfigScope::PerProjectShared,
    )
    .with_origin(origin.clone());

    let mut lockfile = Lockfile::new();
    lockfile.add_skill("pdf".to_string(), skill);

    let json = serde_json::to_string_pretty(&lockfile).expect("serialize lockfile");
    let decoded: Lockfile = serde_json::from_str(&json).expect("deserialize lockfile");

    let loaded = decoded.get_skill("pdf").expect("skill should exist");
    assert_eq!(loaded.origin.as_ref(), Some(&origin));
}
