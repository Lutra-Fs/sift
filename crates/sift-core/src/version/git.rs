//! Git helpers for resolving subdirectory versions.

use std::path::Path;

use git2::{DiffOptions, Repository};

pub fn resolve_subdir_commit(
    repo_path: &Path,
    reference: &str,
    subdir: &Path,
) -> anyhow::Result<String> {
    let repo = Repository::open(repo_path)?;
    let obj = repo.revparse_single(reference)?;
    let commit = obj.peel_to_commit()?;

    let mut revwalk = repo.revwalk()?;
    revwalk.push(commit.id())?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;

    for oid in revwalk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        if commit_touches_path(&repo, &commit, subdir)? {
            return Ok(commit.id().to_string());
        }
    }

    anyhow::bail!(
        "No commit found affecting path '{}' in reference '{}'",
        subdir.display(),
        reference
    )
}

fn commit_touches_path(
    repo: &Repository,
    commit: &git2::Commit,
    subdir: &Path,
) -> anyhow::Result<bool> {
    let tree = commit.tree()?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };

    let mut diff_opts = DiffOptions::new();
    diff_opts.pathspec(subdir);

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut diff_opts))?;
    Ok(diff.deltas().len() > 0)
}
