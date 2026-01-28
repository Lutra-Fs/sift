//! Client targeting policy for selective deployment.

/// Policy for determining which clients to deploy to.
#[derive(Debug, Clone, Default)]
pub struct TargetingPolicy {
    /// Whitelist: only deploy to these clients (if Some)
    targets: Option<Vec<String>>,
    /// Blacklist: skip these clients (if Some, and targets is None)
    ignore_targets: Option<Vec<String>>,
}

impl TargetingPolicy {
    pub fn new(targets: Option<Vec<String>>, ignore_targets: Option<Vec<String>>) -> Self {
        Self {
            targets,
            ignore_targets,
        }
    }

    /// Check if deployment should proceed to the given client.
    pub fn should_deploy_to(&self, client_id: &str) -> bool {
        if let Some(ref whitelist) = self.targets {
            return whitelist.iter().any(|t| t == client_id);
        }
        if let Some(ref blacklist) = self.ignore_targets {
            return !blacklist.iter().any(|t| t == client_id);
        }
        true
    }
}
