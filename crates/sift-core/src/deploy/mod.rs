//! Deploy coordination: scope decisions and client traversal.

pub mod executor;
pub mod scope;
pub mod targeting;

pub use scope::{
    RepoStatus, ResourceKind, ScopeDecision, ScopeRequest, ScopeResolution, ScopeSupport,
    resolve_scope,
};
pub use targeting::TargetingPolicy;
