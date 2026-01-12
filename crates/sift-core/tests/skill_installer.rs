use std::fs;
use tempfile::TempDir;

use sift_core::fs::LinkMode;
use sift_core::lockfile::LockfileStore;
use sift_core::skills::installer::SkillInstaller;
use sift_core::types::ConfigScope;

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Test skill for {name}.\n---\n# {name}\n")
}

#[test]
fn install_skill_updates_lockfile_and_delivery() {
    let temp = TempDir::new().unwrap();
    let cache_dir = temp.path().join("demo");
    let dst_dir = temp.path().join("dst/demo");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("SKILL.md"), skill_md("demo")).unwrap();

    let store_dir = temp.path().join("locks");
    let installer = SkillInstaller::new(store_dir.clone(), None);

    let result = installer
        .install(
            "demo",
            &cache_dir,
            &dst_dir,
            LinkMode::Copy,
            false,
            true,
            "resolved-1",
            "latest",
            "registry:test",
            ConfigScope::Global,
            None,
            None,
        )
        .unwrap();

    assert!(dst_dir.join("SKILL.md").exists());
    assert!(result.changed);

    let lockfile = LockfileStore::load(None, store_dir).unwrap();
    let locked = lockfile.get_skill("demo").unwrap();
    assert!(locked.is_installed());
    assert_eq!(locked.resolved_version, "resolved-1");
}

#[test]
fn install_skill_force_overwrites_stale_dst() {
    let temp = TempDir::new().unwrap();
    let cache_dir = temp.path().join("demo");
    let dst_dir = temp.path().join("dst/demo");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("SKILL.md"), skill_md("demo-v1")).unwrap();

    let store_dir = temp.path().join("locks");
    let installer = SkillInstaller::new(store_dir.clone(), None);

    installer
        .install(
            "demo",
            &cache_dir,
            &dst_dir,
            LinkMode::Copy,
            false,
            true,
            "resolved-1",
            "latest",
            "registry:test",
            ConfigScope::Global,
            None,
            None,
        )
        .unwrap();

    fs::write(cache_dir.join("SKILL.md"), skill_md("demo-v2")).unwrap();

    let result = installer
        .install(
            "demo",
            &cache_dir,
            &dst_dir,
            LinkMode::Copy,
            true,
            true,
            "resolved-2",
            "latest",
            "registry:test",
            ConfigScope::Global,
            None,
            None,
        )
        .unwrap();

    assert!(result.changed);
    let dst_content = fs::read_to_string(dst_dir.join("SKILL.md")).unwrap();
    assert!(dst_content.contains("demo-v2"));

    // Lockfile should have updated version
    let lockfile = LockfileStore::load(None, store_dir).unwrap();
    let locked = lockfile.get_skill("demo").unwrap();
    assert_eq!(locked.resolved_version, "resolved-2");
}

#[test]
fn install_skill_force_updates_lockfile_with_new_hash() {
    let temp = TempDir::new().unwrap();
    let cache_dir = temp.path().join("demo");
    let dst_dir = temp.path().join("dst/demo");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("SKILL.md"), skill_md("demo-v1")).unwrap();

    let store_dir = temp.path().join("locks");
    let installer = SkillInstaller::new(store_dir.clone(), None);

    // First install
    installer
        .install(
            "demo",
            &cache_dir,
            &dst_dir,
            LinkMode::Copy,
            false,
            true,
            "resolved-1",
            "latest",
            "registry:test",
            ConfigScope::Global,
            None,
            None,
        )
        .unwrap();

    let lockfile_v1 = LockfileStore::load(None, store_dir.clone()).unwrap();
    let locked_v1 = lockfile_v1.get_skill("demo").unwrap();
    let hash_v1 = locked_v1.tree_hash.clone().unwrap();

    // Modify cache to get new hash
    fs::write(cache_dir.join("SKILL.md"), skill_md("demo-v2")).unwrap();

    // Force re-install
    installer
        .install(
            "demo",
            &cache_dir,
            &dst_dir,
            LinkMode::Copy,
            true,
            true,
            "resolved-2",
            "latest",
            "registry:test",
            ConfigScope::Global,
            None,
            None,
        )
        .unwrap();

    let lockfile_v2 = LockfileStore::load(None, store_dir).unwrap();
    let locked_v2 = lockfile_v2.get_skill("demo").unwrap();
    let hash_v2 = locked_v2.tree_hash.clone().unwrap();

    // Hash should have changed
    assert_ne!(hash_v1, hash_v2);
    assert_eq!(locked_v2.resolved_version, "resolved-2");
}
