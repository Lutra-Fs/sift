//! Deploy coordination: scope decisions and client traversal.

pub mod scope;

pub use scope::{
    RepoStatus, ResourceKind, ScopeDecision, ScopeRequest, ScopeResolution, ScopeSupport,
    resolve_scope,
};
