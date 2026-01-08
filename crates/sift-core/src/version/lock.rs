//! Version locking and dependency resolution
//!
//! Implements snapshot-based version locking for reproducibility

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Lockfile for resolved package versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    /// Lockfile format version
    pub version: u32,

    /// Timestamp when lockfile was generated
    pub generated_at: chrono::DateTime<chrono::Utc>,

    /// Locked MCP servers
    #[serde(default)]
    pub mcp_servers: HashMap<String, LockedPackage>,

    /// Locked skills
    #[serde(default)]
    pub skills: HashMap<String, LockedPackage>,
}

impl Lockfile {
    /// Create a new empty lockfile
    pub fn new() -> Self {
        Self {
            version: 1,
            generated_at: chrono::Utc::now(),
            mcp_servers: HashMap::new(),
            skills: HashMap::new(),
        }
    }

    /// Add or update a locked MCP server
    pub fn add_mcp_server(&mut self, name: String, package: LockedPackage) {
        self.mcp_servers.insert(name, package);
    }

    /// Add or update a locked skill
    pub fn add_skill(&mut self, name: String, package: LockedPackage) {
        self.skills.insert(name, package);
    }

    /// Get a locked MCP server
    pub fn get_mcp_server(&self, name: &str) -> Option<&LockedPackage> {
        self.mcp_servers.get(name)
    }

    /// Get a locked skill
    pub fn get_skill(&self, name: &str) -> Option<&LockedPackage> {
        self.skills.get(name)
    }

    /// Remove a locked MCP server
    pub fn remove_mcp_server(&mut self, name: &str) -> Option<LockedPackage> {
        self.mcp_servers.remove(name)
    }

    /// Remove a locked skill
    pub fn remove_skill(&mut self, name: &str) -> Option<LockedPackage> {
        self.skills.remove(name)
    }

    /// Validate the lockfile
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != 1 {
            anyhow::bail!("Unsupported lockfile version: {}", self.version);
        }
        Ok(())
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

/// A locked package with resolved version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    /// Package name
    pub name: String,

    /// Resolved version (exact Git SHA, Docker tag, etc.)
    pub resolved_version: String,

    /// Original constraint from sift.toml
    pub constraint: String,

    /// Registry source
    pub registry: String,

    /// Checksum for verification (optional)
    pub checksum: Option<String>,
}

impl LockedPackage {
    /// Create a new locked package
    pub fn new(
        name: String,
        resolved_version: String,
        constraint: String,
        registry: String,
    ) -> Self {
        Self {
            name,
            resolved_version,
            constraint,
            registry,
            checksum: None,
        }
    }

    /// Set the checksum
    pub fn with_checksum(mut self, checksum: String) -> Self {
        self.checksum = Some(checksum);
        self
    }
}

/// Version constraint parser and resolver
pub struct VersionResolver;

impl VersionResolver {
    /// Parse a version constraint
    pub fn parse_constraint(input: &str) -> anyhow::Result<VersionConstraint> {
        let input = input.trim();

        if input == "latest" {
            return Ok(VersionConstraint::Latest);
        }

        if input.starts_with("branch:") {
            return Ok(VersionConstraint::Branch(
                input.strip_prefix("branch:").unwrap().to_string(),
            ));
        }

        // Try exact version first
        if let Ok(version) = semver::Version::parse(input) {
            return Ok(VersionConstraint::Exact(version));
        }

        // Try semver constraint
        if let Ok(version_req) = semver::VersionReq::parse(input) {
            return Ok(VersionConstraint::Semver(version_req));
        }

        // Try Git SHA (40 hex characters)
        if input.len() == 40 && input.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(VersionConstraint::GitSha(input.to_string()));
        }

        // Short Git SHA (7+ hex characters)
        if input.len() >= 7 && input.len() <= 40 && input.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(VersionConstraint::GitSha(input.to_string()));
        }

        anyhow::bail!("Invalid version constraint: {}", input)
    }

    /// Resolve a constraint to a specific version
    ///
    /// This is a stub implementation. In a real system, this would:
    /// - Query the registry for available versions
    /// - Resolve Git branches to specific commits
    /// - Check Docker tags
    pub fn resolve(
        _constraint: &VersionConstraint,
        _registry: &str,
    ) -> anyhow::Result<String> {
        // TODO: Implement actual resolution
        // For now, return a placeholder
        Ok("resolved-placeholder".to_string())
    }

    /// Compare two version strings to determine which is newer
    pub fn compare_versions(a: &str, b: &str) -> anyhow::Result<std::cmp::Ordering> {
        // Try semver comparison first
        let a_ver = semver::Version::parse(a);
        let b_ver = semver::Version::parse(b);

        match (a_ver, b_ver) {
            (Ok(a), Ok(b)) => Ok(a.cmp(&b)),
            _ => {
                // Fall back to string comparison for non-semver versions
                Ok(a.cmp(b))
            }
        }
    }
}

/// Version constraint types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    /// Always use the latest version
    Latest,

    /// Track a specific branch
    Branch(String),

    /// Semantic version requirement (e.g., "^1.0", "~2.1.0")
    Semver(semver::VersionReq),

    /// Exact version
    Exact(semver::Version),

    /// Specific Git commit SHA
    GitSha(String),
}

impl VersionConstraint {
    /// Check if a version satisfies this constraint
    pub fn satisfies(&self, version: &str) -> anyhow::Result<bool> {
        match self {
            VersionConstraint::Latest => Ok(true),
            VersionConstraint::Branch(_) => Ok(true), // Branch always "satisfies"
            VersionConstraint::Semver(req) => {
                let ver = semver::Version::parse(version)?;
                Ok(req.matches(&ver))
            }
            VersionConstraint::Exact(ver) => {
                let parsed = semver::Version::parse(version)?;
                Ok(&parsed == ver)
            }
            VersionConstraint::GitSha(sha) => {
                // For Git SHAs, check if version starts with the SHA (for short SHAs)
                Ok(version.starts_with(sha) || sha.starts_with(version))
            }
        }
    }

    /// Get a string representation of the constraint
    pub fn as_str(&self) -> String {
        match self {
            VersionConstraint::Latest => "latest".to_string(),
            VersionConstraint::Branch(branch) => format!("branch:{}", branch),
            VersionConstraint::Semver(req) => req.to_string(),
            VersionConstraint::Exact(ver) => ver.to_string(),
            VersionConstraint::GitSha(sha) => sha.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_constraint_latest() {
        let constraint = VersionResolver::parse_constraint("latest").unwrap();
        assert_eq!(constraint, VersionConstraint::Latest);
        assert_eq!(constraint.as_str(), "latest");
    }

    #[test]
    fn test_parse_constraint_branch() {
        let constraint = VersionResolver::parse_constraint("branch:main").unwrap();
        assert_eq!(constraint, VersionConstraint::Branch("main".to_string()));
        assert_eq!(constraint.as_str(), "branch:main");
    }

    #[test]
    fn test_parse_constraint_exact() {
        let constraint = VersionResolver::parse_constraint("1.2.3").unwrap();
        match &constraint {
            VersionConstraint::Exact(ver) => {
                assert_eq!(ver.major, 1);
                assert_eq!(ver.minor, 2);
                assert_eq!(ver.patch, 3);
            }
            _ => panic!("Expected Exact constraint"),
        }
        assert_eq!(constraint.as_str(), "1.2.3");
    }

    #[test]
    fn test_parse_constraint_semver() {
        let constraint = VersionResolver::parse_constraint("^1.0").unwrap();
        match &constraint {
            VersionConstraint::Semver(req) => {
                assert!(req.matches(&semver::Version::new(1, 2, 0)));
                assert!(!req.matches(&semver::Version::new(2, 0, 0)));
            }
            _ => panic!("Expected Semver constraint"),
        }
        assert_eq!(constraint.as_str(), "^1.0");
    }

    #[test]
    fn test_parse_constraint_git_sha() {
        let constraint = VersionResolver::parse_constraint("a".repeat(40).as_str()).unwrap();
        match constraint {
            VersionConstraint::GitSha(sha) => {
                assert_eq!(sha.len(), 40);
            }
            _ => panic!("Expected GitSha constraint"),
        }
    }

    #[test]
    fn test_parse_constraint_short_git_sha() {
        // Short Git SHA must be at least 7 characters
        let constraint = VersionResolver::parse_constraint("abcdef1").unwrap();
        match &constraint {
            VersionConstraint::GitSha(sha) => {
                assert_eq!(sha, "abcdef1");
            }
            _ => panic!("Expected GitSha constraint"),
        }
    }

    #[test]
    fn test_parse_constraint_invalid() {
        let result = VersionResolver::parse_constraint("invalid:version:format");
        assert!(result.is_err());
    }

    #[test]
    fn test_constraint_satisfies_semver() {
        let constraint = VersionResolver::parse_constraint("^1.0").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        assert!(constraint.satisfies("1.0.0").unwrap());
        assert!(!constraint.satisfies("2.0.0").unwrap());
    }

    #[test]
    fn test_constraint_satisfies_exact() {
        let constraint = VersionResolver::parse_constraint("1.2.3").unwrap();
        assert!(constraint.satisfies("1.2.3").unwrap());
        assert!(!constraint.satisfies("1.2.4").unwrap());
    }

    #[test]
    fn test_constraint_satisfies_git_sha() {
        let full_sha = "a".repeat(40);
        let constraint = VersionResolver::parse_constraint(&full_sha[..7]).unwrap();
        assert!(constraint.satisfies(&full_sha).unwrap());
        assert!(constraint.satisfies(&full_sha[..7]).unwrap());
    }

    #[test]
    fn test_compare_versions_semver() {
        use std::cmp::Ordering;
        let result = VersionResolver::compare_versions("1.2.3", "1.2.4").unwrap();
        assert_eq!(result, Ordering::Less);

        let result = VersionResolver::compare_versions("2.0.0", "1.9.9").unwrap();
        assert_eq!(result, Ordering::Greater);

        let result = VersionResolver::compare_versions("1.2.3", "1.2.3").unwrap();
        assert_eq!(result, Ordering::Equal);
    }

    #[test]
    fn test_lockfile_new() {
        let lockfile = Lockfile::new();
        assert_eq!(lockfile.version, 1);
        assert!(lockfile.mcp_servers.is_empty());
        assert!(lockfile.skills.is_empty());
        assert!(lockfile.validate().is_ok());
    }

    #[test]
    fn test_lockfile_add_and_get() {
        let mut lockfile = Lockfile::new();
        let package = LockedPackage::new(
            "test-package".to_string(),
            "1.2.3".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
        );

        lockfile.add_mcp_server("test-mcp".to_string(), package.clone());

        let retrieved = lockfile.get_mcp_server("test-mcp").unwrap();
        assert_eq!(retrieved.name, "test-package");
        assert_eq!(retrieved.resolved_version, "1.2.3");
    }

    #[test]
    fn test_locked_package_with_checksum() {
        let package = LockedPackage::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(),
            "registry:official".to_string(),
        )
        .with_checksum("abc123".to_string());

        assert_eq!(package.checksum, Some("abc123".to_string()));
    }

    #[test]
    fn test_lockfile_remove() {
        let mut lockfile = Lockfile::new();
        let package = LockedPackage::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(),
            "registry:official".to_string(),
        );

        lockfile.add_skill("test-skill".to_string(), package);
        let removed = lockfile.remove_skill("test-skill");

        assert!(removed.is_some());
        assert!(lockfile.get_skill("test-skill").is_none());
    }

    #[test]
    fn test_lockfile_invalid_version() {
        let mut lockfile = Lockfile::new();
        lockfile.version = 999;
        assert!(lockfile.validate().is_err());
    }
}
