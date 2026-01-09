//! Directory materialization for delivering skills to clients.

use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};

pub use crate::fs::LinkMode;
use crate::fs::tree_hash::hash_tree;
use crate::version::LockedSkill;

#[derive(Debug, Clone)]
pub struct LinkerOptions {
    pub mode: LinkMode,
    pub force: bool,
    pub allow_symlink: bool,
}

#[derive(Debug, Clone)]
pub struct LinkReport {
    pub mode: LinkMode,
    pub changed: bool,
}

/// Deliver directory with managed install verification
///
/// # Parameters
/// - `src_dir`: Source directory (cache)
/// - `dst_dir`: Destination directory (skill install location)
/// - `options`: Delivery options (mode, force, allow_symlink)
/// - `existing_install`: Optional existing install record from lockfile
/// - `expected_tree_hash`: Expected hash from lockfile (source of truth)
///
/// # Managed判定
/// - dst exists but no lockfile record: hard fail (unmanaged)
/// - dst exists with lockfile record and hash matches: skip (idempotent)
/// - dst exists with lockfile record but hash mismatch: hard fail unless force
///
/// # Cache integrity check
/// - Before delivery: hash_tree(src_dir) == expected_tree_hash
/// - If mismatch: cache dirty error, force required to proceed
///
/// # Delivery
/// - Creates directory structure + hardlinks/copy/symlinks files
/// - Does NOT write marker files
pub fn deliver_dir_managed(
    src_dir: &Path,
    dst_dir: &Path,
    options: &LinkerOptions,
    existing_install: Option<&LockedSkill>,
    expected_tree_hash: &str,
) -> anyhow::Result<LinkReport> {
    ensure_src_dir(src_dir)?;
    ensure_parent_dir(dst_dir)?;

    // Cache integrity check (prevents pollution detection bypass)
    let cache_hash = hash_tree(src_dir)
        .with_context(|| format!("Failed to hash cache: {}", src_dir.display()))?;
    if cache_hash != expected_tree_hash && !options.force {
        anyhow::bail!(
            "Cache is dirty: expected hash {}, got {}. Use force to proceed.",
            expected_tree_hash,
            cache_hash
        );
    }
    // Force: proceed despite dirty cache

    // Managed判定 based on lockfile record
    if dst_dir.exists() {
        match existing_install {
            Some(_install) => {
                let dst_hash = hash_tree(dst_dir)
                    .with_context(|| format!("Failed to hash dst: {}", dst_dir.display()))?;
                if dst_hash == expected_tree_hash {
                    // Idempotent: already installed with correct hash
                    return Ok(LinkReport {
                        mode: options.mode,
                        changed: false,
                    });
                }
                // Hash mismatch: managed but modified
                if !options.force {
                    anyhow::bail!(
                        "Managed install has unexpected hash (expected: {}, got: {}). Use force to override.",
                        expected_tree_hash,
                        dst_hash
                    );
                }
                // Force: proceed with replacement
            }
            None => {
                // No lockfile record: unmanaged directory
                if !options.force {
                    anyhow::bail!(
                        "Destination exists but is not managed by sift: {}. Use force to override.",
                        dst_dir.display()
                    );
                }
                // Force: adopt the directory
            }
        }
    }

    // Proceed with delivery
    deliver_dir(src_dir, dst_dir, options)
}

fn deliver_dir(
    src_dir: &Path,
    dst_dir: &Path,
    options: &LinkerOptions,
) -> anyhow::Result<LinkReport> {
    ensure_src_dir(src_dir)?;
    ensure_parent_dir(dst_dir)?;

    match options.mode {
        LinkMode::Symlink => {
            if options.allow_symlink {
                deliver_symlink(src_dir, dst_dir, options)
            } else {
                deliver_auto(src_dir, dst_dir, options)
            }
        }
        LinkMode::Copy => deliver_copy(src_dir, dst_dir, options),
        LinkMode::Hardlink => deliver_hardlink(src_dir, dst_dir, options),
        LinkMode::Auto => deliver_auto(src_dir, dst_dir, options),
    }
}

fn ensure_src_dir(src_dir: &Path) -> anyhow::Result<()> {
    let meta = fs::metadata(src_dir)
        .with_context(|| format!("Failed to stat source directory: {}", src_dir.display()))?;
    if !meta.is_dir() {
        anyhow::bail!("Source path is not a directory: {}", src_dir.display());
    }
    Ok(())
}

fn ensure_parent_dir(dst_dir: &Path) -> anyhow::Result<()> {
    let parent = dst_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Destination path has no parent: {}", dst_dir.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create destination parent: {}", parent.display()))?;
    Ok(())
}

fn deliver_auto(
    src_dir: &Path,
    dst_dir: &Path,
    options: &LinkerOptions,
) -> anyhow::Result<LinkReport> {
    let mut hardlink_first = options.clone();
    hardlink_first.mode = LinkMode::Hardlink;

    match deliver_hardlink(src_dir, dst_dir, &hardlink_first) {
        Ok(report) => Ok(report),
        Err(err) => {
            if is_cross_device_link_error(&err) {
                let mut fallback = options.clone();
                fallback.mode = LinkMode::Copy;
                deliver_copy(src_dir, dst_dir, &fallback)
            } else {
                Err(err)
            }
        }
    }
}

fn deliver_copy(
    src_dir: &Path,
    dst_dir: &Path,
    options: &LinkerOptions,
) -> anyhow::Result<LinkReport> {
    let tmp_dir = unique_temp_path(dst_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("Failed to create temp directory: {}", tmp_dir.display()))?;

    let result = (|| -> anyhow::Result<()> {
        copy_tree(src_dir, &tmp_dir)?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(err);
    }

    replace_dst_with_tmp(dst_dir, &tmp_dir, options)?;
    Ok(LinkReport {
        mode: LinkMode::Copy,
        changed: true,
    })
}

fn deliver_hardlink(
    src_dir: &Path,
    dst_dir: &Path,
    options: &LinkerOptions,
) -> anyhow::Result<LinkReport> {
    let tmp_dir = unique_temp_path(dst_dir)?;
    fs::create_dir_all(&tmp_dir)
        .with_context(|| format!("Failed to create temp directory: {}", tmp_dir.display()))?;

    let result = (|| -> anyhow::Result<()> {
        hardlink_tree(src_dir, &tmp_dir)
            .with_context(|| format!("Failed to hardlink tree from {}", src_dir.display()))?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(err);
    }

    replace_dst_with_tmp(dst_dir, &tmp_dir, options)?;
    Ok(LinkReport {
        mode: LinkMode::Hardlink,
        changed: true,
    })
}

fn deliver_symlink(
    src_dir: &Path,
    dst_dir: &Path,
    options: &LinkerOptions,
) -> anyhow::Result<LinkReport> {
    if let Ok(target) = fs::read_link(dst_dir)
        && same_path(&target, src_dir)
    {
        return Ok(LinkReport {
            mode: LinkMode::Symlink,
            changed: false,
        });
    }

    let tmp = unique_temp_path(dst_dir)?;

    if let Err(err) = create_dir_symlink(src_dir, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(anyhow::Error::new(err).context("Failed to create symlink"));
    }

    replace_dst_with_tmp(dst_dir, &tmp, options)?;
    Ok(LinkReport {
        mode: LinkMode::Symlink,
        changed: true,
    })
}

fn replace_dst_with_tmp(
    dst_dir: &Path,
    tmp_path: &Path,
    options: &LinkerOptions,
) -> anyhow::Result<()> {
    if dst_dir.exists() {
        if !options.force {
            anyhow::bail!(
                "Destination already exists: {} (use force to override)",
                dst_dir.display()
            );
        }
        remove_path(dst_dir).with_context(|| {
            format!(
                "Failed to remove existing destination: {}",
                dst_dir.display()
            )
        })?;
    }

    fs::rename(tmp_path, dst_dir).with_context(|| {
        format!(
            "Failed to move temp path {} into destination {}",
            tmp_path.display(),
            dst_dir.display()
        )
    })?;
    Ok(())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn unique_temp_path(dst_dir: &Path) -> anyhow::Result<PathBuf> {
    let parent = dst_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Destination path has no parent: {}", dst_dir.display()))?;
    let base = dst_dir.file_name().ok_or_else(|| {
        anyhow::anyhow!("Destination path has no filename: {}", dst_dir.display())
    })?;

    for attempt in 0u32..1000 {
        let name = if attempt == 0 {
            format!(".{}.tmp.{}", base.to_string_lossy(), std::process::id())
        } else {
            format!(
                ".{}.tmp.{}.{}",
                base.to_string_lossy(),
                std::process::id(),
                attempt
            )
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "Failed to allocate a unique temp path for {}",
        dst_dir.display()
    );
}

fn copy_tree(src: &Path, dst: &Path) -> anyhow::Result<()> {
    for entry in
        fs::read_dir(src).with_context(|| format!("Failed to read dir: {}", src.display()))?
    {
        let entry =
            entry.with_context(|| format!("Failed to read dir entry: {}", src.display()))?;
        let ty = entry
            .file_type()
            .with_context(|| format!("Failed to stat dir entry: {}", entry.path().display()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());

        if ty.is_dir() {
            fs::create_dir_all(&to)
                .with_context(|| format!("Failed to create directory: {}", to.display()))?;
            copy_tree(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to).with_context(|| {
                format!(
                    "Failed to copy file from {} to {}",
                    from.display(),
                    to.display()
                )
            })?;
        } else {
            anyhow::bail!("Unsupported filesystem entry type at {}", from.display());
        }
    }
    Ok(())
}

fn hardlink_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());

        if ty.is_dir() {
            fs::create_dir_all(&to)?;
            hardlink_tree(&from, &to)?;
        } else if ty.is_file() {
            fs::hard_link(&from, &to)?;
        } else {
            return Err(std::io::Error::other(format!(
                "Unsupported filesystem entry type at {}",
                from.display()
            )));
        }
    }
    Ok(())
}

fn is_cross_device_link_error(err: &anyhow::Error) -> bool {
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err.as_ref());
    while let Some(e) = cur {
        if let Some(ioe) = e.downcast_ref::<std::io::Error>()
            && is_cross_device_os_error(ioe)
        {
            return true;
        }
        cur = e.source();
    }
    false
}

fn is_cross_device_os_error(err: &std::io::Error) -> bool {
    let Some(code) = err.raw_os_error() else {
        return false;
    };

    #[cfg(unix)]
    {
        const EXDEV: i32 = 18;
        code == EXDEV
    }

    #[cfg(windows)]
    {
        const ERROR_NOT_SAME_DEVICE: i32 = 17;
        code == ERROR_NOT_SAME_DEVICE
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = code;
        false
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b
}

#[cfg(unix)]
fn create_dir_symlink(src_dir: &Path, dst_link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src_dir, dst_link)
}

#[cfg(windows)]
fn create_dir_symlink(src_dir: &Path, dst_link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src_dir, dst_link)
}

#[cfg(not(any(unix, windows)))]
fn create_dir_symlink(_src_dir: &Path, _dst_link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Symlinks are not supported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for deliver_dir_managed will be in the integration test file
    // This module only contains unit tests for internal functions

    #[test]
    fn test_is_cross_device_link_error_with_exdev() {
        #[cfg(unix)]
        {
            use std::io;
            let exdev_err = io::Error::from_raw_os_error(18); // EXDEV
            let anyhow_err = anyhow::Error::from(exdev_err);
            assert!(is_cross_device_link_error(&anyhow_err));
        }
    }

    #[test]
    fn test_replace_dst_without_force_fails_when_exists() {
        let tmp = tempfile::tempdir().expect("tempdir should succeed");
        let parent = tmp.path();
        let dst_dir = parent.join("dst");
        let tmp_dir = parent.join("tmp");

        fs::create_dir(&dst_dir).expect("create_dir should succeed");
        fs::create_dir(&tmp_dir).expect("create_dir should succeed");

        let options = LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        };

        let result = replace_dst_with_tmp(&dst_dir, &tmp_dir, &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("force"));
    }

    #[test]
    fn test_replace_dst_with_force_succeeds_when_exists() {
        let tmp = tempfile::tempdir().expect("tempdir should succeed");
        let parent = tmp.path();
        let dst_dir = parent.join("dst");
        let tmp_dir = parent.join("tmp");

        fs::create_dir(&dst_dir).expect("create_dir should succeed");
        fs::create_dir(&tmp_dir).expect("create_dir should succeed");

        let options = LinkerOptions {
            mode: LinkMode::Copy,
            force: true,
            allow_symlink: false,
        };

        let result = replace_dst_with_tmp(&dst_dir, &tmp_dir, &options);
        assert!(result.is_ok());
        // After rename, tmp_dir becomes dst_dir
        assert!(!tmp_dir.exists());
        assert!(dst_dir.exists());
    }
}
