use serde::{Deserialize, Serialize};

pub const ACTIVATION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceTrustRef(String);

impl WorkspaceTrustRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationSource {
    Explicit,
    TrustedWorkspace,
    App,
    Package,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationStatus {
    Active,
    Stale,
    Disabled,
    Revoked,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationReason {
    Activated,
    ContentDigestMismatch,
    DependencyManifestDigestMismatch,
    UserDisabled,
    UserRevoked,
    SourceRemoved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationRecordInput {
    pub activation_ref: String,
    pub source: ActivationSource,
    pub workspace_trust_ref: WorkspaceTrustRef,
    pub resource_ref: String,
    pub source_identity: String,
    pub content_digest: String,
    pub dependency_manifest_digest: String,
    pub status: ActivationStatus,
    pub reason: ActivationReason,
    pub recorded_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationRecord {
    schema_version: u32,
    activation_ref: String,
    source: ActivationSource,
    workspace_trust_ref: WorkspaceTrustRef,
    resource_ref: String,
    source_identity: String,
    content_digest: String,
    dependency_manifest_digest: String,
    status: ActivationStatus,
    reason: ActivationReason,
    recorded_at_unix_ms: u64,
}

impl ActivationRecord {
    pub fn new(input: ActivationRecordInput) -> Self {
        Self {
            schema_version: ACTIVATION_SCHEMA_VERSION,
            activation_ref: input.activation_ref,
            source: input.source,
            workspace_trust_ref: input.workspace_trust_ref,
            resource_ref: input.resource_ref,
            source_identity: input.source_identity,
            content_digest: input.content_digest,
            dependency_manifest_digest: input.dependency_manifest_digest,
            status: input.status,
            reason: input.reason,
            recorded_at_unix_ms: input.recorded_at_unix_ms,
        }
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn activation_ref(&self) -> &str {
        &self.activation_ref
    }
    pub const fn workspace_trust_ref(&self) -> &WorkspaceTrustRef {
        &self.workspace_trust_ref
    }
    pub fn source_identity(&self) -> &str {
        &self.source_identity
    }
    pub fn resource_ref(&self) -> &str {
        &self.resource_ref
    }
    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }
    pub fn dependency_manifest_digest(&self) -> &str {
        &self.dependency_manifest_digest
    }
    pub const fn status(&self) -> ActivationStatus {
        self.status
    }
    pub const fn reason(&self) -> ActivationReason {
        self.reason
    }

    pub fn diagnose(&self, observed: &ActivationDigestObservation) -> Vec<ActivationDiagnostic> {
        let mut diagnostics = Vec::new();
        if self.content_digest != observed.content_digest {
            diagnostics.push(self.diagnostic(ActivationReason::ContentDigestMismatch));
        }
        if self.dependency_manifest_digest != observed.dependency_manifest_digest {
            diagnostics.push(self.diagnostic(ActivationReason::DependencyManifestDigestMismatch));
        }
        diagnostics
    }

    fn diagnostic(&self, reason: ActivationReason) -> ActivationDiagnostic {
        ActivationDiagnostic {
            activation_ref: self.activation_ref.clone(),
            reason,
        }
    }

    pub(crate) fn transition(&mut self, status: ActivationStatus, reason: ActivationReason) {
        self.status = status;
        self.reason = reason;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationDigestObservation {
    pub content_digest: String,
    pub dependency_manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationDiagnostic {
    pub activation_ref: String,
    pub reason: ActivationReason,
}
