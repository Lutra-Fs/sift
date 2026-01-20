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

mod input_inference_tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_resolver_with_path(project_root: PathBuf) -> SourceResolver {
        SourceResolver::new(PathBuf::from("/tmp/state"), project_root, HashMap::new())
    }

    #[test]
    fn infer_local_source_from_relative_path() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("./my-skill").unwrap();

        assert_eq!(result.name, "my-skill");
        assert_eq!(result.source, "local:./my-skill");
        assert!(!result.source_is_registry);
        assert!(!result.source_explicit);
    }

    #[test]
    fn infer_local_source_from_absolute_path() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("/absolute/path/my-skill").unwrap();

        assert_eq!(result.name, "my-skill");
        assert_eq!(result.source, "local:/absolute/path/my-skill");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_local_source_from_home_relative_path() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("~/skills/my-skill").unwrap();

        assert_eq!(result.name, "my-skill");
        assert_eq!(result.source, "local:~/skills/my-skill");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_local_source_from_parent_relative_path() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("../other-project/skill").unwrap();

        assert_eq!(result.name, "skill");
        assert_eq!(result.source, "local:../other-project/skill");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_local_source_from_existing_directory() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().to_path_buf();
        std::fs::create_dir_all(project.join("my-plugin")).unwrap();

        let resolver = create_test_resolver_with_path(project);
        let result = resolver.infer_input("my-plugin").unwrap();

        assert_eq!(result.name, "my-plugin");
        assert_eq!(result.source, "local:my-plugin");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_git_source_from_https_url() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("https://github.com/org/repo").unwrap();

        assert_eq!(result.name, "repo");
        assert_eq!(result.source, "git:https://github.com/org/repo");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_git_source_from_git_plus_prefix() {
        let resolver = create_test_resolver();
        let result = resolver
            .infer_input("git+https://github.com/org/repo")
            .unwrap();

        assert_eq!(result.name, "repo");
        assert_eq!(result.source, "git:https://github.com/org/repo");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_git_source_from_github_prefix() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("github:org/repo").unwrap();

        assert_eq!(result.name, "repo");
        assert_eq!(result.source, "github:org/repo");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_git_source_from_ssh_url() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("git@github.com:org/repo.git").unwrap();

        assert_eq!(result.name, "repo");
        assert_eq!(result.source, "git:git@github.com:org/repo.git");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_mcpb_source_from_url() {
        let resolver = create_test_resolver();
        let result = resolver
            .infer_input("https://example.com/server.mcpb")
            .unwrap();

        assert_eq!(result.name, "server");
        assert_eq!(result.source, "mcpb:https://example.com/server.mcpb");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn infer_registry_source_for_plain_name() {
        let resolver = create_test_resolver();
        let result = resolver.infer_input("my-package").unwrap();

        assert_eq!(result.name, "my-package");
        assert_eq!(result.source, "registry:my-package");
        assert!(result.source_is_registry);
        assert!(!result.source_explicit);
    }

    #[test]
    fn infer_registry_source_with_explicit_registry() {
        let resolver = create_test_resolver();
        let result = resolver
            .infer_input_with_registry("my-package", Some("custom-reg"))
            .unwrap();

        assert_eq!(result.name, "my-package");
        assert_eq!(result.source, "registry:custom-reg/my-package");
        assert!(result.source_is_registry);
        assert!(result.source_explicit);
    }
}

mod normalize_source_tests {
    use super::*;

    #[test]
    fn normalize_already_prefixed_sources() {
        let resolver = create_test_resolver();

        let (source, warning) = resolver.normalize_source("registry:foo").unwrap();
        assert_eq!(source, "registry:foo");
        assert!(warning.is_none());

        let (source, warning) = resolver.normalize_source("local:./path").unwrap();
        assert_eq!(source, "local:./path");
        assert!(warning.is_none());

        let (source, warning) = resolver.normalize_source("github:org/repo").unwrap();
        assert_eq!(source, "github:org/repo");
        assert!(warning.is_none());

        let (source, warning) = resolver
            .normalize_source("git:https://example.com/repo")
            .unwrap();
        assert_eq!(source, "git:https://example.com/repo");
        assert!(warning.is_none());

        let (source, warning) = resolver
            .normalize_source("mcpb:https://example.com/file.mcpb")
            .unwrap();
        assert_eq!(source, "mcpb:https://example.com/file.mcpb");
        assert!(warning.is_none());
    }

    #[test]
    fn normalize_local_path_adds_prefix() {
        let resolver = create_test_resolver();

        let (source, warning) = resolver.normalize_source("./path/to/skill").unwrap();
        assert_eq!(source, "local:./path/to/skill");
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("Normalized"));
    }

    #[test]
    fn normalize_absolute_path_adds_prefix() {
        let resolver = create_test_resolver();

        let (source, warning) = resolver.normalize_source("/absolute/path").unwrap();
        assert_eq!(source, "local:/absolute/path");
        assert!(warning.is_some());
    }

    #[test]
    fn normalize_git_url_adds_prefix() {
        let resolver = create_test_resolver();

        let (source, warning) = resolver
            .normalize_source("https://github.com/org/repo")
            .unwrap();
        assert_eq!(source, "git:https://github.com/org/repo");
        assert!(warning.is_some());

        let (source, warning) = resolver
            .normalize_source("git+https://github.com/org/repo")
            .unwrap();
        assert_eq!(source, "git:https://github.com/org/repo");
        assert!(warning.is_some());
    }

    #[test]
    fn normalize_mcpb_url_adds_prefix() {
        let resolver = create_test_resolver();

        let (source, warning) = resolver
            .normalize_source("https://example.com/server.mcpb")
            .unwrap();
        assert_eq!(source, "mcpb:https://example.com/server.mcpb");
        assert!(warning.is_some());
    }

    #[test]
    fn normalize_invalid_source_errors() {
        let resolver = create_test_resolver();

        let result = resolver.normalize_source("invalid-source-format");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid source format"));
    }
}

mod resolve_input_tests {
    use super::*;

    #[test]
    fn resolve_input_with_explicit_source() {
        let resolver = create_test_resolver();
        let result = resolver
            .resolve_input("my-name", Some("local:./path"), None)
            .unwrap();

        assert_eq!(result.name, "my-name");
        assert_eq!(result.source, "local:./path");
        assert!(!result.source_is_registry);
        assert!(result.source_explicit);
    }

    #[test]
    fn resolve_input_with_source_and_registry_warns() {
        let resolver = create_test_resolver();
        let result = resolver
            .resolve_input("my-name", Some("local:./path"), Some("reg"))
            .unwrap();

        assert!(!result.warnings.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Ignoring --registry"))
        );
    }

    #[test]
    fn resolve_input_infers_source_when_none_provided() {
        let resolver = create_test_resolver();
        let result = resolver.resolve_input("./my-skill", None, None).unwrap();

        assert_eq!(result.name, "my-skill");
        assert_eq!(result.source, "local:./my-skill");
        assert!(!result.source_is_registry);
    }

    #[test]
    fn resolve_input_normalizes_explicit_source() {
        let resolver = create_test_resolver();
        let result = resolver
            .resolve_input("my-name", Some("./path"), None)
            .unwrap();

        assert_eq!(result.source, "local:./path");
        assert!(!result.warnings.is_empty()); // normalization warning
    }
}

mod derive_name_tests {
    use super::*;

    #[test]
    fn derive_name_from_path_simple() {
        let name = derive_name_from_path("./skills/my-skill").unwrap();
        assert_eq!(name, "my-skill");
    }

    #[test]
    fn derive_name_from_path_with_trailing_slash() {
        let name = derive_name_from_path("./skills/my-skill/").unwrap();
        assert_eq!(name, "my-skill");
    }

    #[test]
    fn derive_name_from_path_absolute() {
        let name = derive_name_from_path("/absolute/path/skill-name").unwrap();
        assert_eq!(name, "skill-name");
    }

    #[test]
    fn derive_name_from_git_https() {
        let name = derive_name_from_git_source("git:https://github.com/org/my-repo").unwrap();
        assert_eq!(name, "my-repo");
    }

    #[test]
    fn derive_name_from_git_with_dot_git_suffix() {
        let name = derive_name_from_git_source("git:https://github.com/org/my-repo.git").unwrap();
        assert_eq!(name, "my-repo");
    }

    #[test]
    fn derive_name_from_github_shorthand() {
        let name = derive_name_from_git_source("github:org/my-repo").unwrap();
        assert_eq!(name, "my-repo");
    }

    #[test]
    fn derive_name_from_git_trailing_slash() {
        let name = derive_name_from_git_source("git:https://github.com/org/my-repo/").unwrap();
        assert_eq!(name, "my-repo");
    }

    #[test]
    fn derive_name_from_git_empty_name_errors() {
        let result = derive_name_from_git_source("git:");
        assert!(result.is_err());
    }
}

mod is_detection_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn is_local_path_detects_relative() {
        assert!(is_local_path("./path", &PathBuf::from("/tmp")));
        assert!(is_local_path("../path", &PathBuf::from("/tmp")));
    }

    #[test]
    fn is_local_path_detects_absolute() {
        assert!(is_local_path("/absolute/path", &PathBuf::from("/tmp")));
    }

    #[test]
    fn is_local_path_detects_home() {
        assert!(is_local_path("~/path", &PathBuf::from("/tmp")));
    }

    #[test]
    fn is_local_path_detects_existing_directory() {
        let temp = TempDir::new().unwrap();
        let project = temp.path();
        std::fs::create_dir_all(project.join("existing-dir")).unwrap();

        assert!(is_local_path("existing-dir", project));
        assert!(!is_local_path("nonexistent-dir", project));
    }

    #[test]
    fn is_git_like_detects_protocols() {
        assert!(is_git_like("http://example.com/repo"));
        assert!(is_git_like("https://github.com/org/repo"));
        assert!(is_git_like("git://github.com/org/repo"));
        assert!(is_git_like("git+https://github.com/org/repo"));
        assert!(is_git_like("github:org/repo"));
        assert!(is_git_like("git:https://github.com/org/repo"));
        assert!(is_git_like("git@github.com:org/repo"));
    }

    #[test]
    fn is_git_like_rejects_non_git() {
        assert!(!is_git_like("local:./path"));
        assert!(!is_git_like("registry:foo"));
        assert!(!is_git_like("plain-name"));
    }
}

mod normalize_git_source_tests {
    use super::*;

    #[test]
    fn normalize_git_plus_prefix() {
        assert_eq!(
            normalize_git_source("git+https://github.com/org/repo"),
            "git:https://github.com/org/repo"
        );
    }

    #[test]
    fn normalize_https_url() {
        assert_eq!(
            normalize_git_source("https://github.com/org/repo"),
            "git:https://github.com/org/repo"
        );
    }

    #[test]
    fn normalize_git_protocol() {
        assert_eq!(
            normalize_git_source("git://github.com/org/repo"),
            "git:git://github.com/org/repo"
        );
    }

    #[test]
    fn normalize_ssh_url() {
        assert_eq!(
            normalize_git_source("git@github.com:org/repo"),
            "git:git@github.com:org/repo"
        );
    }

    #[test]
    fn normalize_already_prefixed() {
        assert_eq!(normalize_git_source("github:org/repo"), "github:org/repo");
        assert_eq!(
            normalize_git_source("git:https://example.com"),
            "git:https://example.com"
        );
    }
}

mod resolver_error_tests {
    use super::*;

    #[test]
    fn resolve_registry_unknown_registry_errors() {
        let resolver = create_test_resolver();
        let result = resolver.resolve("registry:nonexistent/skill");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No registries") || err.contains("Unknown"),
            "Error should indicate registry issue: {}",
            err
        );
    }

    #[test]
    fn resolve_registry_with_key_unknown_registry_errors() {
        let resolver = create_resolver_with_registry("known-registry", "github:anthropics/skills");

        let result = resolver.resolve("registry:unknown-registry/skill");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unknown registry") || err.contains("unknown-registry"),
            "Error should mention unknown registry: {}",
            err
        );
    }

    #[test]
    fn resolve_mcp_registry_no_registries_returns_none_or_error() {
        let resolver = create_test_resolver();
        let result = resolver.resolve_mcp_registry("some-plugin");

        // With no registries, should error
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No registries") || err.contains("registry"));
    }

    #[test]
    fn resolve_sift_registry_not_implemented() {
        let mut registries = HashMap::new();
        registries.insert(
            "sift-reg".to_string(),
            RegistryConfig {
                r#type: RegistryType::Sift,
                url: Some(
                    url::Url::parse("https://sift-registry.example.com")
                        .expect("valid URL for test"),
                ),
                source: None,
            },
        );
        let resolver = SourceResolver::new(
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/project"),
            registries,
        );

        let result = resolver.resolve("registry:sift-reg/some-skill");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not yet implemented") || err.contains("Sift"),
            "Error should indicate Sift registry not implemented: {}",
            err
        );
    }

    #[test]
    fn resolve_mcp_registry_sift_type_not_implemented() {
        let mut registries = HashMap::new();
        registries.insert(
            "sift-reg".to_string(),
            RegistryConfig {
                r#type: RegistryType::Sift,
                url: Some(
                    url::Url::parse("https://sift-registry.example.com")
                        .expect("valid URL for test"),
                ),
                source: None,
            },
        );
        let resolver = SourceResolver::new(
            PathBuf::from("/tmp/state"),
            PathBuf::from("/tmp/project"),
            registries,
        );

        let result = resolver.resolve_mcp_registry("sift-reg/some-plugin");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not yet implemented") || err.contains("Sift"),
            "Error should indicate Sift registry not implemented: {}",
            err
        );
    }
}
