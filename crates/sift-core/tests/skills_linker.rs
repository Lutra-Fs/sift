use std::fs;
use std::path::Path;

use tempfile::TempDir;

use sift_core::skills::linker::{deliver_dir, LinkMode, LinkerOptions};

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create_dir_all should succeed in test temp dirs");
    }
    fs::write(path, content).expect("write should succeed in test temp dirs");
}

fn read_json(path: &Path) -> serde_json::Value {
    let bytes = fs::read(path).expect("read should succeed in test temp dirs");
    serde_json::from_slice(&bytes).expect("managed marker should be valid JSON")
}

fn make_src_tree(tmp: &TempDir) -> std::path::PathBuf {
    let src = tmp.path().join("src-skill");
    fs::create_dir_all(&src).expect("create_dir_all should succeed in test temp dirs");
    write_file(&src.join("SKILL.md"), "# Skill\n");
    write_file(&src.join("scripts").join("run.sh"), "echo hello\n");
    src
}

fn marker_path(dst: &Path) -> std::path::PathBuf {
    dst.join(".sift-managed.json")
}

#[test]
fn copy_materializes_tree_and_writes_marker() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let report = deliver_dir(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
    )
    .expect("copy deliver_dir should succeed");

    assert_eq!(report.mode, LinkMode::Copy);
    assert!(dst.join("SKILL.md").exists());
    assert!(dst.join("scripts").join("run.sh").exists());

    let marker = marker_path(&dst);
    assert!(marker.exists());
    let json = read_json(&marker);
    assert_eq!(json["mode"], "copy");
}

#[test]
fn unmanaged_existing_dir_fails_without_force() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");
    fs::create_dir_all(&dst).expect("create_dir_all should succeed in test temp dirs");
    write_file(&dst.join("SKILL.md"), "local edits\n");

    let err = deliver_dir(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: false,
            allow_symlink: false,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(err.to_lowercase().contains("exists"));
    assert!(err.to_lowercase().contains("unmanaged") || err.to_lowercase().contains("managed"));
}

#[test]
fn force_replaces_unmanaged_dir() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");
    fs::create_dir_all(&dst).expect("create_dir_all should succeed in test temp dirs");
    write_file(&dst.join("SKILL.md"), "local edits\n");

    let report = deliver_dir(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Copy,
            force: true,
            allow_symlink: false,
        },
    )
    .expect("force copy deliver_dir should succeed");

    assert_eq!(report.mode, LinkMode::Copy);
    let content =
        fs::read_to_string(dst.join("SKILL.md")).expect("read_to_string should succeed");
    assert_eq!(content, "# Skill\n");

    let json = read_json(&marker_path(&dst));
    assert_eq!(json["mode"], "copy");
}

#[test]
fn symlink_mode_requires_allow_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let err = deliver_dir(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Symlink,
            force: false,
            allow_symlink: false,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(err.to_lowercase().contains("symlink"));
    assert!(err.to_lowercase().contains("not allowed") || err.to_lowercase().contains("capability"));
}

#[cfg(unix)]
#[test]
fn hardlink_mode_creates_hardlinks_for_files() {
    use std::os::unix::fs::MetadataExt;

    let tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&tmp);

    let out = tempfile::tempdir().expect("tempdir should succeed");
    let dst = out.path().join("dst-skill");

    let report = deliver_dir(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Hardlink,
            force: false,
            allow_symlink: false,
        },
    )
    .expect("hardlink deliver_dir should succeed");

    assert_eq!(report.mode, LinkMode::Hardlink);

    let src_md = fs::metadata(src.join("SKILL.md")).expect("src metadata should succeed");
    let dst_md = fs::metadata(dst.join("SKILL.md")).expect("dst metadata should succeed");
    assert_eq!(src_md.dev(), dst_md.dev());
    assert_eq!(src_md.ino(), dst_md.ino());

    let json = read_json(&marker_path(&dst));
    assert_eq!(json["mode"], "hardlink");
}

#[cfg(unix)]
#[test]
fn auto_falls_back_to_copy_on_cross_device_hardlink() {
    use std::os::unix::fs::MetadataExt;

    let src_tmp = tempfile::tempdir().expect("tempdir should succeed");
    let src = make_src_tree(&src_tmp);
    let src_dev = fs::metadata(&src).expect("src metadata should succeed").dev();

    let dst_root =
        tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("tempdir_in should succeed");
    let dst = dst_root.path().join("dst-skill");
    let dst_dev = fs::metadata(dst_root.path())
        .expect("dst_root metadata should succeed")
        .dev();

    if src_dev == dst_dev {
        return;
    }

    let report = deliver_dir(
        &src,
        &dst,
        &LinkerOptions {
            mode: LinkMode::Auto,
            force: false,
            allow_symlink: false,
        },
    )
    .expect("auto deliver_dir should succeed");

    assert_eq!(report.mode, LinkMode::Copy);
    assert!(dst.join("SKILL.md").exists());

    let json = read_json(&marker_path(&dst));
    assert_eq!(json["mode"], "copy");
}
