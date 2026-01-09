use std::fs;

use tempfile::TempDir;

use sift_core::fs::tree_hash::hash_tree;
use sift_core::skills::linker::{LinkMode, LinkerOptions, deliver_dir_managed};

#[test]
fn symlink_mode_downgrades_when_not_supported() {
    let temp = TempDir::new().expect("tempdir should succeed");
    let src = temp.path().join("src");
    let dst = temp.path().join("dst");
    fs::create_dir_all(&src).expect("create_dir_all should succeed");
    fs::write(src.join("skill.txt"), "hello").expect("write should succeed");

    let expected_hash = hash_tree(&src).expect("hash_tree should succeed");

    let options = LinkerOptions {
        mode: LinkMode::Symlink,
        force: true,
        allow_symlink: false,
    };

    let report = deliver_dir_managed(&src, &dst, &options, None, &expected_hash)
        .expect("deliver_dir_managed should downgrade instead of failing");

    assert_ne!(report.mode, LinkMode::Symlink);
    assert!(matches!(report.mode, LinkMode::Hardlink | LinkMode::Copy));
    assert!(dst.exists());
}
