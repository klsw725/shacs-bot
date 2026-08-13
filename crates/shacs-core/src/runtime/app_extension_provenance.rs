use super::{ActivationReason, ActivationRecord, ActivationStatus};
use serde::{Deserialize, Serialize};
use shacs_app::app::{AppId, AppLifecycleState};
use shacs_app::app_lifecycle::AppProcessState;
use shacs_projection::ResourceCandidateProjection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppExtensionSourceFacts {
    pub source_app_id: AppId,
    pub extension_name: String,
    pub manifest_digest: String,
    pub content_digest: String,
    pub dependency_manifest_digest: String,
    pub lifecycle: AppLifecycleState,
    pub process_state: AppProcessState,
    pub source_identity: String,
}

#[derive(Debug, Clone, Copy)]
pub struct AppExtensionReplayInput<'a> {
    pub source: Option<&'a AppExtensionSourceFacts>,
    pub resource: Option<&'a ResourceCandidateProjection>,
    pub activation: Option<&'a ActivationRecord>,
}

impl<'a> AppExtensionReplayInput<'a> {
    pub const fn new(
        source: Option<&'a AppExtensionSourceFacts>,
        resource: Option<&'a ResourceCandidateProjection>,
        activation: Option<&'a ActivationRecord>,
    ) -> Self {
        Self {
            source,
            resource,
            activation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppExtensionStatus {
    Active,
    Stale,
    Disabled,
    Revoked,
    Removed,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppExtensionBlocker {
    ActivationStale,
    AppDisabled,
    ProcessNotRunning,
    ActivationRevoked,
    SourceRemoved,
    Spec030Untrusted,
    ResourceIdentityMismatch,
    ContentDigestMismatch,
    DependencyManifestDigestMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExtensionReplayDispatchCounters {
    pub discovery: usize,
    pub dependency_preparation: usize,
    pub credential_resolution: usize,
    pub entrypoint_execution: usize,
}

impl AppExtensionReplayDispatchCounters {
    pub const fn total(self) -> usize {
        self.discovery
            + self.dependency_preparation
            + self.credential_resolution
            + self.entrypoint_execution
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppExtensionProvenanceProjection {
    pub source_app_id: Option<String>,
    pub extension_name: String,
    pub manifest_digest: Option<String>,
    pub current_content_digest: Option<String>,
    pub current_dependency_manifest_digest: Option<String>,
    pub activation_ref: String,
    pub activated_content_digest: String,
    pub activated_dependency_manifest_digest: String,
    pub activation_status: ActivationStatus,
    pub activation_reason: ActivationReason,
    pub status: AppExtensionStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<AppExtensionBlocker>,
    pub replay_dispatch_counters: AppExtensionReplayDispatchCounters,
}
