//! Deploy coordination: scope decisions and client traversal.

pub mod executor;
pub mod install;
pub mod scope;
pub mod service;
pub mod targeting;
pub mod uninstall;

pub use install::{InstallMcpRequest, InstallOrchestrator, InstallReport, SkillInstallReport};
pub use scope::{
    RepoStatus, ResourceKind, ScopeDecision, ScopeRequest, ScopeResolution, ScopeSupport,
    resolve_scope,
};
pub use service::{InstallOutcome, InstallService, UninstallOutcome, UninstallService};
pub use targeting::TargetingPolicy;
pub use uninstall::{UninstallOrchestrator, UninstallReport};
