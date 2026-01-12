//! Version constraints and resolution helpers.

pub mod constraints;
pub mod git;

pub use constraints::{VersionConstraint, VersionResolver};
