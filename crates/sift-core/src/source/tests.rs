//! Tests for the source module.

use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
use crate::registry::{RegistryConfig, RegistryType};

fn create_test_resolver() -> SourceResolver {
    SourceResolver::new(
        PathBuf::from("/tmp/state"),
        PathBuf::from("/tmp/project"),
        HashMap::new(),
    )
}

fn create_resolver_with_registry(key: &str, source: &str) -> SourceResolver {
    let mut registries = HashMap::new();
    registries.insert(
        key.to_string(),
        RegistryConfig {
            r#type: RegistryType::ClaudeMarketplace,
            url: None,
            source: Some(source.to_string()),
        },
    );
    SourceResolver::new(
        PathBuf::from("/tmp/state"),
        PathBuf::from("/tmp/project"),
        registries,
    )
}

mod local_source_tests {
    use super::*;

    #[test]
    fn resolve_local_absolute_path() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("local:/absolute/path/to/skill").unwrap();

        assert!(result.is_local());
        let spec = result.as_local().unwrap();
        assert_eq!(spec.path, PathBuf::from("/absolute/path/to/skill"));
    }

    #[test]
    fn resolve_local_relative_path() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("local:./skills/my-skill").unwrap();

        assert!(result.is_local());
        let spec = result.as_local().unwrap();
        assert_eq!(spec.path, PathBuf::from("/tmp/project/skills/my-skill"));
    }

    #[test]
    fn resolve_local_relative_without_dot() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("local:skills/my-skill").unwrap();

        assert!(result.is_local());
        let spec = result.as_local().unwrap();
        assert_eq!(spec.path, PathBuf::from("/tmp/project/skills/my-skill"));
    }
}

mod git_source_tests {
    use super::*;

    #[test]
    fn resolve_github_shorthand() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("github:anthropics/skills").unwrap();

        assert!(result.is_git());
        let spec = result.as_git().unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(spec.reference, None);
        assert_eq!(spec.subdir, None);
    }

    #[test]
    fn resolve_github_with_ref_and_path() {
        let resolver = create_test_resolver();
        let result = resolver
            .resolve("github:anthropics/skills@main/skills/pdf")
            .unwrap();

        assert!(result.is_git());
        let spec = result.as_git().unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, Some("skills/pdf".to_string()));
    }

    #[test]
    fn resolve_git_prefix() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("git:https://gitlab.com/org/repo").unwrap();

        assert!(result.is_git());
        let spec = result.as_git().unwrap();
        assert_eq!(spec.repo_url, "https://gitlab.com/org/repo");
    }

    #[test]
    fn resolve_git_tree_url() {
        let resolver = create_test_resolver();
        let result = resolver
            .resolve("git:https://github.com/org/repo/tree/v1.0.0/plugins/my-plugin")
            .unwrap();

        assert!(result.is_git());
        let spec = result.as_git().unwrap();
        assert_eq!(spec.repo_url, "https://github.com/org/repo");
        assert_eq!(spec.reference, Some("v1.0.0".to_string()));
        assert_eq!(spec.subdir, Some("plugins/my-plugin".to_string()));
    }
}

mod auto_detect_tests {
    use super::*;

    #[test]
    fn auto_detect_absolute_path() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("/absolute/path").unwrap();

        assert!(result.is_local());
    }

    #[test]
    fn auto_detect_relative_path() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("./relative/path").unwrap();

        assert!(result.is_local());
    }

    #[test]
    fn auto_detect_https_url() {
        let resolver = create_test_resolver();
        let result = resolver
            .resolve("https://github.com/org/repo/tree/main/path")
            .unwrap();

        assert!(result.is_git());
        let spec = result.as_git().unwrap();
        assert_eq!(spec.repo_url, "https://github.com/org/repo");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, Some("path".to_string()));
    }
}

mod registry_source_tests {
    use super::*;

    #[test]
    fn parse_registry_source_with_key() {
        let resolver =
            create_resolver_with_registry("anthropic-skills", "github:anthropics/skills");
        let (key, name) = resolver
            .parse_registry_source("anthropic-skills/pdf")
            .unwrap();

        assert_eq!(key, "anthropic-skills");
        assert_eq!(name, "pdf");
    }

    #[test]
    fn parse_registry_source_single_registry_default() {
        let resolver = create_resolver_with_registry("default", "github:anthropics/skills");
        let (key, name) = resolver.parse_registry_source("pdf").unwrap();

        assert_eq!(key, "default");
        assert_eq!(name, "pdf");
    }

    #[test]
    fn parse_registry_source_multiple_registries_requires_key() {
        let mut registries = HashMap::new();
        registries.insert(
            "reg1".to_string(),
            RegistryConfig {
                r#type: RegistryType::ClaudeMarketplace,
                url: None,
                source: Some("github:org1/repo".to_string()),
            },
        );
        registries.insert(
            "reg2".to_string(),
            RegistryConfig {
                r#type: RegistryType::ClaudeMarketplace,
                url: None,
                source: Some("github:org2/repo".to_string()),
            },
        );
        let resolver = SourceResolver::new(
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/project"),
            registries,
        );

        let result = resolver.parse_registry_source("skill-name");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Multiple registries")
        );
    }

    #[test]
    fn parse_registry_source_no_registries() {
        let resolver = create_test_resolver();
        let result = resolver.parse_registry_source("skill-name");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No registries"));
    }
}

mod plugin_source_conversion_tests {
    use super::*;

    #[test]
    fn convert_relative_plugin_source() {
        let resolver = create_test_resolver();
        let marketplace_spec =
            GitSpec::new("https://github.com/anthropics/skills").with_reference("main");

        let result = resolver
            .plugin_source_to_git_spec(
                "local:./skills/pdf",
                &marketplace_spec,
                "github:anthropics/skills",
            )
            .unwrap();

        assert_eq!(result.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(result.reference, Some("main".to_string()));
        assert_eq!(result.subdir, Some("skills/pdf".to_string()));
    }

    #[test]
    fn convert_relative_plugin_source_with_marketplace_subdir() {
        let resolver = create_test_resolver();
        let marketplace_spec = GitSpec::new("https://github.com/anthropics/plugins")
            .with_reference("main")
            .with_subdir("category");

        let result = resolver
            .plugin_source_to_git_spec(
                "local:./my-skill",
                &marketplace_spec,
                "github:anthropics/plugins",
            )
            .unwrap();

        assert_eq!(result.repo_url, "https://github.com/anthropics/plugins");
        assert_eq!(result.reference, Some("main".to_string()));
        assert_eq!(result.subdir, Some("category/my-skill".to_string()));
    }

    #[test]
    fn convert_github_plugin_source() {
        let resolver = create_test_resolver();
        let marketplace_spec = GitSpec::new("https://github.com/anthropics/skills");

        let result = resolver
            .plugin_source_to_git_spec(
                "github:external/repo@v1.0.0/path",
                &marketplace_spec,
                "github:anthropics/skills",
            )
            .unwrap();

        assert_eq!(result.repo_url, "https://github.com/external/repo");
        assert_eq!(result.reference, Some("v1.0.0".to_string()));
        assert_eq!(result.subdir, Some("path".to_string()));
    }

    #[test]
    fn convert_unsupported_source_errors() {
        let resolver = create_test_resolver();
        let marketplace_spec = GitSpec::new("https://github.com/anthropics/skills");

        let result = resolver.plugin_source_to_git_spec(
            "https://example.com/archive.zip",
            &marketplace_spec,
            "github:anthropics/skills",
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported"));
    }
}

mod resolved_source_tests {
    use super::*;

    #[test]
    fn resolved_source_accessors() {
        let git_source = ResolvedSource::Git(GitSpec::new("https://github.com/org/repo"));
        assert!(git_source.is_git());
        assert!(!git_source.is_local());
        assert!(git_source.as_git().is_some());
        assert!(git_source.as_local().is_none());

        let local_source = ResolvedSource::Local(LocalSpec::new("/path/to/skill"));
        assert!(!local_source.is_git());
        assert!(local_source.is_local());
        assert!(local_source.as_git().is_none());
        assert!(local_source.as_local().is_some());
    }
}

mod resolve_with_metadata_tests {
    use super::*;

    #[test]
    fn local_source_no_metadata() {
        let resolver = create_test_resolver();
        let (source, metadata) = resolver.resolve_with_metadata("local:./path").unwrap();

        assert!(source.is_local());
        assert!(metadata.is_none());
    }

    #[test]
    fn git_source_no_metadata() {
        let resolver = create_test_resolver();
        let (source, metadata) = resolver.resolve_with_metadata("github:org/repo").unwrap();

        assert!(source.is_git());
        assert!(metadata.is_none());
    }
}
