//! Lockfile types for resolved install state.
//!
//! Tracks resolved versions, install metadata, and ownership hashes.

use crate::fs::LinkMode;
use crate::types::ConfigScope;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Lockfile for resolved package versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    /// Lockfile format version
    pub version: u32,

    /// Timestamp when lockfile was generated
    pub generated_at: chrono::DateTime<chrono::Utc>,

    /// Locked MCP servers (version only)
    #[serde(default)]
    pub mcp_servers: HashMap<String, LockedMcpServer>,

    /// Locked skills (version + install state)
    #[serde(default)]
    pub skills: HashMap<String, LockedSkill>,

    /// Managed config ownership hashes
    #[serde(default)]
    pub managed_configs: HashMap<String, HashMap<String, String>>,
}

impl Lockfile {
    /// Create a new empty lockfile
    pub fn new() -> Self {
        Self {
            version: 1,
            generated_at: chrono::Utc::now(),
            mcp_servers: HashMap::new(),
            skills: HashMap::new(),
            managed_configs: HashMap::new(),
        }
    }

    /// Add or update a locked MCP server
    pub fn add_mcp_server(&mut self, name: String, server: LockedMcpServer) {
        self.mcp_servers.insert(name, server);
    }

    /// Get a locked MCP server
    pub fn get_mcp_server(&self, name: &str) -> Option<&LockedMcpServer> {
        self.mcp_servers.get(name)
    }

    /// Remove a locked MCP server
    pub fn remove_mcp_server(&mut self, name: &str) -> Option<LockedMcpServer> {
        self.mcp_servers.remove(name)
    }

    /// Add or update a locked skill
    pub fn add_skill(&mut self, name: String, skill: LockedSkill) {
        self.skills.insert(name, skill);
    }

    /// Get a locked skill
    pub fn get_skill(&self, name: &str) -> Option<&LockedSkill> {
        self.skills.get(name)
    }

    /// Remove a locked skill
    pub fn remove_skill(&mut self, name: &str) -> Option<LockedSkill> {
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

/// A locked MCP server with resolved version
///
/// MCP servers only need version tracking, no install state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedMcpServer {
    /// Server name
    pub name: String,

    /// Resolved version (exact Git SHA, Docker tag, etc.)
    pub resolved_version: String,

    /// Original constraint from sift.toml
    pub constraint: String,

    /// Registry source
    pub registry: String,

    /// Configuration scope where this entry was installed from
    pub scope: ConfigScope,

    /// Registry/source metadata (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<ResolvedOrigin>,

    /// Checksum for verification (optional)
    pub checksum: Option<String>,
}

impl LockedMcpServer {
    /// Create a new locked MCP server
    pub fn new(
        name: String,
        resolved_version: String,
        constraint: String,
        registry: String,
        scope: ConfigScope,
    ) -> Self {
        Self {
            name,
            resolved_version,
            constraint,
            registry,
            scope,
            origin: None,
            checksum: None,
        }
    }

    /// Attach registry/source metadata.
    pub fn with_origin(mut self, origin: ResolvedOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Set the checksum
    pub fn with_checksum(mut self, checksum: String) -> Self {
        self.checksum = Some(checksum);
        self
    }
}

/// Registry/source metadata for a resolved entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedOrigin {
    /// Original source string (e.g., "registry:anthropic/pdf").
    pub original_source: String,

    /// Registry key (e.g., "anthropic").
    pub registry_key: String,

    /// Registry-declared version (if any).
    #[serde(default)]
    pub registry_version: Option<String>,

    /// All aliases that may refer to this entry.
    #[serde(default)]
    pub aliases: Vec<String>,

    /// Parent plugin name if nested.
    #[serde(default)]
    pub parent: Option<String>,

    /// True if this entry represents a group alias.
    #[serde(default)]
    pub is_group: bool,
}

/// A locked skill with version and install state
///
/// Note: Paths are stored as JSON strings; non-UTF-8 paths will cause
/// LockfileStore::save() to fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSkill {
    /// Skill name
    pub name: String,

    /// Resolved version (exact Git SHA, Docker tag, etc.)
    pub resolved_version: String,

    /// Original constraint from sift.toml
    pub constraint: String,

    /// Registry source
    pub registry: String,

    /// Configuration scope where this entry was installed from
    pub scope: ConfigScope,

    /// Registry/source metadata (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<ResolvedOrigin>,

    /// Git repository URL for git-sourced skills
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_repo: Option<String>,

    /// Git reference (branch/tag) for git-sourced skills
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,

    /// Git subdirectory for git-sourced skills
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_subdir: Option<String>,

    /// Checksum for verification (optional)
    pub checksum: Option<String>,

    /// Destination path where skill is materialized (optional if not installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst_path: Option<PathBuf>,

    /// Cache source path (optional if not installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_src_path: Option<PathBuf>,

    /// Link mode used (optional if not installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<LinkMode>,

    /// Tree hash for content verification (optional if not installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree_hash: Option<String>,

    /// Timestamp when installed (optional if not installed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl LockedSkill {
    /// Create a new locked skill (not installed yet)
    pub fn new(
        name: String,
        resolved_version: String,
        constraint: String,
        registry: String,
        scope: ConfigScope,
    ) -> Self {
        Self {
            name,
            resolved_version,
            constraint,
            registry,
            scope,
            origin: None,
            git_repo: None,
            git_ref: None,
            git_subdir: None,
            checksum: None,
            dst_path: None,
            cache_src_path: None,
            mode: None,
            tree_hash: None,
            installed_at: None,
        }
    }

    /// Attach registry/source metadata.
    pub fn with_origin(mut self, origin: ResolvedOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Set the checksum
    pub fn with_checksum(mut self, checksum: String) -> Self {
        self.checksum = Some(checksum);
        self
    }

    pub fn with_git_metadata(
        mut self,
        repo: String,
        reference: Option<String>,
        subdir: Option<String>,
    ) -> Self {
        self.git_repo = Some(repo);
        self.git_ref = reference;
        self.git_subdir = subdir;
        self
    }

    /// Mark as installed
    pub fn with_install_state(
        mut self,
        dst_path: PathBuf,
        cache_src_path: PathBuf,
        mode: LinkMode,
        tree_hash: String,
    ) -> Self {
        self.dst_path = Some(dst_path);
        self.cache_src_path = Some(cache_src_path);
        self.mode = Some(mode);
        self.tree_hash = Some(tree_hash);
        self.installed_at = Some(chrono::Utc::now());
        self
    }

    /// Check if this skill is installed
    pub fn is_installed(&self) -> bool {
        self.dst_path.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let server = LockedMcpServer::new(
            "test-server".to_string(),
            "1.2.3".to_string(),
            "^1.0".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        );

        lockfile.add_mcp_server("test-mcp".to_string(), server.clone());

        let retrieved = lockfile.get_mcp_server("test-mcp").unwrap();
        assert_eq!(retrieved.name, "test-server");
        assert_eq!(retrieved.resolved_version, "1.2.3");
    }

    #[test]
    fn test_locked_mcp_server_with_checksum() {
        let server = LockedMcpServer::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        )
        .with_checksum("abc123".to_string());

        assert_eq!(server.checksum, Some("abc123".to_string()));
    }

    #[test]
    fn test_locked_skill_new() {
        let skill = LockedSkill::new(
            "test-skill".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        );

        assert_eq!(skill.name, "test-skill");
        assert_eq!(skill.resolved_version, "1.0.0");
        assert_eq!(skill.scope, ConfigScope::Global);
        assert!(!skill.is_installed());
        assert!(skill.dst_path.is_none());
        assert!(skill.tree_hash.is_none());
    }

    #[test]
    fn test_locked_skill_with_install_state() {
        let skill = LockedSkill::new(
            "test-skill".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(),
            "registry:official".to_string(),
            ConfigScope::PerProjectShared,
        )
        .with_install_state(
            PathBuf::from("/dst/skill"),
            PathBuf::from("/cache/skill"),
            LinkMode::Hardlink,
            "abc123".to_string(),
        );

        assert!(skill.is_installed());
        assert_eq!(skill.dst_path, Some(PathBuf::from("/dst/skill")));
        assert_eq!(skill.tree_hash, Some("abc123".to_string()));
        assert_eq!(skill.mode, Some(LinkMode::Hardlink));
    }

    #[test]
    fn test_lockfile_remove() {
        let mut lockfile = Lockfile::new();
        let skill = LockedSkill::new(
            "test".to_string(),
            "1.0.0".to_string(),
            "latest".to_string(),
            "registry:official".to_string(),
            ConfigScope::Global,
        );

        lockfile.add_skill("test-skill".to_string(), skill);
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
