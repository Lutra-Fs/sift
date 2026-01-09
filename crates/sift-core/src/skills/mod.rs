//! Skills management

pub mod installer;
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

// Legacy manager removed: operations should go through install/update services.
