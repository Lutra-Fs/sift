//! Helpers for managing .git/info/exclude entries.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

pub fn ensure_git_exclude(project_root: &Path, entry: &str) -> anyhow::Result<()> {
    if entry.contains('\n') || entry.contains('\r') {
        anyhow::bail!("git exclude entry contains newline");
    }

    let info_dir = git_info_dir(project_root)?;
    fs::create_dir_all(&info_dir)
        .with_context(|| format!("Failed to create git info dir: {}", info_dir.display()))?;

    let exclude_path = info_dir.join("exclude");
    let existing = if exclude_path.exists() {
        fs::read_to_string(&exclude_path)
            .with_context(|| format!("Failed to read {}", exclude_path.display()))?
    } else {
        String::new()
    };

    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }

    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(entry);
    next.push('\n');

    fs::write(&exclude_path, next)
        .with_context(|| format!("Failed to write {}", exclude_path.display()))?;
    Ok(())
}

fn git_info_dir(project_root: &Path) -> anyhow::Result<PathBuf> {
    let git_dir = project_root.join(".git");
    if !git_dir.exists() {
        anyhow::bail!("Not a git repository: {}", project_root.display());
    }
    Ok(git_dir.join("info"))
}
