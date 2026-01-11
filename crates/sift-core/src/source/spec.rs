//! Source specification types.

use std::path::PathBuf;

use crate::git::GitSpec;

/// A resolved source specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSource {
    /// Git-based source (github:, git:, registry: resolved to git)
    Git(GitSpec),
    /// Local filesystem source
    Local(LocalSpec),
}

impl ResolvedSource {
    /// Check if this is a git source.
    pub fn is_git(&self) -> bool {
        matches!(self, Self::Git(_))
    }

    /// Check if this is a local source.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// Get the git spec if this is a git source.
    pub fn as_git(&self) -> Option<&GitSpec> {
        match self {
            Self::Git(spec) => Some(spec),
            _ => None,
        }
    }

    /// Get the local spec if this is a local source.
    pub fn as_local(&self) -> Option<&LocalSpec> {
        match self {
            Self::Local(spec) => Some(spec),
            _ => None,
        }
    }
}

/// Specification for a local filesystem source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSpec {
    /// Absolute path to the skill directory
    pub path: PathBuf,
}

impl LocalSpec {
    /// Create a new LocalSpec.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}
