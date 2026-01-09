//! Configuration layer merging logic
//!
//! Implements the 3-layer merge strategy:
//! Global -> Project -> Project-Local

use super::schema::{McpConfigEntry, ProjectOverride, SiftConfig, SkillConfigEntry};
use anyhow::Context;
use std::path::Path;

/// Merge multiple configuration layers
///
/// # Arguments
/// * `global` - Global configuration from ~/.config/sift/sift.toml
/// * `project` - Project configuration from ./sift.toml
/// * `project_path` - Absolute path to the project root
///
/// # Returns
/// Merged configuration with project-local overrides applied
pub fn merge_configs(
    global: Option<SiftConfig>,
    project: Option<SiftConfig>,
    project_path: &Path,
) -> anyhow::Result<SiftConfig> {
    // Start with global config as base
    let mut merged = global.unwrap_or_default();

    // Merge project config
    if let Some(proj) = project {
        merge_sift_config(&mut merged, proj, false)?; // is_global = false
    }

    // Extract and apply project-local override from global
    let project_path_buf = project_path.to_path_buf();
    let override_config_to_apply = merged
        .get_project_override(&project_path_buf)
        .map(|(_k, v)| v.clone());
    if let Some(override_config) = override_config_to_apply {
        apply_project_override(&mut merged, &override_config)?;
    }

    // Remove the projects section after applying overrides
    // (it doesn't belong in the merged config)
    merged.projects.clear();

    Ok(merged)
}

/// Merge two SiftConfig instances
fn merge_sift_config(
    base: &mut SiftConfig,
    layer: SiftConfig,
    is_global_layer: bool,
) -> anyhow::Result<()> {
    // Merge MCP configs (deep merge for env vars)
    for (key, entry) in layer.mcp {
        base.mcp
            .entry(key)
            .and_modify(|existing| merge_mcp_entry(existing, entry.clone()))
            .or_insert(entry);
    }

    // Merge skill configs
    for (key, entry) in layer.skill {
        base.skill
            .entry(key)
            .and_modify(|existing| merge_skill_entry(existing, entry.clone()))
            .or_insert(entry);
    }

    // Merge global link mode
    if layer.link_mode.is_some() {
        base.link_mode = layer.link_mode;
    }

    // Merge clients
    for (key, entry) in layer.clients {
        base.clients
            .entry(key)
            .and_modify(|existing| merge_client_entry(existing, entry.clone()))
            .or_insert(entry);
    }

    // Merge registries (additive)
    for (key, entry) in layer.registry {
        base.registry.entry(key).or_insert(entry);
    }

    // ONLY merge projects from global layer
    if !layer.projects.is_empty() {
        if is_global_layer {
            for (key, override_config) in layer.projects {
                base.projects.entry(key).or_insert(override_config);
            }
        } else {
            // Warn but don't fail - ignore projects section from project layers
            eprintln!(
                "Warning: [projects.*] section found in project config (./sift.toml). \
                 This section is only allowed in global config (~/.config/sift/sift.toml). \
                 The projects section will be ignored."
            );
        }
    }

    Ok(())
}

/// Merge MCP config entries
fn merge_mcp_entry(base: &mut McpConfigEntry, overlay: McpConfigEntry) {
    // Handle reset flags first
    if overlay.reset_targets {
        base.targets = None;
    }
    if overlay.reset_ignore_targets {
        base.ignore_targets = None;
    }
    if overlay.reset_env_all {
        base.env.clear();
    } else if let Some(keys) = overlay.reset_env {
        for key in keys {
            base.env.remove(&key);
        }
    }

    // Only override runtime/transport if overlay explicitly set a value (Some)
    // Don't protect default values with Option wrapper - we want explicit override
    if let Some(overlay_runtime) = overlay.runtime {
        base.runtime = Some(overlay_runtime);
    }
    if let Some(overlay_transport) = overlay.transport {
        base.transport = Some(overlay_transport);
    }
    if let Some(overlay_targets) = overlay.targets {
        base.targets = Some(overlay_targets);
    }
    if let Some(overlay_ignore) = overlay.ignore_targets {
        base.ignore_targets = Some(overlay_ignore);
    }

    // Rest of merge logic (source, args, url, headers, env)
    if !overlay.source.is_empty() {
        base.source = overlay.source;
    }
    if !overlay.args.is_empty() {
        base.args = overlay.args;
    }
    if overlay.url.is_some() {
        base.url = overlay.url;
    }
    // Deep merge headers
    for (key, value) in overlay.headers {
        base.headers.insert(key, value);
    }
    // Deep merge env vars (only if not reset_env_all)
    if !overlay.reset_env_all {
        for (key, value) in overlay.env {
            base.env.insert(key, value);
        }
    }
}

/// Merge skill config entries
fn merge_skill_entry(base: &mut SkillConfigEntry, overlay: SkillConfigEntry) {
    // Handle reset flag first
    if overlay.reset_version {
        base.version = None;
    }

    // Only override if overlay explicitly set a value (Some)
    // Don't protect default values with Option wrapper - we want explicit override
    if let Some(overlay_version) = overlay.version {
        base.version = Some(overlay_version);
    }
    if let Some(overlay_targets) = overlay.targets {
        base.targets = Some(overlay_targets);
    }
    if let Some(overlay_ignore) = overlay.ignore_targets {
        base.ignore_targets = Some(overlay_ignore);
    }

    // Rest of merge logic
    if !overlay.source.is_empty() {
        base.source = overlay.source;
    }
}

/// Merge client config entries
fn merge_client_entry(
    base: &mut crate::config::schema::ClientConfigEntry,
    overlay: crate::config::schema::ClientConfigEntry,
) {
    // Overlay values override base
    base.enabled = overlay.enabled;
    if overlay.source.is_some() {
        base.source = overlay.source;
    }
    if overlay.capabilities.is_some() {
        base.capabilities = overlay.capabilities;
    }
}

/// Apply project-local override from global config
fn apply_project_override(
    base: &mut SiftConfig,
    override_config: &ProjectOverride,
) -> anyhow::Result<()> {
    // Apply MCP overrides (only env and runtime)
    for (key, mcp_override) in &override_config.mcp {
        if let Some(existing) = base.mcp.get_mut(key) {
            apply_mcp_override(existing, mcp_override)
                .with_context(|| format!("Invalid MCP override for '{key}'"))?;
        }
    }

    // Apply skill overrides
    for (key, skill_override) in &override_config.skill {
        if let Some(existing) = base.skill.get_mut(key) {
            apply_skill_override(existing, skill_override);
        }
    }

    Ok(())
}

/// Apply MCP override to an existing entry
fn apply_mcp_override(
    base: &mut McpConfigEntry,
    override_config: &crate::config::schema::McpOverrideEntry,
) -> anyhow::Result<()> {
    if let Some(runtime) = &override_config.runtime {
        crate::mcp::RuntimeType::try_from(runtime.as_str())
            .with_context(|| format!("Invalid runtime override: '{runtime}'"))?;
        base.runtime = Some(runtime.clone());
    }
    // Merge env vars
    for (key, value) in &override_config.env {
        base.env.insert(key.clone(), value.clone());
    }

    Ok(())
}

/// Apply skill override to an existing entry
fn apply_skill_override(
    base: &mut SkillConfigEntry,
    override_config: &crate::config::schema::SkillOverrideEntry,
) {
    if let Some(version) = &override_config.version {
        base.version = Some(version.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_mcp_entry(source: &str, runtime: &str) -> McpConfigEntry {
        McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: source.to_string(),
            runtime: Some(runtime.to_string()),
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        }
    }

    fn create_skill_entry(source: &str, version: &str) -> SkillConfigEntry {
        SkillConfigEntry {
            source: source.to_string(),
            version: Some(version.to_string()),
            targets: None,
            ignore_targets: None,
            reset_version: false,
        }
    }

    #[test]
    fn test_merge_configs_no_global() {
        let mut project = SiftConfig::new();
        project.mcp.insert(
            "test-mcp".to_string(),
            create_mcp_entry("registry:test", "node"),
        );

        let merged = merge_configs(None, Some(project), Path::new("/test/project")).unwrap();

        assert_eq!(merged.mcp.len(), 1);
        assert!(merged.mcp.contains_key("test-mcp"));
    }

    #[test]
    fn test_merge_configs_no_project() {
        let mut global = SiftConfig::new();
        global.mcp.insert(
            "test-mcp".to_string(),
            create_mcp_entry("registry:test", "node"),
        );

        let merged = merge_configs(Some(global), None, Path::new("/test/project")).unwrap();

        assert_eq!(merged.mcp.len(), 1);
        assert!(merged.mcp.contains_key("test-mcp"));
    }

    #[test]
    fn test_merge_configs_both_layers() {
        let mut global = SiftConfig::new();
        global.mcp.insert(
            "base-mcp".to_string(),
            create_mcp_entry("registry:base", "node"),
        );
        global.skill.insert(
            "base-skill".to_string(),
            create_skill_entry("registry:base", "^1.0"),
        );

        let mut project = SiftConfig::new();
        project.mcp.insert(
            "project-mcp".to_string(),
            create_mcp_entry("registry:project", "docker"),
        );
        project.skill.insert(
            "project-skill".to_string(),
            create_skill_entry("registry:project", "^2.0"),
        );

        let merged =
            merge_configs(Some(global), Some(project), Path::new("/test/project")).unwrap();

        assert_eq!(merged.mcp.len(), 2);
        assert_eq!(merged.skill.len(), 2);
        assert!(merged.mcp.contains_key("base-mcp"));
        assert!(merged.mcp.contains_key("project-mcp"));
    }

    #[test]
    fn test_merge_mcp_entry() {
        let mut base = McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: "registry:base".to_string(),
            runtime: Some("node".to_string()),
            args: vec!["--arg1".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("BASE_VAR".to_string(), "base_value".to_string());
                map
            },
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        let overlay = McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: "registry:overlay".to_string(),
            runtime: Some("docker".to_string()),
            args: vec!["--arg2".to_string()],
            url: None,
            headers: HashMap::new(),
            targets: Some(vec!["claude-desktop".to_string()]),
            ignore_targets: None,
            env: {
                let mut map = HashMap::new();
                map.insert("OVERLAY_VAR".to_string(), "overlay_value".to_string());
                map
            },
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.source, "registry:overlay");
        assert_eq!(base.runtime, Some("docker".to_string()));
        assert_eq!(base.args, vec!["--arg2".to_string()]);
        assert!(base.targets.is_some());
        assert_eq!(base.env.len(), 2);
        assert!(base.env.contains_key("BASE_VAR"));
        assert!(base.env.contains_key("OVERLAY_VAR"));
    }

    #[test]
    fn test_merge_skill_entry() {
        let mut base = SkillConfigEntry {
            source: "registry:base".to_string(),
            version: Some("^1.0".to_string()),
            targets: None,
            ignore_targets: None,
            reset_version: false,
        };

        let overlay = SkillConfigEntry {
            source: "registry:overlay".to_string(),
            version: Some("^2.0".to_string()),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
            reset_version: false,
        };

        merge_skill_entry(&mut base, overlay);

        assert_eq!(base.source, "registry:overlay");
        assert_eq!(base.version, Some("^2.0".to_string()));
        assert!(base.targets.is_some());
    }

    #[test]
    fn test_merge_mcp_entry_explicit_override() {
        // With Option wrapper, explicit values always override
        let mut base = create_mcp_entry("registry:postgres", "docker");
        let overlay = create_mcp_entry("registry:postgres", "node");

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.runtime, Some("node".to_string())); // Overlay's explicit value wins
    }

    #[test]
    fn test_merge_mcp_entry_allow_compatible_runtime_swap() {
        let mut base = create_mcp_entry("registry:test", "node");
        let mut overlay = create_mcp_entry("registry:test", "bun");
        overlay.source = String::new(); // Don't override source

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.runtime, Some("bun".to_string())); // Explicit bun wins
    }

    #[test]
    fn test_merge_mcp_entry_explicit_non_default_override() {
        let mut base = create_mcp_entry("registry:test", "docker");
        let overlay = create_mcp_entry("registry:test", "python"); // non-default

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.runtime, Some("python".to_string())); // Explicit python wins
    }

    #[test]
    fn test_merge_mcp_entry_warn_on_transport_mismatch() {
        let mut base = McpConfigEntry {
            transport: Some("stdio".to_string()),
            source: "registry:test".to_string(),
            runtime: Some("node".to_string()),
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };
        let overlay = McpConfigEntry {
            transport: Some("http".to_string()),
            source: "https://example.com/mcp".to_string(),
            runtime: Some("node".to_string()),
            args: vec![],
            url: Some("https://example.com/mcp".to_string()),
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
            reset_targets: false,
            reset_ignore_targets: false,
            reset_env: None,
            reset_env_all: false,
        };

        // Should warn but allow
        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.transport, Some("http".to_string())); // Override happens despite warning
        assert_eq!(base.url, Some("https://example.com/mcp".to_string()));
    }

    #[test]
    fn test_merge_skill_entry_explicit_override() {
        // With Option wrapper, explicit values always override
        let mut base = create_skill_entry("registry:test", "^1.0");
        let overlay = create_skill_entry("registry:test", "latest"); // explicit "latest"

        merge_skill_entry(&mut base, overlay);

        assert_eq!(base.version, Some("latest".to_string())); // Overlay's explicit value wins
    }

    #[test]
    fn test_apply_project_override() {
        let mut config = SiftConfig::new();
        config.mcp.insert(
            "test-mcp".to_string(),
            McpConfigEntry {
                transport: Some("stdio".to_string()),
                source: "registry:test".to_string(),
                runtime: Some("node".to_string()),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: {
                    let mut map = HashMap::new();
                    map.insert("BASE_VAR".to_string(), "base_value".to_string());
                    map
                },
                reset_targets: false,
                reset_ignore_targets: false,
                reset_env: None,
                reset_env_all: false,
            },
        );

        let mut override_config = ProjectOverride::default();
        override_config.mcp.insert(
            "test-mcp".to_string(),
            crate::config::schema::McpOverrideEntry {
                runtime: Some("docker".to_string()),
                env: {
                    let mut map = HashMap::new();
                    map.insert("OVERRIDE_VAR".to_string(), "override_value".to_string());
                    map
                },
            },
        );

        apply_project_override(&mut config, &override_config).unwrap();

        let mcp = config.mcp.get("test-mcp").unwrap();
        assert_eq!(mcp.runtime, Some("docker".to_string()));
        assert_eq!(mcp.env.len(), 2);
        assert!(mcp.env.contains_key("BASE_VAR"));
        assert!(mcp.env.contains_key("OVERRIDE_VAR"));
    }

    #[test]
    fn test_merge_configs_with_project_override() {
        let mut global = SiftConfig::new();
        global.mcp.insert(
            "test-mcp".to_string(),
            McpConfigEntry {
                transport: Some("stdio".to_string()),
                source: "registry:test".to_string(),
                runtime: Some("node".to_string()),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: HashMap::new(),
                reset_targets: false,
                reset_ignore_targets: false,
                reset_env: None,
                reset_env_all: false,
            },
        );

        let project_path = Path::new("/test/project");
        let mut override_config = ProjectOverride {
            path: project_path.to_path_buf(),
            ..Default::default()
        };
        override_config.mcp.insert(
            "test-mcp".to_string(),
            crate::config::schema::McpOverrideEntry {
                runtime: Some("docker".to_string()),
                env: HashMap::new(),
            },
        );

        global
            .projects
            .insert(project_path.to_string_lossy().to_string(), override_config);

        let merged = merge_configs(Some(global), None, project_path).unwrap();

        let mcp = merged.mcp.get("test-mcp").unwrap();
        assert_eq!(mcp.runtime, Some("docker".to_string()));
    }

    #[test]
    fn test_merge_registries_additive() {
        let mut global = SiftConfig::new();
        global.registry.insert(
            "official".to_string(),
            crate::config::schema::RegistryConfigEntry {
                r#type: "sift".to_string(),
                url: Some("https://registry.sift.sh".to_string()),
                source: None,
            },
        );

        let mut project = SiftConfig::new();
        project.registry.insert(
            "company".to_string(),
            crate::config::schema::RegistryConfigEntry {
                r#type: "claude-marketplace".to_string(),
                url: None,
                source: Some("github:company/plugins".to_string()),
            },
        );

        let merged =
            merge_configs(Some(global), Some(project), Path::new("/test/project")).unwrap();

        assert_eq!(merged.registry.len(), 2);
        assert!(merged.registry.contains_key("official"));
        assert!(merged.registry.contains_key("company"));
    }

    #[test]
    fn test_projects_cleared_after_merge() {
        let mut global = SiftConfig::new();
        global
            .projects
            .insert("/test/project".to_string(), ProjectOverride::default());

        let merged = merge_configs(Some(global), None, Path::new("/test/project")).unwrap();

        // Projects section should be cleared after applying overrides
        assert!(merged.projects.is_empty());
    }

    #[test]
    fn test_apply_project_override_rejects_invalid_runtime() {
        let mut config = SiftConfig::new();
        config.mcp.insert(
            "test-mcp".to_string(),
            create_mcp_entry("registry:test", "node"),
        );

        let mut override_config = ProjectOverride::default();
        override_config.mcp.insert(
            "test-mcp".to_string(),
            crate::config::schema::McpOverrideEntry {
                runtime: Some("doker".to_string()),
                env: HashMap::new(),
            },
        );

        let result = apply_project_override(&mut config, &override_config);
        assert!(
            result.is_err(),
            "apply_project_override() must fail for invalid override runtime"
        );
    }
}
