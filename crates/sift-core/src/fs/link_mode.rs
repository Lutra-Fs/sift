use serde::{Deserialize, Serialize};

/// How Sift should materialize a cached directory into a target directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    #[default]
    Auto,
    Hardlink,
    Copy,
    Symlink,
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
