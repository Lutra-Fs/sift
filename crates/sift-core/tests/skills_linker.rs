use std::fs;
use std::path::Path;

use tempfile::TempDir;

use sift_core::fs::tree_hash::hash_tree;
use sift_core::skills::linker::{LinkMode, LinkerOptions, deliver_dir_managed};
use sift_core::version::LockedSkill;

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create_dir_all should succeed in test temp dirs");
    }
    fs::write(path, content).expect("write should succeed in test temp dirs");
}

fn make_src_tree(tmp: &TempDir) -> std::path::PathBuf {
    let src = tmp.path().join("src-skill");
    fs::create_dir_all(&src).expect("create_dir_all should succeed in test temp dirs");
    write_file(&src.join("SKILL.md"), "# Skill\n");
    write_file(&src.join("scripts").join("run.sh"), "echo hello\n");
    src
}

fn make_locked_skill_with_hash(
    dst: &Path,
    cache_src: &Path,
    mode: LinkMode,
    tree_hash: &str,
) -> LockedSkill {
    LockedSkill::new(
        "test-skill".to_string(),
        "1.0.0".to_string(),
        "latest".to_string(),
        "registry:official".to_string(),
    )
    .with_install_state(
        dst.to_path_buf(),
        cache_src.to_path_buf(),
        mode,
        tree_hash.to_string(),
    )
}

// ==================== Linker behavior tests (unmanaged delivery) ====================

#[test]
fn symlink_mode_requires_allow_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let err = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Symlink,
            force: false,
            allow_symlink: false,
        },
        None,
        &src_hash,
    )
    .unwrap_err()
    .to_string();

    assert!(err.to_lowercase().contains("symlink"));
    assert!(
        err.to_lowercase().contains("not allowed") || err.to_lowercase().contains("capability")
    );
}

#[cfg(unix)]
#[test]
fn auto_falls_back_to_copy_on_cross_device_hardlink() {
    use std::os::unix::fs::MetadataExt;

    let src_tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&src_tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");
    let src_dev = fs::metadata(&src)
        .expect("src metadata should succeed")
        .dev();

    let dst_root =
        tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("tempdir_in should succeed");
    let dst = dst_root.path().join("dst-skill");
    let dst_dev = fs::metadata(dst_root.path())
        .expect("dst_root metadata should succeed")
        .dev();

    if src_dev == dst_dev {
        return;
    }

    let report = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Auto,
            force: false,
            allow_symlink: false,
        },
        None,
        &src_hash,
    )
    .expect("auto deliver_dir_managed should succeed");

    assert_eq!(report.mode, LinkMode::Copy);
    assert!(dst.join("SKILL.md").exists());
}

// ==================== Tests for deliver_dir_managed (new API) ====================

#[test]
fn deliver_managed_unmanaged_dir_fails_without_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");
    fs::create_dir_all(&dst).expect("create_dir_all should succeed");
    write_file(&dst.join("different.txt"), "different content");

    let err = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        None, // No lockfile record = unmanaged
        &src_hash,
    )
    .unwrap_err()
    .to_string();

    assert!(err.to_lowercase().contains("not managed"));
}

#[test]
fn deliver_managed_unmanaged_dir_succeeds_with_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");
    fs::create_dir_all(&dst).expect("create_dir_all should succeed");
    write_file(&dst.join("different.txt"), "different content");

    let report = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: true,
            allow_symlink: false,
        },
        None, // No lockfile record
        &src_hash,
    )
    .expect("force deliver should succeed");

    assert!(report.changed);
    assert!(dst.join("SKILL.md").exists());

    // Verify hash matches
    let dst_hash = hash_tree(&dst).expect("hash_tree should succeed");
    assert_eq!(src_hash, dst_hash);
}

#[test]
fn deliver_managed_with_matching_hash_skips() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    // First delivery
    let existing_skill = None;
    let report1 = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        existing_skill,
        &src_hash,
    )
    .expect("first delivery should succeed");
    assert!(report1.changed);

    // Create a lockfile record
    let locked_skill = make_locked_skill_with_hash(&dst, &src, LinkMode::Copy, &src_hash);

    // Second delivery: hash matches, should skip
    let report2 = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        Some(&locked_skill),
        &src_hash,
    )
    .expect("second delivery should succeed");
    assert!(!report2.changed);
}

#[test]
fn deliver_managed_with_hash_mismatch_fails_without_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    // Create initial installation
    let locked_skill = None;
    deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        locked_skill,
        &src_hash,
    )
    .expect("first delivery should succeed");

    // Modify dst
    write_file(&dst.join("SKILL.md"), "# Modified Skill\n");

    // Create a lockfile record with old hash
    let locked_skill = make_locked_skill_with_hash(&dst, &src, LinkMode::Copy, &src_hash);

    // Try to deliver again - should fail due to hash mismatch
    let err = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        Some(&locked_skill),
        &src_hash,
    )
    .unwrap_err()
    .to_string();

    assert!(err.to_lowercase().contains("hash"));
}

#[test]
fn deliver_managed_with_hash_mismatch_succeeds_with_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    // Create initial installation
    let locked_skill = None;
    deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        locked_skill,
        &src_hash,
    )
    .expect("first delivery should succeed");

    // Modify dst
    write_file(&dst.join("SKILL.md"), "# Modified Skill\n");

    // Create a lockfile record with old hash
    let locked_skill = make_locked_skill_with_hash(&dst, &src, LinkMode::Copy, &src_hash);

    // Force delivery should succeed
    let report = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: true,
            allow_symlink: false,
        },
        Some(&locked_skill),
        &src_hash,
    )
    .expect("force delivery should succeed");

    assert!(report.changed);

    // Verify hash is now correct
    let dst_hash = hash_tree(&dst).expect("hash_tree should succeed");
    assert_eq!(src_hash, dst_hash);
}

#[test]
fn deliver_managed_cache_dirty_fails_without_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    // Wrong hash (simulating cache corruption)
    let wrong_hash = "deadbeef".repeat(16);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let err = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        None,
        &wrong_hash,
    )
    .unwrap_err()
    .to_string();

    assert!(err.to_lowercase().contains("dirty"));
}

#[test]
fn deliver_managed_cache_dirty_succeeds_with_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    // Wrong hash (simulating cache corruption)
    let wrong_hash = "deadbeef".repeat(16);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let report = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: true,
            allow_symlink: false,
        },
        None,
        &wrong_hash,
    )
    .expect("force delivery should succeed");

    assert!(report.changed);
}

#[test]
fn deliver_managed_creates_correct_hash_for_copy() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let report = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
        None,
        &src_hash,
    )
    .expect("copy delivery should succeed");

    assert_eq!(report.mode, LinkMode::Copy);
    assert!(report.changed);

    let dst_hash = hash_tree(&dst).expect("hash_tree should succeed");
    assert_eq!(src_hash, dst_hash);
}

#[cfg(unix)]
#[test]
fn deliver_managed_creates_correct_hash_for_hardlink() {
    use std::os::unix::fs::MetadataExt;

    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);
    let src_hash = hash_tree(&src).expect("hash_tree should succeed");

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let report = deliver_dir_managed(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Hardlink,
            force: false,
            allow_symlink: false,
        },
        None,
        &src_hash,
    )
    .expect("hardlink delivery should succeed");

    assert_eq!(report.mode, LinkMode::Hardlink);

    // Verify hardlinks were created
    let src_md = fs::metadata(src.join("SKILL.md")).expect("src metadata should succeed");
    let dst_md = fs::metadata(dst.join("SKILL.md")).expect("dst metadata should succeed");
    assert_eq!(src_md.dev(), dst_md.dev());
    assert_eq!(src_md.ino(), dst_md.ino());

    // Verify hash matches
    let dst_hash = hash_tree(&dst).expect("hash_tree should succeed");
    assert_eq!(src_hash, dst_hash);
}
