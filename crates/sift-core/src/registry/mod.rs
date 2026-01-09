//! Registry configuration and marketplace adapters
//!
//! Defines registry sources for discovering and resolving packages,
//! with support for both native Sift registries and Claude marketplace.

pub mod marketplace;
pub mod resolver;
pub mod schema;

// Re-export the schema types
pub use schema::{RegistryConfig, RegistryType};
