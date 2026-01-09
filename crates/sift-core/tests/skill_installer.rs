use std::fs;
use tempfile::TempDir;

use sift_core::fs::LinkMode;
use sift_core::skills::installer::SkillInstaller;
use sift_core::version::store::LockfileStore;

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
        )
        .unwrap();

    assert!(dst_dir.join("SKILL.md").exists());
    assert!(result.changed);

    let lockfile = LockfileStore::load(None, store_dir).unwrap();
    let locked = lockfile.get_skill("demo").unwrap();
    assert!(locked.is_installed());
    assert_eq!(locked.resolved_version, "resolved-1");
}
