//! Source resolution for skills and MCP servers.
//!
//! This module provides a unified interface for resolving source strings
//! into fetchable specifications. It handles:
//! - Direct git sources (github:, git:)
//! - Local file sources (local:)
//! - Registry sources (registry:) - resolved via marketplace adapters

mod resolver;
mod spec;

pub use resolver::{
    McpRegistryResolution, RegistryMetadata, RegistryResolution, ResolvedInput, SourceResolver,
    derive_name_from_git_source, derive_name_from_path, is_git_like, is_local_path,
    normalize_git_source,
};
pub use spec::{LocalSpec, ResolvedSource};

// Re-export GitSpec from git module for convenience
pub use crate::git::GitSpec;

#[cfg(test)]
mod tests;
