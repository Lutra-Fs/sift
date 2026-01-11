//! Claude Marketplace compatibility layer
//!
//! Adapters for converting various Claude marketplace formats to Sift's native format.

pub mod adapter;

// Re-export public API from adapter module
pub use adapter::{
    MarketplaceAdapter, MarketplaceInfo, MarketplaceManifest, MarketplaceOwner, MarketplacePlugin,
    MarketplaceSource, MarketplaceSourceObject, Metadata, RawMarketplaceManifest, SkillsOrPaths,
    SourceType,
};

pub fn capabilities() -> crate::registry::RegistryCapabilities {
    crate::registry::RegistryCapabilities {
        supports_version_pinning: false,
    }
}
