//! Low-level config read/write services for install and uninstall operations.

use crate::config::{ConfigStore, McpConfigEntry, SiftConfig, SkillConfigEntry};
use crate::types::ConfigScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Changed,
    NoOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallOutcome {
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
        if scope == ConfigScope::PerProjectLocal {
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
        if scope == ConfigScope::PerProjectLocal {
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

#[derive(Debug)]
pub struct UninstallService {
    store: ConfigStore,
}

impl UninstallService {
    pub fn new(store: ConfigStore) -> Self {
        Self { store }
    }

    pub fn config_store(&self) -> &ConfigStore {
        &self.store
    }

    pub fn contains_mcp(&self, name: &str) -> anyhow::Result<bool> {
        let config = self.store.load()?;
        let scope = self.store.scope();
        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            return Ok(config
                .projects
                .get(&project_key)
                .map(|project| project.mcp.contains_key(name))
                .unwrap_or(false));
        }
        Ok(config.mcp.contains_key(name))
    }

    pub fn contains_skill(&self, name: &str) -> anyhow::Result<bool> {
        let config = self.store.load()?;
        let scope = self.store.scope();
        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            return Ok(config
                .projects
                .get(&project_key)
                .map(|project| project.skill.contains_key(name))
                .unwrap_or(false));
        }
        Ok(config.skill.contains_key(name))
    }

    pub fn remove_mcp(&self, name: &str) -> anyhow::Result<UninstallOutcome> {
        let mut config = self.store.load()?;
        let scope = self.store.scope();
        let mut removed = false;

        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            if let Some(project) = config.projects.get_mut(&project_key)
                && project.mcp.remove(name).is_some()
            {
                removed = true;
            }
            if removed {
                cleanup_project_entry(&mut config, &project_key);
            }
        } else if config.mcp.remove(name).is_some() {
            removed = true;
        }

        if removed {
            self.store.save(&config)?;
            Ok(UninstallOutcome::Changed)
        } else {
            Ok(UninstallOutcome::NoOp)
        }
    }

    pub fn remove_skill(&self, name: &str) -> anyhow::Result<UninstallOutcome> {
        let mut config = self.store.load()?;
        let scope = self.store.scope();
        let mut removed = false;

        if scope == ConfigScope::PerProjectLocal {
            let project_key = self.store.project_root().to_string_lossy().to_string();
            if let Some(project) = config.projects.get_mut(&project_key)
                && project.skill.remove(name).is_some()
            {
                removed = true;
            }
            if removed {
                cleanup_project_entry(&mut config, &project_key);
            }
        } else if config.skill.remove(name).is_some() {
            removed = true;
        }

        if removed {
            self.store.save(&config)?;
            Ok(UninstallOutcome::Changed)
        } else {
            Ok(UninstallOutcome::NoOp)
        }
    }
}

fn entries_equal<T: serde::Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

fn cleanup_project_entry(config: &mut SiftConfig, project_key: &str) {
    let is_empty = config
        .projects
        .get(project_key)
        .map(|project| {
            project.mcp.is_empty()
                && project.skill.is_empty()
                && project.mcp_overrides.is_empty()
                && project.skill_overrides.is_empty()
        })
        .unwrap_or(false);
    if is_empty {
        config.projects.remove(project_key);
    }
}
