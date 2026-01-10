//! Status collection and verification for Sift-managed resources.
//!
//! This module provides APIs to collect the current state of:
//! - MCP servers (declared, locked, deployed)
//! - Skills (declared, locked, installed)
//! - Client configurations

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::claude_code::ClaudeCodeClient;
use crate::client::{ClientAdapter, ClientContext, PathRoot};
use crate::config::ownership_store::OwnershipStore;
use crate::config::schema::{McpConfigEntry, SkillConfigEntry};
use crate::config::{ConfigScope, SiftConfig};
use crate::fs::LinkMode;
use crate::version::lock::{LockedMcpServer, LockedSkill};
use crate::version::store::LockfileStore;

// =============================================================================
// Data Structures
// =============================================================================

/// Overall system status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub project_root: Option<PathBuf>,
    pub scope_filter: Option<ConfigScope>,
    pub link_mode: LinkMode,
    pub mcp_servers: Vec<McpServerStatus>,
    pub skills: Vec<SkillStatus>,
    pub clients: Vec<ClientStatus>,
    pub summary: StatusSummary,
}

/// Summary counts for quick overview
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSummary {
    pub total_mcp: usize,
    pub total_skills: usize,
    pub issues: usize,
}

/// MCP server status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub runtime: Option<String>,
    pub constraint: String,
    pub resolved_version: Option<String>,
    pub registry: String,
    pub scope: ConfigScope,
    pub source_file: PathBuf,
    pub state: EntryState,
    pub deployments: Vec<ClientDeployment>,
}

impl McpServerStatus {
    /// Compute aggregated integrity status from all deployments
    pub fn aggregated_integrity(&self) -> AggregatedIntegrity {
        let total = self.deployments.len();
        let ok_count = self
            .deployments
            .iter()
            .filter(|d| d.integrity == DeploymentIntegrity::Ok)
            .count();
        let not_deployed = self
            .deployments
            .iter()
            .filter(|d| d.integrity == DeploymentIntegrity::NotDeployed)
            .count();

        // Exclude NotDeployed from total (not expected to be deployed)
        let expected = total - not_deployed;

        match (ok_count, expected) {
            (o, e) if o == e && e > 0 => AggregatedIntegrity::AllOk(o),
            (o, e) if o > 0 && o < e => AggregatedIntegrity::Partial { ok: o, total: e },
            (0, e) if e > 0 => AggregatedIntegrity::AllFailed(e),
            _ => AggregatedIntegrity::NotApplicable,
        }
    }
}

/// Skill status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStatus {
    pub name: String,
    pub constraint: String,
    pub resolved_version: Option<String>,
    pub registry: String,
    pub scope: ConfigScope,
    pub source_file: PathBuf,
    pub state: EntryState,
    pub deployments: Vec<SkillDeployment>,
    pub mode: Option<LinkMode>,
    pub dst_path: Option<PathBuf>,
}

impl SkillStatus {
    /// Compute aggregated integrity status from all deployments
    pub fn aggregated_integrity(&self) -> AggregatedIntegrity {
        let total = self.deployments.len();
        let ok_count = self
            .deployments
            .iter()
            .filter(|d| d.integrity == SkillIntegrity::Installed)
            .count();
        let not_deployed = self
            .deployments
            .iter()
            .filter(|d| d.integrity == SkillIntegrity::NotDeployed)
            .count();

        // Exclude NotDeployed from total (not expected to be deployed)
        let expected = total - not_deployed;

        match (ok_count, expected) {
            (o, e) if o == e && e > 0 => AggregatedIntegrity::AllOk(o),
            (o, e) if o > 0 && o < e => AggregatedIntegrity::Partial { ok: o, total: e },
            (0, e) if e > 0 => AggregatedIntegrity::AllFailed(e),
            _ => AggregatedIntegrity::NotApplicable,
        }
    }
}

/// Client status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientStatus {
    pub id: String,
    pub enabled: bool,
    pub mcp_scopes: Vec<ConfigScope>,
    pub skill_scopes: Vec<ConfigScope>,
    pub supports_symlinks: bool,
    pub delivery_mode: String,
}

/// Per-client MCP deployment status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientDeployment {
    pub client_id: String,
    pub config_path: PathBuf,
    pub scope: ConfigScope,
    pub integrity: DeploymentIntegrity,
}

/// Per-client skill deployment status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDeployment {
    pub client_id: String,
    pub dst_path: PathBuf,
    pub scope: ConfigScope,
    pub mode: LinkMode,
    pub integrity: SkillIntegrity,
}

// =============================================================================
// State Enums
// =============================================================================

/// Entry state comparing config vs lockfile
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryState {
    /// declared + locked, constraints match
    Ok,
    /// in config but not lockfile
    NotLocked,
    /// constraint changed since lock
    Stale,
    /// in lockfile but not config
    Orphaned,
}

/// MCP config deployment integrity (with --verify)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentIntegrity {
    /// Entry exists and hash matches ownership record
    Ok,
    /// Entry exists but hash differs (user modified)
    Modified,
    /// Entry missing from config file
    Missing,
    /// No ownership record (never deployed to this client)
    NotDeployed,
}

/// Skill installation integrity (with --verify)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillIntegrity {
    /// Files exist, hash matches
    Installed,
    /// Files exist, hash mismatch
    Modified,
    /// dst_path set but doesn't exist
    NotFound,
    /// Symlink target missing
    BrokenLink,
    /// Never installed to this client
    NotDeployed,
}

/// Aggregated integrity across multiple clients
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregatedIntegrity {
    /// All expected deployments are OK
    AllOk(usize),
    /// Some deployments OK, some failed
    Partial { ok: usize, total: usize },
    /// All deployments failed
    AllFailed(usize),
    /// No deployments expected
    NotApplicable,
}

// =============================================================================
// Core Functions
// =============================================================================

/// Determine entry state by comparing config and lockfile
///
/// Generic over config entry type - works for both skills and MCP servers.
///
/// Note: If the config-side constraint is empty (as with MCP servers which don't
/// have version fields), we treat it as "any version is acceptable" and consider
/// it a match with whatever is in the lockfile.
pub fn determine_entry_state<C, L>(
    name: &str,
    config_entries: &HashMap<String, C>,
    locked_entries: &HashMap<String, L>,
) -> EntryState
where
    C: HasConstraint,
    L: HasConstraint,
{
    let in_config = config_entries.get(name);
    let in_lockfile = locked_entries.get(name);

    match (in_config, in_lockfile) {
        (Some(cfg), Some(locked)) => {
            let cfg_constraint = cfg.constraint();
            // Empty constraint from config means "any version acceptable" (e.g., MCP servers)
            if cfg_constraint.is_empty() || cfg_constraint == locked.constraint() {
                EntryState::Ok
            } else {
                EntryState::Stale
            }
        }
        (Some(_), None) => EntryState::NotLocked,
        (None, Some(_)) => EntryState::Orphaned,
        (None, None) => {
            // This shouldn't happen if called correctly, but handle gracefully
            EntryState::NotLocked
        }
    }
}

/// Trait for types that have a constraint/version field
pub trait HasConstraint {
    fn constraint(&self) -> &str;
}

impl HasConstraint for SkillConfigEntry {
    fn constraint(&self) -> &str {
        self.version.as_deref().unwrap_or("latest")
    }
}

impl HasConstraint for LockedSkill {
    fn constraint(&self) -> &str {
        &self.constraint
    }
}

impl HasConstraint for McpConfigEntry {
    fn constraint(&self) -> &str {
        // MCP entries don't have version in config - they use source-based versioning
        // For comparison purposes, we'll use an empty string as placeholder
        ""
    }
}

impl HasConstraint for LockedMcpServer {
    fn constraint(&self) -> &str {
        &self.constraint
    }
}

/// Verify skill installation integrity
pub fn verify_skill_integrity(
    dst_path: &Path,
    expected_hash: Option<&str>,
    mode: LinkMode,
) -> SkillIntegrity {
    // Check if path exists
    if !dst_path.exists() {
        // Check if it's a broken symlink
        if mode == LinkMode::Symlink && dst_path.symlink_metadata().is_ok() {
            return SkillIntegrity::BrokenLink;
        }
        return SkillIntegrity::NotFound;
    }

    // Check if it's a broken symlink (exists but target doesn't)
    if mode == LinkMode::Symlink
        && let Ok(metadata) = dst_path.symlink_metadata()
        && metadata.file_type().is_symlink()
        && let Ok(target) = std::fs::read_link(dst_path)
        && !target.exists()
        && !dst_path.join(&target).exists()
    {
        return SkillIntegrity::BrokenLink;
    }

    // Verify hash if provided
    if let Some(expected) = expected_hash {
        match crate::fs::tree_hash::hash_tree(dst_path) {
            Ok(actual_hash) => {
                if actual_hash == expected {
                    SkillIntegrity::Installed
                } else {
                    SkillIntegrity::Modified
                }
            }
            Err(_) => SkillIntegrity::Modified,
        }
    } else {
        // No hash to verify - assume installed if exists
        SkillIntegrity::Installed
    }
}

/// Verify MCP deployment integrity in a client config file
pub fn verify_mcp_deployment(
    config_content: &Value,
    json_path: &[&str],
    entry_name: &str,
    ownership: &HashMap<String, String>,
) -> DeploymentIntegrity {
    // Check if we have ownership record
    let expected_hash = match ownership.get(entry_name) {
        Some(hash) => hash,
        None => return DeploymentIntegrity::NotDeployed,
    };

    // Navigate down to the target JSON object
    let mut target = config_content;
    for segment in json_path {
        match target.get(segment) {
            Some(value) => target = value,
            None => return DeploymentIntegrity::Missing,
        }
    }

    let entry_value = match target.as_object().and_then(|map| map.get(entry_name)) {
        Some(value) => value,
        None => return DeploymentIntegrity::Missing,
    };

    let actual_hash = crate::config::ownership::hash_json(entry_value);
    if actual_hash == *expected_hash {
        DeploymentIntegrity::Ok
    } else {
        DeploymentIntegrity::Modified
    }
}

/// Collect overall system status using default paths
pub fn collect_status(
    project_root: &Path,
    scope_filter: Option<ConfigScope>,
    verify: bool,
) -> anyhow::Result<SystemStatus> {
    let global_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
        .join("sift");
    let state_dir = LockfileStore::default_state_dir()?;
    collect_status_with_paths(project_root, &global_dir, &state_dir, scope_filter, verify)
}

/// Collect overall system status with custom paths (for testing)
pub fn collect_status_with_paths(
    project_root: &Path,
    global_dir: &Path,
    state_dir: &Path,
    scope_filter: Option<ConfigScope>,
    verify: bool,
) -> anyhow::Result<SystemStatus> {
    // 1. Load configs from both global and project locations
    let global_config_path = global_dir.join("sift.toml");
    let project_config_path = project_root.join("sift.toml");

    let global_config = if global_config_path.exists() {
        let content = std::fs::read_to_string(&global_config_path)?;
        Some(toml::from_str::<SiftConfig>(&content)?)
    } else {
        None
    };

    let project_config = if project_config_path.exists() {
        let content = std::fs::read_to_string(&project_config_path)?;
        Some(toml::from_str::<SiftConfig>(&content)?)
    } else {
        None
    };

    // Track which entries come from which scope
    // Key: entry name, Value: (scope, source_file)
    //
    // Scope determination uses "declaration location" semantics:
    // - Entries declared in global config get Global scope
    // - Entries declared in project config get PerProjectShared scope
    // - Project overrides ([projects."/path".mcp.*]) don't change scope,
    //   they only modify the entry's values (runtime, env, etc.)
    let mut skill_scopes: HashMap<String, (ConfigScope, PathBuf)> = HashMap::new();
    let mut mcp_scopes: HashMap<String, (ConfigScope, PathBuf)> = HashMap::new();

    // First add global entries (these can be overridden by project)
    if let Some(ref gc) = global_config {
        for name in gc.skill.keys() {
            skill_scopes.insert(
                name.clone(),
                (ConfigScope::Global, global_config_path.clone()),
            );
        }
        for name in gc.mcp.keys() {
            mcp_scopes.insert(
                name.clone(),
                (ConfigScope::Global, global_config_path.clone()),
            );
        }
    }

    // Then add project entries (override global scope tracking)
    if let Some(ref pc) = project_config {
        for name in pc.skill.keys() {
            skill_scopes.insert(
                name.clone(),
                (ConfigScope::PerProjectShared, project_config_path.clone()),
            );
        }
        for name in pc.mcp.keys() {
            mcp_scopes.insert(
                name.clone(),
                (ConfigScope::PerProjectShared, project_config_path.clone()),
            );
        }
    }

    // Merge configs for actual entry data (project overrides global)
    let merged_config =
        crate::config::merge_configs(global_config.clone(), project_config.clone(), project_root)?;

    // 2. Load lockfile from state directory
    let lockfile = LockfileStore::load(Some(project_root.to_path_buf()), state_dir.to_path_buf())?;

    // 3. Create ownership store for verification
    let ownership_store =
        OwnershipStore::new(state_dir.to_path_buf(), Some(project_root.to_path_buf()));

    // 4. Get registered clients (currently only Claude Code)
    let clients: Vec<Box<dyn ClientAdapter>> = vec![Box::new(ClaudeCodeClient::new())];

    // 5. Build client context
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let ctx = ClientContext {
        home_dir,
        project_root: project_root.to_path_buf(),
    };

    // 6. Collect skill statuses
    let mut skills = Vec::new();

    for (name, entry) in &merged_config.skill {
        // Get the scope this entry came from
        let (entry_scope, source_file) = skill_scopes
            .get(name)
            .cloned()
            .unwrap_or((ConfigScope::PerProjectShared, project_config_path.clone()));

        // Apply scope_filter
        if let Some(filter) = scope_filter
            && entry_scope != filter
        {
            continue;
        }

        let state = determine_entry_state(name, &merged_config.skill, &lockfile.skills);

        // Get locked info if available
        let locked = lockfile.skills.get(name);
        let resolved_version = locked.map(|l| l.resolved_version.clone());
        let registry = locked
            .map(|l| l.registry.clone())
            .unwrap_or_else(|| "registry:official".to_string());
        let dst_path = locked.and_then(|l| l.dst_path.clone());
        let mode = locked.and_then(|l| l.mode);
        let tree_hash = locked.and_then(|l| l.tree_hash.clone());

        // Collect per-client deployment status if verify
        let mut deployments = Vec::new();
        if verify {
            for client in &clients {
                // Check each scope the client supports
                let caps = client.capabilities();
                let skill_client_scopes = [
                    (ConfigScope::Global, caps.skills.global),
                    (ConfigScope::PerProjectShared, caps.skills.project),
                ];

                for (scope, supported) in skill_client_scopes {
                    if !supported {
                        continue;
                    }

                    if let Ok(plan) = client.plan_skill(&ctx, scope) {
                        let root_path = resolve_plan_path(&ctx, plan.root, &plan.relative_path);
                        let skill_dst = root_path.join(name);

                        let integrity = if let Some(ref expected_hash) = tree_hash {
                            verify_skill_integrity(
                                &skill_dst,
                                Some(expected_hash),
                                mode.unwrap_or(LinkMode::Symlink),
                            )
                        } else if skill_dst.exists() {
                            SkillIntegrity::Installed
                        } else {
                            SkillIntegrity::NotDeployed
                        };

                        deployments.push(SkillDeployment {
                            client_id: client.id().to_string(),
                            dst_path: skill_dst,
                            scope,
                            mode: mode.unwrap_or(LinkMode::Symlink),
                            integrity,
                        });
                    }
                }
            }
        }

        skills.push(SkillStatus {
            name: name.clone(),
            constraint: entry
                .version
                .clone()
                .unwrap_or_else(|| "latest".to_string()),
            resolved_version,
            registry,
            scope: entry_scope,
            source_file,
            state,
            deployments,
            mode,
            dst_path,
        });
    }

    // 7. Collect MCP statuses
    let mut mcp_servers = Vec::new();

    for (name, entry) in &merged_config.mcp {
        // Get the scope this entry came from
        let (entry_scope, source_file) = mcp_scopes
            .get(name)
            .cloned()
            .unwrap_or((ConfigScope::PerProjectShared, project_config_path.clone()));

        // Apply scope_filter
        if let Some(filter) = scope_filter
            && entry_scope != filter
        {
            continue;
        }

        let state = determine_entry_state(name, &merged_config.mcp, &lockfile.mcp_servers);

        // Get locked info if available
        let locked = lockfile.mcp_servers.get(name);
        let resolved_version = locked.map(|l| l.resolved_version.clone());
        let constraint = locked.map(|l| l.constraint.clone()).unwrap_or_default();
        let registry = locked
            .map(|l| l.registry.clone())
            .unwrap_or_else(|| "registry:official".to_string());

        // Collect per-client deployment status if verify
        let mut deployments = Vec::new();
        if verify {
            for client in &clients {
                // Check each scope the client supports
                let caps = client.capabilities();
                let mcp_client_scopes = [
                    (ConfigScope::Global, caps.mcp.global),
                    (ConfigScope::PerProjectShared, caps.mcp.project),
                    (ConfigScope::PerProjectLocal, caps.mcp.local),
                ];

                for (scope, supported) in mcp_client_scopes {
                    if !supported {
                        continue;
                    }

                    // Get the config path for this client/scope combination
                    if let Ok(plan) = client.plan_mcp(&ctx, scope, &[]) {
                        let config_file_path =
                            resolve_plan_path(&ctx, plan.root, &plan.relative_path);

                        // Load ownership for this config file
                        let json_path: Vec<&str> = if plan.json_path.is_empty() {
                            vec!["mcpServers"]
                        } else {
                            plan.json_path.iter().map(|s| s.as_str()).collect()
                        };
                        let field_key = json_path.join(".");
                        let ownership = ownership_store
                            .load_for_field(&config_file_path, &field_key)
                            .unwrap_or_default();

                        // Check integrity
                        let integrity = if config_file_path.exists() {
                            match std::fs::read_to_string(&config_file_path) {
                                Ok(content) => match serde_json::from_str::<Value>(&content) {
                                    Ok(json) => {
                                        verify_mcp_deployment(&json, &json_path, name, &ownership)
                                    }
                                    Err(_) => DeploymentIntegrity::NotDeployed,
                                },
                                Err(_) => DeploymentIntegrity::NotDeployed,
                            }
                        } else {
                            DeploymentIntegrity::NotDeployed
                        };

                        deployments.push(ClientDeployment {
                            client_id: client.id().to_string(),
                            config_path: config_file_path,
                            scope,
                            integrity,
                        });
                    }
                }
            }
        }

        mcp_servers.push(McpServerStatus {
            name: name.clone(),
            runtime: entry.runtime.clone(),
            constraint,
            resolved_version,
            registry,
            scope: entry_scope,
            source_file,
            state,
            deployments,
        });
    }

    // 8. Also check for orphaned entries in lockfile
    // Orphaned entries use their original scope from lockfile
    // They are only shown when their scope matches the filter (or when no filter is set)
    for (name, locked) in &lockfile.skills {
        if !merged_config.skill.contains_key(name) {
            // Apply scope_filter to orphaned entries
            if let Some(filter) = scope_filter
                && locked.scope != filter
            {
                continue; // Skip orphaned entries from different scopes
            }

            skills.push(SkillStatus {
                name: name.clone(),
                constraint: locked.constraint.clone(),
                resolved_version: Some(locked.resolved_version.clone()),
                registry: locked.registry.clone(),
                scope: locked.scope, // Use original scope from lockfile!
                source_file: PathBuf::from("<orphaned>"),
                state: EntryState::Orphaned,
                deployments: vec![],
                mode: locked.mode,
                dst_path: locked.dst_path.clone(),
            });
        }
    }

    for (name, locked) in &lockfile.mcp_servers {
        if !merged_config.mcp.contains_key(name) {
            // Apply scope_filter to orphaned entries
            if let Some(filter) = scope_filter
                && locked.scope != filter
            {
                continue; // Skip orphaned entries from different scopes
            }

            mcp_servers.push(McpServerStatus {
                name: name.clone(),
                runtime: None,
                constraint: locked.constraint.clone(),
                resolved_version: Some(locked.resolved_version.clone()),
                registry: locked.registry.clone(),
                scope: locked.scope, // Use original scope from lockfile!
                source_file: PathBuf::from("<orphaned>"),
                state: EntryState::Orphaned,
                deployments: vec![],
            });
        }
    }

    // 9. Collect client statuses
    let mut client_statuses = Vec::new();
    for client in &clients {
        let caps = client.capabilities();
        let mut mcp_client_scopes = Vec::new();
        let mut skill_client_scopes = Vec::new();

        if caps.mcp.global {
            mcp_client_scopes.push(ConfigScope::Global);
        }
        if caps.mcp.project {
            mcp_client_scopes.push(ConfigScope::PerProjectShared);
        }
        if caps.mcp.local {
            mcp_client_scopes.push(ConfigScope::PerProjectLocal);
        }

        if caps.skills.global {
            skill_client_scopes.push(ConfigScope::Global);
        }
        if caps.skills.project {
            skill_client_scopes.push(ConfigScope::PerProjectShared);
        }

        let delivery_mode = match caps.skill_delivery {
            crate::client::SkillDeliveryMode::Filesystem { .. } => "Filesystem".to_string(),
            crate::client::SkillDeliveryMode::ConfigReference => "ConfigReference".to_string(),
            crate::client::SkillDeliveryMode::None => "None".to_string(),
        };

        client_statuses.push(ClientStatus {
            id: client.id().to_string(),
            enabled: true, // TODO: check config.clients
            mcp_scopes: mcp_client_scopes,
            skill_scopes: skill_client_scopes,
            supports_symlinks: caps.supports_symlinked_skills,
            delivery_mode,
        });
    }

    // Calculate totals and issues based on filtered results
    let total_mcp = mcp_servers.len();
    let total_skills = skills.len();
    let issues = mcp_servers
        .iter()
        .filter(|m| m.state != EntryState::Ok)
        .count()
        + skills.iter().filter(|s| s.state != EntryState::Ok).count();

    Ok(SystemStatus {
        project_root: Some(project_root.to_path_buf()),
        scope_filter,
        link_mode: merged_config.link_mode.unwrap_or(LinkMode::Symlink),
        mcp_servers,
        skills,
        clients: client_statuses,
        summary: StatusSummary {
            total_mcp,
            total_skills,
            issues,
        },
    })
}

fn resolve_plan_path(ctx: &ClientContext, root: PathRoot, relative: &Path) -> PathBuf {
    let base = match root {
        PathRoot::User => &ctx.home_dir,
        PathRoot::Project => &ctx.project_root,
    };
    base.join(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregated_integrity_all_ok() {
        let deployments = vec![
            ClientDeployment {
                client_id: "test".to_string(),
                config_path: PathBuf::new(),
                scope: ConfigScope::Global,
                integrity: DeploymentIntegrity::Ok,
            },
            ClientDeployment {
                client_id: "test2".to_string(),
                config_path: PathBuf::new(),
                scope: ConfigScope::Global,
                integrity: DeploymentIntegrity::Ok,
            },
        ];

        let status = McpServerStatus {
            name: "test".to_string(),
            runtime: None,
            constraint: "".to_string(),
            resolved_version: None,
            registry: "".to_string(),
            scope: ConfigScope::Global,
            source_file: PathBuf::new(),
            state: EntryState::Ok,
            deployments,
        };

        assert_eq!(status.aggregated_integrity(), AggregatedIntegrity::AllOk(2));
    }
}
