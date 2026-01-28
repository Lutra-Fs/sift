//! Scope resolution for client installs.

use crate::types::ConfigScope;

pub use crate::client::ScopeSupport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Mcp,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRequest {
    Explicit(ConfigScope),
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoStatus {
    Git,
    NotGit,
}

impl RepoStatus {
    pub fn from_project_root(project_root: &std::path::Path) -> Self {
        if project_root.join(".git").exists() {
            RepoStatus::Git
        } else {
            RepoStatus::NotGit
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDecision {
    pub scope: ConfigScope,
    pub use_git_exclude: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeResolution {
    Apply(ScopeDecision),
    Skip { warning: String },
}

pub fn resolve_scope(
    resource: ResourceKind,
    request: ScopeRequest,
    support: ScopeSupport,
    repo: RepoStatus,
) -> anyhow::Result<ScopeResolution> {
    match request {
        ScopeRequest::Explicit(scope) => resolve_explicit(resource, scope, support, repo),
        ScopeRequest::Auto => resolve_auto(resource, support),
    }
}

fn resolve_explicit(
    resource: ResourceKind,
    scope: ConfigScope,
    support: ScopeSupport,
    repo: RepoStatus,
) -> anyhow::Result<ScopeResolution> {
    match resource {
        ResourceKind::Mcp => resolve_explicit_mcp(scope, support),
        ResourceKind::Skill => resolve_explicit_skill(scope, support, repo),
    }
}

fn resolve_explicit_mcp(
    scope: ConfigScope,
    support: ScopeSupport,
) -> anyhow::Result<ScopeResolution> {
    match scope {
        ConfigScope::PerProjectLocal => {
            if support.local {
                Ok(ScopeResolution::Apply(ScopeDecision {
                    scope,
                    use_git_exclude: false,
                }))
            } else {
                anyhow::bail!("MCP local scope is not supported by this client");
            }
        }
        ConfigScope::PerProjectShared => {
            if support.project {
                Ok(ScopeResolution::Apply(ScopeDecision {
                    scope,
                    use_git_exclude: false,
                }))
            } else {
                anyhow::bail!("MCP project scope is not supported by this client");
            }
        }
        ConfigScope::Global => {
            if support.global {
                Ok(ScopeResolution::Apply(ScopeDecision {
                    scope,
                    use_git_exclude: false,
                }))
            } else {
                anyhow::bail!("MCP global scope is not supported by this client");
            }
        }
    }
}

fn resolve_explicit_skill(
    scope: ConfigScope,
    support: ScopeSupport,
    repo: RepoStatus,
) -> anyhow::Result<ScopeResolution> {
    match scope {
        ConfigScope::PerProjectLocal => {
            if support.local {
                return Ok(ScopeResolution::Apply(ScopeDecision {
                    scope,
                    use_git_exclude: false,
                }));
            }
            if support.project {
                if repo == RepoStatus::Git {
                    return Ok(ScopeResolution::Apply(ScopeDecision {
                        scope: ConfigScope::PerProjectShared,
                        use_git_exclude: true,
                    }));
                }
                anyhow::bail!(
                    "Local skill scope requires a git repository. Use project or global."
                );
            }
            anyhow::bail!("Skill local scope is not supported by this client");
        }
        ConfigScope::PerProjectShared => {
            if support.project {
                Ok(ScopeResolution::Apply(ScopeDecision {
                    scope,
                    use_git_exclude: false,
                }))
            } else {
                anyhow::bail!("Skill project scope is not supported by this client");
            }
        }
        ConfigScope::Global => {
            if support.global {
                Ok(ScopeResolution::Apply(ScopeDecision {
                    scope,
                    use_git_exclude: false,
                }))
            } else {
                anyhow::bail!("Skill global scope is not supported by this client");
            }
        }
    }
}

fn resolve_auto(resource: ResourceKind, support: ScopeSupport) -> anyhow::Result<ScopeResolution> {
    let pick = match resource {
        ResourceKind::Mcp => pick_mcp_scope(support),
        ResourceKind::Skill => pick_skill_scope(support),
    };

    if let Some(scope) = pick {
        Ok(ScopeResolution::Apply(ScopeDecision {
            scope,
            use_git_exclude: false,
        }))
    } else {
        Ok(ScopeResolution::Skip {
            warning: "Client does not support any requested scopes".to_string(),
        })
    }
}

fn pick_mcp_scope(support: ScopeSupport) -> Option<ConfigScope> {
    if support.local {
        return Some(ConfigScope::PerProjectLocal);
    }
    if support.project {
        return Some(ConfigScope::PerProjectShared);
    }
    if support.global {
        return Some(ConfigScope::Global);
    }
    None
}

fn pick_skill_scope(support: ScopeSupport) -> Option<ConfigScope> {
    if support.project {
        return Some(ConfigScope::PerProjectShared);
    }
    if support.global {
        return Some(ConfigScope::Global);
    }
    if support.local {
        return Some(ConfigScope::PerProjectLocal);
    }
    None
}
