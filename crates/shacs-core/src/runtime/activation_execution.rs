use super::{
    ActivationReason, ActivationRecord, ActivationStatus, ExecutionSnapshot, WorkspaceTrustRef,
};
use shacs_app::app::AppLifecycleState;
use shacs_projection::{ResourceActivation, ResourceCandidateProjection, ResourceLoadStatus};
use std::fmt;

pub struct ActivationLiveFacts<'a> {
    resource: &'a ResourceCandidateProjection,
    identity: ActivationCurrentIdentity,
}

impl<'a> ActivationLiveFacts<'a> {
    pub const fn new(
        resource: &'a ResourceCandidateProjection,
        identity: ActivationCurrentIdentity,
    ) -> Self {
        Self { resource, identity }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationCurrentIdentity {
    workspace_trust_ref: WorkspaceTrustRef,
    source_identity: String,
    dependency_manifest_digest: String,
    lifecycle: AppLifecycleState,
}

impl ActivationCurrentIdentity {
    pub fn new(
        workspace_trust_ref: WorkspaceTrustRef,
        source_identity: impl Into<String>,
        dependency_manifest_digest: impl Into<String>,
        lifecycle: AppLifecycleState,
    ) -> Self {
        Self {
            workspace_trust_ref,
            source_identity: source_identity.into(),
            dependency_manifest_digest: dependency_manifest_digest.into(),
            lifecycle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationSnapshotCandidate {
    record: ActivationRecord,
    current: ActivationCurrentIdentity,
}

impl ActivationSnapshotCandidate {
    pub const fn new(record: ActivationRecord, current: ActivationCurrentIdentity) -> Self {
        Self { record, current }
    }

    pub(crate) const fn record(&self) -> &ActivationRecord {
        &self.record
    }
    pub(crate) fn live_facts<'a>(
        &self,
        resource: &'a ResourceCandidateProjection,
    ) -> ActivationLiveFacts<'a> {
        ActivationLiveFacts::new(resource, self.current.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationAdmissionError {
    PersistedState,
    Spec030Eligibility,
    Spec032Lifecycle,
    WorkspaceIdentity,
    SourceIdentity,
    ContentDigest,
    DependencyManifestDigest,
}

impl fmt::Display for ActivationAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "activation admission rejected: {self:?}")
    }
}
impl std::error::Error for ActivationAdmissionError {}

pub fn admit_activation_for_execution(
    record: &ActivationRecord,
    facts: &ActivationLiveFacts<'_>,
) -> Result<(), ActivationAdmissionError> {
    if record.status() != ActivationStatus::Active {
        return Err(ActivationAdmissionError::PersistedState);
    }
    if facts.resource.resource_ref != record.resource_ref()
        || facts.resource.activation == ResourceActivation::Inactive
        || facts.resource.load_status != ResourceLoadStatus::Loaded
    {
        return Err(ActivationAdmissionError::Spec030Eligibility);
    }
    match facts.identity.lifecycle {
        AppLifecycleState::Enabled => {}
        AppLifecycleState::Installed
        | AppLifecycleState::Disabled
        | AppLifecycleState::Unavailable
        | AppLifecycleState::Uninstalling => {
            return Err(ActivationAdmissionError::Spec032Lifecycle)
        }
    }
    if &facts.identity.workspace_trust_ref != record.workspace_trust_ref() {
        return Err(ActivationAdmissionError::WorkspaceIdentity);
    }
    if facts.identity.source_identity != record.source_identity() {
        return Err(ActivationAdmissionError::SourceIdentity);
    }
    if facts.resource.content_sha256.as_deref() != Some(record.content_digest()) {
        return Err(ActivationAdmissionError::ContentDigest);
    }
    if facts.identity.dependency_manifest_digest != record.dependency_manifest_digest() {
        return Err(ActivationAdmissionError::DependencyManifestDigest);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplayDispatchCounters {
    pub discovery: usize,
    pub dependency_preparation: usize,
    pub credential_resolution: usize,
    pub entrypoint_execution: usize,
}

impl ReplayDispatchCounters {
    pub const fn total(self) -> usize {
        self.discovery
            + self.dependency_preparation
            + self.credential_resolution
            + self.entrypoint_execution
    }
}

pub struct ActivationReplay {
    activation_refs: Vec<String>,
    counters: ReplayDispatchCounters,
    transcript: [&'static str; 2],
}

impl ActivationReplay {
    pub fn diagnostic(snapshot: &ExecutionSnapshot) -> Self {
        let activation_refs = snapshot
            .selected_resources
            .iter()
            .filter_map(|resource| resource.activation_ref.clone())
            .collect();
        Self {
            activation_refs,
            counters: ReplayDispatchCounters::default(),
            transcript: ["snapshot_loaded", "diagnostic_projection_emitted"],
        }
    }
    pub const fn counters(&self) -> ReplayDispatchCounters {
        self.counters
    }
    pub const fn transcript(&self) -> [&'static str; 2] {
        self.transcript
    }
    pub fn activation_refs(&self) -> Vec<&str> {
        self.activation_refs.iter().map(String::as_str).collect()
    }
}

pub const fn digest_reason(error: ActivationAdmissionError) -> Option<ActivationReason> {
    match error {
        ActivationAdmissionError::ContentDigest => Some(ActivationReason::ContentDigestMismatch),
        ActivationAdmissionError::DependencyManifestDigest => {
            Some(ActivationReason::DependencyManifestDigestMismatch)
        }
        ActivationAdmissionError::PersistedState
        | ActivationAdmissionError::Spec030Eligibility
        | ActivationAdmissionError::Spec032Lifecycle
        | ActivationAdmissionError::WorkspaceIdentity
        | ActivationAdmissionError::SourceIdentity => None,
    }
}
