//! Skills management

pub mod linker;
pub mod schema;

use serde::{Deserialize, Serialize};

// Re-export the new schema types
pub use schema::{SkillConfig, SkillConfigOverride};

/// Represents a skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Unique identifier for the skill
    pub id: String,
    /// Display name
    pub name: String,
    /// Description of what the skill does
    pub description: String,
    /// Source of the skill (local file, remote URL, etc.)
    pub source: String,
}

/// Manager for skills
#[derive(Debug)]
pub struct SkillManager;

impl SkillManager {
    /// Create a new skill manager
    pub fn new() -> Self {
        Self
    }

    /// List all configured skills
    pub fn list_skills(&self) -> Vec<Skill> {
        Vec::new()
    }

    /// Add a new skill
    pub fn add_skill(&self, _skill: Skill) -> anyhow::Result<()> {
        Ok(())
    }

    /// Remove a skill
    pub fn remove_skill(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Default for SkillManager {
    fn default() -> Self {
        Self::new()
    }
}
