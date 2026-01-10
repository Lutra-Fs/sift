use crate::registry::RegistryCapabilities;

pub fn capabilities() -> RegistryCapabilities {
    RegistryCapabilities {
        supports_version_pinning: true,
    }
}
