//! Install/update/remove orchestration for config entries.

use crate::config::{ConfigStore, McpConfigEntry, SkillConfigEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Changed,
    NoOp,
}

#[derive(Debug)]
pub struct InstallService {
    store: ConfigStore,
}

impl InstallService {
    pub fn new(store: ConfigStore) -> Self {
        Self { store }
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.store
    }

    pub fn install_skill(
        &self,
        name: &str,
        entry: SkillConfigEntry,
        force: bool,
    ) -> anyhow::Result<InstallOutcome> {
        let mut config = self.store.load()?;
        let scope = self.store.scope();
        if scope == crate::config::ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            let project_override = config.projects.entry(project_key.clone()).or_default();
            project_override.path = self.store.project_root().to_path_buf();

            match project_override.skill.get(name) {
                None => {
                    project_override.skill.insert(name.to_string(), entry);
                    self.store.save(&config)?;
                    return Ok(InstallOutcome::Changed);
                }
                Some(existing) => {
                    if entries_equal(existing, &entry) {
                        return Ok(InstallOutcome::NoOp);
                    }
                    if !force {
                        anyhow::bail!(
                            "Skill '{}' already exists with different configuration. Use update or --force.",
                            name
                        );
                    }
                    project_override.skill.insert(name.to_string(), entry);
                    self.store.save(&config)?;
                    return Ok(InstallOutcome::Changed);
                }
            }
        }

        match config.skill.get(name) {
            None => {
                config.skill.insert(name.to_string(), entry);
                self.store.save(&config)?;
                Ok(InstallOutcome::Changed)
            }
            Some(existing) => {
                if entries_equal(existing, &entry) {
                    return Ok(InstallOutcome::NoOp);
                }
                if !force {
                    anyhow::bail!(
                        "Skill '{}' already exists with different configuration. Use update or --force.",
                        name
                    );
                }
                config.skill.insert(name.to_string(), entry);
                self.store.save(&config)?;
                Ok(InstallOutcome::Changed)
            }
        }
    }

    pub fn install_mcp(
        &self,
        name: &str,
        entry: McpConfigEntry,
        force: bool,
    ) -> anyhow::Result<InstallOutcome> {
        let mut config = self.store.load()?;
        let scope = self.store.scope();
        if scope == crate::config::ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            let project_override = config.projects.entry(project_key.clone()).or_default();
            project_override.path = self.store.project_root().to_path_buf();

            match project_override.mcp.get(name) {
                None => {
                    project_override.mcp.insert(name.to_string(), entry);
                    self.store.save(&config)?;
                    return Ok(InstallOutcome::Changed);
                }
                Some(existing) => {
                    if entries_equal(existing, &entry) {
                        return Ok(InstallOutcome::NoOp);
                    }
                    if !force {
                        anyhow::bail!(
                            "MCP '{}' already exists with different configuration. Use update or --force.",
                            name
                        );
                    }
                    project_override.mcp.insert(name.to_string(), entry);
                    self.store.save(&config)?;
                    return Ok(InstallOutcome::Changed);
                }
            }
        }

        match config.mcp.get(name) {
            None => {
                config.mcp.insert(name.to_string(), entry);
                self.store.save(&config)?;
                Ok(InstallOutcome::Changed)
            }
            Some(existing) => {
                if entries_equal(existing, &entry) {
                    return Ok(InstallOutcome::NoOp);
                }
                if !force {
                    anyhow::bail!(
                        "MCP '{}' already exists with different configuration. Use update or --force.",
                        name
                    );
                }
                config.mcp.insert(name.to_string(), entry);
                self.store.save(&config)?;
                Ok(InstallOutcome::Changed)
            }
        }
    }
}

fn entries_equal<T: serde::Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

pub mod git_exclude;
pub mod orchestrator;
pub mod scope;
pub mod uninstall;
