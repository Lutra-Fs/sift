//! MCPB security utilities
//!
//! Path validation and sanitization for MCPB bundles to prevent
//! path traversal attacks from malicious manifests.

use std::path::{Component, Path, PathBuf};

/// Validate that an entry_point path is safe to use within a bundle directory.
///
/// Returns the validated full path on success.
///
/// # Security
///
/// This prevents path traversal attacks where a malicious manifest specifies
/// entry_point as an absolute path (e.g., "/bin/sh") or a path with traversal
/// components (e.g., "../../../usr/bin/python").
///
/// # Errors
///
/// Returns an error if:
/// - The entry_point is an absolute path
/// - The entry_point contains path traversal that escapes the extract_dir
pub fn validate_entry_point(
    entry_point: &str,
    extract_dir: &Path,
    manifest_name: &str,
) -> anyhow::Result<PathBuf> {
    let entry_path = Path::new(entry_point);

    // Reject absolute paths immediately
    if entry_path.is_absolute() {
        anyhow::bail!(
            "MCPB manifest '{}' has invalid entry_point: absolute paths are not allowed (got '{}')",
            manifest_name,
            entry_point
        );
    }

    // Join and normalize the path to resolve ".." components
    let full_path = extract_dir.join(entry_point);

    // Use lexical normalization to resolve ".." without requiring filesystem access.
    // This handles cases like "dist/../../../etc/passwd" -> escapes extract_dir.
    let normalized = normalize_path(&full_path);
    let normalized_extract = normalize_path(extract_dir);

    // Verify the normalized path starts with the extract directory
    if !normalized.starts_with(&normalized_extract) {
        anyhow::bail!(
            "MCPB manifest '{}' has invalid entry_point: path traversal detected \
            (entry_point '{}' resolves outside bundle directory)",
            manifest_name,
            entry_point
        );
    }

    Ok(normalized)
}

/// Lexically normalize a path by resolving `.` and `..` components without filesystem access.
///
/// Unlike `canonicalize()`, this doesn't require the path to exist and doesn't follow symlinks.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Pop the last component if possible (but don't go above root)
                if !components.is_empty() && !matches!(components.last(), Some(Component::RootDir))
                {
                    components.pop();
                }
            }
            Component::CurDir => {
                // Skip "." components
            }
            c => {
                components.push(c);
            }
        }
    }

    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // validate_entry_point Tests
    // =========================================================================

    #[test]
    fn validate_entry_point_accepts_valid_relative_path() {
        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let result = validate_entry_point("dist/index.js", &extract_dir, "test");

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/cache/bundles/abc123/dist/index.js")
        );
    }

    #[test]
    fn validate_entry_point_accepts_deeply_nested_path() {
        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let result = validate_entry_point("src/lib/utils/helper.py", &extract_dir, "test");

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/cache/bundles/abc123/src/lib/utils/helper.py")
        );
    }

    #[test]
    fn validate_entry_point_rejects_absolute_path() {
        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let result = validate_entry_point("/bin/sh", &extract_dir, "test");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("absolute"));
    }

    #[test]
    fn validate_entry_point_rejects_simple_traversal() {
        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let result = validate_entry_point("../../../etc/passwd", &extract_dir, "test");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("traversal") || err.contains("outside"));
    }

    #[test]
    fn validate_entry_point_rejects_hidden_traversal() {
        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        let result = validate_entry_point("dist/../../../etc/passwd", &extract_dir, "test");

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("traversal") || err.contains("outside"));
    }

    #[test]
    fn validate_entry_point_allows_internal_dotdot() {
        let extract_dir = PathBuf::from("/cache/bundles/abc123");
        // This should be allowed: dist/../src/index.js stays within extract_dir
        let result = validate_entry_point("dist/../src/index.js", &extract_dir, "test");

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/cache/bundles/abc123/src/index.js")
        );
    }

    // =========================================================================
    // normalize_path Tests
    // =========================================================================

    #[test]
    fn normalize_path_resolves_dotdot() {
        let path = PathBuf::from("/a/b/../c");
        assert_eq!(normalize_path(&path), PathBuf::from("/a/c"));
    }

    #[test]
    fn normalize_path_resolves_dot() {
        let path = PathBuf::from("/a/./b/./c");
        assert_eq!(normalize_path(&path), PathBuf::from("/a/b/c"));
    }

    #[test]
    fn normalize_path_resolves_multiple_dotdot() {
        let path = PathBuf::from("/a/b/c/../../d");
        assert_eq!(normalize_path(&path), PathBuf::from("/a/d"));
    }

    #[test]
    fn normalize_path_preserves_root() {
        let path = PathBuf::from("/../../etc/passwd");
        // Can't go above root
        assert_eq!(normalize_path(&path), PathBuf::from("/etc/passwd"));
    }

    #[test]
    fn normalize_path_handles_relative_path() {
        let path = PathBuf::from("a/b/../c");
        assert_eq!(normalize_path(&path), PathBuf::from("a/c"));
    }
}
