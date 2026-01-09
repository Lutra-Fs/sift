use sift_core::registry::resolver::MarketplaceResolver;

#[test]
fn resolve_skill_from_marketplace_manifest() {
    let json = r#"
    {
      "marketplace": {"name": "test"},
      "plugins": [
        {
          "name": "alpha",
          "description": "Alpha skill",
          "version": "1.2.3",
          "source": "github:org/repo"
        }
      ]
    }
    "#;

    let resolver = MarketplaceResolver::from_json(json).unwrap();
    let resolved = resolver.resolve_skill("alpha").unwrap();

    assert_eq!(resolved.name, "alpha");
    assert_eq!(resolved.declared_version, "1.2.3");
    assert_eq!(resolved.config.source, "github:org/repo");
}
