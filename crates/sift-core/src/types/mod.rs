//! Shared core types used across configuration and lockfile layers.

use serde::{Deserialize, Serialize};

/// Configuration scope levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfigScope {
    /// Global/system-wide configuration.
    Global,
    /// Per-project, local (not shared).
    PerProjectLocal,
    /// Per-project, shared (e.g., checked into version control).
    PerProjectShared,
}
