use crate::runtime::{ContainmentSnapshotRef, PermissionModeSnapshot, SafetyCapability};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

pub const POLICY_SAFETY_SNAPSHOT_SCHEMA_V1: &str = "policy_safety_snapshot.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySafetySnapshotError {
    UnknownSchema {
        schema_id: String,
    },
    MissingRef,
    MissingField {
        field: String,
    },
    Malformed {
        detail: String,
    },
    DigestMismatch {
        expected: String,
        actual: String,
    },
    SnapshotIdMismatch {
        expected: String,
        actual: String,
    },
    StaleSnapshot {
        expired_at_unix_ms: u64,
        now_unix_ms: u64,
    },
    RawMaterialRejected {
        field: String,
    },
}

impl fmt::Display for PolicySafetySnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for PolicySafetySnapshotError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicySafetySnapshotSchemaId {
    #[serde(rename = "policy_safety_snapshot.v1")]
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PolicySafetySnapshotId(pub String);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PolicySafetyDigest(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySafetySnapshotRef {
    pub schema_id: PolicySafetySnapshotSchemaId,
    pub snapshot_id: PolicySafetySnapshotId,
    pub policy_safety_digest: PolicySafetyDigest,
    pub created_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_ms: Option<u64>,
    pub redacted_summary: RedactedPolicySafetySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySafetySnapshot {
    pub schema_id: PolicySafetySnapshotSchemaId,
    pub snapshot_id: PolicySafetySnapshotId,
    pub created_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_unix_ms: Option<u64>,
    pub permission_mode: PermissionModeSnapshot,
    pub capability_ceiling: CapabilityCeilingRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment: Option<ContainmentSnapshotRef>,
    pub source_refs: Vec<PolicySafetySourceRef>,
    pub provenance_refs: Vec<PolicySafetyProvenanceRef>,
    pub creation_reason: PolicySafetySnapshotCreationReason,
    pub redacted_summary: RedactedPolicySafetySummary,
    #[serde(skip)]
    pub(super) policy_safety_digest: PolicySafetyDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySafetySnapshotInput {
    pub snapshot_id: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub permission_mode: PermissionModeSnapshot,
    pub capability_ceiling: CapabilityCeilingRef,
    pub containment: Option<ContainmentSnapshotRef>,
    pub source_refs: Vec<PolicySafetySourceRef>,
    pub provenance_refs: Vec<PolicySafetyProvenanceRef>,
    pub creation_reason: PolicySafetySnapshotCreationReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityCeilingRef {
    pub capabilities: Vec<SafetyCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactedPolicySafetySummary {
    pub permission_mode: String,
    pub capability_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containment_digest: Option<String>,
    pub source_ref_count: usize,
    pub provenance_ref_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySafetySnapshotCreationReason {
    PermissionedAction,
    ApprovalRequest,
    ApprovalReplay,
    DiagnosticsReplay,
    DownstreamConsumer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySafetySourceRef {
    pub kind: PolicySafetySourceKind,
    pub ref_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySafetySourceKind {
    PermissionConfig,
    SessionOption,
    InheritedContext,
    ContainmentEvidence,
    RuntimePolicy,
    ExternalExecutionSnapshotRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySafetyProvenanceRef {
    pub kind: PolicySafetyProvenanceKind,
    pub ref_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicySafetyProvenanceKind {
    ConfigProfileRef,
    ContextSnapshotRef,
    ProviderExecutionSnapshotRef,
    TrustRecordRef,
    RuntimeEventRef,
    DiagnosticsRef,
}
