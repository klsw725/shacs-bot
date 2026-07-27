use crate::runtime::{PermissionMode, PolicySafetyDigest, RuntimeBoundaryOrigin, SafetyCapability};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBoundaryKind {
    UserTurn,
    Subagent,
    McpStdio,
    ExecTool,
    AppProcess,
    PluginCommand,
    PluginTool,
    PluginHook,
    DependencyPreparation,
    VerifiedEntrypoint,
    DeferredBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainmentEvidenceState {
    ConfirmedNonPrivileged,
    ConfirmedEquivalent,
    NarrowerHardened,
    NativeUnknown,
    EvidenceMissing,
    UnsafePrivileged,
    Mismatched,
    Stale,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceScopeProof {
    pub parent_workspace_ref: String,
    pub child_workspace_ref: String,
    pub parent_scope_digest: String,
    pub child_scope_digest: String,
    pub narrowing_reason: String,
}

impl WorkspaceScopeProof {
    pub fn same(workspace_ref: &str, scope_digest: &str) -> Self {
        Self {
            parent_workspace_ref: workspace_ref.to_owned(),
            child_workspace_ref: workspace_ref.to_owned(),
            parent_scope_digest: scope_digest.to_owned(),
            child_scope_digest: scope_digest.to_owned(),
            narrowing_reason: "same_workspace".to_owned(),
        }
    }

    pub fn narrower(parent: &str, child: &str, parent_digest: &str, child_digest: &str) -> Self {
        Self {
            parent_workspace_ref: parent.to_owned(),
            child_workspace_ref: child.to_owned(),
            parent_scope_digest: parent_digest.to_owned(),
            child_scope_digest: child_digest.to_owned(),
            narrowing_reason: "subdirectory".to_owned(),
        }
    }

    pub fn wider(parent: &str, child: &str, parent_digest: &str, child_digest: &str) -> Self {
        Self {
            parent_workspace_ref: parent.to_owned(),
            child_workspace_ref: child.to_owned(),
            parent_scope_digest: parent_digest.to_owned(),
            child_scope_digest: child_digest.to_owned(),
            narrowing_reason: "wider".to_owned(),
        }
    }

    pub fn malformed(raw_ref: &str) -> Self {
        Self {
            parent_workspace_ref: "workspace".to_owned(),
            child_workspace_ref: raw_ref.to_owned(),
            parent_scope_digest: "scope".to_owned(),
            child_scope_digest: "scope".to_owned(),
            narrowing_reason: "malformed".to_owned(),
        }
    }

    pub fn from_parent_child(parent_ref: &str, child_ref: &str) -> Self {
        Self {
            parent_workspace_ref: parent_ref.to_owned(),
            child_workspace_ref: child_ref.to_owned(),
            parent_scope_digest: format!("scope:{parent_ref}"),
            child_scope_digest: format!("scope:{child_ref}"),
            narrowing_reason: "derived_from_authoritative_parent".to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionCeilingProofInput {
    pub parent_mode: PermissionMode,
    pub requested_mode: PermissionMode,
    pub parent_capabilities: Vec<SafetyCapability>,
    pub requested_capabilities: Vec<SafetyCapability>,
    pub approved_scope_refs: Vec<String>,
    pub requested_scope_ref: String,
    pub per_action_evaluation_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainmentBoundaryRef {
    pub boundary_id: String,
    pub boundary_kind: RuntimeBoundaryKind,
    pub origin: RuntimeBoundaryOrigin,
    pub containment_state: ContainmentEvidenceState,
    pub containment_digest: Option<String>,
    pub workspace_scope: WorkspaceScopeProof,
    pub permission_ceiling: PermissionCeilingProofInput,
    pub created_at_unix_ms: u64,
}

impl ContainmentBoundaryRef {
    pub fn parent_boundary(&self) -> Self {
        let mut parent_ceiling = self.permission_ceiling.clone();
        parent_ceiling.requested_mode = parent_ceiling.parent_mode;
        parent_ceiling.requested_capabilities = parent_ceiling.parent_capabilities.clone();
        Self {
            boundary_id: "parent-user-turn".to_owned(),
            boundary_kind: RuntimeBoundaryKind::UserTurn,
            origin: RuntimeBoundaryOrigin::UserTurn,
            containment_state: ContainmentEvidenceState::ConfirmedNonPrivileged,
            containment_digest: self.containment_digest.clone(),
            workspace_scope: WorkspaceScopeProof::same(
                &self.workspace_scope.parent_workspace_ref,
                &self.workspace_scope.parent_scope_digest,
            ),
            permission_ceiling: parent_ceiling,
            created_at_unix_ms: self.created_at_unix_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainmentPermissionInput {
    pub parent: ContainmentBoundaryRef,
    pub child: ContainmentBoundaryRef,
    pub policy_safety_digest: PolicySafetyDigest,
    pub process_envelope_id: String,
    pub now_unix_ms: u64,
    pub cancelled_at_unix_ms: Option<u64>,
    pub untrusted_metadata: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainmentComparisonOutcome {
    EqualContainment,
    NarrowerContainment,
    UnknownContainment,
    UnsafeContainment,
    MissingEvidence,
    MismatchedContainment,
    StaleContainment,
    MalformedContainment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceComparisonOutcome {
    SameScope,
    NarrowerScope,
    UnknownScope,
    WiderScope,
    MismatchedScopeRef,
    MalformedScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionCeilingComparisonOutcome {
    EqualCeiling,
    NarrowerCeiling,
    ModeWidening,
    CapabilityWidening,
    ScopeWidening,
    DeferredGateBypass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessEnvelopeAdmission {
    Admit,
    AskRequired,
    Deny,
    RejectMalformed,
    RejectStale,
    RejectMismatch,
    BlockedExternalSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainmentProofViolation {
    MissingPolicySnapshotRef,
    ContainmentDigestMismatch,
    WorkspaceWidening,
    CapabilityWidening,
    ModeWidening,
    DeferredGateBypass,
    UnsafeContainment,
    UnknownContainment,
    StaleEvidence,
    MalformedInput,
    CancelledAdmissionReuse,
    BlockedExternalSurface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedExternalSurfaceReason {
    MissingOwnerEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedExternalSurface {
    pub status: String,
    pub owner: String,
    pub evidence_reason: String,
    pub reason: BlockedExternalSurfaceReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainmentPermissionProofProjectionInput {
    pub proof_id: String,
    pub envelope_id: String,
    pub policy_safety_digest: PolicySafetyDigest,
    pub parent_boundary_kind: RuntimeBoundaryKind,
    pub child_boundary_kind: RuntimeBoundaryKind,
    pub admission: ProcessEnvelopeAdmission,
    pub redacted_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainmentPermissionProof {
    pub proof_id: String,
    pub policy_safety_digest: PolicySafetyDigest,
    pub envelope_id: String,
    pub containment_outcome: ContainmentComparisonOutcome,
    pub workspace_outcome: WorkspaceComparisonOutcome,
    pub ceiling_outcome: PermissionCeilingComparisonOutcome,
    pub admission: ProcessEnvelopeAdmission,
    pub violations: Vec<ContainmentProofViolation>,
    pub diagnostics_input: ContainmentPermissionProofProjectionInput,
    pub blocked_external_surface: Option<BlockedExternalSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainmentPermissionError {
    MissingPolicySnapshotRef,
    MissingProcessEnvelopeRef,
}

impl fmt::Display for ContainmentPermissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ContainmentPermissionError {}
