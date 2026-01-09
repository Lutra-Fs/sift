use std::fs;
use std::path::Path;

use git2::{IndexAddOption, Repository};
use tempfile::TempDir;

use sift_core::version::git::resolve_subdir_commit;

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Test skill for {name}.\n---\n# {name}\n")
}

fn commit_all(repo: &Repository, message: &str) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
        .unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();

    let sig = repo.signature().unwrap();
    let head = repo.head();

    match head {
        Ok(head) => {
            let parent = repo.find_commit(head.target().unwrap()).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                .unwrap()
        }
        Err(_) => repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
            .unwrap(),
    }
}

#[test]
fn resolve_subdir_commit_ignores_unrelated_changes() {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path()).unwrap();

    let skill_dir = temp.path().join("skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), skill_md("skill")).unwrap();
    let first = commit_all(&repo, "add skill");

    fs::write(temp.path().join("other.txt"), "unrelated").unwrap();
    commit_all(&repo, "unrelated");

    let resolved = resolve_subdir_commit(temp.path(), "HEAD", Path::new("skill")).unwrap();

    assert_eq!(resolved, first.to_string());
}
