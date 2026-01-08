//! Fetch plugin.json from GitHub repositories
//!
//! Provides utilities to fetch nested plugin.json files from GitHub repos
//! for marketplace plugins that use the life-sciences format.

use anyhow::Context;
use serde_json::Value;

/// Fetches plugin.json from GitHub repositories
pub struct GitHubFetcher;

impl GitHubFetcher {
    /// Construct raw.githubusercontent.com URL for plugin.json
    ///
    /// # Arguments
    /// * `marketplace_source` - "github:anthropics/life-sciences"
    /// * `plugin_source` - "./10x-genomics" (relative path from marketplace root)
    /// * `ref_` - Optional git ref (branch, tag, defaults to "main")
    ///
    /// # Example
    /// ```ignore
    /// let url = GitHubFetcher::construct_plugin_json_url(
    ///     "github:anthropics/life-sciences",
    ///     "./10x-genomics",
    ///     None,
    /// ).unwrap();
    /// // Returns: "https://raw.githubusercontent.com/anthropics/life-sciences/main/10x-genomics/.claude-plugin/plugin.json"
    /// ```
    pub fn construct_plugin_json_url(
        marketplace_source: &str,
        plugin_source: &str,
        ref_: Option<&str>,
    ) -> anyhow::Result<String> {
        // Parse "github:anthropics/life-sciences" -> ("anthropics", "life-sciences")
        let repo_path = marketplace_source
            .strip_prefix("github:")
            .ok_or_else(|| anyhow::anyhow!("Invalid GitHub source: {}", marketplace_source))?;

        let (owner, repo) = Self::parse_github_repo(repo_path)?;

        // Normalize plugin source path
        // "./10x-genomics" -> "10x-genomics"
        let plugin_path = plugin_source.strip_prefix("./").unwrap_or(plugin_source);

        // Use "main" as default branch
        let git_ref = ref_.unwrap_or("main");

        // Construct: https://raw.githubusercontent.com/anthropics/life-sciences/main/10x-genomics/.claude-plugin/plugin.json
        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}/.claude-plugin/plugin.json",
            owner, repo, git_ref, plugin_path
        );

        Ok(url)
    }

    /// Fetch and parse plugin.json from GitHub (async)
    ///
    /// # Arguments
    /// * `marketplace_source` - "github:anthropics/life-sciences"
    /// * `plugin_source` - "./10x-genomics" (relative path from marketplace root)
    /// * `ref_` - Optional git ref (branch, tag, defaults to "main")
    ///
    /// # Returns
    /// Parsed JSON from plugin.json
    ///
    /// # Example
    /// ```ignore
    /// let plugin_json = GitHubFetcher::fetch_plugin_json(
    ///     "github:anthropics/life-sciences",
    ///     "./10x-genomics",
    ///     None,
    /// ).await?;
    /// ```
    pub async fn fetch_plugin_json(
        marketplace_source: &str,
        plugin_source: &str,
        ref_: Option<&str>,
    ) -> anyhow::Result<Value> {
        let url = Self::construct_plugin_json_url(marketplace_source, plugin_source, ref_)?;

        let client = reqwest::Client::builder()
            .user_agent("sift/0.1.0")
            .build()
            .context("Failed to build HTTP client")?;

        let response = client
            .get(&url)
            .send()
            .await
            .context(format!("Failed to fetch plugin.json from {}", url))?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch plugin.json: HTTP {} from {}",
                response.status(),
                url
            );
        }

        let json: Value = response
            .json()
            .await
            .context("Failed to parse plugin.json response")?;

        Ok(json)
    }

    /// Parse "owner/repo" or "owner/repo@ref" into components
    fn parse_github_repo(path: &str) -> anyhow::Result<(String, String)> {
        let repo_parts: Vec<&str> = path.split('@').collect();
        let repo_part = repo_parts[0];
        let parts: Vec<&str> = repo_part.split('/').collect();

        if parts.len() != 2 {
            anyhow::bail!("Invalid GitHub repo format: {}", path);
        }

        Ok((parts[0].to_string(), parts[1].to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_construct_plugin_json_url() {
        let url = GitHubFetcher::construct_plugin_json_url(
            "github:anthropics/life-sciences",
            "./10x-genomics",
            None,
        )
        .unwrap();

        assert_eq!(
            url,
            "https://raw.githubusercontent.com/anthropics/life-sciences/main/10x-genomics/.claude-plugin/plugin.json"
        );
    }

    #[test]
    fn test_construct_plugin_json_url_with_ref() {
        let url = GitHubFetcher::construct_plugin_json_url(
            "github:anthropics/life-sciences",
            "./10x-genomics",
            Some("v1.0.0"),
        )
        .unwrap();

        assert_eq!(
            url,
            "https://raw.githubusercontent.com/anthropics/life-sciences/v1.0.0/10x-genomics/.claude-plugin/plugin.json"
        );
    }

    #[test]
    fn test_construct_plugin_json_url_nested_path() {
        let url = GitHubFetcher::construct_plugin_json_url(
            "github:anthropics/life-sciences",
            "./category/subcategory/plugin-name",
            None,
        )
        .unwrap();

        assert_eq!(
            url,
            "https://raw.githubusercontent.com/anthropics/life-sciences/main/category/subcategory/plugin-name/.claude-plugin/plugin.json"
        );
    }

    #[test]
    fn test_construct_plugin_json_url_invalid_source() {
        let result = GitHubFetcher::construct_plugin_json_url("invalid:format", "./test", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_github_repo() {
        let (owner, repo) = GitHubFetcher::parse_github_repo("anthropics/skills").unwrap();
        assert_eq!(owner, "anthropics");
        assert_eq!(repo, "skills");
    }

    #[test]
    fn test_parse_github_repo_invalid() {
        let result = GitHubFetcher::parse_github_repo("invalid-format");
        assert!(result.is_err());
    }
}
