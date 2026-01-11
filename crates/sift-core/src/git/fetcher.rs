//! Git fetcher for cloning and exporting skill content.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

use super::GitSpec;

/// Result of a successful fetch operation.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// Path to the cached skill content
    pub cache_dir: PathBuf,
    /// Resolved commit SHA
    pub commit_sha: String,
}

/// Fetches git repositories and exports subdirectories.
#[derive(Debug)]
pub struct GitFetcher {
    state_dir: PathBuf,
}

impl GitFetcher {
    /// Create a new GitFetcher with the given state directory.
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    /// Ensure git version is 2.25+ (required for sparse checkout).
    pub fn ensure_git_version() -> anyhow::Result<()> {
        let output = Command::new("git")
            .arg("--version")
            .output()
            .context("Failed to invoke git --version")?;
        if !output.status.success() {
            anyhow::bail!("Failed to run git --version");
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout
            .split_whitespace()
            .nth(2)
            .ok_or_else(|| anyhow::anyhow!("Unexpected git version output: {}", stdout))?;
        let mut parts = version.split('.');
        let major: u32 = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid git version: {}", version))?
            .parse()?;
        let minor: u32 = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid git version: {}", version))?
            .parse()?;
        if major > 2 || (major == 2 && minor >= 25) {
            return Ok(());
        }
        anyhow::bail!("Git 2.25+ is required for sparse checkout. Please upgrade git.");
    }

    /// Fetch a skill from a git repository.
    ///
    /// Returns the cache directory and resolved commit SHA.
    pub fn fetch(
        &self,
        spec: &GitSpec,
        skill_name: &str,
        force: bool,
    ) -> anyhow::Result<FetchResult> {
        Self::ensure_git_version()?;

        let cache_dir = self.skill_cache_dir(skill_name);
        let skill_marker = cache_dir.join("SKILL.md");

        // Check if cache is already valid
        if cache_dir.exists() && skill_marker.exists() && !force {
            let bare_dir = self.ensure_bare_repo(spec, false)?;
            let commit = self.resolve_commit(&bare_dir, spec, false)?;
            return Ok(FetchResult {
                cache_dir,
                commit_sha: commit,
            });
        }

        // Clear invalid cache
        if cache_dir.exists() {
            if force || Self::is_empty_dir(&cache_dir)? {
                std::fs::remove_dir_all(&cache_dir).with_context(|| {
                    format!("Failed to remove cache directory: {}", cache_dir.display())
                })?;
            } else {
                anyhow::bail!(
                    "Skill cache exists but is missing SKILL.md: {}. Use --force to refresh.",
                    cache_dir.display()
                );
            }
        }

        let bare_dir = self.ensure_bare_repo(spec, force)?;
        let commit = self.resolve_commit(&bare_dir, spec, force)?;
        self.export_subdir(&bare_dir, &cache_dir, spec, &commit)?;

        Ok(FetchResult {
            cache_dir,
            commit_sha: commit,
        })
    }

    /// Read a file from a git repository without full checkout.
    ///
    /// Useful for reading marketplace.json or other metadata files.
    pub fn read_file(&self, spec: &GitSpec, file_path: &str) -> anyhow::Result<String> {
        let bare_dir = self.ensure_bare_repo(spec, false)?;
        let commit = self.resolve_commit(&bare_dir, spec, false)?;

        let full_path = if let Some(ref subdir) = spec.subdir {
            format!("{}/{}", subdir, file_path)
        } else {
            file_path.to_string()
        };

        let output = Command::new("git")
            .args(["show", &format!("{}:{}", commit, full_path)])
            .current_dir(&bare_dir)
            .output()
            .with_context(|| format!("Failed to run git show for {}", full_path))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "File not found in repository: {} ({})",
                full_path,
                stderr.trim()
            );
        }

        String::from_utf8(output.stdout).context("File content is not valid UTF-8")
    }

    /// Read a file from the root of the repository (ignoring spec.subdir).
    pub fn read_root_file(&self, spec: &GitSpec, file_path: &str) -> anyhow::Result<String> {
        let bare_dir = self.ensure_bare_repo(spec, false)?;
        let commit = self.resolve_commit(&bare_dir, spec, false)?;

        let output = Command::new("git")
            .args(["show", &format!("{}:{}", commit, file_path)])
            .current_dir(&bare_dir)
            .output()
            .with_context(|| format!("Failed to run git show for {}", file_path))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "File not found in repository: {} ({})",
                file_path,
                stderr.trim()
            );
        }

        String::from_utf8(output.stdout).context("File content is not valid UTF-8")
    }

    /// Get the cache directory for a skill.
    fn skill_cache_dir(&self, skill_name: &str) -> PathBuf {
        self.state_dir.join("cache").join("skills").join(skill_name)
    }

    /// Ensure the bare repository exists and is up to date.
    fn ensure_bare_repo(&self, spec: &GitSpec, force: bool) -> anyhow::Result<PathBuf> {
        let bare_dir = spec.bare_repo_dir(&self.state_dir);

        if bare_dir.exists() {
            if force && spec.reference.is_none() {
                std::fs::remove_dir_all(&bare_dir).with_context(|| {
                    format!("Failed to remove bare repo: {}", bare_dir.display())
                })?;
            } else {
                return Ok(bare_dir);
            }
        }

        std::fs::create_dir_all(
            bare_dir
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Bare repo directory has no parent"))?,
        )
        .with_context(|| {
            format!(
                "Failed to create git cache directory: {}",
                bare_dir.display()
            )
        })?;

        Self::run_git(
            None,
            &[
                "clone",
                "--filter=blob:none",
                "--bare",
                &spec.repo_url,
                bare_dir
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid bare repo dir"))?,
            ],
        )?;

        Ok(bare_dir)
    }

    /// Resolve the git reference to a commit SHA.
    fn resolve_commit(
        &self,
        bare_dir: &Path,
        spec: &GitSpec,
        force: bool,
    ) -> anyhow::Result<String> {
        let reference = spec.reference.as_deref().unwrap_or("HEAD");

        if force && spec.reference.is_some() {
            Self::run_git(
                Some(bare_dir),
                &["fetch", "--filter=blob:none", "origin", reference],
            )?;
            return Self::git_rev_parse(Some(bare_dir), "FETCH_HEAD");
        }

        match Self::git_rev_parse(Some(bare_dir), reference) {
            Ok(commit) => Ok(commit),
            Err(err) => {
                if spec.reference.is_some() {
                    Self::run_git(
                        Some(bare_dir),
                        &["fetch", "--filter=blob:none", "origin", reference],
                    )?;
                    Self::git_rev_parse(Some(bare_dir), "FETCH_HEAD")
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Export a subdirectory from the bare repo to the cache directory.
    fn export_subdir(
        &self,
        bare_dir: &Path,
        cache_dir: &Path,
        spec: &GitSpec,
        commit: &str,
    ) -> anyhow::Result<()> {
        let worktree_dir = self.unique_temp_worktree_dir()?;

        Self::run_git(
            Some(bare_dir),
            &[
                "worktree",
                "add",
                "--detach",
                worktree_dir
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid worktree dir"))?,
                commit,
            ],
        )?;

        if let Some(ref subdir) = spec.subdir {
            Self::run_git(Some(&worktree_dir), &["sparse-checkout", "init", "--cone"])?;
            Self::run_git(Some(&worktree_dir), &["sparse-checkout", "set", subdir])?;
            Self::run_git(Some(&worktree_dir), &["checkout", commit])?;
        }

        let src_root = if let Some(ref subdir) = spec.subdir {
            worktree_dir.join(subdir)
        } else {
            worktree_dir.clone()
        };

        if !src_root.exists() {
            anyhow::bail!(
                "Git checkout did not create expected path: {}",
                src_root.display()
            );
        }

        std::fs::create_dir_all(cache_dir).with_context(|| {
            format!("Failed to create cache directory: {}", cache_dir.display())
        })?;
        Self::copy_tree_filtered(&src_root, cache_dir)?;

        Self::run_git(
            Some(bare_dir),
            &[
                "worktree",
                "remove",
                "--force",
                worktree_dir
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid worktree dir"))?,
            ],
        )?;
        let _ = std::fs::remove_dir_all(&worktree_dir);

        Ok(())
    }

    /// Create a unique temporary worktree directory.
    fn unique_temp_worktree_dir(&self) -> anyhow::Result<PathBuf> {
        let worktree_base = self.state_dir.join("worktrees");
        std::fs::create_dir_all(&worktree_base).with_context(|| {
            format!(
                "Failed to create worktrees directory: {}",
                worktree_base.display()
            )
        })?;
        Self::clean_stale_worktrees(&worktree_base)?;

        for attempt in 0..100 {
            let thread_id = format!("{:?}", std::thread::current().id());
            let name = format!("{}.{}.{}", std::process::id(), thread_id, attempt);
            let candidate = worktree_base.join(name);
            if !candidate.exists() {
                return Ok(candidate);
            }
        }
        anyhow::bail!(
            "Failed to allocate a temp worktree directory in {}",
            worktree_base.display()
        );
    }

    /// Clean up stale worktree directories.
    fn clean_stale_worktrees(base: &Path) -> anyhow::Result<()> {
        let current_pid = std::process::id();
        for entry in std::fs::read_dir(base)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(pid_str) = name_str.split('.').next()
                && let Ok(pid) = pid_str.parse::<u32>()
                && pid != current_pid
                && !Self::is_process_alive(pid)
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        Ok(())
    }

    /// Check if a process is likely alive (conservative check).
    ///
    /// On Unix, we check /proc/{pid} exists. On other systems, we assume alive.
    fn is_process_alive(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new(&format!("/proc/{}", pid)).exists()
        }
        #[cfg(target_os = "macos")]
        {
            // On macOS, use ps to check if process exists
            std::process::Command::new("ps")
                .args(["-p", &pid.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(true)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            // Conservative: assume process is alive on other platforms
            true
        }
    }

    /// Run a git command.
    fn run_git(cwd: Option<&Path>, args: &[&str]) -> anyhow::Result<()> {
        let mut cmd = Command::new("git");
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd
            .output()
            .with_context(|| format!("Failed to run git {:?}", args))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Git command failed {:?}: {}", args, stderr.trim());
        }
        Ok(())
    }

    /// Run git rev-parse and return the result.
    fn git_rev_parse(cwd: Option<&Path>, rev: &str) -> anyhow::Result<String> {
        let mut cmd = Command::new("git");
        cmd.args(["rev-parse", rev]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd
            .output()
            .with_context(|| format!("Failed to run git rev-parse {}", rev))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git rev-parse {} failed: {}", rev, stderr.trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check if a directory is empty.
    fn is_empty_dir(path: &Path) -> anyhow::Result<bool> {
        let mut entries = std::fs::read_dir(path)
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;
        Ok(entries.next().is_none())
    }

    /// Copy a directory tree, excluding .git directories.
    fn copy_tree_filtered(src: &Path, dst: &Path) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let file_name = entry.file_name();
            if file_name == ".git" {
                continue;
            }
            let src_path = entry.path();
            let dst_path = dst.join(&file_name);
            if src_path.is_dir() {
                std::fs::create_dir_all(&dst_path)?;
                Self::copy_tree_filtered(&src_path, &dst_path)?;
            } else {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }
}
