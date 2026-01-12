//! Lockfile types and persistence.
//!
//! Represents resolved install state and ownership tracking.

pub mod store;
pub mod types;

pub use store::{LockfileService, LockfileStore};
pub use types::{LockedMcpServer, LockedSkill, Lockfile, ResolvedOrigin};
