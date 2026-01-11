//! Registry configuration and marketplace adapters
//!
//! Defines registry sources for discovering and resolving packages,
//! with support for both native Sift registries and Claude marketplace.

pub mod marketplace;
pub mod schema;
pub mod sift;

// Re-export the schema types
pub use schema::{RegistryCapabilities, RegistryConfig, RegistryType};

pub fn capabilities_for(config: &RegistryConfig) -> RegistryCapabilities {
    match config.r#type {
        RegistryType::Sift => sift::capabilities(),
        RegistryType::ClaudeMarketplace => marketplace::capabilities(),
    }
}
