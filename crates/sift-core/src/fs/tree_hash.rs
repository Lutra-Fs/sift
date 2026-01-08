//! Deterministic tree hashing for content verification
//!
//! Computes a stable hash of a directory tree, useful for:
//! - Content integrity verification
//! - Change detection
//! - Deterministic comparison of directory structures

use anyhow::Context;
use std::fs;
use std::path::Path;

/// Compute deterministic tree hash of a directory
///
/// # Algorithm
/// - Recursive directory traversal
/// - Sort paths lexicographically for determinism
/// - Hash format: `blake3(relative_path || 0x00 || content)`
/// - Output: hex string
///
/// # Notes
/// - Creates directory structure + hardlinks files (NOT directory hardlinks)
/// - Empty directories are included in hash
/// - Symlinks are not followed (may error)
///
/// # Example
/// ```no_run
/// use sift_core::fs::tree_hash::hash_tree;
/// use std::path::Path;
///
/// let hash = hash_tree(Path::new("/path/to/dir"))?;
/// assert_eq!(hash.len(), 64); // blake3 hex output
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn hash_tree(path: &Path) -> anyhow::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hash_dir_recursive(&mut hasher, path, "")?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn hash_dir_recursive(hasher: &mut blake3::Hasher, dir: &Path, base: &str) -> anyhow::Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

    // Collect and sort entries for deterministic ordering
    let mut sorted_entries: Vec<_> = entries
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("Failed to read directory entries: {}", dir.display()))?;
    sorted_entries.sort_by_key(|e| e.file_name());

    for entry in sorted_entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let rel_path = if base.is_empty() {
            name_str.to_string()
        } else {
            format!("{}/{}", base, name_str)
        };

        let ty = entry
            .file_type()
            .with_context(|| format!("Failed to stat file: {}", entry.path().display()))?;

        if ty.is_dir() {
            // Hash directory entry
            hasher.update(rel_path.as_bytes());
            hasher.update(&[0xFF]); // Directory marker
            hash_dir_recursive(hasher, &entry.path(), &rel_path)?;
        } else if ty.is_file() {
            // Hash file: path || 0x00 || content
            hasher.update(rel_path.as_bytes());
            hasher.update(&[0x00]); // Path separator
            let content = fs::read(&entry.path())
                .with_context(|| format!("Failed to read file: {}", entry.path().display()))?;
            hasher.update(&content);
        } else if ty.is_symlink() {
            anyhow::bail!("Symlinks are not supported: {}", entry.path().display());
        } else {
            anyhow::bail!(
                "Unsupported filesystem entry type: {}",
                entry.path().display()
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create_dir_all should succeed in test temp dirs");
        }
        fs::write(path, content).expect("write should succeed in test temp dirs");
    }

    #[test]
    fn test_empty_directory_hash() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let hash = hash_tree(tmp.path()).expect("hash_tree should succeed");
        // Empty directory should produce a stable hash
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_single_file_hash() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let file = tmp.path().join("test.txt");
        write_file(&file, "hello world");

        let hash = hash_tree(tmp.path()).expect("hash_tree should succeed");
        assert_eq!(hash.len(), 64);

        // Same content should produce same hash
        let tmp2 = TempDir::new().expect("tempdir should succeed");
        let file2 = tmp2.path().join("test.txt");
        write_file(&file2, "hello world");
        let hash2 = hash_tree(tmp2.path()).expect("hash_tree should succeed");
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_deterministic_order() {
        let tmp1 = TempDir::new().expect("tempdir should succeed");
        // Create files in order: a, b, c
        write_file(&tmp1.path().join("a.txt"), "content a");
        write_file(&tmp1.path().join("b.txt"), "content b");
        write_file(&tmp1.path().join("c.txt"), "content c");

        let tmp2 = TempDir::new().expect("tempdir should succeed");
        // Create files in reverse order: c, b, a
        write_file(&tmp2.path().join("c.txt"), "content c");
        write_file(&tmp2.path().join("b.txt"), "content b");
        write_file(&tmp2.path().join("a.txt"), "content a");

        let hash1 = hash_tree(tmp1.path()).expect("hash_tree should succeed");
        let hash2 = hash_tree(tmp2.path()).expect("hash_tree should succeed");
        assert_eq!(
            hash1, hash2,
            "Hashes should be identical regardless of creation order"
        );
    }

    #[test]
    fn test_nested_directories() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        write_file(&tmp.path().join("level1.txt"), "level1");
        write_file(&tmp.path().join("dir1").join("level2.txt"), "level2");
        write_file(
            &tmp.path().join("dir1").join("dir2").join("level3.txt"),
            "level3",
        );

        let hash = hash_tree(tmp.path()).expect("hash_tree should succeed");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_hash_changes_with_content() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let file = tmp.path().join("test.txt");
        write_file(&file, "original content");

        let hash1 = hash_tree(tmp.path()).expect("hash_tree should succeed");

        // Modify content
        write_file(&file, "modified content");

        let hash2 = hash_tree(tmp.path()).expect("hash_tree should succeed");
        assert_ne!(hash1, hash2, "Hash should change when content changes");
    }

    #[test]
    fn test_hash_changes_with_filename() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        write_file(&tmp.path().join("a.txt"), "content");

        let hash1 = hash_tree(tmp.path()).expect("hash_tree should succeed");

        // Rename file (remove old, add new)
        fs::remove_file(tmp.path().join("a.txt")).expect("remove should succeed");
        write_file(&tmp.path().join("b.txt"), "content");

        let hash2 = hash_tree(tmp.path()).expect("hash_tree should succeed");
        assert_ne!(hash1, hash2, "Hash should change when filename changes");
    }

    #[test]
    fn test_hardlink_creates_same_hash() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("create_dir_all should succeed");
        write_file(&src.join("file1.txt"), "content1");
        write_file(&src.join("file2.txt"), "content2");

        let dst = tmp.path().join("dst");
        fs::create_dir_all(&dst).expect("create_dir_all should succeed");

        // Create hardlinks
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            fs::hard_link(src.join("file1.txt"), dst.join("file1.txt"))
                .expect("hard_link should succeed");
            fs::hard_link(src.join("file2.txt"), dst.join("file2.txt"))
                .expect("hard_link should succeed");

            // Verify they are hardlinks (same inode)
            let src_md = fs::metadata(src.join("file1.txt")).expect("metadata should succeed");
            let dst_md = fs::metadata(dst.join("file1.txt")).expect("metadata should succeed");
            assert_eq!(src_md.ino(), dst_md.ino(), "Should be hardlinked");
        }

        #[cfg(not(unix))]
        {
            // On non-Unix, just copy for test compatibility
            fs::copy(src.join("file1.txt"), dst.join("file1.txt")).expect("copy should succeed");
            fs::copy(src.join("file2.txt"), dst.join("file2.txt")).expect("copy should succeed");
        }

        let src_hash = hash_tree(&src).expect("hash_tree should succeed");
        let dst_hash = hash_tree(&dst).expect("hash_tree should succeed");
        assert_eq!(
            src_hash, dst_hash,
            "Hardlinked files should produce same hash"
        );
    }

    #[test]
    fn test_nonexistent_path_fails() {
        let result = hash_tree(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(
            result.is_err(),
            "hash_tree should fail for nonexistent path"
        );
    }

    #[test]
    fn test_file_instead_of_directory_fails() {
        let tmp = TempDir::new().expect("tempdir should succeed");
        let file = tmp.path().join("file.txt");
        write_file(&file, "content");

        let result = hash_tree(&file);
        assert!(
            result.is_err(),
            "hash_tree should fail for file instead of directory"
        );
    }
}
