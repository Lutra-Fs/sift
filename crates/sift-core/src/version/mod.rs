//! Version locking and dependency resolution
//!
//! Implements snapshot-based version locking for reproducibility

pub mod git;
pub mod lock;
pub mod store;

// Re-export the lockfile types
pub use lock::{LockedMcpServer, LockedSkill, Lockfile, VersionConstraint, VersionResolver};
pub use store::LockfileStore;
