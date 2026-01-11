//! Tests for the git module.

use super::*;

mod git_spec_tests {
    use super::*;

    #[test]
    fn parse_github_shorthand() {
        let spec = GitSpec::parse("github:anthropics/skills").unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(spec.reference, None);
        assert_eq!(spec.subdir, None);
    }

    #[test]
    fn parse_github_with_ref() {
        let spec = GitSpec::parse("github:anthropics/skills@main").unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, None);
    }

    #[test]
    fn parse_github_with_ref_and_path() {
        let spec = GitSpec::parse("github:anthropics/skills@main/skills/pdf").unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, Some("skills/pdf".to_string()));
    }

    #[test]
    fn parse_git_prefix_with_https() {
        let spec = GitSpec::parse("git:https://github.com/org/repo").unwrap();
        assert_eq!(spec.repo_url, "https://github.com/org/repo");
        assert_eq!(spec.reference, None);
        assert_eq!(spec.subdir, None);
    }

    #[test]
    fn parse_tree_url() {
        let spec =
            GitSpec::parse("https://github.com/anthropics/skills/tree/main/skills/pdf").unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/skills");
        assert_eq!(spec.reference, Some("main".to_string()));
        assert_eq!(spec.subdir, Some("skills/pdf".to_string()));
    }

    #[test]
    fn parse_tree_url_with_nested_path() {
        let spec = GitSpec::parse(
            "https://github.com/anthropics/life-sciences/tree/v1.0.0/category/subcategory/plugin",
        )
        .unwrap();
        assert_eq!(spec.repo_url, "https://github.com/anthropics/life-sciences");
        assert_eq!(spec.reference, Some("v1.0.0".to_string()));
        assert_eq!(spec.subdir, Some("category/subcategory/plugin".to_string()));
    }

    #[test]
    fn parse_tree_url_missing_path_errors() {
        let result = GitSpec::parse("https://github.com/org/repo/tree/main/");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing a path after")
        );
    }

    #[test]
    fn bare_repo_dir_is_deterministic() {
        let state_dir = std::path::Path::new("/tmp/sift");
        let spec1 = GitSpec::new("https://github.com/anthropics/skills");
        let spec2 = GitSpec::new("https://github.com/anthropics/skills");
        let spec3 = GitSpec::new("https://github.com/different/repo");

        let dir1 = spec1.bare_repo_dir(state_dir);
        let dir2 = spec2.bare_repo_dir(state_dir);
        let dir3 = spec3.bare_repo_dir(state_dir);

        assert_eq!(dir1, dir2, "Same repo URL should produce same dir");
        assert_ne!(
            dir1, dir3,
            "Different repo URLs should produce different dirs"
        );
        assert!(dir1.to_string_lossy().ends_with(".git"));
    }

    #[test]
    fn builder_pattern() {
        let spec = GitSpec::new("https://github.com/org/repo")
            .with_reference("v1.0.0")
            .with_subdir("skills/my-skill");

        assert_eq!(spec.repo_url, "https://github.com/org/repo");
        assert_eq!(spec.reference, Some("v1.0.0".to_string()));
        assert_eq!(spec.subdir, Some("skills/my-skill".to_string()));
    }
}

mod git_fetcher_tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run_git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("Failed to invoke git");
        assert!(status.success(), "git command failed: {:?}", args);
    }

    fn init_test_repo(repo: &Path, skill_path: &str, skill_name: &str) {
        std::fs::create_dir_all(repo).expect("Failed to create repo dir");
        run_git(repo, &["init"]);
        run_git(repo, &["checkout", "-b", "main"]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "Test User"]);
        run_git(repo, &["config", "commit.gpgsign", "false"]);

        // Create skill
        let skill_dir = repo.join(skill_path);
        std::fs::create_dir_all(&skill_dir).expect("Failed to create skill dir");
        let content = format!(
            "---\nname: {}\ndescription: Test skill\n---\n\nTest instructions.\n",
            skill_name
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).expect("Failed to write SKILL.md");

        // Create a root file for read_root_file test
        std::fs::write(repo.join("marketplace.json"), r#"{"plugins": []}"#)
            .expect("Failed to write marketplace.json");

        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "init"]);
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

    #[test]
    fn ensure_git_version_succeeds() {
        // This test requires git to be installed
        let result = GitFetcher::ensure_git_version();
        assert!(
            result.is_ok(),
            "Git version check should succeed: {:?}",
            result
        );
    }

    #[test]
    fn fetch_skill_from_local_repo() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");
        let skill_path = "skills/test-skill";

        init_test_repo(&repo_root, skill_path, "test-skill");
        let expected_commit = git_rev_parse(&repo_root, "HEAD");

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        let spec = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir(skill_path);

        let fetcher = GitFetcher::new(state_dir);
        let result = fetcher.fetch(&spec, "test-skill", false).unwrap();

        assert_eq!(result.commit_sha, expected_commit);
        assert!(result.cache_dir.join("SKILL.md").exists());
    }

    #[test]
    fn fetch_is_idempotent() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");
        let skill_path = "skills/test-skill";

        init_test_repo(&repo_root, skill_path, "test-skill");

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        let spec = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir(skill_path);

        let fetcher = GitFetcher::new(state_dir);

        // First fetch
        let result1 = fetcher.fetch(&spec, "test-skill", false).unwrap();

        // Second fetch (should use cache)
        let result2 = fetcher.fetch(&spec, "test-skill", false).unwrap();

        assert_eq!(result1.commit_sha, result2.commit_sha);
        assert_eq!(result1.cache_dir, result2.cache_dir);
    }

    #[test]
    fn fetch_force_refreshes_cache() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");
        let skill_path = "skills/test-skill";

        init_test_repo(&repo_root, skill_path, "test-skill");

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        let spec = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir(skill_path);

        let fetcher = GitFetcher::new(state_dir);

        // First fetch
        let result1 = fetcher.fetch(&spec, "test-skill", false).unwrap();

        // Modify the skill file in cache (simulating corruption)
        std::fs::write(result1.cache_dir.join("SKILL.md"), "corrupted")
            .expect("Failed to corrupt cache");

        // Force fetch should restore original content
        let result2 = fetcher.fetch(&spec, "test-skill", true).unwrap();

        let content = std::fs::read_to_string(result2.cache_dir.join("SKILL.md")).unwrap();
        assert!(content.contains("Test skill"));
    }

    #[test]
    fn multiple_skills_share_bare_repo() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");

        // Create repo with two skills
        std::fs::create_dir_all(&repo_root).expect("Failed to create repo dir");
        run_git(&repo_root, &["init"]);
        run_git(&repo_root, &["checkout", "-b", "main"]);
        run_git(&repo_root, &["config", "user.email", "test@example.com"]);
        run_git(&repo_root, &["config", "user.name", "Test User"]);
        run_git(&repo_root, &["config", "commit.gpgsign", "false"]);

        for (path, name) in [("skills/skill-a", "skill-a"), ("skills/skill-b", "skill-b")] {
            let skill_dir = repo_root.join(path);
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {}\n---\n", name),
            )
            .unwrap();
        }

        run_git(&repo_root, &["add", "."]);
        run_git(&repo_root, &["commit", "-m", "init"]);

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        let fetcher = GitFetcher::new(state_dir.clone());

        // Fetch first skill
        let spec_a = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir("skills/skill-a");
        fetcher.fetch(&spec_a, "skill-a", false).unwrap();

        // Fetch second skill
        let spec_b = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir("skills/skill-b");
        fetcher.fetch(&spec_b, "skill-b", false).unwrap();

        // Count bare repos - should be exactly 1
        let git_dir = state_dir.join("git");
        let bare_repos: Vec<_> = std::fs::read_dir(&git_dir)
            .expect("git dir should exist")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".git"))
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(
            bare_repos.len(),
            1,
            "Expected exactly 1 bare repo, found: {:?}",
            bare_repos
        );
    }

    #[test]
    fn read_root_file_from_repo() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");

        init_test_repo(&repo_root, "skills/test", "test");

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        // Use a spec with subdir, but read from root
        let spec = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir("skills/test");

        let fetcher = GitFetcher::new(state_dir);

        // read_root_file should read from repo root, ignoring subdir
        let content = fetcher.read_root_file(&spec, "marketplace.json").unwrap();
        assert!(content.contains("plugins"));
    }

    #[test]
    fn read_file_respects_subdir() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");

        init_test_repo(&repo_root, "skills/test", "test");

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        let spec = GitSpec::new(&repo_url)
            .with_reference("main")
            .with_subdir("skills/test");

        let fetcher = GitFetcher::new(state_dir);

        // read_file should prepend subdir to path
        let content = fetcher.read_file(&spec, "SKILL.md").unwrap();
        assert!(content.contains("name: test"));
    }

    #[test]
    fn read_file_not_found_errors() {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let repo_root = temp.path().join("repo");
        let state_dir = temp.path().join("state");

        init_test_repo(&repo_root, "skills/test", "test");

        let repo_url = url::Url::from_directory_path(&repo_root)
            .expect("repo root should convert to file URL")
            .to_string();

        let spec = GitSpec::new(&repo_url).with_reference("main");

        let fetcher = GitFetcher::new(state_dir);

        let result = fetcher.read_root_file(&spec, "nonexistent.json");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
