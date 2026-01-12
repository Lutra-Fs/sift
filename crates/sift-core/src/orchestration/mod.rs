//! Install/update/remove orchestration for config entries.

pub mod install;
pub mod scope;
pub mod service;
pub mod uninstall;

pub use install::{InstallMcpRequest, InstallOrchestrator, InstallReport, SkillInstallReport};
pub use service::{InstallOutcome, InstallService, UninstallOutcome, UninstallService};
pub use uninstall::{UninstallOrchestrator, UninstallReport, remove_path_if_exists};
