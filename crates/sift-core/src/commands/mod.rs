//! High-level commands for sift operations.
//!
//! This module provides the public API for orchestrating install, uninstall,
//! and other sift operations. These commands are designed to be called by
//! CLI, TUI, and GUI frontends.

pub mod install;
pub mod registry;
pub mod uninstall;

pub use install::{InstallCommand, InstallOptions, InstallReport, InstallTarget};
pub use registry::{
    AddOptions as RegistryAddOptions, ListOptions as RegistryListOptions, RegistryCommand,
    RegistryEntry, RegistryReport, RemoveOptions as RegistryRemoveOptions,
};
pub use uninstall::{
    UninstallCommand, UninstallOptions, UninstallReport, UninstallScope, UninstallTarget,
};

// Re-export status command types from the status module
pub use crate::status::{StatusCommand, StatusOptions, StatusReport};
