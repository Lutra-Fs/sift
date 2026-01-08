//! Configuration layer merging logic
//!
//! Implements the 3-layer merge strategy:
//! Global -> Project -> Project-Local

use super::schema::{McpConfigEntry, ProjectOverride, SiftConfig, SkillConfigEntry};
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
        merge_sift_config(&mut merged, proj)?;
    }

    // Extract and apply project-local override from global
    let project_path_buf = project_path.to_path_buf();
    let override_config = merged.get_project_override(&project_path_buf).cloned();
    if let Some(override_config) = override_config {
        apply_project_override(&mut merged, &override_config)?;
    }

    // Remove the projects section after applying overrides
    // (it doesn't belong in the merged config)
    merged.projects.clear();

    Ok(merged)
}

/// Merge two SiftConfig instances
fn merge_sift_config(base: &mut SiftConfig, layer: SiftConfig) -> anyhow::Result<()> {
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

    // Merge project overrides (only in global config)
    for (key, override_config) in layer.projects {
        base.projects.entry(key).or_insert(override_config);
    }

    Ok(())
}

/// Merge MCP config entries
fn merge_mcp_entry(base: &mut McpConfigEntry, overlay: McpConfigEntry) {
    // Warn on transport mismatch (but allow override, like Claude Code)
    if base.transport != overlay.transport {
        eprintln!(
            "Warning: MCP config '{}' transport mismatch (base: {}, overlay: {})\n\
             STDIO and HTTP are fundamentally different configurations. Using overlay transport: {}",
            base.source, base.transport, overlay.transport, overlay.transport
        );
    }

    // For STDIO, validate runtime compatibility
    if base.transport == "stdio" && overlay.transport == "stdio" {
        let base_runtime = crate::mcp::RuntimeType::try_from(base.runtime.as_str());
        let overlay_runtime = crate::mcp::RuntimeType::try_from(overlay.runtime.as_str());

        if let (Ok(base_rt), Ok(overlay_rt)) = (base_runtime, overlay_runtime)
            && !base_rt.is_compatible_with(&overlay_rt)
        {
            eprintln!(
                "Warning: MCP config '{}' runtime change (base: {}, overlay: {})\n\
                 Incompatible runtime change detected.\n\
                 Compatible: Node ↔ Bun. Incompatible: Docker, Python, Shell, and others.",
                base.source, base.runtime, overlay.runtime
            );
        }
    }

    // Protect non-default runtime from being overridden by default
    // Only override if: overlay is non-default OR base is already default
    if overlay.runtime != "node" || base.runtime == "node" {
        base.runtime = overlay.runtime;
    }

    // Similar protection for transport
    if overlay.transport != "stdio" || base.transport == "stdio" {
        base.transport = overlay.transport;
    }

    // Rest of merge logic (source, args, url, etc.)
    if !overlay.source.is_empty() {
        base.source = overlay.source;
    }
    if !overlay.args.is_empty() {
        base.args = overlay.args;
    }
    if overlay.url.is_some() {
        base.url = overlay.url;
    }
    if overlay.targets.is_some() {
        base.targets = overlay.targets;
    }
    if overlay.ignore_targets.is_some() {
        base.ignore_targets = overlay.ignore_targets;
    }
    // Deep merge headers
    for (key, value) in overlay.headers {
        base.headers.insert(key, value);
    }
    // Deep merge env vars
    for (key, value) in overlay.env {
        base.env.insert(key, value);
    }
}

/// Merge skill config entries
fn merge_skill_entry(base: &mut SkillConfigEntry, overlay: SkillConfigEntry) {
    // Protect non-default version from being overridden by default
    // Only override if: overlay is non-default OR base is already default
    if overlay.version != "latest" || base.version == "latest" {
        base.version = overlay.version;
    }

    // Rest of merge logic
    if !overlay.source.is_empty() {
        base.source = overlay.source;
    }
    if overlay.targets.is_some() {
        base.targets = overlay.targets;
    }
    if overlay.ignore_targets.is_some() {
        base.ignore_targets = overlay.ignore_targets;
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
    if overlay.link_mode.is_some() {
        base.link_mode = overlay.link_mode;
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
            apply_mcp_override(existing, mcp_override);
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
fn apply_mcp_override(base: &mut McpConfigEntry, override_config: &crate::config::schema::McpOverrideEntry) {
    if let Some(runtime) = &override_config.runtime {
        base.runtime = runtime.clone();
    }
    // Merge env vars
    for (key, value) in &override_config.env {
        base.env.insert(key.clone(), value.clone());
    }
}

/// Apply skill override to an existing entry
fn apply_skill_override(
    base: &mut SkillConfigEntry,
    override_config: &crate::config::schema::SkillOverrideEntry,
) {
    if let Some(version) = &override_config.version {
        base.version = version.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_mcp_entry(source: &str, runtime: &str) -> McpConfigEntry {
        McpConfigEntry {
            transport: "stdio".to_string(),
            source: source.to_string(),
            runtime: runtime.to_string(),
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        }
    }

    fn create_skill_entry(source: &str, version: &str) -> SkillConfigEntry {
        SkillConfigEntry {
            source: source.to_string(),
            version: version.to_string(),
            targets: None,
            ignore_targets: None,
        }
    }

    #[test]
    fn test_merge_configs_no_global() {
        let mut project = SiftConfig::new();
        project
            .mcp
            .insert("test-mcp".to_string(), create_mcp_entry("registry:test", "node"));

        let merged = merge_configs(None, Some(project), Path::new("/test/project")).unwrap();

        assert_eq!(merged.mcp.len(), 1);
        assert!(merged.mcp.contains_key("test-mcp"));
    }

    #[test]
    fn test_merge_configs_no_project() {
        let mut global = SiftConfig::new();
        global
            .mcp
            .insert("test-mcp".to_string(), create_mcp_entry("registry:test", "node"));

        let merged =
            merge_configs(Some(global), None, Path::new("/test/project")).unwrap();

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
            transport: "stdio".to_string(),
            source: "registry:base".to_string(),
            runtime: "node".to_string(),
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
        };

        let overlay = McpConfigEntry {
            transport: "stdio".to_string(),
            source: "registry:overlay".to_string(),
            runtime: "docker".to_string(),
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
        };

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.source, "registry:overlay");
        assert_eq!(base.runtime, "docker");
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
            version: "^1.0".to_string(),
            targets: None,
            ignore_targets: None,
        };

        let overlay = SkillConfigEntry {
            source: "registry:overlay".to_string(),
            version: "^2.0".to_string(),
            targets: Some(vec!["claude-code".to_string()]),
            ignore_targets: None,
        };

        merge_skill_entry(&mut base, overlay);

        assert_eq!(base.source, "registry:overlay");
        assert_eq!(base.version, "^2.0");
        assert!(base.targets.is_some());
    }

    #[test]
    fn test_merge_mcp_entry_preserve_docker_runtime() {
        let mut base = create_mcp_entry("registry:postgres", "docker");
        let overlay = create_mcp_entry("registry:postgres", "node"); // default

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.runtime, "docker"); // Should preserve docker
    }

    #[test]
    fn test_merge_mcp_entry_allow_compatible_runtime_swap() {
        let mut base = create_mcp_entry("registry:test", "node");
        let mut overlay = create_mcp_entry("registry:test", "bun");
        overlay.source = String::new(); // Don't override source

        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.runtime, "bun"); // Should allow node→bun
    }

    #[test]
    fn test_merge_mcp_entry_warn_on_incompatible_runtime_swap() {
        let mut base = create_mcp_entry("registry:test", "docker");
        let overlay = create_mcp_entry("registry:test", "python"); // non-default

        // Should warn but allow
        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.runtime, "python"); // Override happens for non-default overlay
    }

    #[test]
    fn test_merge_mcp_entry_warn_on_transport_mismatch() {
        let mut base = McpConfigEntry {
            transport: "stdio".to_string(),
            source: "registry:test".to_string(),
            runtime: "node".to_string(),
            args: vec![],
            url: None,
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        };
        let overlay = McpConfigEntry {
            transport: "http".to_string(),
            source: "https://example.com/mcp".to_string(),
            runtime: "node".to_string(),
            args: vec![],
            url: Some("https://example.com/mcp".to_string()),
            headers: HashMap::new(),
            targets: None,
            ignore_targets: None,
            env: HashMap::new(),
        };

        // Should warn but allow
        merge_mcp_entry(&mut base, overlay);

        assert_eq!(base.transport, "http"); // Override happens despite warning
        assert_eq!(base.url, Some("https://example.com/mcp".to_string()));
    }

    #[test]
    fn test_merge_skill_entry_preserve_pinned_version() {
        let mut base = create_skill_entry("registry:test", "^1.0");
        let overlay = create_skill_entry("registry:test", "latest"); // default

        merge_skill_entry(&mut base, overlay);

        assert_eq!(base.version, "^1.0"); // Should preserve pinned version
    }

    #[test]
    fn test_apply_project_override() {
        let mut config = SiftConfig::new();
        config.mcp.insert(
            "test-mcp".to_string(),
            McpConfigEntry {
                transport: "stdio".to_string(),
                source: "registry:test".to_string(),
                runtime: "node".to_string(),
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
        assert_eq!(mcp.runtime, "docker");
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
                transport: "stdio".to_string(),
                source: "registry:test".to_string(),
                runtime: "node".to_string(),
                args: vec![],
                url: None,
                headers: HashMap::new(),
                targets: None,
                ignore_targets: None,
                env: HashMap::new(),
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
        assert_eq!(mcp.runtime, "docker");
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
        global.projects.insert(
            "/test/project".to_string(),
            ProjectOverride::default(),
        );

        let merged = merge_configs(Some(global), None, Path::new("/test/project")).unwrap();

        // Projects section should be cleared after applying overrides
        assert!(merged.projects.is_empty());
    }
}
