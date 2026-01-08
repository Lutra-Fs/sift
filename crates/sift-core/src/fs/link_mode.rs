use serde::{Deserialize, Serialize};

/// How Sift should materialize a cached directory into a target directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    Auto,
    Hardlink,
    Copy,
    Symlink,
}

impl Default for LinkMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl LinkMode {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkMode::Auto => "auto",
            LinkMode::Hardlink => "hardlink",
            LinkMode::Copy => "copy",
            LinkMode::Symlink => "symlink",
        }
    }
}
