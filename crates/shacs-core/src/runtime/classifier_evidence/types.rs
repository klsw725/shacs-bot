use crate::runtime::{
    AutoEvaluatorVerdictKind, EvaluatorConfidence, EvaluatorScopeMatch, SafetyCapability,
};
use serde::{Deserialize, Serialize};

pub const CLASSIFIER_EVIDENCE_SCHEMA_V1: &str = "permission_classifier_evidence.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierDecisionEvidence {
    pub schema_id: ClassifierEvidenceSchemaId,
    pub evidence_id: ClassifierEvidenceId,
    pub created_at_unix_ms: u64,
    pub request: ClassifierRequestCorrelation,
    pub action: ClassifierActionCorrelation,
    pub route: ClassifierRouteEvidence,
    pub model: ClassifierModelEvidence,
    pub token_accounting: ClassifierTokenAccounting,
    pub latency: ClassifierLatencyAccounting,
    pub cost: ClassifierCostAccounting,
    pub verdict: ClassifierVerdictEvidence,
    pub precedence: StaticPolicyPrecedence,
    pub disposition: ClassifierDisposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<ClassifierFallbackEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<RedactedDiagnosticRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClassifierEvidenceSchemaId {
    #[serde(rename = "permission_classifier_evidence.v1")]
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClassifierEvidenceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierRequestCorrelation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_call_id: Option<String>,
    pub classifier_request_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierActionCorrelation {
    pub action_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tool_call_id: Option<String>,
    pub tool_name: String,
    pub action_digest: String,
    pub argument_digest: String,
    pub snapshot_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_snapshot_ref: Option<crate::runtime::PolicySafetySnapshotRef>,
    pub capabilities: Vec<SafetyCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierRouteEvidence {
    pub route_id: String,
    pub kind: ClassifierRouteKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifierRouteKind {
    Primary,
    Fallback,
    Skipped,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierModelEvidence {
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierTokenAccounting {
    pub input: AccountingValue,
    pub output: AccountingValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierLatencyAccounting {
    pub duration_ms: AccountingValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierCostAccounting {
    pub total: AccountingValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountingValue {
    pub state: AccountingState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<AccountingUnavailableReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimator_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<EvaluatorConfidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountingState {
    Measured,
    Estimated,
    Unavailable,
    Skipped,
    Failed,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountingUnavailableReason {
    ProviderOmittedUsage,
    TokenizerUnavailable,
    PriceUnconfigured,
    ConfigUnavailable,
    ClockUnavailable,
    ProviderError,
    ParseFailure,
    MalformedAccountingInput,
    StaticPolicyNotReviewable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierVerdictEvidence {
    pub verdict: AutoEvaluatorVerdictKind,
    pub confidence: EvaluatorConfidence,
    pub scope_match: EvaluatorScopeMatch,
    pub prompt_injection_signal_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub explanation_refs: Vec<RedactedDiagnosticRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifierDisposition {
    NotInvokedStaticPolicy,
    NotInvokedCeiling,
    NotInvokedIneligible,
    AllowCandidateConsumed,
    AskUser,
    DenyCandidateRecorded,
    FallbackUsed,
    FailedClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticPolicyPrecedence {
    StaticDenyWins,
    CeilingWins,
    StaticAskBlocksClassifier,
    ClassifierReviewable,
    ApprovalRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifierFallbackEvidence {
    pub fallback_cause: ClassifierFallbackCause,
    pub previous_route_id: String,
    pub selected_route_id: String,
    pub provider_call_attempted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifierFallbackCause {
    PrimaryUnavailable,
    ProviderError,
    ProviderTimeout,
    ParseFailure,
    MissingUserRequest,
    IneligibleCapability,
    StaticPolicyNotReviewable,
    ConfigUnavailable,
    AccountingUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedDiagnosticRef {
    pub ref_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}
