//! Registry resolver for marketplace manifests.

use crate::registry::marketplace::{MarketplaceAdapter, MarketplaceManifest};

#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    pub name: String,
    pub declared_version: String,
    pub config: crate::skills::SkillConfig,
}

#[derive(Debug, Clone)]
pub struct MarketplaceResolver {
    manifest: MarketplaceManifest,
}

impl MarketplaceResolver {
    pub fn new(manifest: MarketplaceManifest) -> Self {
        Self { manifest }
    }

    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let manifest = MarketplaceAdapter::parse(json)?;
        Ok(Self::new(manifest))
    }

    pub fn resolve_skill(&self, name: &str) -> anyhow::Result<ResolvedSkill> {
        let plugin = MarketplaceAdapter::find_plugin(&self.manifest, name)
            .ok_or_else(|| anyhow::anyhow!("Plugin not found: {}", name))?;
        let config = MarketplaceAdapter::plugin_to_skill_config(plugin)?;
        Ok(ResolvedSkill {
            name: plugin.name.clone(),
            declared_version: plugin.version.clone(),
            config,
        })
    }
}
