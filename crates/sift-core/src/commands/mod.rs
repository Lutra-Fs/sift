//! High-level commands for sift operations.
//!
//! This module provides the public API for orchestrating install, uninstall,
//! and other sift operations. These commands are designed to be called by
//! CLI, TUI, and GUI frontends.

pub mod install;
pub mod registry;

pub use install::{InstallCommand, InstallOptions, InstallReport, InstallTarget};
pub use registry::{
    AddOptions as RegistryAddOptions, ListOptions as RegistryListOptions, RegistryCommand,
    RegistryEntry, RegistryReport, RemoveOptions as RegistryRemoveOptions,
};
