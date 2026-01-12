use std::fs;

use tempfile::TempDir;

use sift_core::git::ensure_git_exclude;

#[test]
fn ensure_git_exclude_creates_file_and_adds_entry() {
    let temp = TempDir::new().expect("tempdir should succeed");
    let project = temp.path();
    let git_info = project.join(".git/info");
    fs::create_dir_all(&git_info).expect("create_dir_all should succeed");

    ensure_git_exclude(project, ".claude/skills").expect("ensure_git_exclude should succeed");

    let content = fs::read_to_string(git_info.join("exclude")).expect("read should succeed");
    assert!(content.lines().any(|line| line == ".claude/skills"));
}

#[test]
fn ensure_git_exclude_is_idempotent() {
    let temp = TempDir::new().expect("tempdir should succeed");
    let project = temp.path();
    let git_info = project.join(".git/info");
    fs::create_dir_all(&git_info).expect("create_dir_all should succeed");

    ensure_git_exclude(project, ".claude/skills").expect("ensure_git_exclude should succeed");
    ensure_git_exclude(project, ".claude/skills").expect("ensure_git_exclude should succeed");

    let content = fs::read_to_string(git_info.join("exclude")).expect("read should succeed");
    let count = content
        .lines()
        .filter(|line| *line == ".claude/skills")
        .count();
    assert_eq!(count, 1);
}
