//! Git source specification types.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Specification for a git source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSpec {
    /// Repository URL (e.g., "https://github.com/org/repo")
    pub repo_url: String,
    /// Git reference (branch, tag, or commit SHA)
    pub reference: Option<String>,
    /// Subdirectory within the repository
    pub subdir: Option<String>,
}

impl GitSpec {
    /// Create a new GitSpec with just a repo URL.
    pub fn new(repo_url: impl Into<String>) -> Self {
        Self {
            repo_url: repo_url.into(),
            reference: None,
            subdir: None,
        }
    }

    /// Set the git reference (branch, tag, or commit).
    pub fn with_reference(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }

    /// Set the subdirectory path.
    pub fn with_subdir(mut self, subdir: impl Into<String>) -> Self {
        self.subdir = Some(subdir.into());
        self
    }

    /// Parse a git source string into a GitSpec.
    ///
    /// Supports formats:
    /// - `git:https://github.com/org/repo`
    /// - `github:org/repo`
    /// - `github:org/repo@ref`
    /// - `github:org/repo@ref/path`
    /// - `https://github.com/org/repo/tree/ref/path`
    pub fn parse(source: &str) -> anyhow::Result<Self> {
        let raw = source.strip_prefix("git:").unwrap_or(source);
        let raw = if let Some(stripped) = raw.strip_prefix("github:") {
            Self::expand_github_shorthand(stripped)
        } else {
            raw.to_string()
        };

        // Handle #ref= fragment (from github:org/repo@ref without path)
        if let Some((url_part, fragment)) = raw.split_once("#ref=") {
            return Ok(Self {
                repo_url: url_part.to_string(),
                reference: Some(fragment.to_string()),
                subdir: None,
            });
        }

        // Handle /tree/ URL format
        if let Some((repo, reference, subdir)) = Self::split_tree_path(&raw) {
            if subdir.is_empty() {
                anyhow::bail!("Git URL is missing a path after /tree/<ref>/");
            }
            return Ok(Self {
                repo_url: repo,
                reference: Some(reference),
                subdir: Some(subdir),
            });
        }

        Ok(Self {
            repo_url: raw,
            reference: None,
            subdir: None,
        })
    }

    /// Expand github shorthand like "org/repo@ref/path" to full URL.
    fn expand_github_shorthand(shorthand: &str) -> String {
        // Handle org/repo@ref/path format
        if let Some((repo_part, rest)) = shorthand.split_once('@') {
            // rest could be "ref" or "ref/path"
            if let Some((reference, path)) = rest.split_once('/') {
                format!(
                    "https://github.com/{}/tree/{}/{}",
                    repo_part, reference, path
                )
            } else {
                // Just ref, no path - use special marker that won't trigger tree path parsing
                format!("https://github.com/{}#ref={}", repo_part, rest)
            }
        } else {
            format!("https://github.com/{}", shorthand)
        }
    }

    /// Split a URL with /tree/ pattern into (repo, ref, subdir).
    fn split_tree_path(raw: &str) -> Option<(String, String, String)> {
        let marker = "/tree/";
        let idx = raw.find(marker)?;
        let repo = raw[..idx].to_string();
        let rest = &raw[idx + marker.len()..];
        let mut parts = rest.splitn(2, '/');
        let reference = parts.next()?.to_string();
        let subdir = parts.next().unwrap_or("").to_string();
        Some((repo, reference, subdir))
    }

    /// Compute the bare repo directory path for this spec.
    pub fn bare_repo_dir(&self, state_dir: &Path) -> std::path::PathBuf {
        let hash = blake3::hash(self.repo_url.as_bytes()).to_hex().to_string();
        state_dir.join("git").join(format!("{}.git", hash))
    }
}
