use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxStatusProjection {
    pub availability: Spec030Availability,
    pub status: SandboxStatus,
    pub fallback: SandboxFallback,
    pub applied_adapters: Vec<ProcessAdapterKind>,
    pub filesystem_policy: SandboxFilesystemPolicy,
    pub network_policy: SandboxNetworkPolicy,
}

impl SandboxStatusProjection {
    pub(super) const fn unavailable() -> Self {
        Self {
            availability: Spec030Availability::Unavailable,
            status: SandboxStatus::Unknown,
            fallback: SandboxFallback::Unknown,
            applied_adapters: Vec::new(),
            filesystem_policy: SandboxFilesystemPolicy::Unknown,
            network_policy: SandboxNetworkPolicy::Unknown,
        }
    }
}
