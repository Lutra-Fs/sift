//! High-level commands for sift operations.
//!
//! This module provides the public API for orchestrating install, uninstall,
//! and other sift operations. These commands are designed to be called by
//! CLI, TUI, and GUI frontends.

pub mod install;

pub use install::{InstallCommand, InstallOptions, InstallReport, InstallTarget};
