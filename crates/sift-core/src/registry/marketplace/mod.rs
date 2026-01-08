//! Claude Marketplace compatibility layer
//!
//! Adapters for converting various Claude marketplace formats to Sift's native format.

pub mod adapter;
pub mod github_fetcher;
pub mod merge_nested;

// Re-export public API from adapter module
pub use adapter::{
    CommandsOrPaths, MarketplaceAdapter, MarketplaceInfo, MarketplaceManifest, MarketplaceOwner,
    MarketplacePlugin, MarketplaceSource, MarketplaceSourceObject, Metadata, RawMarketplaceManifest,
    SkillsOrPaths, SourceType,
};

pub use github_fetcher::GitHubFetcher;
pub use merge_nested::merge_plugin_with_nested;
