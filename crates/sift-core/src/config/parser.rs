//! TOML parser with helpful error messages

use super::schema::SiftConfig;
use anyhow::{Context, Result};
use std::path::Path;

/// Parse sift.toml with detailed error messages
pub fn parse_sift_toml(path: &Path) -> Result<SiftConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    parse_sift_toml_str(&content).with_context(|| {
        format!("Failed to parse config file: {}", path.display())
    })
}

/// Parse sift.toml content from string
pub fn parse_sift_toml_str(content: &str) -> Result<SiftConfig> {
    let config: SiftConfig = toml::from_str(content)
        .map_err(|e| enhance_toml_error(e, content))?;

    // Validate configuration
    validate_config(&config)?;

    Ok(config)
}

/// Enhance TOML parsing errors with helpful context
fn enhance_toml_error(error: toml::de::Error, content: &str) -> anyhow::Error {
    let error_msg = error.to_string();

    // Try to extract line number from error message
    let line_hint = error_msg
        .lines()
        .find(|line| line.contains("line "))
        .and_then(|line| {
            line.split("line ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<usize>().ok())
        });

    if let Some(line_num) = line_hint {
        let context = get_line_context(content, line_num);
        anyhow::anyhow!(
            "TOML parsing error at line {}:\n{}\n\nError: {}",
            line_num,
            context,
            error_msg
        )
    } else {
        anyhow::anyhow!("TOML parsing error: {}", error_msg)
    }
}

/// Get context lines around an error
fn get_line_context(content: &str, line_num: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = line_num.saturating_sub(2);
    let end = (line_num + 2).min(lines.len());

    lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let num = start + i + 1;
            let marker = if num == line_num { ">>>" } else { "   " };
            format!("{} {:4} | {}", marker, num, line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Validate configuration after parsing
fn validate_config(config: &SiftConfig) -> Result<()> {
    config.validate()?;
    Ok(())
}

/// Serialize a configuration to TOML string
pub fn to_toml(config: &SiftConfig) -> Result<String> {
    toml::to_string_pretty(config)
        .with_context(|| "Failed to serialize configuration to TOML")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_valid_config() {
        let toml = r#"
[mcp.postgres]
source = "registry:postgres-mcp"
runtime = "docker"
args = ["--readonly"]

[skill.pdf-processing]
source = "registry:anthropic/pdf"
version = "^1.0"
targets = ["claude-code"]
"#;

        let config = parse_sift_toml_str(toml).unwrap();
        assert_eq!(config.mcp.len(), 1);
        assert_eq!(config.skill.len(), 1);
        assert!(config.mcp.contains_key("postgres"));
        assert!(config.skill.contains_key("pdf-processing"));
    }

    #[test]
    fn test_parse_empty_config() {
        let config = parse_sift_toml_str("").unwrap();
        assert_eq!(config.mcp.len(), 0);
        assert_eq!(config.skill.len(), 0);
    }

    #[test]
    fn test_parse_invalid_toml() {
        let toml = r#"
[mcp.postgres
source = "registry:postgres-mcp"
"#; // Missing closing bracket

        let result = parse_sift_toml_str(toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_config_with_projects() {
        let toml = r#"
[mcp.test-mcp]
source = "registry:test"
runtime = "node"

[projects."/Users/test/project"]
mcp_override = []

[projects."/Users/test/project".mcp.test-mcp]
runtime = "docker"
"#;

        let config = parse_sift_toml_str(toml).unwrap();
        assert!(config.is_global());
        assert_eq!(config.projects.len(), 1);
    }

    #[test]
    fn test_validate_config_with_both_targets_and_ignore() {
        let toml = r#"
[mcp.invalid]
source = "registry:test"
runtime = "node"
targets = ["claude-desktop"]
ignore_targets = ["vscode"]
"#;

        let result = parse_sift_toml_str(toml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // The error should mention the validation failure
        assert!(err.contains("Cannot specify both") || err.contains("Invalid MCP server"));
    }

    #[test]
    fn test_to_toml_roundtrip() {
        let mut original = SiftConfig::new();

        original.mcp.insert(
            "postgres".to_string(),
            crate::config::schema::McpConfigEntry {
                transport: "stdio".to_string(),
                source: "registry:postgres-mcp".to_string(),
                runtime: "docker".to_string(),
                args: vec!["--readonly".to_string()],
                url: None,
                headers: HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: {
                    let mut map = HashMap::new();
                    map.insert("DB_URL".to_string(), "postgres://localhost".to_string());
                    map
                },
            },
        );

        original.skill.insert(
            "pdf".to_string(),
            crate::config::schema::SkillConfigEntry {
                source: "registry:anthropic/pdf".to_string(),
                version: "^1.0".to_string(),
                targets: Some(vec!["claude-code".to_string()]),
                ignore_targets: None,
            },
        );

        let toml_str = to_toml(&original).unwrap();
        let parsed = parse_sift_toml_str(&toml_str).unwrap();

        assert_eq!(parsed.mcp.len(), original.mcp.len());
        assert_eq!(parsed.skill.len(), original.skill.len());
        assert_eq!(parsed.mcp["postgres"].source, original.mcp["postgres"].source);
        assert_eq!(parsed.skill["pdf"].source, original.skill["pdf"].source);
    }

    #[test]
    fn test_parse_from_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"
[mcp.test]
source = "registry:test"
runtime = "node"
"#
        )
        .unwrap();

        let config = parse_sift_toml(temp_file.path()).unwrap();
        assert_eq!(config.mcp.len(), 1);
        assert!(config.mcp.contains_key("test"));
    }

    #[test]
    fn test_parse_nonexistent_file() {
        let result = parse_sift_toml(Path::new("/nonexistent/path/sift.toml"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Failed to read config file"));
    }

    #[test]
    fn test_enhance_toml_error() {
        let toml = "invalid = [unclosed";
        let result = parse_sift_toml_str(toml);
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        // Error should mention line number
        assert!(err.contains("line ") || err.contains("TOML parsing error"));
    }

    #[test]
    fn test_parse_with_all_sections() {
        let toml = r#"
[mcp.server1]
source = "registry:server1"
runtime = "node"
args = ["--arg1"]

[skill.skill1]
source = "registry:skill1"
version = "1.0.0"

[clients.claude-desktop]
enabled = true
fs_strategy = "auto"

[registry.official]
type = "sift"
url = "https://registry.sift.sh/v1"

[registry.company]
type = "claude-marketplace"
source = "github:company/plugins"
"#;

        let config = parse_sift_toml_str(toml).unwrap();
        assert_eq!(config.mcp.len(), 1);
        assert_eq!(config.skill.len(), 1);
        assert_eq!(config.clients.len(), 1);
        assert_eq!(config.registry.len(), 2);
    }
}
