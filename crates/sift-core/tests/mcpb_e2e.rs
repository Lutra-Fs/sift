//! End-to-end integration tests for MCPB bundle flow
//!
//! Tests the complete flow: bundle fetch → manifest parse → McpResolvedServer → client config
//! These tests use in-memory bundle creation to avoid network dependencies.

use sift_core::mcpb::{McpbFetcher, manifest_to_server};
use std::io::Write;
use tempfile::TempDir;

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

        // Add a dist/index.js file for node servers
        zip.start_file("dist/index.js", options)
            .expect("Failed to start dist/index.js");
        zip.write_all(b"console.log('MCP Server');")
            .expect("Failed to write dist/index.js");

        zip.finish().expect("Failed to finish zip");
    }
    buf.into_inner()
}

fn node_manifest_json() -> &'static str {
    r#"{
        "manifest_version": "0.3",
        "name": "test-mcp-server",
        "version": "1.0.0",
        "description": "Test MCP server for e2e testing",
        "author": { "name": "Test Author" },
        "server": {
            "type": "node",
            "entry_point": "dist/index.js"
        }
    }"#
}

fn python_manifest_json() -> &'static str {
    r#"{
        "manifest_version": "0.3",
        "name": "py-mcp-server",
        "version": "2.0.0",
        "description": "Python MCP server",
        "author": { "name": "Test Author" },
        "server": {
            "type": "python",
            "entry_point": "src/main.py"
        }
    }"#
}

fn manifest_with_mcp_config() -> &'static str {
    r#"{
        "manifest_version": "0.3",
        "name": "custom-config-server",
        "version": "1.5.0",
        "description": "Server with explicit mcp_config",
        "author": { "name": "Test Author" },
        "server": {
            "type": "node",
            "mcp_config": {
                "command": "${__dirname}/node_modules/.bin/server",
                "args": ["--port", "3000", "--config", "${__dirname}/config.json"],
                "env": { "NODE_ENV": "production", "LOG_LEVEL": "info" }
            }
        }
    }"#
}

fn manifest_with_user_config() -> &'static str {
    r#"{
        "manifest_version": "0.3",
        "name": "user-config-server",
        "version": "1.0.0",
        "description": "Server with user config defaults",
        "author": { "name": "Test Author" },
        "server": {
            "type": "node",
            "entry_point": "dist/index.js"
        },
        "user_config": {
            "api_key": { "type": "string", "title": "API Key", "required": true },
            "workspace": {
                "type": "directory",
                "title": "Workspace",
                "default": "${HOME}/Documents"
            },
            "port": {
                "type": "number",
                "title": "Port",
                "default": 3000
            }
        }
    }"#
}

// =========================================================================
// E2E Flow Tests: Bundle → Manifest → McpResolvedServer
// =========================================================================

#[test]
fn e2e_node_server_flow() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    let zip_data = create_test_mcpb_zip(node_manifest_json());

    // Step 1: Extract bundle
    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    // Step 2: Parse manifest
    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    // Verify manifest parsing
    assert_eq!(manifest.name, "test-mcp-server");
    assert_eq!(manifest.version, "1.0.0");

    // Step 3: Convert to McpResolvedServer
    let server =
        manifest_to_server("test-mcp-server", &manifest, &extract_dir).expect("Should convert");

    // Verify final server configuration
    assert_eq!(server.name, "test-mcp-server");
    assert_eq!(server.command, Some("node".to_string()));
    assert_eq!(server.args.len(), 1);
    assert!(
        server.args[0].ends_with("dist/index.js"),
        "Args should contain entry point path: {:?}",
        server.args
    );
    assert!(
        server.args[0].starts_with(extract_dir.to_str().unwrap()),
        "Entry point should be inside extract dir"
    );
}

#[test]
fn e2e_python_server_flow() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());

    // Create zip with python server
    let manifest_json = python_manifest_json();
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();

        zip.start_file("manifest.json", options)
            .expect("Failed to start file");
        zip.write_all(manifest_json.as_bytes())
            .expect("Failed to write");

        zip.start_file("src/main.py", options)
            .expect("Failed to start file");
        zip.write_all(b"print('Python MCP')")
            .expect("Failed to write");

        zip.finish().expect("Failed to finish");
    }
    let zip_data = buf.into_inner();

    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    let server =
        manifest_to_server("py-mcp-server", &manifest, &extract_dir).expect("Should convert");

    assert_eq!(server.name, "py-mcp-server");
    assert_eq!(server.command, Some("python".to_string()));
    assert!(server.args[0].ends_with("src/main.py"));
}

#[test]
fn e2e_explicit_mcp_config_flow() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    let zip_data = create_test_mcpb_zip(manifest_with_mcp_config());

    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    let server = manifest_to_server("custom-config-server", &manifest, &extract_dir)
        .expect("Should convert");

    // Verify ${__dirname} substitution in command
    let expected_cmd = format!("{}/node_modules/.bin/server", extract_dir.display());
    assert_eq!(server.command, Some(expected_cmd));

    // Verify ${__dirname} substitution in args
    assert_eq!(server.args.len(), 4);
    assert_eq!(server.args[0], "--port");
    assert_eq!(server.args[1], "3000");
    assert_eq!(server.args[2], "--config");
    let expected_config = format!("{}/config.json", extract_dir.display());
    assert_eq!(server.args[3], expected_config);

    // Verify environment variables
    assert_eq!(server.env.get("NODE_ENV"), Some(&"production".to_string()));
    assert_eq!(server.env.get("LOG_LEVEL"), Some(&"info".to_string()));
}

#[test]
fn e2e_user_config_defaults_in_env() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    let zip_data = create_test_mcpb_zip(manifest_with_user_config());

    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    let server =
        manifest_to_server("user-config-server", &manifest, &extract_dir).expect("Should convert");

    // Only string defaults should appear in env
    assert_eq!(
        server.env.get("WORKSPACE"),
        Some(&"${HOME}/Documents".to_string())
    );
    // Required field without default should not appear
    assert!(!server.env.contains_key("API_KEY"));
    // Number default should not appear (only strings are supported currently)
    assert!(!server.env.contains_key("PORT"));
}

// =========================================================================
// Security E2E Tests
// =========================================================================

#[test]
fn e2e_rejects_malicious_absolute_path_manifest() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let malicious_manifest = r#"{
        "manifest_version": "0.3",
        "name": "malicious-server",
        "version": "1.0.0",
        "description": "Malicious server with absolute path",
        "author": { "name": "Attacker" },
        "server": {
            "type": "node",
            "entry_point": "/bin/sh"
        }
    }"#;

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    let zip_data = create_test_mcpb_zip(malicious_manifest);

    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    // Conversion should fail due to absolute path
    let result = manifest_to_server("malicious-server", &manifest, &extract_dir);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("absolute"),
        "Error should mention absolute path: {}",
        err
    );
}

#[test]
fn e2e_rejects_malicious_traversal_manifest() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let malicious_manifest = r#"{
        "manifest_version": "0.3",
        "name": "malicious-traversal",
        "version": "1.0.0",
        "description": "Malicious server with path traversal",
        "author": { "name": "Attacker" },
        "server": {
            "type": "binary",
            "entry_point": "dist/../../../etc/passwd"
        }
    }"#;

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    let zip_data = create_test_mcpb_zip(malicious_manifest);

    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    // Conversion should fail due to path traversal
    let result = manifest_to_server("malicious-traversal", &manifest, &extract_dir);
    assert!(result.is_err());
}

// =========================================================================
// Cache Behavior Tests
// =========================================================================

#[test]
fn e2e_cache_hash_is_consistent() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let fetcher = McpbFetcher::new(temp.path().to_path_buf());

    let url = "https://github.com/org/repo/releases/v1.0.0/my-server.mcpb";

    let hash1 = fetcher.hash_url(url);
    let hash2 = fetcher.hash_url(url);

    assert_eq!(hash1, hash2, "Cache hash should be deterministic");
    assert_eq!(hash1.len(), 32, "Hash should be 32 hex chars");
}

#[test]
fn e2e_different_urls_have_different_hashes() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let fetcher = McpbFetcher::new(temp.path().to_path_buf());

    let hash1 = fetcher.hash_url("https://example.com/server1.mcpb");
    let hash2 = fetcher.hash_url("https://example.com/server2.mcpb");

    assert_ne!(hash1, hash2, "Different URLs should have different hashes");
}

// =========================================================================
// Manifest Validation E2E Tests
// =========================================================================

#[test]
fn e2e_invalid_manifest_fails_gracefully() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    // Create zip with invalid manifest (missing required fields)
    let invalid_manifest = r#"{"name": "incomplete"}"#;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(invalid_manifest.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let zip_data = buf.into_inner();

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let result = fetcher.read_manifest(&manifest_path);

    assert!(result.is_err(), "Invalid manifest should fail to parse");
}

#[test]
fn e2e_missing_entry_point_and_mcp_config_fails() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let extract_dir = temp.path().join("extracted");
    std::fs::create_dir_all(&extract_dir).expect("Failed to create extract dir");

    let manifest_without_entry = r#"{
        "manifest_version": "0.3",
        "name": "no-entry-server",
        "version": "1.0.0",
        "description": "Server without entry point or mcp_config",
        "author": { "name": "Test" },
        "server": {
            "type": "node"
        }
    }"#;

    let fetcher = McpbFetcher::new(temp.path().to_path_buf());
    let zip_data = create_test_mcpb_zip(manifest_without_entry);

    fetcher
        .extract(&zip_data, &extract_dir)
        .expect("Extraction should succeed");

    let manifest_path = extract_dir.join("manifest.json");
    let manifest = fetcher
        .read_manifest(&manifest_path)
        .expect("Should parse manifest");

    let result = manifest_to_server("no-entry-server", &manifest, &extract_dir);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("entry_point") || err.contains("mcp_config"),
        "Error should mention missing entry_point/mcp_config: {}",
        err
    );
}
