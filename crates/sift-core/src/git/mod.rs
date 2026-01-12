//! Git operations for fetching skills and reading files from repositories.
//!
//! This module provides a unified interface for git operations:
//! - Fetching skills via sparse checkout
//! - Reading files from bare repos (e.g., marketplace.json)
//! - Managing bare repo lifecycle

mod exclude;
mod fetcher;
mod spec;

pub use exclude::ensure_git_exclude;
pub use fetcher::{FetchResult, GitFetcher};
pub use spec::GitSpec;

#[cfg(test)]
mod tests;
