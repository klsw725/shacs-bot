use crate::runtime::{
    AccountingState, ClassifierDecisionEvidence, ContainmentComparisonOutcome,
    ContainmentPermissionProof, PermissionCeilingComparisonOutcome, PermissionDiagnosticsSummary,
    PermissionPolicyDecisionKind, PermissionPolicyReason,
    PermissionPolicySafetySnapshotAuditStatus, PermissionSecretRefStatus, ProcessAdapterKind,
    ProcessEnvelopeAdmission, ProcessExecutionReceipt, ProcessTerminalOutcome,
    SkillTrustPermissionDecision, SkillTrustPermissionDecisionKind, SkillTrustRejectionReason,
    StaticPolicyPrecedence, WorkspaceComparisonOutcome,
};
use serde::{Deserialize, Serialize, Serializer};
use std::{error::Error, fmt};

pub struct CoreDiagnosticsAggregateInput<'a> {
    pub permission: &'a PermissionDiagnosticsSummary,
    pub process_receipts: &'a [ProcessExecutionReceipt],
    pub containment_proofs: &'a [ContainmentPermissionProof],
    pub classifier_evidence: &'a [ClassifierDecisionEvidence],
    pub trust_decisions: &'a [SkillTrustPermissionDecision],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreDiagnosticsAggregate {
    pub schema_id: &'static str,
    pub policy_safety: PolicySafetyDiagnosticsDto,
    pub secrets: SecretDiagnosticsDto,
    pub process: ProcessDiagnosticsDto,
    pub containment: ContainmentDiagnosticsDto,
    pub classifier: ClassifierDiagnosticsDto,
    pub trust: TrustDiagnosticsDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySafetyDiagnosticsDto {
    pub present_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub malformed_count: u64,
    pub refs: Vec<PolicySafetyRefDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySafetyRefDiagnostic {
    pub status: PermissionPolicySafetySnapshotAuditStatus,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub snapshot_id: Option<String>,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub policy_safety_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretDiagnosticsDto {
    pub resolved_count: u64,
    pub unresolved_count: u64,
    pub missing_count: u64,
    pub stale_count: u64,
    pub unsupported_count: u64,
    pub malformed_count: u64,
    pub refs: Vec<SecretRefDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRefDiagnostic {
    #[serde(serialize_with = "serialize_safe_string")]
    pub ref_id: String,
    #[serde(serialize_with = "serialize_safe_string")]
    pub source_kind: String,
    pub status: PermissionSecretRefStatus,
    #[serde(serialize_with = "serialize_safe_string")]
    pub redaction_evidence_ref: String,
    #[serde(serialize_with = "serialize_safe_string")]
    pub requested_consumer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDiagnosticsDto {
    pub receipt_count: usize,
    pub total_dispatch_count: usize,
    pub receipts: Vec<ProcessReceiptDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessReceiptDiagnostic {
    #[serde(serialize_with = "serialize_safe_string")]
    pub receipt_id: String,
    pub adapter: ProcessAdapterKind,
    pub terminal_outcome: ProcessTerminalOutcome,
    pub dispatch_count: usize,
    pub policy_decision: PermissionPolicyDecisionKind,
    pub policy_reason: PermissionPolicyReason,
    #[serde(serialize_with = "serialize_safe_string")]
    pub policy_safety_snapshot_id: String,
    #[serde(serialize_with = "serialize_safe_string")]
    pub policy_safety_digest: String,
    pub secret_ref_count: usize,
    pub redacted_target_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainmentDiagnosticsDto {
    pub proof_count: usize,
    pub proofs: Vec<ContainmentProofDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainmentProofDiagnostic {
    #[serde(serialize_with = "serialize_safe_string")]
    pub proof_id: String,
    #[serde(serialize_with = "serialize_safe_string")]
    pub envelope_id: String,
    #[serde(serialize_with = "serialize_safe_string")]
    pub policy_safety_digest: String,
    pub containment_outcome: ContainmentComparisonOutcome,
    pub workspace_outcome: WorkspaceComparisonOutcome,
    pub ceiling_outcome: PermissionCeilingComparisonOutcome,
    pub admission: ProcessEnvelopeAdmission,
    pub violation_count: usize,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub blocked_external_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierDiagnosticsDto {
    pub evidence_count: usize,
    pub items: Vec<ClassifierEvidenceDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierEvidenceDiagnostic {
    #[serde(serialize_with = "serialize_safe_string")]
    pub evidence_id: String,
    pub route_kind: crate::runtime::ClassifierRouteKind,
    pub disposition: crate::runtime::ClassifierDisposition,
    pub precedence: StaticPolicyPrecedence,
    pub input_token_state: AccountingState,
    pub output_token_state: AccountingState,
    pub latency_state: AccountingState,
    pub cost_state: AccountingState,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub policy_safety_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustDiagnosticsDto {
    pub decision_count: usize,
    pub validated_count: usize,
    pub rejected_count: usize,
    pub blocked_external_count: usize,
    pub decisions: Vec<TrustDecisionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustDecisionDiagnostic {
    pub kind: SkillTrustPermissionDecisionKind,
    pub reason: Option<SkillTrustRejectionReason>,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub blocked_status: Option<String>,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub blocked_owner: Option<String>,
    pub dispatch_count: usize,
}

const REDACTED: &str = "[REDACTED]";

fn serialize_safe_string<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&safe_string(value))
}

fn serialize_optional_safe_string<S>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value
        .as_ref()
        .map(|item| safe_string(item))
        .serialize(serializer)
}

fn safe_string(value: &str) -> String {
    if raw_text(value) {
        REDACTED.to_owned()
    } else {
        value.to_owned()
    }
}

fn raw_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let normalized: String = text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    lower.contains("sk-")
        || lower.contains("bearer ")
        || lower.contains("private key")
        || lower.contains("-----begin private key-----")
        || text.contains("RAW_")
        || text.contains("/Users/")
        || text.contains("/home/")
        || text.starts_with('/')
        || text.starts_with("\\\\")
        || contains_windows_drive_path(text)
        || lower.contains("provider-secret")
        || lower.contains("process_handle")
        || normalized.contains("processhandle")
        || normalized.contains("rawstdout")
        || normalized.contains("rawstderr")
        || normalized.contains("standardoutputraw")
        || normalized.contains("rawproviderpayload")
}

fn contains_windows_drive_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreDiagnosticsError {
    Serialization,
    RawDiagnosticMaterial,
}

impl fmt::Display for CoreDiagnosticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization => formatter.write_str("core diagnostics serialization failed"),
            Self::RawDiagnosticMaterial => formatter.write_str("raw diagnostic material rejected"),
        }
    }
}

impl Error for CoreDiagnosticsError {}
