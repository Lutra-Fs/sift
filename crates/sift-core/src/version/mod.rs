//! Version locking and dependency resolution
//!
//! Implements snapshot-based version locking for reproducibility

pub mod lock;

// Re-export the lockfile types
pub use lock::{LockedPackage, Lockfile, VersionConstraint, VersionResolver};
