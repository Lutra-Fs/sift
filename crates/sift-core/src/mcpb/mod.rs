//! MCPB (MCP Bundles) support
//!
//! Handles downloading, extracting, and parsing `.mcpb` bundle archives
//! containing local MCP servers with their manifest.json configuration.

pub mod bundle;
pub mod converter;
pub mod manifest;

pub use bundle::{McpbBundle, McpbFetcher};
pub use converter::manifest_to_server;
pub use manifest::{
    McpbCompatibility, McpbManifest, McpbMcpConfig, McpbServer, McpbServerType, McpbUserConfig,
    McpbUserConfigType,
};

/// Check if a URL points to an MCPB bundle file
///
/// Returns true if the URL ends with `.mcpb` extension
pub fn is_mcpb_url(url: &str) -> bool {
    // Strip query string and fragment before checking extension
    let path = url
        .split('?')
        .next()
        .and_then(|s| s.split('#').next())
        .unwrap_or(url);

    path.ends_with(".mcpb")
}

/// Normalize a raw MCPB URL to the `mcpb:` source format
///
/// If the input already has `mcpb:` prefix, returns as-is.
/// If the input is a URL ending in `.mcpb`, normalizes to `mcpb:<url>`.
pub fn normalize_mcpb_source(input: &str) -> Option<String> {
    if input.starts_with("mcpb:") {
        return Some(input.to_string());
    }

    if is_mcpb_url(input) {
        return Some(format!("mcpb:{}", input));
    }

    None
}

/// Derive a name from an MCPB URL
///
/// Extracts the filename (without .mcpb extension) from the URL path.
pub fn derive_name_from_mcpb_url(url: &str) -> anyhow::Result<String> {
    let raw = url.strip_prefix("mcpb:").unwrap_or(url);

    // Strip query string and fragment
    let path = raw
        .split('?')
        .next()
        .and_then(|s| s.split('#').next())
        .unwrap_or(raw);

    // Get the last path segment
    let segment = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid MCPB URL: no path segment found"))?;

    // Remove .mcpb extension
    let name = segment
        .strip_suffix(".mcpb")
        .ok_or_else(|| anyhow::anyhow!("Invalid MCPB URL: missing .mcpb extension"))?;

    if name.is_empty() {
        anyhow::bail!("Invalid MCPB URL: empty name");
    }

    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // MCPB URL Detection Tests
    // =========================================================================

    #[test]
    fn test_is_mcpb_url_simple() {
        assert!(is_mcpb_url(
            "https://github.com/org/repo/releases/download/v1.0/my-server.mcpb"
        ));
    }

    #[test]
    fn test_is_mcpb_url_with_query_string() {
        assert!(is_mcpb_url(
            "https://example.com/download/server.mcpb?token=abc123"
        ));
    }

    #[test]
    fn test_is_mcpb_url_with_fragment() {
        assert!(is_mcpb_url("https://example.com/server.mcpb#section"));
    }

    #[test]
    fn test_is_mcpb_url_with_query_and_fragment() {
        assert!(is_mcpb_url("https://example.com/server.mcpb?v=1#section"));
    }

    #[test]
    fn test_is_mcpb_url_negative_cases() {
        // Not MCPB URLs
        assert!(!is_mcpb_url("https://github.com/org/repo"));
        assert!(!is_mcpb_url("https://example.com/server.zip"));
        assert!(!is_mcpb_url("https://example.com/mcpb")); // No dot before mcpb
        assert!(!is_mcpb_url("https://example.com/server.mcpb.zip"));
        assert!(!is_mcpb_url("./local/path/server.js"));
    }

    #[test]
    fn test_is_mcpb_url_case_sensitive() {
        // .mcpb extension is case-sensitive
        assert!(!is_mcpb_url("https://example.com/server.MCPB"));
        assert!(!is_mcpb_url("https://example.com/server.Mcpb"));
    }

    // =========================================================================
    // MCPB Source Normalization Tests
    // =========================================================================

    #[test]
    fn test_normalize_mcpb_source_already_prefixed() {
        let input = "mcpb:https://example.com/server.mcpb";
        assert_eq!(normalize_mcpb_source(input), Some(input.to_string()));
    }

    #[test]
    fn test_normalize_mcpb_source_raw_url() {
        let url = "https://github.com/10XGenomics/txg-mcp/releases/latest/download/txg-node.mcpb";
        let expected = format!("mcpb:{}", url);
        assert_eq!(normalize_mcpb_source(url), Some(expected));
    }

    #[test]
    fn test_normalize_mcpb_source_not_mcpb() {
        assert_eq!(normalize_mcpb_source("https://example.com/repo.git"), None);
        assert_eq!(normalize_mcpb_source("registry:my-server"), None);
        assert_eq!(normalize_mcpb_source("local:/path/to/server"), None);
    }

    // =========================================================================
    // Name Derivation Tests
    // =========================================================================

    #[test]
    fn test_derive_name_from_mcpb_url_simple() {
        let url = "https://example.com/releases/my-server.mcpb";
        assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "my-server");
    }

    #[test]
    fn test_derive_name_from_mcpb_url_with_prefix() {
        let url = "mcpb:https://github.com/org/repo/txg-node.mcpb";
        assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "txg-node");
    }

    #[test]
    fn test_derive_name_from_mcpb_url_with_query() {
        let url = "https://example.com/server.mcpb?version=1.0";
        assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "server");
    }

    #[test]
    fn test_derive_name_from_mcpb_url_trailing_slash() {
        // Edge case: trailing slash after .mcpb (unusual but handle gracefully)
        let url = "https://example.com/my-tool.mcpb/";
        // After stripping trailing slash, we get "my-tool.mcpb" as segment
        assert_eq!(derive_name_from_mcpb_url(url).unwrap(), "my-tool");
    }

    #[test]
    fn test_derive_name_from_mcpb_url_invalid_no_extension() {
        let url = "https://example.com/server";
        assert!(derive_name_from_mcpb_url(url).is_err());
    }

    #[test]
    fn test_derive_name_from_mcpb_url_invalid_empty_name() {
        let url = "https://example.com/.mcpb";
        assert!(derive_name_from_mcpb_url(url).is_err());
    }
}
