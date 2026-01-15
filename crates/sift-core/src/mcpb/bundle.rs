//! MCPB Bundle downloading and extraction
//!
//! Handles downloading `.mcpb` zip archives from URLs and extracting
//! them to the local cache, parsing the manifest.json inside.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::McpbManifest;

/// Result of downloading and extracting an MCPB bundle
#[derive(Debug, Clone)]
pub struct McpbBundle {
    /// Parsed manifest from the bundle
    pub manifest: McpbManifest,
    /// Path to the extracted bundle directory
    pub extract_dir: PathBuf,
}

/// Downloads and extracts MCPB bundles
#[derive(Debug)]
pub struct McpbFetcher {
    /// Cache directory for downloaded bundles
    cache_dir: PathBuf,
}

impl McpbFetcher {
    /// Create a new McpbFetcher with the given cache directory
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Get the cache directory
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Fetch an MCPB bundle from a URL
    ///
    /// Downloads the .mcpb zip file, extracts it, and parses manifest.json.
    /// The bundle is cached by URL hash for reuse.
    pub async fn fetch(&self, url: &str, force: bool) -> anyhow::Result<McpbBundle> {
        let bundle_hash = self.hash_url(url);
        let extract_dir = self.cache_dir.join("mcpb").join(&bundle_hash);

        // Check if already cached and valid
        if !force && extract_dir.exists() {
            let manifest_path = extract_dir.join("manifest.json");
            if manifest_path.exists() {
                let manifest = self.read_manifest(&manifest_path)?;
                return Ok(McpbBundle {
                    manifest,
                    extract_dir,
                });
            }
        }

        // Clear existing cache if force or invalid
        if extract_dir.exists() {
            std::fs::remove_dir_all(&extract_dir).with_context(|| {
                format!("Failed to remove existing cache: {}", extract_dir.display())
            })?;
        }

        // Download the bundle
        let bundle_bytes = self.download(url).await?;

        // Extract to cache
        self.extract(&bundle_bytes, &extract_dir)?;

        // Parse manifest
        let manifest_path = extract_dir.join("manifest.json");
        if !manifest_path.exists() {
            anyhow::bail!(
                "Invalid MCPB bundle: missing manifest.json in {}",
                extract_dir.display()
            );
        }
        let manifest = self.read_manifest(&manifest_path)?;

        Ok(McpbBundle {
            manifest,
            extract_dir,
        })
    }

    /// Download a file from a URL
    async fn download(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let response = reqwest::get(url)
            .await
            .with_context(|| format!("Failed to download MCPB bundle from {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to download MCPB bundle: HTTP {} from {}",
                response.status(),
                url
            );
        }

        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("Failed to read response body from {}", url))?;

        Ok(bytes.to_vec())
    }

    /// Extract a zip archive to a directory
    fn extract(&self, data: &[u8], dest: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dest)
            .with_context(|| format!("Failed to create extract directory: {}", dest.display()))?;

        let cursor = std::io::Cursor::new(data);
        let mut archive =
            zip::ZipArchive::new(cursor).context("Failed to read MCPB bundle as zip archive")?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .with_context(|| format!("Failed to read zip entry {}", i))?;

            // Sanitize the file path to prevent path traversal attacks
            let outpath = match file.enclosed_name() {
                Some(path) => dest.join(path),
                None => continue, // Skip entries with unsafe paths
            };

            if file.is_dir() {
                std::fs::create_dir_all(&outpath).with_context(|| {
                    format!("Failed to create directory: {}", outpath.display())
                })?;
            } else {
                // Ensure parent directory exists
                if let Some(parent) = outpath.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create parent directory: {}", parent.display())
                    })?;
                }

                let mut outfile = std::fs::File::create(&outpath)
                    .with_context(|| format!("Failed to create file: {}", outpath.display()))?;

                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)
                    .with_context(|| format!("Failed to read zip entry: {}", file.name()))?;

                outfile
                    .write_all(&buffer)
                    .with_context(|| format!("Failed to write file: {}", outpath.display()))?;

                // Set executable permissions on Unix for binary files
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = file.unix_mode() {
                        std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))
                            .ok(); // Ignore permission errors
                    }
                }
            }
        }

        Ok(())
    }

    /// Read and parse manifest.json from the extracted bundle
    fn read_manifest(&self, path: &Path) -> anyhow::Result<McpbManifest> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read manifest: {}", path.display()))?;

        McpbManifest::from_json(&content)
            .with_context(|| format!("Failed to parse manifest: {}", path.display()))
    }

    /// Generate a hash for cache key from URL
    fn hash_url(&self, url: &str) -> String {
        let hash = blake3::hash(url.as_bytes());
        // Use first 16 bytes (32 hex chars) for brevity
        hash.to_hex()[..32].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // =========================================================================
    // Helper Functions
    // =========================================================================

    /// Create a minimal valid MCPB zip archive in memory
    fn create_test_mcpb_zip(manifest_json: &str) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            zip.start_file("manifest.json", options)
                .expect("Failed to start manifest.json");
            zip.write_all(manifest_json.as_bytes())
                .expect("Failed to write manifest.json");

            zip.finish().expect("Failed to finish zip");
        }
        buf.into_inner()
    }

    fn minimal_manifest_json() -> &'static str {
        r#"{
            "manifest_version": "0.3",
            "name": "test-server",
            "version": "1.0.0",
            "description": "Test MCP server",
            "author": { "name": "Test Author" },
            "server": {
                "type": "node",
                "entry_point": "dist/index.js"
            }
        }"#
    }

    // =========================================================================
    // URL Hash Tests
    // =========================================================================

    #[test]
    fn hash_url_is_deterministic() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let url = "https://example.com/server.mcpb";

        let hash1 = fetcher.hash_url(url);
        let hash2 = fetcher.hash_url(url);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 32); // 16 bytes = 32 hex chars
    }

    #[test]
    fn hash_url_differs_for_different_urls() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));

        let hash1 = fetcher.hash_url("https://example.com/server1.mcpb");
        let hash2 = fetcher.hash_url("https://example.com/server2.mcpb");

        assert_ne!(hash1, hash2);
    }

    // =========================================================================
    // Extraction Tests
    // =========================================================================

    #[test]
    fn extract_valid_mcpb_zip() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let temp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let extract_dir = temp.path().join("extracted");

        let zip_data = create_test_mcpb_zip(minimal_manifest_json());

        fetcher
            .extract(&zip_data, &extract_dir)
            .expect("Extraction should succeed");

        // Verify manifest.json was extracted
        let manifest_path = extract_dir.join("manifest.json");
        assert!(manifest_path.exists(), "manifest.json should exist");

        // Verify content
        let content = std::fs::read_to_string(&manifest_path).expect("Should read manifest");
        assert!(content.contains("test-server"));
    }

    #[test]
    fn extract_mcpb_with_nested_directories() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let temp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let extract_dir = temp.path().join("extracted");

        // Create zip with nested structure
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();

            // Add manifest.json at root
            zip.start_file("manifest.json", options)
                .expect("Failed to start file");
            zip.write_all(minimal_manifest_json().as_bytes())
                .expect("Failed to write");

            // Add nested directory structure
            zip.add_directory("dist/", options)
                .expect("Failed to add dir");
            zip.start_file("dist/index.js", options)
                .expect("Failed to start file");
            zip.write_all(b"console.log('hello');")
                .expect("Failed to write");

            zip.finish().expect("Failed to finish");
        }
        let zip_data = buf.into_inner();

        fetcher
            .extract(&zip_data, &extract_dir)
            .expect("Extraction should succeed");

        // Verify nested file was extracted
        let index_path = extract_dir.join("dist").join("index.js");
        assert!(index_path.exists(), "dist/index.js should exist");
    }

    #[test]
    fn extract_invalid_zip_fails() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let temp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let extract_dir = temp.path().join("extracted");

        let invalid_data = b"not a zip file";

        let result = fetcher.extract(invalid_data, &extract_dir);
        assert!(result.is_err(), "Should fail on invalid zip");
    }

    // =========================================================================
    // Manifest Reading Tests
    // =========================================================================

    #[test]
    fn read_manifest_parses_valid_json() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let temp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp.path().join("manifest.json");

        std::fs::write(&manifest_path, minimal_manifest_json()).expect("Should write manifest");

        let manifest = fetcher
            .read_manifest(&manifest_path)
            .expect("Should parse manifest");

        assert_eq!(manifest.name, "test-server");
        assert_eq!(manifest.version, "1.0.0");
    }

    #[test]
    fn read_manifest_fails_on_invalid_json() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let temp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp.path().join("manifest.json");

        std::fs::write(&manifest_path, "{ invalid json }").expect("Should write file");

        let result = fetcher.read_manifest(&manifest_path);
        assert!(result.is_err(), "Should fail on invalid JSON");
    }

    #[test]
    fn read_manifest_fails_on_missing_file() {
        let fetcher = McpbFetcher::new(PathBuf::from("/tmp/cache"));
        let temp = tempfile::TempDir::new().expect("Failed to create temp dir");
        let manifest_path = temp.path().join("nonexistent.json");

        let result = fetcher.read_manifest(&manifest_path);
        assert!(result.is_err(), "Should fail on missing file");
    }

    // =========================================================================
    // Cache Directory Tests
    // =========================================================================

    #[test]
    fn cache_dir_is_accessible() {
        let cache_path = PathBuf::from("/tmp/test-cache");
        let fetcher = McpbFetcher::new(cache_path.clone());

        assert_eq!(fetcher.cache_dir(), &cache_path);
    }
}
