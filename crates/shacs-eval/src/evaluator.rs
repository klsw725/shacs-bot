use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_utils::redaction::{redact_string, redact_value};

pub const EVALUATE_NOTIFICATION_TOOL: &str = "evaluate_notification";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorKind {
    GoalCompletion,
    SafetyCapability,
    TaskOutcome,
    Replay,
    RedactionCheck,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationTriggerSource {
    SessionTurn,
    ScheduledJob,
    Heartbeat,
    Subagent,
    AppTask,
    Channel,
    LocalApi,
    ManualReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    SessionEvent,
    ToolPayload,
    DiagnosticRecord,
    TaskResult,
    ChannelMessage,
    FileArtifact,
    ProviderSnapshot,
    MemoryEvidenceSet,
    FrozenSessionSearchSnapshot,
    EvaluatorSummary,
    SkillDisclosure,
    AuthoredSkill,
    CuratorRecommendation,
    ImprovementProposal,
    ImprovementCheckpoint,
    ImprovementApplyRecord,
    ImprovementVerification,
    McpExposureProjection,
    TrajectoryRecord,
    ProviderModelSnapshot,
    ReplayRecord,
    QualityRegressionCase,
    JudgeRoutingDecision,
    ReplayResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionStatus {
    Redacted,
    AlreadySafe,
    RedactionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    Pass,
    Fail,
    Denied,
    Stale,
    Expired,
    RedactionFailed,
    LowConfidence,
    ConflictingEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedNextAction {
    None,
    ContinueSession,
    AskUser,
    RetryEvaluation,
    RequestRollback,
    EscalateToOrchestrator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluatorAuthority {
    AdvisoryOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityActionKind {
    Read,
    Write,
    DestructiveWrite,
    Restore,
    Rollback,
    SelfImprovementApply,
    AppTaskMutation,
    ProcessExecution,
    NetworkAccess,
    SecretAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Plan,
    Default,
    Auto,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionModeSnapshot {
    pub mode: PermissionMode,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    #[serde(default)]
    pub denied_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalContext {
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_ref: Option<ApprovalRequestRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_ref: Option<ApprovalDecisionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_evidence_ref: Option<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointHint {
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_ref: Option<String>,
    pub inspectable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityEvaluationInput {
    pub action_id: String,
    pub action_kind: CapabilityActionKind,
    pub action_digest: String,
    pub target_digest: String,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
    pub permission_mode_snapshot: PermissionModeSnapshot,
    pub approval_context: ApprovalContext,
    pub checkpoint_hint: CheckpointHint,
    pub correlation_id: String,
    pub snapshot_id: String,
    pub snapshot_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDecisionHint {
    AllowCandidate,
    DenyCandidate,
    NeedsApproval,
    NeedsCheckpoint,
    NeedsSecret,
    InsufficientContext,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityVerdict {
    pub hint: CapabilityDecisionHint,
    pub reason: String,
    pub risk_level: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_recommendation: Option<String>,
}

impl CapabilityVerdict {
    pub fn authority_boundary(&self) -> EvaluatorAuthority {
        EvaluatorAuthority::AdvisoryOnly
    }

    pub fn grants_execution_authority(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRequestStatus {
    Pending,
    Approved,
    Denied,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequestRef {
    pub request_id: String,
    pub action_digest: String,
    pub snapshot_digest: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub displayed_risk_summary: String,
    pub status: ApprovalRequestStatus,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalDecisionRef {
    pub decision_id: String,
    pub request_id: String,
    pub action_digest: String,
    pub snapshot_digest: String,
    pub decision: ApprovalDecisionKind,
    pub decided_at_ms: u64,
    pub actor_local_user: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointGateStatus {
    Required,
    Optional,
    Skipped,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointGateDecision {
    pub status: CheckpointGateStatus,
    pub required: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeniedRetryClass {
    RetryAfterUserAction,
    RetryWithFreshSnapshot,
    RetryAfterCheckpoint,
    NotRetryable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeniedOutcome {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_evidence_ref: Option<EvidenceRef>,
    pub retry_class: DeniedRetryClass,
    pub required_next_step: String,
    pub correlation_id: String,
}

pub fn consume_permission_mode_before_verdict(
    input: &CapabilityEvaluationInput,
    verdict: &CapabilityVerdict,
) -> Result<CapabilityVerdict, Box<DeniedOutcome>> {
    if let Some(outcome) = permission_denial_outcome(input) {
        return Err(Box::new(outcome));
    }
    Ok(verdict.clone())
}

pub fn permission_denial_outcome(input: &CapabilityEvaluationInput) -> Option<DeniedOutcome> {
    if input.permission_mode_snapshot.mode == PermissionMode::Deny {
        return Some(denied_outcome(
            "permission_mode_denied",
            "Permission mode denied this action before evaluator hints were considered.",
            input.approval_context.redacted_evidence_ref.clone(),
            DeniedRetryClass::RetryAfterUserAction,
            "Change permission mode or choose a lower-risk action.",
            &input.correlation_id,
        ));
    }

    let denied_capability = input.requested_capabilities.iter().any(|capability| {
        input
            .permission_mode_snapshot
            .denied_capabilities
            .contains(capability)
    });
    if denied_capability {
        return Some(denied_outcome(
            "permission_capability_denied",
            "Permission mode denied at least one requested capability before evaluator hints were considered.",
            input.approval_context.redacted_evidence_ref.clone(),
            DeniedRetryClass::RetryAfterUserAction,
            "Remove the denied capability or request explicit approval through the owner gate.",
            &input.correlation_id,
        ));
    }

    None
}

pub fn validate_approval_decision_consumption(
    request: &ApprovalRequestRef,
    decision: &ApprovalDecisionRef,
    current_snapshot_digest: &str,
    now_ms: u64,
    evidence_ref: Option<EvidenceRef>,
) -> Result<(), Box<DeniedOutcome>> {
    if request.request_id != decision.request_id {
        return Err(Box::new(denied_outcome(
            "approval_request_mismatch",
            "Approval decision does not match the pending approval request.",
            evidence_ref,
            DeniedRetryClass::RetryAfterUserAction,
            "Review the current risk summary and approve the matching request again.",
            &request.correlation_id,
        )));
    }

    if request.action_digest != decision.action_digest {
        return Err(Box::new(denied_outcome(
            "approval_action_mismatch",
            "Approval decision was for a different action and cannot be consumed.",
            evidence_ref,
            DeniedRetryClass::RetryAfterUserAction,
            "Review the current action and approve the matching request again.",
            &request.correlation_id,
        )));
    }

    if request.snapshot_digest != current_snapshot_digest
        || decision.snapshot_digest != current_snapshot_digest
    {
        return Err(Box::new(denied_outcome(
            "stale_approval_snapshot",
            "Approval was based on a stale evaluation snapshot and cannot be consumed.",
            evidence_ref,
            DeniedRetryClass::RetryWithFreshSnapshot,
            "Re-run evaluation with a fresh snapshot before requesting approval again.",
            &request.correlation_id,
        )));
    }

    if request.expires_at_ms <= now_ms || request.status == ApprovalRequestStatus::Expired {
        return Err(Box::new(denied_outcome(
            "approval_expired",
            "Approval expired before it could be consumed.",
            evidence_ref,
            DeniedRetryClass::RetryAfterUserAction,
            "Request approval again before executing the action.",
            &request.correlation_id,
        )));
    }

    if request.status == ApprovalRequestStatus::Denied
        || decision.decision != ApprovalDecisionKind::Approved
    {
        return Err(Box::new(denied_outcome(
            "approval_denied",
            "Approval decision denied this action.",
            evidence_ref,
            DeniedRetryClass::NotRetryable,
            "Choose a different action or revise the requested capability.",
            &request.correlation_id,
        )));
    }

    Ok(())
}

pub fn decide_checkpoint_gate(
    action_kind: &CapabilityActionKind,
    hint: &CheckpointHint,
) -> CheckpointGateDecision {
    let required = hint.required || action_requires_checkpoint(action_kind);
    if required && (hint.checkpoint_ref.is_none() || !hint.inspectable) {
        return CheckpointGateDecision {
            status: CheckpointGateStatus::Blocked,
            required,
            reason: "Checkpoint is required before this action and must be inspectable.".to_owned(),
            checkpoint_ref: hint.checkpoint_ref.clone(),
        };
    }

    if required {
        return CheckpointGateDecision {
            status: CheckpointGateStatus::Required,
            required,
            reason: "Inspectable checkpoint is available for this checkpoint-required action."
                .to_owned(),
            checkpoint_ref: hint.checkpoint_ref.clone(),
        };
    }

    if hint.checkpoint_ref.is_some() {
        return CheckpointGateDecision {
            status: CheckpointGateStatus::Optional,
            required,
            reason: "Checkpoint is present but not required for this action.".to_owned(),
            checkpoint_ref: hint.checkpoint_ref.clone(),
        };
    }

    CheckpointGateDecision {
        status: CheckpointGateStatus::Skipped,
        required,
        reason: "Checkpoint is not required for this action.".to_owned(),
        checkpoint_ref: None,
    }
}

pub fn action_requires_checkpoint(action_kind: &CapabilityActionKind) -> bool {
    matches!(
        action_kind,
        CapabilityActionKind::DestructiveWrite
            | CapabilityActionKind::Restore
            | CapabilityActionKind::Rollback
            | CapabilityActionKind::SelfImprovementApply
            | CapabilityActionKind::AppTaskMutation
    )
}

pub fn denied_outcome(
    code: impl Into<String>,
    message: impl Into<String>,
    redacted_evidence_ref: Option<EvidenceRef>,
    retry_class: DeniedRetryClass,
    required_next_step: impl Into<String>,
    correlation_id: impl Into<String>,
) -> DeniedOutcome {
    DeniedOutcome {
        code: code.into(),
        message: message.into(),
        redacted_evidence_ref,
        retry_class,
        required_next_step: required_next_step.into(),
        correlation_id: correlation_id.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub kind: EvidenceKind,
    pub id: String,
    pub digest: String,
    pub summary: String,
    pub redaction_status: RedactionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_spec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenEvaluationSnapshot {
    pub snapshot_id: String,
    pub created_at_ms: u64,
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub source_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_summary_digest: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub redaction_profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_snapshot_ref: Option<String>,
    #[serde(default)]
    pub redacted_payload: Value,
}

impl FrozenEvaluationSnapshot {
    pub fn new_redacted(
        snapshot_id: impl Into<String>,
        created_at_ms: u64,
        correlation_id: impl Into<String>,
        redaction_profile: impl Into<String>,
        payload: &Value,
    ) -> Self {
        Self {
            snapshot_id: snapshot_id.into(),
            created_at_ms,
            correlation_id: correlation_id.into(),
            session_id: None,
            turn_id: None,
            source_event_ids: Vec::new(),
            context_summary_digest: None,
            evidence_refs: Vec::new(),
            redaction_profile: redaction_profile.into(),
            provider_snapshot_ref: None,
            redacted_payload: redact_value(payload),
        }
    }

    pub fn digest(&self) -> Result<String, serde_json::Error> {
        stable_sha256_digest(&serde_json::to_value(self)?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEvidenceBudget {
    pub max_result_refs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryEvidenceOmittedReason {
    OmittedByBudget,
    OmittedByRedaction,
    OmittedByCutoff,
    OmittedByRelevance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEvidenceRequest {
    pub request_id: String,
    pub session_id: String,
    pub query: String,
    pub evaluator_kind: EvaluatorKind,
    pub budget: MemoryEvidenceBudget,
    pub cutoff: String,
    pub redaction_profile: String,
    pub caller_reason: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEvidenceSet {
    pub evidence_id: String,
    pub request_id: String,
    pub query: String,
    pub source_scope: String,
    pub cutoff: String,
    pub budget: MemoryEvidenceBudget,
    pub created_at_ms: u64,
    pub frozen_at_ms: u64,
    pub candidate_count: usize,
    pub result_count: usize,
    pub omitted_count: usize,
    #[serde(default)]
    pub result_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_ref: Option<EvaluatorSummaryRef>,
    pub redaction_profile: String,
    pub result_digest: String,
    #[serde(default)]
    pub omitted_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_reason: Option<MemoryEvidenceOmittedReason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundedMemoryEvidenceSetInput {
    pub evidence_id: String,
    pub request_id: String,
    pub query: String,
    pub source_scope: String,
    pub cutoff: String,
    pub max_result_refs: usize,
    pub created_at_ms: u64,
    pub frozen_at_ms: u64,
    #[serde(default)]
    pub candidate_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_ref: Option<EvaluatorSummaryRef>,
    pub redaction_profile: String,
    pub omitted_reason: MemoryEvidenceOmittedReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenSessionSearchSnapshot {
    pub snapshot_id: String,
    pub search_input_digest: String,
    #[serde(default)]
    pub matched_event_refs: Vec<EvidenceRef>,
    pub created_at_ms: u64,
    pub result_digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorSummaryRef {
    pub summary_id: String,
    #[serde(default)]
    pub source_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub omitted_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_reason: Option<String>,
    pub summary_digest: String,
    pub confidence: f32,
    pub redaction_status: RedactionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillDisclosureLevel {
    List,
    View,
    Reference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillDisclosureRecord {
    pub disclosure_id: String,
    pub level: SkillDisclosureLevel,
    pub skill_name: String,
    pub source: String,
    pub status: String,
    pub short_description: String,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_manifest_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_task_boundary_ref: Option<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoredSkillLifecycleState {
    Draft,
    DryRunPending,
    DryRunFailed,
    ApprovalPending,
    ActiveCandidate,
    Active,
    Stale,
    Archived,
}

pub type AuthoredSkillRuntimeState = AuthoredSkillLifecycleState;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthoredSkillLifecycle {
    pub skill_id: String,
    pub state: AuthoredSkillLifecycleState,
    pub dry_run_passed: bool,
    pub approval_granted: bool,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorTargetKind {
    Memory,
    SessionSearch,
    Skill,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorActionProposed {
    DeleteMemory,
    ArchiveSkill,
    ActivateSkill,
    DisableSkill,
    RefreshSkill,
    MergeSkill,
    Keep,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorRecommendation {
    pub recommendation_id: String,
    pub target_kind: CuratorTargetKind,
    pub action_proposed: CuratorActionProposed,
    pub reason: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuratorProposalFinalStatus {
    Proposed,
    ApprovalPending,
    Approved,
    Rejected,
    Recorded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuratorProposal {
    pub proposal_id: String,
    pub target_kind: CuratorTargetKind,
    #[serde(default)]
    pub target_refs: Vec<EvidenceRef>,
    pub reason: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub suggested_action: CuratorActionProposed,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<EvidenceRef>,
    pub final_status: CuratorProposalFinalStatus,
}

pub fn memory_result_digest(result_refs: &[EvidenceRef]) -> Result<String, serde_json::Error> {
    stable_sha256_digest(&serde_json::to_value(result_refs)?)
}

pub fn build_bounded_memory_evidence_set(
    input: BoundedMemoryEvidenceSetInput,
) -> Result<MemoryEvidenceSet, serde_json::Error> {
    let candidate_count = input.candidate_refs.len();
    let result_refs = input
        .candidate_refs
        .iter()
        .take(input.max_result_refs)
        .cloned()
        .collect::<Vec<_>>();
    let omitted_refs = input
        .candidate_refs
        .iter()
        .skip(input.max_result_refs)
        .cloned()
        .collect::<Vec<_>>();
    let omitted_count = omitted_refs.len();
    let omitted_reason = if omitted_refs.is_empty() {
        None
    } else {
        Some(input.omitted_reason)
    };
    let result_digest = memory_result_digest(&result_refs)?;

    Ok(MemoryEvidenceSet {
        evidence_id: input.evidence_id,
        request_id: input.request_id,
        query: input.query,
        source_scope: input.source_scope,
        cutoff: input.cutoff,
        budget: MemoryEvidenceBudget {
            max_result_refs: input.max_result_refs,
        },
        created_at_ms: input.created_at_ms,
        frozen_at_ms: input.frozen_at_ms,
        candidate_count,
        result_count: result_refs.len(),
        omitted_count,
        result_refs,
        summary_ref: input.summary_ref,
        redaction_profile: input.redaction_profile,
        result_digest,
        omitted_refs,
        omitted_reason,
    })
}

pub fn session_search_result_digest(
    matched_event_refs: &[EvidenceRef],
) -> Result<String, serde_json::Error> {
    stable_sha256_digest(&serde_json::to_value(matched_event_refs)?)
}

pub fn search_input_digest(search_input: &Value) -> Result<String, serde_json::Error> {
    stable_sha256_digest(search_input)
}

pub fn frozen_session_search_snapshot(
    snapshot_id: impl Into<String>,
    search_input: &Value,
    matched_event_refs: Vec<EvidenceRef>,
    created_at_ms: u64,
) -> Result<FrozenSessionSearchSnapshot, serde_json::Error> {
    let search_input_digest = search_input_digest(search_input)?;
    let result_digest = session_search_result_digest(&matched_event_refs)?;

    Ok(FrozenSessionSearchSnapshot {
        snapshot_id: snapshot_id.into(),
        search_input_digest,
        matched_event_refs,
        created_at_ms,
        result_digest,
    })
}

pub fn frozen_session_search_snapshot_is_fresh(
    snapshot: &FrozenSessionSearchSnapshot,
    current_matched_event_refs: &[EvidenceRef],
) -> Result<bool, serde_json::Error> {
    Ok(snapshot.result_digest == session_search_result_digest(current_matched_event_refs)?)
}

pub fn evaluator_summary_ref(
    summary_id: impl Into<String>,
    source_refs: Vec<EvidenceRef>,
    omitted_refs: Vec<EvidenceRef>,
    omitted_reason: Option<String>,
    redacted_summary: impl AsRef<str>,
    confidence: f32,
    redaction_status: RedactionStatus,
) -> Result<EvaluatorSummaryRef, serde_json::Error> {
    let summary_digest = stable_sha256_digest(&json!(redacted_summary.as_ref()))?;

    Ok(EvaluatorSummaryRef {
        summary_id: summary_id.into(),
        source_refs,
        omitted_refs,
        omitted_reason,
        summary_digest,
        confidence,
        redaction_status,
    })
}

pub fn skill_list_disclosure(
    disclosure_id: impl Into<String>,
    skill_name: impl Into<String>,
    source: impl Into<String>,
    status: impl Into<String>,
    short_description: impl Into<String>,
    digest: impl Into<String>,
) -> SkillDisclosureRecord {
    SkillDisclosureRecord {
        disclosure_id: disclosure_id.into(),
        level: SkillDisclosureLevel::List,
        skill_name: skill_name.into(),
        source: source.into(),
        status: status.into(),
        short_description: short_description.into(),
        digest: digest.into(),
        redacted_body: None,
        body_digest: None,
        evidence_ref: None,
        requester: None,
        approval_ref: None,
        app_manifest_ref: None,
        app_task_boundary_ref: None,
    }
}

pub fn skill_view_disclosure(
    list_record: &SkillDisclosureRecord,
    skill_body: impl AsRef<str>,
) -> Result<SkillDisclosureRecord, serde_json::Error> {
    let skill_body = skill_body.as_ref();
    let body_digest = stable_sha256_digest(&json!(skill_body))?;

    Ok(SkillDisclosureRecord {
        disclosure_id: list_record.disclosure_id.clone(),
        level: SkillDisclosureLevel::View,
        skill_name: list_record.skill_name.clone(),
        source: list_record.source.clone(),
        status: list_record.status.clone(),
        short_description: list_record.short_description.clone(),
        digest: list_record.digest.clone(),
        redacted_body: Some(redact_string(skill_body)),
        body_digest: Some(body_digest),
        evidence_ref: None,
        requester: list_record.requester.clone(),
        approval_ref: list_record.approval_ref.clone(),
        app_manifest_ref: list_record.app_manifest_ref.clone(),
        app_task_boundary_ref: list_record.app_task_boundary_ref.clone(),
    })
}

pub fn skill_reference_disclosure(list_record: &SkillDisclosureRecord) -> SkillDisclosureRecord {
    SkillDisclosureRecord {
        disclosure_id: list_record.disclosure_id.clone(),
        level: SkillDisclosureLevel::Reference,
        skill_name: list_record.skill_name.clone(),
        source: list_record.source.clone(),
        status: list_record.status.clone(),
        short_description: list_record.short_description.clone(),
        digest: list_record.digest.clone(),
        redacted_body: None,
        body_digest: None,
        evidence_ref: Some(EvidenceRef {
            kind: EvidenceKind::SkillDisclosure,
            id: list_record.disclosure_id.clone(),
            digest: list_record.digest.clone(),
            summary: list_record.short_description.clone(),
            redaction_status: RedactionStatus::AlreadySafe,
            owner_spec: Some("018-evaluation-automation-and-self-improvement".to_owned()),
            locator: Some(list_record.skill_name.clone()),
            retention_hint: Some("audit_replay".to_owned()),
        }),
        requester: list_record.requester.clone(),
        approval_ref: list_record.approval_ref.clone(),
        app_manifest_ref: list_record.app_manifest_ref.clone(),
        app_task_boundary_ref: list_record.app_task_boundary_ref.clone(),
    }
}

pub fn authored_skill_lifecycle_draft(
    skill_id: impl Into<String>,
    evidence_refs: Vec<EvidenceRef>,
) -> AuthoredSkillLifecycle {
    AuthoredSkillLifecycle {
        skill_id: skill_id.into(),
        state: AuthoredSkillLifecycleState::Draft,
        dry_run_passed: false,
        approval_granted: false,
        evidence_refs,
    }
}

pub fn authored_skill_can_become_active(lifecycle: &AuthoredSkillLifecycle) -> bool {
    lifecycle.dry_run_passed
        && lifecycle.approval_granted
        && matches!(
            lifecycle.state,
            AuthoredSkillLifecycleState::ActiveCandidate | AuthoredSkillLifecycleState::Active
        )
}

pub fn authored_skill_is_disable_candidate(lifecycle: &AuthoredSkillLifecycle) -> bool {
    lifecycle.state == AuthoredSkillLifecycleState::Stale
}

pub fn authored_skill_is_active_injection_candidate(lifecycle: &AuthoredSkillLifecycle) -> bool {
    lifecycle.state == AuthoredSkillLifecycleState::Active
        && authored_skill_can_become_active(lifecycle)
}

pub fn authored_skill_remains_replay_evidence(lifecycle: &AuthoredSkillLifecycle) -> bool {
    !lifecycle.evidence_refs.is_empty()
        || matches!(
            lifecycle.state,
            AuthoredSkillLifecycleState::Archived | AuthoredSkillLifecycleState::Stale
        )
}

pub fn curator_action_requires_approval(action: &CuratorActionProposed) -> bool {
    !matches!(action, CuratorActionProposed::Keep)
}

pub fn curator_recommendation_allows_execution(
    recommendation: &CuratorRecommendation,
    approval_granted: bool,
) -> bool {
    if matches!(
        recommendation.action_proposed,
        CuratorActionProposed::DeleteMemory | CuratorActionProposed::ActivateSkill
    ) && !approval_granted
    {
        return false;
    }

    !recommendation.requires_approval || approval_granted
}

pub fn curator_proposal(
    proposal_id: impl Into<String>,
    target_kind: CuratorTargetKind,
    target_refs: Vec<EvidenceRef>,
    reason: impl Into<String>,
    evidence_refs: Vec<EvidenceRef>,
    suggested_action: CuratorActionProposed,
    approval_ref: Option<EvidenceRef>,
) -> CuratorProposal {
    let final_status = if approval_ref.is_some() {
        CuratorProposalFinalStatus::ApprovalPending
    } else {
        CuratorProposalFinalStatus::Proposed
    };

    CuratorProposal {
        proposal_id: proposal_id.into(),
        target_kind,
        target_refs,
        reason: reason.into(),
        evidence_refs,
        suggested_action,
        approval_ref,
        final_status,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementProposalStatus {
    Proposed,
    ApprovalPending,
    BlockedApprovalRequired,
    Approved,
    BlockedCheckpointUnavailable,
    Checkpointed,
    Applied,
    AppliedUnverified,
    Verified,
    Recorded,
    RolledBack,
    BlockedRollbackUnavailable,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementTargetKind {
    ConfigProfile,
    Skill,
    Prompt,
    ToolExposure,
    AppManifestRef,
    AutomationRule,
}

impl ImprovementTargetKind {
    pub fn as_scope(&self) -> &'static str {
        match self {
            Self::ConfigProfile => "config_profile",
            Self::Skill => "skill",
            Self::Prompt => "prompt",
            Self::ToolExposure => "tool_exposure",
            Self::AppManifestRef => "app_manifest_ref",
            Self::AutomationRule => "automation_rule",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerPrimitiveRef {
    pub owner_spec: String,
    pub primitive_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementProposal {
    pub proposal_id: String,
    pub target_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_source: Option<String>,
    pub proposed_diff_summary_ref: EvidenceRef,
    pub risk_summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub expected_benefit: String,
    pub rollback_plan: String,
    pub status: ImprovementProposalStatus,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementApproval {
    pub proposal_id: String,
    pub request_ref: ApprovalRequestRef,
    pub decision_ref: ApprovalDecisionRef,
    #[serde(default)]
    pub approved_scope: Vec<String>,
    pub expires_at_ms: u64,
    pub actor_local_user: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementCheckpoint {
    pub checkpoint_ref: String,
    pub target_digest_before: String,
    pub inspect_ref: EvidenceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_capability: Option<OwnerPrimitiveRef>,
    pub proposal_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementApplyRecord {
    pub apply_id: String,
    pub proposal_id: String,
    pub owner_spec: String,
    pub action_ref: OwnerPrimitiveRef,
    pub input_digest: String,
    pub outcome_ref: EvidenceRef,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementRollbackResult {
    RolledBack,
    BlockedManualRecoveryRequired,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementRollbackFinalState {
    RestoredCheckpoint,
    ManualRecoveryRequired,
    RollbackFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementRollbackRecord {
    pub rollback_id: String,
    pub proposal_id: String,
    pub checkpoint_ref: String,
    pub verify_failure_ref: EvidenceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_rollback_ref: Option<OwnerPrimitiveRef>,
    pub result: ImprovementRollbackResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_recovery_hint: Option<String>,
    pub final_state: ImprovementRollbackFinalState,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementVerificationNextAction {
    RecordSuccess,
    Rollback,
    ReportFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementVerification {
    pub verification_id: String,
    pub expected_behavior: String,
    pub observed_result_ref: EvidenceRef,
    pub passed: bool,
    pub next_action: ImprovementVerificationNextAction,
    pub proposal_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpExposureProjection {
    pub tool_or_resource_id: String,
    pub requested_exposure: String,
    pub current_exposure: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_deny_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_ref: Option<ApprovalDecisionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementActorAuthority {
    Evaluator,
    AppTask,
    LocalUser,
    OwnerPrimitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementAuthorityAction {
    CreateProposal,
    Approve,
    Apply,
    Rollback,
}

pub fn improvement_proposal_can_affect_runtime(proposal: &ImprovementProposal) -> bool {
    matches!(
        proposal.status,
        ImprovementProposalStatus::Applied
            | ImprovementProposalStatus::AppliedUnverified
            | ImprovementProposalStatus::Verified
            | ImprovementProposalStatus::Recorded
            | ImprovementProposalStatus::RolledBack
    )
}

pub fn improvement_approval_changes_runtime_behavior(
    proposal: &ImprovementProposal,
    _approval: &ImprovementApproval,
) -> bool {
    improvement_proposal_can_affect_runtime(proposal)
}

pub fn app_task_improvement_authority(
    actor: &ImprovementActorAuthority,
    action: &ImprovementAuthorityAction,
) -> bool {
    match (actor, action) {
        (ImprovementActorAuthority::AppTask, ImprovementAuthorityAction::CreateProposal) => true,
        (ImprovementActorAuthority::AppTask, _) => false,
        (ImprovementActorAuthority::LocalUser, ImprovementAuthorityAction::Approve) => true,
        (ImprovementActorAuthority::OwnerPrimitive, ImprovementAuthorityAction::Apply) => true,
        (ImprovementActorAuthority::OwnerPrimitive, ImprovementAuthorityAction::Rollback) => true,
        (ImprovementActorAuthority::Evaluator, ImprovementAuthorityAction::CreateProposal) => true,
        (ImprovementActorAuthority::Evaluator, _) => false,
        _ => false,
    }
}

pub fn validate_improvement_apply_readiness(
    proposal: &ImprovementProposal,
    approval: &ImprovementApproval,
    checkpoint: &ImprovementCheckpoint,
    checkpoint_gate: &CheckpointGateDecision,
    now_ms: u64,
    evidence_ref: Option<EvidenceRef>,
) -> Result<(), Box<DeniedOutcome>> {
    if proposal.status != ImprovementProposalStatus::Checkpointed {
        return Err(Box::new(denied_outcome(
            "improvement_not_checkpointed",
            "Self-improvement apply requires an approved proposal with an inspectable checkpoint.",
            evidence_ref,
            DeniedRetryClass::RetryAfterCheckpoint,
            "Approve the proposal and create an inspectable checkpoint before applying.",
            &proposal.correlation_id,
        )));
    }

    if approval.proposal_id != proposal.proposal_id
        || approval.decision_ref.decision != ApprovalDecisionKind::Approved
        || approval.request_ref.status != ApprovalRequestStatus::Approved
        || !improvement_approved_scope_matches(proposal, approval)
        || approval.expires_at_ms <= now_ms
        || approval.request_ref.expires_at_ms <= now_ms
    {
        return Err(Box::new(denied_outcome(
            "improvement_approval_not_ready",
            "Self-improvement apply requires a matching approved approval request and decision.",
            evidence_ref,
            DeniedRetryClass::RetryAfterUserAction,
            "Request and consume matching local-user approval before applying.",
            &proposal.correlation_id,
        )));
    }

    if checkpoint.proposal_id != proposal.proposal_id
        || checkpoint.rollback_capability.is_none()
        || checkpoint_gate.status == CheckpointGateStatus::Blocked
        || checkpoint_gate.checkpoint_ref.as_deref() != Some(checkpoint.checkpoint_ref.as_str())
    {
        return Err(Box::new(denied_outcome(
            "improvement_checkpoint_not_ready",
            "Self-improvement apply requires PRD 002 checkpoint and rollback readiness.",
            evidence_ref,
            DeniedRetryClass::RetryAfterCheckpoint,
            "Create an inspectable checkpoint with an owner rollback primitive before applying.",
            &proposal.correlation_id,
        )));
    }

    Ok(())
}

fn improvement_approved_scope_matches(
    proposal: &ImprovementProposal,
    approval: &ImprovementApproval,
) -> bool {
    match proposal.target_ref.as_ref() {
        Some(target_ref) => approval
            .approved_scope
            .iter()
            .any(|scope| scope == target_ref),
        None => approval
            .approved_scope
            .iter()
            .any(|scope| scope == &proposal.target_kind),
    }
}

pub fn default_mcp_exposure_projection(
    tool_or_resource_id: impl Into<String>,
    requested_exposure: impl Into<String>,
    current_exposure: impl Into<String>,
    default_deny_reason: impl Into<String>,
) -> McpExposureProjection {
    McpExposureProjection {
        tool_or_resource_id: tool_or_resource_id.into(),
        requested_exposure: requested_exposure.into(),
        current_exposure: current_exposure.into(),
        default_deny_reason: Some(default_deny_reason.into()),
        approval_ref: None,
        proposal_id: None,
        correlation_id: None,
    }
}

pub fn mcp_exposure_can_widen(projection: &McpExposureProjection) -> bool {
    if projection.requested_exposure == projection.current_exposure {
        return true;
    }

    projection.proposal_id.is_some()
        && projection.correlation_id.is_some()
        && projection
            .approval_ref
            .as_ref()
            .is_some_and(|approval| approval.decision == ApprovalDecisionKind::Approved)
}

pub fn failed_improvement_verification_next_action(
    verification: &ImprovementVerification,
    checkpoint: Option<&ImprovementCheckpoint>,
    owner_rollback_primitive_ready: bool,
) -> ImprovementVerificationNextAction {
    if verification.passed {
        return ImprovementVerificationNextAction::RecordSuccess;
    }

    if owner_rollback_primitive_ready
        && checkpoint
            .and_then(|checkpoint| checkpoint.rollback_capability.as_ref())
            .is_some()
    {
        ImprovementVerificationNextAction::Rollback
    } else {
        ImprovementVerificationNextAction::ReportFailed
    }
}

pub fn self_improvement_records_share_correlation_id(
    proposal: &ImprovementProposal,
    approval: Option<&ImprovementApproval>,
    checkpoint: Option<&ImprovementCheckpoint>,
    apply_record: Option<&ImprovementApplyRecord>,
    verification: Option<&ImprovementVerification>,
    exposure: Option<&McpExposureProjection>,
) -> bool {
    let correlation_id = proposal.correlation_id.as_str();
    let approval_matches = match approval {
        Some(approval) => approval.correlation_id == correlation_id,
        None => true,
    };
    let checkpoint_matches = match checkpoint {
        Some(checkpoint) => checkpoint.correlation_id == correlation_id,
        None => true,
    };
    let apply_record_matches = match apply_record {
        Some(apply_record) => apply_record.correlation_id == correlation_id,
        None => true,
    };
    let verification_matches = match verification {
        Some(verification) => verification.correlation_id == correlation_id,
        None => true,
    };
    let exposure_matches = match exposure {
        Some(exposure) => exposure.correlation_id.as_deref() == Some(correlation_id),
        None => true,
    };

    approval_matches
        && checkpoint_matches
        && apply_record_matches
        && verification_matches
        && exposure_matches
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorRequestEnvelope {
    pub request_id: String,
    pub evaluator_kind: EvaluatorKind,
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub source: EvaluationTriggerSource,
    pub snapshot_digest: String,
    pub redaction_profile: String,
    pub caller_intent: String,
}

impl EvaluatorRequestEnvelope {
    pub fn authority_boundary(&self) -> EvaluatorAuthority {
        EvaluatorAuthority::AdvisoryOnly
    }

    pub fn grants_execution_authority(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorVerdictEnvelope {
    pub verdict_kind: VerdictKind,
    pub reason: String,
    pub confidence: f32,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub suggested_next_action: SuggestedNextAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    pub redaction_status: RedactionStatus,
    pub evaluator_version: String,
}

impl EvaluatorVerdictEnvelope {
    pub fn authority_boundary(&self) -> EvaluatorAuthority {
        EvaluatorAuthority::AdvisoryOnly
    }

    pub fn grants_execution_authority(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryStats {
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    pub model_call_count: u32,
    pub tool_call_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrajectoryRecord {
    pub trajectory_id: String,
    pub session_ref: EvidenceRef,
    #[serde(default)]
    pub event_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub model_call_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub tool_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub evaluator_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub provider_snapshot_refs: Vec<EvidenceRef>,
    pub redaction_profile: String,
    pub stats: TrajectoryStats,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRouteRole {
    MainAssistant,
    AuxiliaryJudge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuxiliaryJudgeRole {
    #[serde(rename = "goal_completion_judge")]
    GoalCompletion,
    #[serde(rename = "capability_judge")]
    Capability,
    #[serde(rename = "task_outcome_judge")]
    TaskOutcome,
    #[serde(rename = "replay_comparison_judge")]
    ReplayJudge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeFallbackReason {
    DefaultProviderFailed,
    PrimaryUnavailable,
    ModelUnavailable,
    BudgetExceeded,
    PolicyDenied,
    Timeout,
    InvalidOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgeDeniedReason {
    BudgetExceeded,
    PolicyDenied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFallbackStep {
    pub provider_id: String,
    pub model_id: String,
    pub reason: JudgeFallbackReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelSnapshot {
    pub snapshot_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub profile_ref: String,
    pub role: ProviderRouteRole,
    pub routing_reason: String,
    #[serde(default)]
    pub fallback_chain: Vec<ProviderFallbackStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_role: Option<AuxiliaryJudgeRole>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeRoutingDecision {
    pub decision_id: String,
    pub evaluator_kind: EvaluatorKind,
    pub judge_role: AuxiliaryJudgeRole,
    pub preferred_provider_id: String,
    pub preferred_model_id: String,
    pub selected_provider_id: String,
    pub selected_model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<JudgeFallbackReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denied_reason: Option<JudgeDeniedReason>,
    pub provider_snapshot_ref: EvidenceRef,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayMode {
    Deterministic,
    JudgeAssisted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplaySafeEffectsPolicy {
    RecordedOutcomeOnly,
    RecordedOrSafeMockOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayResultClassification {
    Pass,
    Regression,
    Inconclusive,
    InvalidFixture,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub classification: ReplayResultClassification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_verdict: Option<VerdictKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_outcome: Option<TaskOutcomeClass>,
    pub diff_summary_ref: EvidenceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayRecord {
    pub replay_id: String,
    pub trajectory_id: String,
    pub mode: ReplayMode,
    pub safe_effects_policy: ReplaySafeEffectsPolicy,
    pub started_at_ms: u64,
    pub result: ReplayResult,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityRegressionCase {
    pub case_id: String,
    pub trajectory_ref: EvidenceRef,
    pub expected_verdict: VerdictKind,
    pub expected_outcome: TaskOutcomeClass,
    #[serde(default)]
    pub redacted_evidence_refs: Vec<EvidenceRef>,
    pub owner_note: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceBand {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayDatasetItem {
    pub dataset_id: String,
    pub case_id: String,
    #[serde(default)]
    pub trajectory_refs: Vec<EvidenceRef>,
    pub expected_verdict: VerdictKind,
    pub expected_outcome: TaskOutcomeClass,
    pub expected_projection_status: ProjectionStatus,
    pub expected_confidence_band: ConfidenceBand,
    #[serde(default)]
    pub allowed_judge_roles: Vec<AuxiliaryJudgeRole>,
    pub redaction_profile: String,
    #[serde(default)]
    pub tool_outcome_policies: Vec<ReplayToolOutcomePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_verdict: Option<VerdictKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_outcome: Option<TaskOutcomeClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_projection_status: Option<ProjectionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_confidence_band: Option<ConfidenceBand>,
    #[serde(default)]
    pub auxiliary_judge_routes: Vec<AuxiliaryJudgeRoute>,
    #[serde(default)]
    pub diagnostics_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub coverage_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplaySafeMockOutcome {
    pub mock_reason: String,
    pub source: String,
    pub expected_schema_digest: String,
    #[serde(default)]
    pub limitations: Vec<String>,
    pub outcome_ref: EvidenceRef,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayToolOutcomePolicy {
    pub tool_call_ref: EvidenceRef,
    pub expected_schema_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_outcome_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_mock_outcome: Option<ReplaySafeMockOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuxiliaryJudgeRouteFinalStatus {
    PrimarySelected,
    FallbackSelected,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuxiliaryJudgeRoute {
    pub route_id: String,
    pub judge_role: AuxiliaryJudgeRole,
    pub provider_snapshot: ProviderModelSnapshot,
    #[serde(default)]
    pub fallback_chain: Vec<ProviderFallbackStep>,
    pub routing_reason: String,
    pub final_status: AuxiliaryJudgeRouteFinalStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayComparisonStatus {
    Match,
    VerdictKindMismatch,
    TaskOutcomeMismatch,
    ConfidenceBandMismatch,
    ProjectionStatusMismatch,
    MissingActual,
    BlockedMissingReplayOutcome,
    SchemaMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayComparisonSeverity {
    None,
    Low,
    Medium,
    High,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayRunStatus {
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayCaseResult {
    pub case_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_verdict: Option<VerdictKind>,
    pub comparison_status: ReplayComparisonStatus,
    pub severity: ReplayComparisonSeverity,
    pub diff_summary: String,
    #[serde(default)]
    pub judge_route_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub diagnostics_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub coverage_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayRunRecord {
    pub run_id: String,
    pub dataset_id: String,
    #[serde(default)]
    pub selected_cases: Vec<String>,
    pub started_at_ms: u64,
    pub completed_at_ms: u64,
    pub status: ReplayRunStatus,
    pub diagnostics_ref: EvidenceRef,
    #[serde(default)]
    pub case_results: Vec<ReplayCaseResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffectKind {
    ReadOnly,
    SafeWrite,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolReplayInput {
    pub tool_ref: EvidenceRef,
    pub effect: ToolEffectKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_outcome_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_mock_outcome_ref: Option<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayToolOutcomeSource {
    RecordedOutcome,
    SafeMockOutcome,
    InvalidFixture,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayToolPlan {
    pub execute_live_tool: bool,
    pub outcome_source: ReplayToolOutcomeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_classification: Option<ReplayResultClassification>,
}

pub fn trajectory_preserves_replay_refs_with_redaction(trajectory: &TrajectoryRecord) -> bool {
    !trajectory.trajectory_id.is_empty()
        && !trajectory.redaction_profile.is_empty()
        && !trajectory.correlation_id.is_empty()
        && is_redacted_evidence(&trajectory.session_ref)
        && !trajectory.event_refs.is_empty()
        && !trajectory.model_call_refs.is_empty()
        && !trajectory.tool_refs.is_empty()
        && !trajectory.evaluator_refs.is_empty()
        && !trajectory.provider_snapshot_refs.is_empty()
        && trajectory
            .all_replay_refs()
            .into_iter()
            .all(is_redacted_evidence)
}

impl TrajectoryRecord {
    pub fn all_replay_refs(&self) -> Vec<&EvidenceRef> {
        let mut refs = Vec::with_capacity(
            1 + self.event_refs.len()
                + self.model_call_refs.len()
                + self.tool_refs.len()
                + self.evaluator_refs.len()
                + self.provider_snapshot_refs.len(),
        );
        refs.push(&self.session_ref);
        refs.extend(self.event_refs.iter());
        refs.extend(self.model_call_refs.iter());
        refs.extend(self.tool_refs.iter());
        refs.extend(self.evaluator_refs.iter());
        refs.extend(self.provider_snapshot_refs.iter());
        refs
    }
}

pub fn replay_tool_plan(
    policy: &ReplaySafeEffectsPolicy,
    input: &ToolReplayInput,
) -> ReplayToolPlan {
    if input.recorded_outcome_ref.is_some() {
        return ReplayToolPlan {
            execute_live_tool: false,
            outcome_source: ReplayToolOutcomeSource::RecordedOutcome,
            result_classification: None,
        };
    }

    if matches!(policy, ReplaySafeEffectsPolicy::RecordedOrSafeMockOutcome)
        && input.safe_mock_outcome_ref.is_some()
    {
        return ReplayToolPlan {
            execute_live_tool: false,
            outcome_source: ReplayToolOutcomeSource::SafeMockOutcome,
            result_classification: None,
        };
    }

    ReplayToolPlan {
        execute_live_tool: false,
        outcome_source: ReplayToolOutcomeSource::InvalidFixture,
        result_classification: Some(ReplayResultClassification::InvalidFixture),
    }
}

pub fn blocked_missing_replay_outcome(input: &ToolReplayInput) -> ReplayToolPlan {
    ReplayToolPlan {
        execute_live_tool: false,
        outcome_source: ReplayToolOutcomeSource::InvalidFixture,
        result_classification: Some(if matches!(input.effect, ToolEffectKind::Destructive) {
            ReplayResultClassification::InvalidFixture
        } else {
            ReplayResultClassification::Inconclusive
        }),
    }
}

pub fn replay_safe_mock_schema_matches(policy: &ReplayToolOutcomePolicy) -> bool {
    policy
        .safe_mock_outcome
        .as_ref()
        .is_some_and(|mock| mock.expected_schema_digest == policy.expected_schema_digest)
}

pub fn compare_replay_dataset_item(item: &ReplayDatasetItem) -> ReplayCaseResult {
    let (comparison_status, severity, diff_summary) = match (
        &item.actual_verdict,
        &item.actual_outcome,
        &item.actual_projection_status,
        &item.actual_confidence_band,
    ) {
        (None, _, _, _) | (_, None, _, _) | (_, _, None, _) | (_, _, _, None) => (
            ReplayComparisonStatus::MissingActual,
            ReplayComparisonSeverity::Medium,
            "missing replay actual result".to_owned(),
        ),
        (
            Some(actual_verdict),
            Some(actual_outcome),
            Some(actual_projection),
            Some(actual_band),
        ) => {
            if actual_verdict != &item.expected_verdict {
                (
                    ReplayComparisonStatus::VerdictKindMismatch,
                    ReplayComparisonSeverity::High,
                    "actual verdict kind differed from expected verdict".to_owned(),
                )
            } else if actual_outcome != &item.expected_outcome {
                (
                    ReplayComparisonStatus::TaskOutcomeMismatch,
                    ReplayComparisonSeverity::Medium,
                    "actual task outcome differed from expected outcome".to_owned(),
                )
            } else if actual_projection != &item.expected_projection_status {
                (
                    ReplayComparisonStatus::ProjectionStatusMismatch,
                    ReplayComparisonSeverity::Medium,
                    "actual projection status differed from expected projection".to_owned(),
                )
            } else if actual_band != &item.expected_confidence_band {
                (
                    ReplayComparisonStatus::ConfidenceBandMismatch,
                    ReplayComparisonSeverity::Low,
                    "actual confidence band differed from expected confidence band".to_owned(),
                )
            } else {
                (
                    ReplayComparisonStatus::Match,
                    ReplayComparisonSeverity::None,
                    "actual replay matched expected result".to_owned(),
                )
            }
        }
    };

    ReplayCaseResult {
        case_id: item.case_id.clone(),
        actual_verdict: item.actual_verdict.clone(),
        comparison_status,
        severity,
        diff_summary,
        judge_route_refs: item
            .auxiliary_judge_routes
            .iter()
            .map(|route| EvidenceRef {
                kind: EvidenceKind::JudgeRoutingDecision,
                id: route.route_id.clone(),
                digest: route.provider_snapshot.snapshot_id.clone(),
                summary: route.routing_reason.clone(),
                redaction_status: RedactionStatus::Redacted,
                owner_spec: Some("018".to_owned()),
                locator: Some(format!("replay://judge-route/{}", route.route_id)),
                retention_hint: Some("local".to_owned()),
            })
            .collect(),
        blocked_reason: None,
        diagnostics_refs: item.diagnostics_refs.clone(),
        coverage_refs: item.coverage_refs.clone(),
    }
}

pub fn judge_routing_records_distinct_fallback_and_denial(decision: &JudgeRoutingDecision) -> bool {
    decision.fallback_reason.is_some()
        && decision.denied_reason.is_some()
        && !matches!(
            (&decision.fallback_reason, &decision.denied_reason),
            (
                Some(JudgeFallbackReason::BudgetExceeded),
                Some(JudgeDeniedReason::BudgetExceeded)
            ) | (
                Some(JudgeFallbackReason::PolicyDenied),
                Some(JudgeDeniedReason::PolicyDenied)
            )
        )
}

pub fn compare_regression_case(
    regression_case: &QualityRegressionCase,
    replay_result: &ReplayResult,
) -> ReplayResultClassification {
    match replay_result.classification {
        ReplayResultClassification::InvalidFixture => ReplayResultClassification::InvalidFixture,
        ReplayResultClassification::Inconclusive => ReplayResultClassification::Inconclusive,
        ReplayResultClassification::Pass | ReplayResultClassification::Regression => {
            match (&replay_result.actual_verdict, &replay_result.actual_outcome) {
                (Some(actual_verdict), Some(actual_outcome))
                    if actual_verdict == &regression_case.expected_verdict
                        && actual_outcome == &regression_case.expected_outcome =>
                {
                    ReplayResultClassification::Pass
                }
                (Some(_), Some(_)) => ReplayResultClassification::Regression,
                _ => ReplayResultClassification::Inconclusive,
            }
        }
    }
}

pub fn diagnostics_export_allowed_for_replay_evidence(evidence_refs: &[EvidenceRef]) -> bool {
    !evidence_refs.is_empty()
        && evidence_refs.iter().all(|evidence_ref| {
            is_redacted_evidence(evidence_ref) && is_replay_evidence(evidence_ref)
        })
}

pub fn provider_routes_are_separated_and_ledgerable(
    main_snapshot: &ProviderModelSnapshot,
    evaluator_snapshot: &ProviderModelSnapshot,
    routing_decision: &JudgeRoutingDecision,
) -> bool {
    main_snapshot.role == ProviderRouteRole::MainAssistant
        && main_snapshot.evaluator_role.is_none()
        && evaluator_snapshot.role == ProviderRouteRole::AuxiliaryJudge
        && evaluator_snapshot.evaluator_role.as_ref() == Some(&routing_decision.judge_role)
        && routing_decision.provider_snapshot_ref.id == evaluator_snapshot.snapshot_id
        && routing_decision.selected_provider_id == evaluator_snapshot.provider_id
        && routing_decision.selected_model_id == evaluator_snapshot.model_id
        && routing_decision.fallback_reason.is_some()
}

fn is_redacted_evidence(evidence_ref: &EvidenceRef) -> bool {
    evidence_ref.redaction_status == RedactionStatus::Redacted
}

fn is_replay_evidence(evidence_ref: &EvidenceRef) -> bool {
    matches!(
        evidence_ref.kind,
        EvidenceKind::TrajectoryRecord
            | EvidenceKind::ProviderModelSnapshot
            | EvidenceKind::ReplayRecord
            | EvidenceKind::QualityRegressionCase
            | EvidenceKind::JudgeRoutingDecision
            | EvidenceKind::ReplayResult
            | EvidenceKind::EvaluatorSummary
            | EvidenceKind::DiagnosticRecord
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLedgerStatus {
    Queued,
    Running,
    Delivered,
    TimedOut,
    RetryRequested,
    RollbackRequested,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskLedgerRecord {
    pub record_id: String,
    pub task_id: String,
    pub correlation_id: String,
    pub source: EvaluationTriggerSource,
    pub status: TaskLedgerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_or_rollback_ref: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationLedgerRecord {
    pub record_id: String,
    pub evaluator_kind: EvaluatorKind,
    pub request: EvaluatorRequestEnvelope,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    pub verdict: EvaluatorVerdictEnvelope,
    pub authority_boundary: EvaluatorAuthority,
    pub created_at_ms: u64,
}

impl EvaluationLedgerRecord {
    pub fn projection(&self) -> EvaluationLedgerProjection {
        EvaluationLedgerProjection {
            record_id: self.record_id.clone(),
            evaluator_kind: self.evaluator_kind.clone(),
            request_id: self.request.request_id.clone(),
            correlation_id: self.request.correlation_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            snapshot_digest: self.snapshot_digest.clone(),
            verdict_kind: self.verdict.verdict_kind.clone(),
            authority_boundary: self.authority_boundary.clone(),
            created_at_ms: self.created_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationLedgerProjection {
    pub record_id: String,
    pub evaluator_kind: EvaluatorKind,
    pub request_id: String,
    pub correlation_id: String,
    pub snapshot_id: String,
    pub snapshot_digest: String,
    pub verdict_kind: VerdictKind,
    pub authority_boundary: EvaluatorAuthority,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ledger_kind", content = "record", rename_all = "snake_case")]
pub enum EvaluatorLedgerRecord {
    Task(TaskLedgerRecord),
    Evaluation(EvaluationLedgerRecord),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationJobSource {
    User,
    ApprovedRuntime,
    App,
    Skill,
    LocalApi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationExecutionMode {
    SkillBackedAgent,
    ScriptOnly,
    NoAgentCheck,
    AppTask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationSchedule {
    OneShot {
        scheduled_at_ms: u64,
    },
    Recurring {
        schedule_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_run_at_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationJobStatus {
    Active,
    Paused,
    Completed,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationJob {
    pub job_id: String,
    pub source: AutomationJobSource,
    pub schedule: AutomationSchedule,
    #[serde(default)]
    pub execution_modes: Vec<AutomationExecutionMode>,
    #[serde(default)]
    pub capability_requirements: Vec<String>,
    pub owner_ref: String,
    pub status: AutomationJobStatus,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerKind {
    ScheduledWake,
    Manual,
    Continuation,
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRun {
    pub run_id: String,
    pub job_id: String,
    pub trigger: AutomationTriggerKind,
    pub status: AutomationRunStatus,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<BackgroundResultRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_verdict_id: Option<String>,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunTriggerKind {
    Heartbeat,
    Cron,
    SubagentResult,
    AppTaskResult,
    ChannelEvent,
    LocalApiBackground,
    ManualResume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunState {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Suppressed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTriggerRef {
    pub runtime_service_event_id: String,
    pub source_type: String,
    pub source_owner: String,
    pub received_at_ms: u64,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRunRequest {
    pub run_id: String,
    pub job_id: String,
    pub trigger_kind: AutomationRunTriggerKind,
    pub trigger_ref: AutomationTriggerRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    pub execution_mode: AutomationExecutionMode,
    pub timeout_policy_ref: String,
    pub retry_policy_ref: String,
    pub delivery_policy_ref: String,
    pub recursion_guard_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRunStateRecord {
    pub run_id: String,
    pub job_id: String,
    pub trigger_kind: AutomationRunTriggerKind,
    pub trigger_ref: AutomationTriggerRef,
    pub state: AutomationRunState,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_status: Option<ProjectionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppress_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTimeoutDisposition {
    Terminal,
    Retryable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTimeoutRecord {
    pub timeout_id: String,
    pub run_id: String,
    pub disposition: AutomationTimeoutDisposition,
    pub retry_policy_ref: String,
    pub attempt_number: u32,
    pub last_failure_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_eligible_wake_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRetryRecord {
    pub retry_id: String,
    pub run_id: String,
    pub retry_policy_ref: String,
    pub attempt_number: u32,
    pub last_failure_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_eligible_wake_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationDeliveryRecord {
    pub delivery_id: String,
    pub run_id: String,
    pub target_surface: ProjectionSurface,
    pub severity: DeliverySeverity,
    pub redacted_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppress_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRecursionGuard {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    pub depth: u32,
    pub max_depth: u32,
    #[serde(default)]
    pub parent_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationRecursionGuardDecision {
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcomeClass {
    Notify,
    Suppress,
    #[serde(rename = "continue")]
    ContinueTask,
    Escalate,
    Verify,
    Rollback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcomeNextAction {
    Delivery,
    NoDelivery,
    ContinueWithGates,
    UserAttention,
    VerificationRequired,
    RollbackRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOutcomeNextActionSemantics {
    pub action: TaskOutcomeNextAction,
    pub delivery_required: bool,
    pub continuation_requires_persistent_goal: bool,
    pub continuation_requires_budget: bool,
    pub continuation_requires_recursion_guard: bool,
    pub continuation_requires_permission_gate: bool,
    pub user_attention_required: bool,
    pub verification_required: bool,
    pub automatic_success: bool,
    pub rollback_requires_checkpoint: bool,
    pub rollback_requires_primitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskOutcomeVerdict {
    pub verdict_id: String,
    pub class: TaskOutcomeClass,
    pub reason: String,
    pub severity: DeliverySeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_hint: Option<String>,
    pub next_action_hint: TaskOutcomeNextAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_hint: Option<String>,
    pub run_id: String,
    pub job_id: String,
    pub result_ref: BackgroundResultRef,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSourceKind {
    Heartbeat,
    Cron,
    Subagent,
    App,
    Channel,
    LocalApi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundExitStatus {
    Success,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundResultRef {
    pub source_kind: BackgroundSourceKind,
    pub source_id: String,
    pub redacted_payload_digest: String,
    pub exit_status: BackgroundExitStatus,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_class: Option<String>,
    pub correlation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundResultTiming {
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecursionGuard {
    pub root_trigger_id: String,
    pub depth: u32,
    #[serde(default)]
    pub source_chain: Vec<String>,
    pub max_depth: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecursionGuardDecision {
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliverySeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Delivered,
    Suppressed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryRecord {
    pub delivery_id: String,
    pub run_id: String,
    pub job_id: String,
    pub result_ref: BackgroundResultRef,
    pub outcome_verdict_id: String,
    pub correlation_id: String,
    pub destination: String,
    pub rendered_summary_ref: String,
    pub severity: DeliverySeverity,
    pub status: DeliveryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_hint: Option<String>,
}

pub fn normalize_background_result_ref(
    source_kind: BackgroundSourceKind,
    source_id: impl Into<String>,
    redacted_payload_digest: impl Into<String>,
    exit_status: BackgroundExitStatus,
    timing: BackgroundResultTiming,
    error_class: Option<String>,
    correlation_id: impl Into<String>,
) -> BackgroundResultRef {
    BackgroundResultRef {
        source_kind,
        source_id: source_id.into(),
        redacted_payload_digest: redacted_payload_digest.into(),
        exit_status,
        started_at_ms: timing.started_at_ms,
        completed_at_ms: timing.completed_at_ms,
        error_class,
        correlation_id: correlation_id.into(),
    }
}

pub fn automation_run_idempotency_key(job_id: &str, trigger_ref: &AutomationTriggerRef) -> String {
    let mut hasher = Sha256::new();
    hasher.update(job_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(trigger_ref.idempotency_key.as_bytes());
    let digest = hasher.finalize();
    format!(
        "automation-run-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
    )
}

pub fn automation_trigger_evaluation_source(
    trigger_kind: &AutomationRunTriggerKind,
) -> EvaluationTriggerSource {
    match trigger_kind {
        AutomationRunTriggerKind::Heartbeat => EvaluationTriggerSource::Heartbeat,
        AutomationRunTriggerKind::Cron => EvaluationTriggerSource::ScheduledJob,
        AutomationRunTriggerKind::SubagentResult => EvaluationTriggerSource::Subagent,
        AutomationRunTriggerKind::AppTaskResult => EvaluationTriggerSource::AppTask,
        AutomationRunTriggerKind::ChannelEvent => EvaluationTriggerSource::Channel,
        AutomationRunTriggerKind::LocalApiBackground => EvaluationTriggerSource::LocalApi,
        AutomationRunTriggerKind::ManualResume => EvaluationTriggerSource::ManualReplay,
    }
}

pub fn automation_trigger_background_source(
    trigger_kind: &AutomationRunTriggerKind,
) -> Option<BackgroundSourceKind> {
    match trigger_kind {
        AutomationRunTriggerKind::Heartbeat => Some(BackgroundSourceKind::Heartbeat),
        AutomationRunTriggerKind::Cron => Some(BackgroundSourceKind::Cron),
        AutomationRunTriggerKind::SubagentResult => Some(BackgroundSourceKind::Subagent),
        AutomationRunTriggerKind::AppTaskResult => Some(BackgroundSourceKind::App),
        AutomationRunTriggerKind::ChannelEvent => Some(BackgroundSourceKind::Channel),
        AutomationRunTriggerKind::LocalApiBackground => Some(BackgroundSourceKind::LocalApi),
        AutomationRunTriggerKind::ManualResume => None,
    }
}

pub fn automation_run_state_projection_status(
    state: &AutomationRunState,
) -> Option<ProjectionStatus> {
    match state {
        AutomationRunState::Queued | AutomationRunState::Running => Some(ProjectionStatus::Pending),
        AutomationRunState::Succeeded => Some(ProjectionStatus::Success),
        AutomationRunState::Failed
        | AutomationRunState::TimedOut
        | AutomationRunState::Cancelled => Some(ProjectionStatus::Failed),
        AutomationRunState::Suppressed => Some(ProjectionStatus::Blocked),
    }
}

pub fn automation_run_state_should_evaluate(state: &AutomationRunState) -> bool {
    matches!(
        state,
        AutomationRunState::Queued | AutomationRunState::Running
    )
}

pub fn classify_automation_timeout(
    timeout_id: impl Into<String>,
    run_id: impl Into<String>,
    retry_policy_ref: impl Into<String>,
    attempt_number: u32,
    max_attempts: u32,
    last_failure_reason: impl Into<String>,
    next_eligible_wake_ref: Option<String>,
) -> AutomationTimeoutRecord {
    let retryable = attempt_number < max_attempts && next_eligible_wake_ref.is_some();
    AutomationTimeoutRecord {
        timeout_id: timeout_id.into(),
        run_id: run_id.into(),
        disposition: if retryable {
            AutomationTimeoutDisposition::Retryable
        } else {
            AutomationTimeoutDisposition::Terminal
        },
        retry_policy_ref: retry_policy_ref.into(),
        attempt_number,
        last_failure_reason: last_failure_reason.into(),
        next_eligible_wake_ref,
    }
}

impl AutomationRecursionGuard {
    pub fn evaluate_next_run(&self, next_run_id: &str) -> AutomationRecursionGuardDecision {
        if self.source_run_id.as_deref() == Some(next_run_id)
            || self
                .parent_refs
                .iter()
                .any(|parent_ref| parent_ref == next_run_id)
        {
            return AutomationRecursionGuardDecision {
                allowed: false,
                blocked_reason: Some("self-triggered automation loop".to_owned()),
            };
        }

        if self.depth >= self.max_depth {
            return AutomationRecursionGuardDecision {
                allowed: false,
                blocked_reason: Some("automation recursion depth overflow".to_owned()),
            };
        }

        AutomationRecursionGuardDecision {
            allowed: true,
            blocked_reason: None,
        }
    }
}

pub fn task_outcome_next_action_semantics(
    class: &TaskOutcomeClass,
) -> TaskOutcomeNextActionSemantics {
    match class {
        TaskOutcomeClass::Notify => TaskOutcomeNextActionSemantics {
            action: TaskOutcomeNextAction::Delivery,
            delivery_required: true,
            continuation_requires_persistent_goal: false,
            continuation_requires_budget: false,
            continuation_requires_recursion_guard: false,
            continuation_requires_permission_gate: false,
            user_attention_required: false,
            verification_required: false,
            automatic_success: true,
            rollback_requires_checkpoint: false,
            rollback_requires_primitive: false,
        },
        TaskOutcomeClass::Suppress => TaskOutcomeNextActionSemantics {
            action: TaskOutcomeNextAction::NoDelivery,
            delivery_required: false,
            continuation_requires_persistent_goal: false,
            continuation_requires_budget: false,
            continuation_requires_recursion_guard: false,
            continuation_requires_permission_gate: false,
            user_attention_required: false,
            verification_required: false,
            automatic_success: true,
            rollback_requires_checkpoint: false,
            rollback_requires_primitive: false,
        },
        TaskOutcomeClass::ContinueTask => TaskOutcomeNextActionSemantics {
            action: TaskOutcomeNextAction::ContinueWithGates,
            delivery_required: false,
            continuation_requires_persistent_goal: true,
            continuation_requires_budget: true,
            continuation_requires_recursion_guard: true,
            continuation_requires_permission_gate: true,
            user_attention_required: false,
            verification_required: false,
            automatic_success: false,
            rollback_requires_checkpoint: false,
            rollback_requires_primitive: false,
        },
        TaskOutcomeClass::Escalate => TaskOutcomeNextActionSemantics {
            action: TaskOutcomeNextAction::UserAttention,
            delivery_required: true,
            continuation_requires_persistent_goal: false,
            continuation_requires_budget: false,
            continuation_requires_recursion_guard: false,
            continuation_requires_permission_gate: false,
            user_attention_required: true,
            verification_required: false,
            automatic_success: false,
            rollback_requires_checkpoint: false,
            rollback_requires_primitive: false,
        },
        TaskOutcomeClass::Verify => TaskOutcomeNextActionSemantics {
            action: TaskOutcomeNextAction::VerificationRequired,
            delivery_required: false,
            continuation_requires_persistent_goal: false,
            continuation_requires_budget: false,
            continuation_requires_recursion_guard: false,
            continuation_requires_permission_gate: false,
            user_attention_required: false,
            verification_required: true,
            automatic_success: false,
            rollback_requires_checkpoint: false,
            rollback_requires_primitive: false,
        },
        TaskOutcomeClass::Rollback => TaskOutcomeNextActionSemantics {
            action: TaskOutcomeNextAction::RollbackRequired,
            delivery_required: false,
            continuation_requires_persistent_goal: false,
            continuation_requires_budget: false,
            continuation_requires_recursion_guard: false,
            continuation_requires_permission_gate: false,
            user_attention_required: false,
            verification_required: false,
            automatic_success: false,
            rollback_requires_checkpoint: true,
            rollback_requires_primitive: true,
        },
    }
}

pub fn automation_job_is_schedulable(job: &AutomationJob) -> bool {
    match (&job.schedule, &job.status) {
        (_, AutomationJobStatus::Paused | AutomationJobStatus::Disabled) => false,
        (AutomationSchedule::OneShot { .. }, AutomationJobStatus::Completed) => false,
        (AutomationSchedule::Recurring { .. }, AutomationJobStatus::Active) => true,
        (AutomationSchedule::OneShot { .. }, AutomationJobStatus::Active) => true,
        (AutomationSchedule::Recurring { .. }, AutomationJobStatus::Completed) => true,
    }
}

pub fn task_result_status(run: &AutomationRun) -> AutomationRunStatus {
    run.status.clone()
}

pub fn delivery_failure_poisoned_task_result(
    run: &AutomationRun,
    delivery: &DeliveryRecord,
) -> bool {
    delivery.status == DeliveryStatus::Failed
        && run.status == AutomationRunStatus::Failed
        && run
            .result_ref
            .as_ref()
            .is_some_and(|result| result.exit_status == BackgroundExitStatus::Success)
}

pub fn delivery_status_is_independent_from_task_result(
    run: &AutomationRun,
    delivery: &DeliveryRecord,
) -> bool {
    run.run_id == delivery.run_id && !delivery_failure_poisoned_task_result(run, delivery)
}

pub fn rollback_action_ready(
    verdict: &TaskOutcomeVerdict,
    checkpoint_ready: bool,
    rollback_primitive_ready: bool,
) -> bool {
    verdict.class == TaskOutcomeClass::Rollback && checkpoint_ready && rollback_primitive_ready
}

impl RecursionGuard {
    pub fn evaluate_continuation(&self, next_source: &str) -> RecursionGuardDecision {
        if let Some(reason) = &self.blocked_reason {
            return RecursionGuardDecision {
                allowed: false,
                blocked_reason: Some(reason.clone()),
            };
        }

        if self.depth >= self.max_depth {
            return RecursionGuardDecision {
                allowed: false,
                blocked_reason: Some("max_depth_exceeded".to_owned()),
            };
        }

        if next_source == self.root_trigger_id
            || self.source_chain.iter().any(|source| source == next_source)
        {
            return RecursionGuardDecision {
                allowed: false,
                blocked_reason: Some("self_triggering_loop".to_owned()),
            };
        }

        RecursionGuardDecision {
            allowed: true,
            blocked_reason: None,
        }
    }
}

pub fn stable_sha256_digest(value: &Value) -> Result<String, serde_json::Error> {
    let canonical = stable_json_string(value)?;
    let digest = Sha256::digest(canonical.as_bytes());
    Ok(format!("{digest:x}"))
}

fn stable_json_string(value: &Value) -> Result<String, serde_json::Error> {
    match value {
        Value::Null => Ok("null".to_owned()),
        Value::Bool(flag) => Ok(flag.to_string()),
        Value::Number(number) => Ok(number.to_string()),
        Value::String(text) => serde_json::to_string(text),
        Value::Array(items) => {
            let encoded = items
                .iter()
                .map(stable_json_string)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", encoded.join(",")))
        }
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            let encoded = entries
                .into_iter()
                .map(|(key, nested)| {
                    Ok(format!(
                        "{}:{}",
                        serde_json::to_string(key)?,
                        stable_json_string(nested)?
                    ))
                })
                .collect::<Result<Vec<_>, serde_json::Error>>()?;
            Ok(format!("{{{}}}", encoded.join(",")))
        }
    }
}

pub trait NotificationEvaluator {
    fn evaluate_response(&self, prompt: &[Value]) -> Result<Option<bool>, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NotifyOnEvaluatorFailure;

impl NotificationEvaluator for NotifyOnEvaluatorFailure {
    fn evaluate_response(&self, _prompt: &[Value]) -> Result<Option<bool>, String> {
        Ok(Some(true))
    }
}

pub fn build_evaluator_messages(task: &str, response: &str) -> Vec<Value> {
    vec![
        json!({"role": "system", "content": "Decide whether this background result should notify the user."}),
        json!({"role": "user", "content": format!("Task:\n{task}\n\nResponse:\n{response}")}),
    ]
}

pub fn evaluate_notification_tool_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": EVALUATE_NOTIFICATION_TOOL,
            "description": "Decide whether the user should be notified about this background task result.",
            "parameters": {
                "type": "object",
                "properties": {
                    "should_notify": {
                        "type": "boolean",
                        "description": "true = result contains actionable/important info the user should see; false = routine or empty, safe to suppress"
                    },
                    "reason": {
                        "type": "string",
                        "description": "One-sentence reason for the decision"
                    }
                },
                "required": ["should_notify"]
            }
        }
    })
}

pub fn parse_notification_decision(response: &Value) -> bool {
    if !should_execute_tools(response) {
        return true;
    }
    response
        .get("tool_calls")
        .and_then(Value::as_array)
        .and_then(|calls| calls.first())
        .and_then(|call| {
            tool_call_arguments(call)
                .and_then(|arguments| arguments.get("should_notify").and_then(Value::as_bool))
        })
        .unwrap_or(true)
}

fn should_execute_tools(response: &Value) -> bool {
    response
        .get("finish_reason")
        .and_then(Value::as_str)
        .is_some_and(|reason| matches!(reason, "tool_calls" | "stop"))
        && response
            .get("tool_calls")
            .and_then(Value::as_array)
            .is_some_and(|calls| !calls.is_empty())
}

fn tool_call_arguments(call: &Value) -> Option<Value> {
    if let Some(arguments) = call.get("arguments").and_then(Value::as_object) {
        return Some(Value::Object(arguments.clone()));
    }
    let arguments = call
        .get("function")
        .and_then(|function| function.get("arguments"))
        .or_else(|| call.get("arguments"))?;
    match arguments {
        Value::String(text) => serde_json::from_str(text).ok(),
        Value::Object(map) => Some(Value::Object(map.clone())),
        _ => None,
    }
}

pub const SPEC018_PROJECTION_SCHEMA_LABEL: &str = "018Projection";
pub const SPEC018_PROJECTION_SCHEMA_VERSION: &str = "018Projection.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ProjectionStatusKind {
    Idle,
    Running,
    WaitingForUser,
    ApprovalRequired,
    Blocked,
    VerificationPending,
    VerificationFailed,
    RollbackAvailable,
    RolledBack,
    Completed,
    Suppressed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018BlockedReasonClass {
    ApprovalRequired,
    CapabilityDenied,
    CheckpointUnavailable,
    RecursionLimit,
    VerificationFailed,
    RollbackUnavailable,
    ExternalDependency,
    RedactionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018RetryEligibility {
    Retryable,
    RetryAfterUserAction,
    RetryWithFreshEvidence,
    NotRetryable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018RollbackEligibility {
    Available,
    Unavailable,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018VerificationResultKind {
    NotRun,
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ApprovalDecisionKind {
    Approve,
    Reject,
    Defer,
    InspectEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec018AllowedDecision {
    pub decision: Spec018ApprovalDecisionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018ProjectionStatus {
    pub kind: Spec018ProjectionStatusKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Spec018ReleaseBlockerSeverity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason_class: Option<Spec018BlockedReasonClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_action_hint: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_eligibility: Option<Spec018RetryEligibility>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018GoalSummary {
    pub goal_id: String,
    pub summary: String,
    pub status: Spec018ProjectionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018BlockedProjectionItem {
    pub source_kind: String,
    pub source_ref: String,
    pub blocked_reason_class: Spec018BlockedReasonClass,
    pub blocked_reason: String,
    pub user_action_hint: String,
    pub retry_eligibility: Spec018RetryEligibility,
    pub diagnostics_ref: EvidenceRef,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018ApprovalProjectionItem {
    pub proposal_id: String,
    pub target_kind: String,
    #[serde(default)]
    pub requested_scope: Vec<String>,
    pub risk_summary: String,
    pub rollback_summary: String,
    #[serde(default)]
    pub allowed_decisions: Vec<Spec018AllowedDecision>,
    pub status: Spec018ProjectionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018VerificationProjectionItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_case_id: Option<String>,
    pub expected_behavior: String,
    pub last_result: Spec018VerificationResultKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    pub rollback_eligibility: Spec018RollbackEligibility,
    pub status: Spec018ProjectionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018AutomationDeliveryStatus {
    pub delivery_id: String,
    pub run_id: String,
    pub target_surface: ProjectionSurface,
    pub severity: DeliverySeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppress_reason: Option<String>,
    pub acknowledged: bool,
    pub status: Spec018ProjectionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018ReplayRegressionSummary {
    pub replay_id: String,
    pub trajectory_id: String,
    pub expected_summary: String,
    pub actual_summary: String,
    pub status: Spec018ProjectionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018EvaluatorDecisionSummary {
    pub evaluator_kind: EvaluatorKind,
    pub verdict: VerdictKind,
    pub summary: String,
    pub status: Spec018ProjectionStatus,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018Projection {
    pub schema_label: String,
    pub schema_version: String,
    pub generated_at_ms: u64,
    pub session_id: String,
    #[serde(default)]
    pub goal_summaries: Vec<Spec018GoalSummary>,
    #[serde(default)]
    pub automation_summaries: Vec<Spec018AutomationDeliveryStatus>,
    #[serde(default)]
    pub approval_summaries: Vec<Spec018ApprovalProjectionItem>,
    #[serde(default)]
    pub blocked_summaries: Vec<Spec018BlockedProjectionItem>,
    #[serde(default)]
    pub verification_summaries: Vec<Spec018VerificationProjectionItem>,
    #[serde(default)]
    pub replay_summaries: Vec<Spec018ReplayRegressionSummary>,
    #[serde(default)]
    pub recent_evaluator_decision_summaries: Vec<Spec018EvaluatorDecisionSummary>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec018ProjectionSurfaceStatus {
    pub surface: ProjectionSurface,
    pub status: Spec018ProjectionStatusKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ChannelProjectionEventKind {
    Notify,
    Escalate,
    Blocked,
    ApprovalRequired,
    VerificationFailed,
}

pub fn spec018_allowed_decisions(
    approve_unavailable_reason: Option<String>,
    reject_unavailable_reason: Option<String>,
    defer_unavailable_reason: Option<String>,
    inspect_unavailable_reason: Option<String>,
) -> Vec<Spec018AllowedDecision> {
    vec![
        Spec018AllowedDecision {
            decision: Spec018ApprovalDecisionKind::Approve,
            unavailable_reason: approve_unavailable_reason,
        },
        Spec018AllowedDecision {
            decision: Spec018ApprovalDecisionKind::Reject,
            unavailable_reason: reject_unavailable_reason,
        },
        Spec018AllowedDecision {
            decision: Spec018ApprovalDecisionKind::Defer,
            unavailable_reason: defer_unavailable_reason,
        },
        Spec018AllowedDecision {
            decision: Spec018ApprovalDecisionKind::InspectEvidence,
            unavailable_reason: inspect_unavailable_reason,
        },
    ]
}

pub fn spec018_blocked_status_is_valid(status: &Spec018ProjectionStatus) -> bool {
    if status.kind != Spec018ProjectionStatusKind::Blocked {
        return true;
    }

    status.blocked_reason_class.is_some()
        && status.severity.is_some()
        && status
            .user_action_hint
            .as_deref()
            .is_some_and(|hint| !hint.trim().is_empty())
        && !status.evidence_refs.is_empty()
        && status.retry_eligibility.is_some()
}

pub fn spec018_approval_item_can_be_actionable(item: &Spec018ApprovalProjectionItem) -> bool {
    let has_user_decision = item.allowed_decisions.iter().any(|decision| {
        matches!(
            decision.decision,
            Spec018ApprovalDecisionKind::Approve
                | Spec018ApprovalDecisionKind::Reject
                | Spec018ApprovalDecisionKind::Defer
        ) && decision.unavailable_reason.is_none()
    });

    !has_user_decision
        || (!item.requested_scope.is_empty() && !item.rollback_summary.trim().is_empty())
}

pub fn spec018_acknowledgement_is_user_decision(
    _delivery: &Spec018AutomationDeliveryStatus,
    _approval: &Spec018ApprovalProjectionItem,
) -> bool {
    false
}

pub fn spec018_shared_surface_status_semantics(
    status: Spec018ProjectionStatusKind,
) -> Vec<Spec018ProjectionSurfaceStatus> {
    [
        ProjectionSurface::Cli,
        ProjectionSurface::Tui,
        ProjectionSurface::LocalApi,
        ProjectionSurface::Channel,
    ]
    .into_iter()
    .map(|surface| Spec018ProjectionSurfaceStatus {
        surface,
        status: status.clone(),
    })
    .collect()
}

pub fn spec018_surfaces_share_status_semantics(
    statuses: &[Spec018ProjectionSurfaceStatus],
) -> bool {
    let Some(first) = statuses.first() else {
        return false;
    };

    statuses.iter().all(|status| status.status == first.status)
        && [
            ProjectionSurface::Cli,
            ProjectionSurface::Tui,
            ProjectionSurface::LocalApi,
            ProjectionSurface::Channel,
        ]
        .into_iter()
        .all(|surface| statuses.iter().any(|status| status.surface == surface))
}

pub fn spec018_channel_event_kind_for_status(
    status: &Spec018ProjectionStatus,
) -> Option<Spec018ChannelProjectionEventKind> {
    match status.kind {
        Spec018ProjectionStatusKind::WaitingForUser | Spec018ProjectionStatusKind::Completed => {
            Some(Spec018ChannelProjectionEventKind::Notify)
        }
        Spec018ProjectionStatusKind::ApprovalRequired => {
            Some(Spec018ChannelProjectionEventKind::ApprovalRequired)
        }
        Spec018ProjectionStatusKind::Blocked => Some(Spec018ChannelProjectionEventKind::Blocked),
        Spec018ProjectionStatusKind::VerificationFailed => {
            Some(Spec018ChannelProjectionEventKind::VerificationFailed)
        }
        Spec018ProjectionStatusKind::RollbackAvailable => {
            Some(Spec018ChannelProjectionEventKind::Escalate)
        }
        Spec018ProjectionStatusKind::Idle
        | Spec018ProjectionStatusKind::Running
        | Spec018ProjectionStatusKind::VerificationPending
        | Spec018ProjectionStatusKind::RolledBack
        | Spec018ProjectionStatusKind::Suppressed => None,
    }
}

pub fn spec018_approval_item_channel_visible(item: &Spec018ApprovalProjectionItem) -> bool {
    if spec018_channel_event_kind_for_status(&item.status).is_none() {
        return false;
    }

    item.allowed_decisions.iter().any(|decision| {
        !matches!(
            decision.decision,
            Spec018ApprovalDecisionKind::InspectEvidence
        ) && decision.unavailable_reason.is_none()
    })
}

pub fn spec018_evidence_refs_are_redacted(refs: &[EvidenceRef]) -> bool {
    refs.iter().all(|evidence| {
        matches!(
            evidence.redaction_status,
            RedactionStatus::Redacted | RedactionStatus::AlreadySafe
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionSurface {
    Cli,
    Tui,
    LocalApi,
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionStatus {
    Success,
    Pending,
    Blocked,
    Failed,
    Stale,
    Denied,
    RedactionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionSurfaceStatus {
    pub surface: ProjectionSurface,
    pub status: ProjectionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelProjectionEventClass {
    Notify,
    Escalate,
    Blocked,
    ApprovalRequired,
    VerificationFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceSummary {
    pub active_count: usize,
    pub stale_count: usize,
    pub denied_count: usize,
    pub min_confidence: f32,
    pub average_confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationProjection {
    #[serde(default)]
    pub active_verdicts: Vec<EvaluatorVerdictEnvelope>,
    #[serde(default)]
    pub stale_verdicts: Vec<EvaluatorVerdictEnvelope>,
    #[serde(default)]
    pub denied_outcomes: Vec<DeniedOutcome>,
    pub confidence_summary: ConfidenceSummary,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub redaction_status: RedactionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_failure_marker: Option<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationProjection {
    #[serde(default)]
    pub jobs: Vec<AutomationJob>,
    #[serde(default)]
    pub runs: Vec<AutomationRun>,
    #[serde(default)]
    pub outcomes: Vec<TaskOutcomeVerdict>,
    #[serde(default)]
    pub delivery_states: Vec<DeliveryRecord>,
    #[serde(default)]
    pub recursion_guard_state: Vec<RecursionGuard>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImprovementProjection {
    #[serde(default)]
    pub proposals: Vec<ImprovementProposal>,
    #[serde(default)]
    pub approval_state: Vec<ImprovementApproval>,
    #[serde(default)]
    pub checkpoint_state: Vec<ImprovementCheckpoint>,
    #[serde(default)]
    pub verification_state: Vec<ImprovementVerification>,
    #[serde(default)]
    pub rollback_state: Vec<ImprovementRollbackRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayProjection {
    #[serde(default)]
    pub dataset_cases: Vec<QualityRegressionCase>,
    #[serde(default)]
    pub replay_runs: Vec<ReplayRecord>,
    #[serde(default)]
    pub regressions: Vec<ReplayRecord>,
    #[serde(default)]
    pub inconclusive_runs: Vec<ReplayRecord>,
    #[serde(default)]
    pub invalid_fixtures: Vec<QualityRegressionCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LedgerInspectQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerInspectSubjectKind {
    Task,
    Evaluation,
    Improvement,
    Replay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerInspectSubject {
    pub kind: LedgerInspectSubjectKind,
    pub record_id: String,
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trajectory_id: Option<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "entry_kind", content = "record", rename_all = "snake_case")]
pub enum LedgerInspectEntry {
    Task(TaskLedgerRecord),
    Evaluation(EvaluationLedgerProjection),
    Improvement(LedgerInspectSubject),
    Replay(LedgerInspectSubject),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DiagnosticsEvidenceBundleRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frozen_snapshot_digest: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_verdict_summary: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub denied_outcome: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_ref: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_failure: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_result: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<EvidenceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_failure_status: Option<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseGateStatus {
    Pass,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018CoverageEntry {
    pub prd_id: String,
    pub requirement_id: String,
    #[serde(default)]
    pub test_evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub diagnostics_evidence: Vec<EvidenceRef>,
    pub release_gate_status: ReleaseGateStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseBlockerFamily {
    RedactionFailure,
    ApprovalBypass,
    StaleVerdictApply,
    DestructiveReplayEffect,
    SilentSelfModification,
    UnboundedContinuationLoop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionAssumptions {
    pub runtime_scope: String,
    pub primary_actor: String,
    #[serde(default)]
    pub excluded_workflows: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018DiagnosticsEvidenceCategory {
    Evaluator,
    Ledger,
    Automation,
    Memory,
    Improvement,
    Replay,
    Projection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018SkippedEvidenceClassification {
    Stale,
    Expired,
    Duplicate,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018SkippedEvidence {
    pub source_ref: EvidenceRef,
    pub classification: Spec018SkippedEvidenceClassification,
    pub redacted_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec018DiagnosticsRedactionSummary {
    pub redaction_profile: String,
    pub redacted_ref_count: usize,
    pub already_safe_ref_count: usize,
    pub failed_ref_count: usize,
    #[serde(default)]
    pub skipped_ref_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018DiagnosticsEvidenceManifest {
    pub manifest_id: String,
    pub generated_at_ms: u64,
    #[serde(default)]
    pub evaluator_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub ledger_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub automation_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub memory_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub improvement_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub replay_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub projection_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub skipped_evidence: Vec<Spec018SkippedEvidence>,
    #[serde(default)]
    pub diagnostics_artifact_refs: Vec<EvidenceRef>,
    pub redaction_summary: Spec018DiagnosticsRedactionSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018LedgerInspectQueryKind {
    VerdictId,
    GoalId,
    TaskRunId,
    ProposalId,
    ReplayRunId,
    ProjectionItemId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec018LedgerInspectQuery {
    pub query_kind: Spec018LedgerInspectQueryKind,
    pub target_ref: String,
    pub include_skipped: bool,
    pub include_diagnostics_refs: bool,
    pub redaction_profile: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018LedgerInspectResult {
    pub query: Spec018LedgerInspectQuery,
    #[serde(default)]
    pub source_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub consumption_record_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub runtime_decision_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub projection_item_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub diagnostics_artifact_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub skipped_evidence: Vec<Spec018SkippedEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ClosureCoverageBucket {
    EvaluatorFoundation,
    GoalContinuation,
    ApprovalGate,
    AutomationRuntime,
    MemorySkillIntegration,
    SelfImprovementWiring,
    ReplayRunner,
    ProjectionSemantics,
    DiagnosticsIntegration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ReleaseCoverageStatus {
    Pass,
    MissingEvidence,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018ReleaseCoverageEntry {
    pub entry_id: String,
    pub capability_area: Spec018ClosureCoverageBucket,
    #[serde(default)]
    pub required_evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub test_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub replay_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub manual_refs: Vec<EvidenceRef>,
    #[serde(default)]
    pub diagnostics_artifact_refs: Vec<EvidenceRef>,
    pub status: Spec018ReleaseCoverageStatus,
    #[serde(default)]
    pub blocker_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ReleaseBlockerCategory {
    BlockedApproval,
    UnverifiedAppliedImprovement,
    FailedReplayRegression,
    MissingRedactionEvidence,
    MissingLedgerConsumptionEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec018ReleaseBlockerSeverity {
    Warning,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018ReleaseBlocker {
    pub blocker_id: String,
    pub category: Spec018ReleaseBlockerCategory,
    pub source_ref: EvidenceRef,
    pub severity: Spec018ReleaseBlockerSeverity,
    pub redacted_summary: String,
    pub resolution_hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec018ReleaseGateOutcome {
    pub status: ReleaseGateStatus,
    #[serde(default)]
    pub coverage_entries: Vec<Spec018ReleaseCoverageEntry>,
    #[serde(default)]
    pub blockers: Vec<Spec018ReleaseBlocker>,
    #[serde(default)]
    pub missing_buckets: Vec<Spec018ClosureCoverageBucket>,
    pub final_closure_passed: bool,
}

pub fn projection_status_from_verdict(verdict: &EvaluatorVerdictEnvelope) -> ProjectionStatus {
    match verdict.verdict_kind {
        VerdictKind::Pass => {
            if verdict.redaction_status == RedactionStatus::RedactionFailed {
                ProjectionStatus::RedactionFailed
            } else {
                ProjectionStatus::Success
            }
        }
        VerdictKind::Denied => ProjectionStatus::Denied,
        VerdictKind::Stale | VerdictKind::Expired => ProjectionStatus::Stale,
        VerdictKind::RedactionFailed => ProjectionStatus::RedactionFailed,
        VerdictKind::Fail | VerdictKind::LowConfidence | VerdictKind::ConflictingEvidence => {
            ProjectionStatus::Failed
        }
    }
}

pub fn projection_status_from_task_outcome(outcome: &TaskOutcomeVerdict) -> ProjectionStatus {
    match outcome.class {
        TaskOutcomeClass::Notify | TaskOutcomeClass::Suppress => ProjectionStatus::Success,
        TaskOutcomeClass::ContinueTask | TaskOutcomeClass::Verify | TaskOutcomeClass::Rollback => {
            ProjectionStatus::Pending
        }
        TaskOutcomeClass::Escalate => ProjectionStatus::Blocked,
    }
}

pub fn projection_status_from_run(run: &AutomationRun) -> ProjectionStatus {
    match run.status {
        AutomationRunStatus::Succeeded => ProjectionStatus::Success,
        AutomationRunStatus::Queued | AutomationRunStatus::Running => ProjectionStatus::Pending,
        AutomationRunStatus::Failed
        | AutomationRunStatus::TimedOut
        | AutomationRunStatus::Cancelled => ProjectionStatus::Failed,
    }
}

pub fn projection_status_from_delivery(delivery: &DeliveryRecord) -> ProjectionStatus {
    match delivery.status {
        DeliveryStatus::Delivered | DeliveryStatus::Suppressed => ProjectionStatus::Success,
        DeliveryStatus::Failed => ProjectionStatus::Failed,
    }
}

pub fn projection_status_is_success(status: &ProjectionStatus) -> bool {
    matches!(status, ProjectionStatus::Success)
}

pub fn shared_surface_status_semantics(status: ProjectionStatus) -> Vec<ProjectionSurfaceStatus> {
    [
        ProjectionSurface::Cli,
        ProjectionSurface::Tui,
        ProjectionSurface::LocalApi,
        ProjectionSurface::Channel,
    ]
    .into_iter()
    .map(|surface| ProjectionSurfaceStatus {
        surface,
        status: status.clone(),
    })
    .collect()
}

pub fn surfaces_share_status_semantics(statuses: &[ProjectionSurfaceStatus]) -> bool {
    let Some(first) = statuses.first() else {
        return false;
    };

    statuses.iter().all(|status| status.status == first.status)
        && [
            ProjectionSurface::Cli,
            ProjectionSurface::Tui,
            ProjectionSurface::LocalApi,
            ProjectionSurface::Channel,
        ]
        .into_iter()
        .all(|surface| statuses.iter().any(|status| status.surface == surface))
}

pub fn channel_event_class_for_status(
    status: &ProjectionStatus,
) -> Option<ChannelProjectionEventClass> {
    match status {
        ProjectionStatus::Success => Some(ChannelProjectionEventClass::Notify),
        ProjectionStatus::Pending => Some(ChannelProjectionEventClass::ApprovalRequired),
        ProjectionStatus::Blocked => Some(ChannelProjectionEventClass::Blocked),
        ProjectionStatus::Failed => Some(ChannelProjectionEventClass::VerificationFailed),
        ProjectionStatus::Stale | ProjectionStatus::Denied | ProjectionStatus::RedactionFailed => {
            Some(ChannelProjectionEventClass::Escalate)
        }
    }
}

pub fn channel_projection_filters_user_visible_events(
    event_classes: &[ChannelProjectionEventClass],
) -> bool {
    event_classes.iter().all(|event_class| {
        matches!(
            event_class,
            ChannelProjectionEventClass::Notify
                | ChannelProjectionEventClass::Escalate
                | ChannelProjectionEventClass::Blocked
                | ChannelProjectionEventClass::ApprovalRequired
                | ChannelProjectionEventClass::VerificationFailed
        )
    })
}

pub fn confidence_summary(
    active_verdicts: &[EvaluatorVerdictEnvelope],
    stale_verdicts: &[EvaluatorVerdictEnvelope],
    denied_outcomes: &[DeniedOutcome],
) -> ConfidenceSummary {
    let count = active_verdicts.len() + stale_verdicts.len();
    let confidence_sum: f32 = active_verdicts
        .iter()
        .chain(stale_verdicts.iter())
        .map(|verdict| verdict.confidence)
        .sum();
    let min_confidence = active_verdicts
        .iter()
        .chain(stale_verdicts.iter())
        .map(|verdict| verdict.confidence)
        .reduce(f32::min)
        .unwrap_or(0.0);

    ConfidenceSummary {
        active_count: active_verdicts.len(),
        stale_count: stale_verdicts.len(),
        denied_count: denied_outcomes.len(),
        min_confidence,
        average_confidence: if count == 0 {
            0.0
        } else {
            confidence_sum / count as f32
        },
    }
}

pub fn evaluation_projection(
    verdicts: Vec<EvaluatorVerdictEnvelope>,
    denied_outcomes: Vec<DeniedOutcome>,
    mut evidence_refs: Vec<EvidenceRef>,
    redaction_status: RedactionStatus,
) -> EvaluationProjection {
    let (stale_verdicts, active_verdicts): (Vec<_>, Vec<_>) =
        verdicts.into_iter().partition(|verdict| {
            matches!(
                verdict.verdict_kind,
                VerdictKind::Stale | VerdictKind::Expired
            )
        });
    evidence_refs.extend(
        active_verdicts
            .iter()
            .chain(stale_verdicts.iter())
            .flat_map(|verdict| verdict.evidence_refs.clone()),
    );
    let redaction_failure_marker =
        (redaction_status == RedactionStatus::RedactionFailed).then(|| {
            redacted_evidence_ref(
                EvidenceKind::DiagnosticRecord,
                "redaction-failure",
                "redaction failure marker",
                RedactionStatus::RedactionFailed,
            )
        });

    EvaluationProjection {
        confidence_summary: confidence_summary(&active_verdicts, &stale_verdicts, &denied_outcomes),
        active_verdicts,
        stale_verdicts,
        denied_outcomes,
        evidence_refs,
        redaction_status,
        redaction_failure_marker,
    }
}

pub fn ledger_inspect_matches_subject(
    query: &LedgerInspectQuery,
    subject: &LedgerInspectSubject,
) -> bool {
    query
        .correlation_id
        .as_ref()
        .map_or(true, |id| id == &subject.correlation_id)
        && query
            .session_id
            .as_ref()
            .map_or(true, |id| subject.session_id.as_ref() == Some(id))
        && query
            .goal_id
            .as_ref()
            .map_or(true, |id| subject.goal_id.as_ref() == Some(id))
        && query
            .job_id
            .as_ref()
            .map_or(true, |id| subject.job_id.as_ref() == Some(id))
        && query
            .proposal_id
            .as_ref()
            .map_or(true, |id| subject.proposal_id.as_ref() == Some(id))
        && query
            .trajectory_id
            .as_ref()
            .map_or(true, |id| subject.trajectory_id.as_ref() == Some(id))
        && query
            .from_ms
            .map_or(true, |from_ms| subject.created_at_ms >= from_ms)
        && query
            .to_ms
            .map_or(true, |to_ms| subject.created_at_ms <= to_ms)
}

pub fn ledger_subject_for_record(record: &EvaluatorLedgerRecord) -> LedgerInspectSubject {
    match record {
        EvaluatorLedgerRecord::Task(task) => LedgerInspectSubject {
            kind: LedgerInspectSubjectKind::Task,
            record_id: task.record_id.clone(),
            correlation_id: task.correlation_id.clone(),
            session_id: None,
            goal_id: Some(task.task_id.clone()),
            job_id: task.job_id.clone(),
            proposal_id: None,
            trajectory_id: None,
            created_at_ms: task.created_at_ms,
        },
        EvaluatorLedgerRecord::Evaluation(evaluation) => LedgerInspectSubject {
            kind: LedgerInspectSubjectKind::Evaluation,
            record_id: evaluation.record_id.clone(),
            correlation_id: evaluation.request.correlation_id.clone(),
            session_id: evaluation.request.session_id.clone(),
            goal_id: None,
            job_id: None,
            proposal_id: None,
            trajectory_id: None,
            created_at_ms: evaluation.created_at_ms,
        },
    }
}

pub fn ledger_subject_for_improvement(
    proposal: &ImprovementProposal,
    created_at_ms: u64,
) -> LedgerInspectSubject {
    LedgerInspectSubject {
        kind: LedgerInspectSubjectKind::Improvement,
        record_id: proposal.proposal_id.clone(),
        correlation_id: proposal.correlation_id.clone(),
        session_id: None,
        goal_id: None,
        job_id: None,
        proposal_id: Some(proposal.proposal_id.clone()),
        trajectory_id: None,
        created_at_ms,
    }
}

pub fn ledger_subject_for_replay(replay: &ReplayRecord) -> LedgerInspectSubject {
    LedgerInspectSubject {
        kind: LedgerInspectSubjectKind::Replay,
        record_id: replay.replay_id.clone(),
        correlation_id: replay.correlation_id.clone(),
        session_id: None,
        goal_id: None,
        job_id: None,
        proposal_id: None,
        trajectory_id: Some(replay.trajectory_id.clone()),
        created_at_ms: replay.started_at_ms,
    }
}

pub fn ledger_inspect_entry_matches(
    query: &LedgerInspectQuery,
    entry: &LedgerInspectEntry,
) -> bool {
    match entry {
        LedgerInspectEntry::Task(record) => ledger_inspect_matches_subject(
            query,
            &ledger_subject_for_record(&EvaluatorLedgerRecord::Task(record.clone())),
        ),
        LedgerInspectEntry::Evaluation(projection) => ledger_inspect_matches_subject(
            query,
            &LedgerInspectSubject {
                kind: LedgerInspectSubjectKind::Evaluation,
                record_id: projection.record_id.clone(),
                correlation_id: projection.correlation_id.clone(),
                session_id: None,
                goal_id: None,
                job_id: None,
                proposal_id: None,
                trajectory_id: None,
                created_at_ms: projection.created_at_ms,
            },
        ),
        LedgerInspectEntry::Improvement(subject) | LedgerInspectEntry::Replay(subject) => {
            ledger_inspect_matches_subject(query, subject)
        }
    }
}

pub struct DiagnosticsEvidenceBundleInput<'a> {
    pub frozen_snapshot_digest: Option<String>,
    pub evaluator_verdict_summary: Option<String>,
    pub denied_outcome: Option<&'a DeniedOutcome>,
    pub checkpoint_ref: Option<&'a ImprovementCheckpoint>,
    pub delivery_failure: Option<&'a DeliveryRecord>,
    pub replay_result: Option<&'a ReplayResult>,
    pub redaction_profile: Option<String>,
    pub redaction_failure: bool,
}

pub fn diagnostics_bundle_evidence_refs(
    input: DiagnosticsEvidenceBundleInput<'_>,
) -> DiagnosticsEvidenceBundleRefs {
    DiagnosticsEvidenceBundleRefs {
        frozen_snapshot_digest: input.frozen_snapshot_digest.map(|digest| {
            redacted_evidence_ref(
                EvidenceKind::FrozenSessionSearchSnapshot,
                "frozen-snapshot-digest",
                digest,
                RedactionStatus::Redacted,
            )
        }),
        evaluator_verdict_summary: input.evaluator_verdict_summary.map(|summary| {
            redacted_evidence_ref(
                EvidenceKind::EvaluatorSummary,
                "evaluator-verdict-summary",
                summary,
                RedactionStatus::Redacted,
            )
        }),
        denied_outcome: input.denied_outcome.map(|outcome| {
            redacted_evidence_ref(
                EvidenceKind::EvaluatorSummary,
                format!("denied-outcome:{}", outcome.code),
                "denied outcome payload withheld",
                RedactionStatus::Redacted,
            )
        }),
        checkpoint_ref: input.checkpoint_ref.map(|checkpoint| {
            redacted_evidence_ref(
                EvidenceKind::ImprovementCheckpoint,
                checkpoint.checkpoint_ref.clone(),
                "checkpoint reference",
                RedactionStatus::Redacted,
            )
        }),
        delivery_failure: input.delivery_failure.map(|delivery| {
            redacted_evidence_ref(
                EvidenceKind::TaskResult,
                delivery.delivery_id.clone(),
                "delivery failure",
                RedactionStatus::Redacted,
            )
        }),
        replay_result: input.replay_result.map(|result| {
            redacted_evidence_ref(
                EvidenceKind::ReplayResult,
                "replay-result",
                format!("{:?}", result.classification),
                RedactionStatus::Redacted,
            )
        }),
        redaction_profile: input.redaction_profile.map(|profile| {
            redacted_evidence_ref(
                EvidenceKind::DiagnosticRecord,
                "redaction-profile",
                profile,
                RedactionStatus::Redacted,
            )
        }),
        redaction_failure_status: input.redaction_failure.then(|| {
            redacted_evidence_ref(
                EvidenceKind::DiagnosticRecord,
                "redaction-failure-status",
                "redaction failed; payload withheld",
                RedactionStatus::RedactionFailed,
            )
        }),
    }
}

pub fn diagnostics_evidence_export_is_redacted(bundle: &DiagnosticsEvidenceBundleRefs) -> bool {
    [
        bundle.frozen_snapshot_digest.as_ref(),
        bundle.evaluator_verdict_summary.as_ref(),
        bundle.denied_outcome.as_ref(),
        bundle.checkpoint_ref.as_ref(),
        bundle.delivery_failure.as_ref(),
        bundle.replay_result.as_ref(),
        bundle.redaction_profile.as_ref(),
        bundle.redaction_failure_status.as_ref(),
    ]
    .into_iter()
    .flatten()
    .all(|evidence_ref| {
        matches!(
            evidence_ref.redaction_status,
            RedactionStatus::Redacted | RedactionStatus::RedactionFailed
        ) && evidence_ref.locator.is_none()
    })
}

pub fn release_gate_blocks_missing_coverage_or_blockers(
    coverage: &[Spec018CoverageEntry],
    required_prd_ids: &[&str],
    blockers: &[ReleaseBlockerFamily],
) -> bool {
    !blockers.is_empty()
        || required_prd_ids.iter().any(|required_prd_id| {
            !coverage.iter().any(|entry| {
                entry.prd_id == *required_prd_id
                    && !entry.test_evidence.is_empty()
                    && !entry.diagnostics_evidence.is_empty()
                    && entry.release_gate_status == ReleaseGateStatus::Pass
            })
        })
}

pub fn default_projection_assumptions() -> ProjectionAssumptions {
    ProjectionAssumptions {
        runtime_scope: "self_hosted_personal_use".to_owned(),
        primary_actor: "local_user".to_owned(),
        excluded_workflows: vec![
            "admin_dashboard".to_owned(),
            "organization_release_approval".to_owned(),
            "fleet_workflow".to_owned(),
        ],
    }
}

pub fn projection_assumptions_are_self_hosted_personal_use(
    assumptions: &ProjectionAssumptions,
) -> bool {
    assumptions.runtime_scope == "self_hosted_personal_use"
        && assumptions.primary_actor == "local_user"
        && assumptions
            .excluded_workflows
            .iter()
            .any(|workflow| workflow == "admin_dashboard")
        && assumptions
            .excluded_workflows
            .iter()
            .any(|workflow| workflow == "organization_release_approval")
        && assumptions
            .excluded_workflows
            .iter()
            .any(|workflow| workflow == "fleet_workflow")
}

pub fn spec018_all_diagnostics_evidence_refs(
    manifest: &Spec018DiagnosticsEvidenceManifest,
) -> Vec<&EvidenceRef> {
    manifest
        .evaluator_refs
        .iter()
        .chain(manifest.ledger_refs.iter())
        .chain(manifest.automation_refs.iter())
        .chain(manifest.memory_refs.iter())
        .chain(manifest.improvement_refs.iter())
        .chain(manifest.replay_refs.iter())
        .chain(manifest.projection_refs.iter())
        .chain(manifest.diagnostics_artifact_refs.iter())
        .chain(
            manifest
                .skipped_evidence
                .iter()
                .map(|skipped| &skipped.source_ref),
        )
        .collect()
}

pub fn spec018_evidence_ref_has_owner_and_redaction(evidence_ref: &EvidenceRef) -> bool {
    evidence_ref
        .owner_spec
        .as_ref()
        .is_some_and(|owner_spec| !owner_spec.trim().is_empty())
        && matches!(
            evidence_ref.redaction_status,
            RedactionStatus::Redacted | RedactionStatus::AlreadySafe
        )
}

pub fn spec018_manifest_redaction_is_valid(manifest: &Spec018DiagnosticsEvidenceManifest) -> bool {
    let evidence_refs = spec018_all_diagnostics_evidence_refs(manifest);
    let redacted_ref_count = evidence_refs
        .iter()
        .filter(|evidence_ref| evidence_ref.redaction_status == RedactionStatus::Redacted)
        .count();
    let already_safe_ref_count = evidence_refs
        .iter()
        .filter(|evidence_ref| evidence_ref.redaction_status == RedactionStatus::AlreadySafe)
        .count();
    let failed_ref_count = evidence_refs
        .iter()
        .filter(|evidence_ref| evidence_ref.redaction_status == RedactionStatus::RedactionFailed)
        .count();

    manifest.redaction_summary.redacted_ref_count == redacted_ref_count
        && manifest.redaction_summary.already_safe_ref_count == already_safe_ref_count
        && manifest.redaction_summary.failed_ref_count == failed_ref_count
        && manifest.redaction_summary.skipped_ref_count == manifest.skipped_evidence.len()
        && failed_ref_count == 0
        && evidence_refs
            .iter()
            .all(|evidence_ref| spec018_evidence_ref_has_owner_and_redaction(evidence_ref))
}

pub fn spec018_manifest_includes_all_evidence_categories(
    manifest: &Spec018DiagnosticsEvidenceManifest,
) -> bool {
    !manifest.evaluator_refs.is_empty()
        && !manifest.ledger_refs.is_empty()
        && !manifest.automation_refs.is_empty()
        && !manifest.memory_refs.is_empty()
        && !manifest.improvement_refs.is_empty()
        && !manifest.replay_refs.is_empty()
        && !manifest.projection_refs.is_empty()
}

pub fn spec018_skipped_evidence_is_non_blocking(skipped: &Spec018SkippedEvidence) -> bool {
    matches!(
        skipped.classification,
        Spec018SkippedEvidenceClassification::Stale
            | Spec018SkippedEvidenceClassification::Expired
            | Spec018SkippedEvidenceClassification::Duplicate
            | Spec018SkippedEvidenceClassification::Superseded
    ) && spec018_evidence_ref_has_owner_and_redaction(&skipped.source_ref)
}

pub fn spec018_ledger_inspect_links_runtime_projection_and_diagnostics(
    result: &Spec018LedgerInspectResult,
) -> bool {
    !result.source_refs.is_empty()
        && !result.consumption_record_refs.is_empty()
        && !result.runtime_decision_refs.is_empty()
        && !result.projection_item_refs.is_empty()
        && !result.diagnostics_artifact_refs.is_empty()
        && result
            .source_refs
            .iter()
            .chain(result.consumption_record_refs.iter())
            .chain(result.runtime_decision_refs.iter())
            .chain(result.projection_item_refs.iter())
            .chain(result.diagnostics_artifact_refs.iter())
            .all(spec018_evidence_ref_has_owner_and_redaction)
        && result
            .skipped_evidence
            .iter()
            .all(spec018_skipped_evidence_is_non_blocking)
}

pub fn spec018_release_coverage_entry_passes(entry: &Spec018ReleaseCoverageEntry) -> bool {
    let has_required_evidence = !entry.required_evidence.is_empty();
    let has_verification_refs = !entry.test_refs.is_empty()
        || !entry.replay_refs.is_empty()
        || !entry.manual_refs.is_empty();
    let has_diagnostics_refs = !entry.diagnostics_artifact_refs.is_empty();
    let refs_are_valid = entry
        .required_evidence
        .iter()
        .chain(entry.test_refs.iter())
        .chain(entry.replay_refs.iter())
        .chain(entry.manual_refs.iter())
        .chain(entry.diagnostics_artifact_refs.iter())
        .all(spec018_evidence_ref_has_owner_and_redaction);

    entry.status == Spec018ReleaseCoverageStatus::Pass
        && entry.blocker_refs.is_empty()
        && has_required_evidence
        && has_verification_refs
        && has_diagnostics_refs
        && refs_are_valid
}

pub fn spec018_release_gate_blocks_for(
    blockers: &[Spec018ReleaseBlocker],
    category: Spec018ReleaseBlockerCategory,
) -> bool {
    blockers.iter().any(|blocker| {
        blocker.category == category
            && blocker.severity == Spec018ReleaseBlockerSeverity::Blocking
            && spec018_evidence_ref_has_owner_and_redaction(&blocker.source_ref)
    })
}

pub fn spec018_required_closure_buckets() -> Vec<Spec018ClosureCoverageBucket> {
    vec![
        Spec018ClosureCoverageBucket::EvaluatorFoundation,
        Spec018ClosureCoverageBucket::GoalContinuation,
        Spec018ClosureCoverageBucket::ApprovalGate,
        Spec018ClosureCoverageBucket::AutomationRuntime,
        Spec018ClosureCoverageBucket::MemorySkillIntegration,
        Spec018ClosureCoverageBucket::SelfImprovementWiring,
        Spec018ClosureCoverageBucket::ReplayRunner,
        Spec018ClosureCoverageBucket::ProjectionSemantics,
        Spec018ClosureCoverageBucket::DiagnosticsIntegration,
    ]
}

pub fn spec018_missing_closure_buckets(
    entries: &[Spec018ReleaseCoverageEntry],
) -> Vec<Spec018ClosureCoverageBucket> {
    spec018_required_closure_buckets()
        .into_iter()
        .filter(|bucket| {
            !entries.iter().any(|entry| {
                &entry.capability_area == bucket && spec018_release_coverage_entry_passes(entry)
            })
        })
        .collect()
}

pub fn spec018_final_closure_passes(
    entries: &[Spec018ReleaseCoverageEntry],
    blockers: &[Spec018ReleaseBlocker],
) -> bool {
    spec018_missing_closure_buckets(entries).is_empty()
        && blockers.iter().all(|blocker| {
            blocker.severity != Spec018ReleaseBlockerSeverity::Blocking
                && spec018_evidence_ref_has_owner_and_redaction(&blocker.source_ref)
        })
}

pub fn spec018_release_gate_outcome(
    entries: Vec<Spec018ReleaseCoverageEntry>,
    blockers: Vec<Spec018ReleaseBlocker>,
) -> Spec018ReleaseGateOutcome {
    let missing_buckets = spec018_missing_closure_buckets(&entries);
    let final_closure_passed = missing_buckets.is_empty()
        && blockers.iter().all(|blocker| {
            blocker.severity != Spec018ReleaseBlockerSeverity::Blocking
                && spec018_evidence_ref_has_owner_and_redaction(&blocker.source_ref)
        });

    Spec018ReleaseGateOutcome {
        status: if final_closure_passed {
            ReleaseGateStatus::Pass
        } else {
            ReleaseGateStatus::Blocked
        },
        coverage_entries: entries,
        blockers,
        missing_buckets,
        final_closure_passed,
    }
}

fn redacted_evidence_ref(
    kind: EvidenceKind,
    id: impl Into<String>,
    summary: impl Into<String>,
    redaction_status: RedactionStatus,
) -> EvidenceRef {
    let id = id.into();
    let summary = summary.into();
    let digest = Sha256::digest(format!("{kind:?}:{id}:{summary}:{redaction_status:?}"));

    EvidenceRef {
        kind,
        id,
        digest: format!("{digest:x}"),
        summary,
        redaction_status,
        owner_spec: Some("018".to_owned()),
        locator: None,
        retention_hint: Some("diagnostics_bundle".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shacs_utils::redaction::REDACTED;

    fn automation_trigger_ref(idempotency_key: &str) -> AutomationTriggerRef {
        AutomationTriggerRef {
            runtime_service_event_id: "event-1".to_owned(),
            source_type: "cron".to_owned(),
            source_owner: "runtime".to_owned(),
            received_at_ms: 10,
            idempotency_key: idempotency_key.to_owned(),
        }
    }

    #[test]
    fn automation_run_idempotency_key_uses_job_and_trigger_ref() {
        let trigger_ref = automation_trigger_ref("wake-1");

        assert_eq!(
            automation_run_idempotency_key("job-1", &trigger_ref),
            automation_run_idempotency_key("job-1", &trigger_ref)
        );
        assert_ne!(
            automation_run_idempotency_key("job-1", &trigger_ref),
            automation_run_idempotency_key("job-2", &trigger_ref)
        );
    }

    #[test]
    fn automation_trigger_kind_maps_to_evaluator_and_background_sources() {
        assert_eq!(
            automation_trigger_evaluation_source(&AutomationRunTriggerKind::SubagentResult),
            EvaluationTriggerSource::Subagent
        );
        assert_eq!(
            automation_trigger_background_source(&AutomationRunTriggerKind::LocalApiBackground),
            Some(BackgroundSourceKind::LocalApi)
        );
        assert_eq!(
            automation_trigger_evaluation_source(&AutomationRunTriggerKind::ManualResume),
            EvaluationTriggerSource::ManualReplay
        );
        assert_eq!(
            automation_trigger_background_source(&AutomationRunTriggerKind::ManualResume),
            None
        );
    }

    #[test]
    fn automation_timeout_classification_records_terminal_and_retryable_cases() {
        let retryable = classify_automation_timeout(
            "timeout-1",
            "run-1",
            "retry-policy-1",
            1,
            3,
            "deadline exceeded",
            Some("wake-2".to_owned()),
        );
        let terminal = classify_automation_timeout(
            "timeout-2",
            "run-2",
            "retry-policy-1",
            3,
            3,
            "deadline exceeded",
            Some("wake-3".to_owned()),
        );

        assert_eq!(
            retryable.disposition,
            AutomationTimeoutDisposition::Retryable
        );
        assert_eq!(retryable.next_eligible_wake_ref.as_deref(), Some("wake-2"));
        assert_eq!(terminal.disposition, AutomationTimeoutDisposition::Terminal);
    }

    #[test]
    fn automation_recursion_guard_blocks_loop_and_depth_overflow() {
        let loop_guard = AutomationRecursionGuard {
            token: "guard-1".to_owned(),
            source_run_id: Some("run-1".to_owned()),
            depth: 1,
            max_depth: 3,
            parent_refs: vec!["run-parent".to_owned()],
            blocked_reason: None,
        };
        let depth_guard = AutomationRecursionGuard {
            token: "guard-2".to_owned(),
            source_run_id: None,
            depth: 3,
            max_depth: 3,
            parent_refs: Vec::new(),
            blocked_reason: None,
        };

        assert_eq!(
            loop_guard
                .evaluate_next_run("run-1")
                .blocked_reason
                .as_deref(),
            Some("self-triggered automation loop")
        );
        assert_eq!(
            depth_guard
                .evaluate_next_run("run-4")
                .blocked_reason
                .as_deref(),
            Some("automation recursion depth overflow")
        );
    }

    #[test]
    fn default_evaluator_is_safe_notify_true_boundary() {
        let evaluator = NotifyOnEvaluatorFailure;
        assert_eq!(evaluator.evaluate_response(&[]), Ok(Some(true)));
        let messages = build_evaluator_messages("cron", "done");
        assert!(messages[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("cron"));
        assert_eq!(
            evaluate_notification_tool_schema()["function"]["name"],
            EVALUATE_NOTIFICATION_TOOL
        );
    }

    #[test]
    fn parser_defaults_to_notify_unless_valid_tool_decision_suppresses() {
        assert!(parse_notification_decision(
            &json!({"finish_reason": "stop"})
        ));
        assert!(!parse_notification_decision(&json!({
            "finish_reason": "tool_calls",
            "tool_calls": [{"function": {"arguments": "{\"should_notify\": false}"}}]
        })));
        assert!(parse_notification_decision(&json!({
            "finish_reason": "length",
            "tool_calls": [{"arguments": {"should_notify": false}}]
        })));
    }

    #[test]
    fn trajectory_record_preserves_redacted_replay_refs_without_raw_secret_fields() {
        let trajectory = trajectory_record();
        let serialized = serde_json::to_string(&trajectory).expect("trajectory should serialize");

        assert!(trajectory_preserves_replay_refs_with_redaction(&trajectory));
        assert_eq!(
            trajectory.model_call_refs[0].kind,
            EvidenceKind::ProviderSnapshot
        );
        assert_eq!(trajectory.tool_refs[0].kind, EvidenceKind::ToolPayload);
        assert_eq!(
            trajectory.evaluator_refs[0].kind,
            EvidenceKind::EvaluatorSummary
        );
        assert_eq!(
            trajectory.provider_snapshot_refs[0].kind,
            EvidenceKind::ProviderModelSnapshot
        );
        assert!(!serialized.contains("sk-hidden"));
        assert!(!serialized.contains("api_key"));
    }

    #[test]
    fn provider_snapshot_separates_main_route_from_auxiliary_judge_with_fallback_chain() {
        let main_snapshot = provider_snapshot(
            "provider-main",
            "model-main",
            ProviderRouteRole::MainAssistant,
            None,
            Vec::new(),
        );
        let evaluator_snapshot = provider_snapshot(
            "provider-judge-fallback",
            "model-judge-fallback",
            ProviderRouteRole::AuxiliaryJudge,
            Some(AuxiliaryJudgeRole::ReplayJudge),
            vec![ProviderFallbackStep {
                provider_id: "provider-judge-default".to_owned(),
                model_id: "model-judge-default".to_owned(),
                reason: JudgeFallbackReason::DefaultProviderFailed,
            }],
        );

        assert_eq!(main_snapshot.role, ProviderRouteRole::MainAssistant);
        assert_eq!(main_snapshot.evaluator_role, None);
        assert_eq!(evaluator_snapshot.role, ProviderRouteRole::AuxiliaryJudge);
        assert_eq!(
            evaluator_snapshot.evaluator_role,
            Some(AuxiliaryJudgeRole::ReplayJudge)
        );
        assert_eq!(evaluator_snapshot.fallback_chain.len(), 1);
    }

    #[test]
    fn auxiliary_judge_roles_serialize_to_prd012_role_names() {
        let serialized = serde_json::to_value([
            AuxiliaryJudgeRole::GoalCompletion,
            AuxiliaryJudgeRole::Capability,
            AuxiliaryJudgeRole::TaskOutcome,
            AuxiliaryJudgeRole::ReplayJudge,
        ])
        .expect("auxiliary judge roles should serialize");

        assert_eq!(serialized[0], "goal_completion_judge");
        assert_eq!(serialized[1], "capability_judge");
        assert_eq!(serialized[2], "task_outcome_judge");
        assert_eq!(serialized[3], "replay_comparison_judge");
    }

    #[test]
    fn judge_routing_fallback_reasons_distinguish_provider_model_budget_and_policy() {
        let reasons = [
            JudgeFallbackReason::DefaultProviderFailed,
            JudgeFallbackReason::PrimaryUnavailable,
            JudgeFallbackReason::ModelUnavailable,
            JudgeFallbackReason::BudgetExceeded,
            JudgeFallbackReason::PolicyDenied,
            JudgeFallbackReason::Timeout,
            JudgeFallbackReason::InvalidOutput,
        ];
        let serialized = serde_json::to_value(reasons).expect("fallback reasons should serialize");

        assert_eq!(serialized[0], "default_provider_failed");
        assert_eq!(serialized[1], "primary_unavailable");
        assert_eq!(serialized[2], "model_unavailable");
        assert_eq!(serialized[3], "budget_exceeded");
        assert_eq!(serialized[4], "policy_denied");
        assert_eq!(serialized[5], "timeout");
        assert_eq!(serialized[6], "invalid_output");

        let decision = judge_routing_decision(Some(JudgeFallbackReason::ModelUnavailable));
        assert!(judge_routing_records_distinct_fallback_and_denial(
            &decision
        ));
    }

    #[test]
    fn replay_safe_mock_schema_match_blocks_mismatched_fixture() {
        let mut policy = ReplayToolOutcomePolicy {
            tool_call_ref: replay_evidence_ref(EvidenceKind::ToolPayload, "tool-schema"),
            expected_schema_digest: "schema-a".to_owned(),
            recorded_outcome_ref: None,
            safe_mock_outcome: Some(ReplaySafeMockOutcome {
                mock_reason: "local destructive request substitute".to_owned(),
                source: "owner fixture".to_owned(),
                expected_schema_digest: "schema-a".to_owned(),
                limitations: vec!["does not prove live side effects".to_owned()],
                outcome_ref: replay_evidence_ref(EvidenceKind::ReplayResult, "mock-schema"),
            }),
            blocked_reason: None,
        };

        assert!(replay_safe_mock_schema_matches(&policy));
        if let Some(mock) = policy.safe_mock_outcome.as_mut() {
            mock.expected_schema_digest = "schema-b".to_owned();
        }
        assert!(!replay_safe_mock_schema_matches(&policy));
    }

    #[test]
    fn replay_dataset_comparison_separates_verdict_confidence_and_projection_mismatches() {
        let mut item = replay_dataset_item();
        item.actual_verdict = Some(VerdictKind::Fail);
        let verdict_mismatch = compare_replay_dataset_item(&item);

        item.actual_verdict = Some(VerdictKind::Pass);
        item.actual_confidence_band = Some(ConfidenceBand::Medium);
        let confidence_mismatch = compare_replay_dataset_item(&item);

        item.actual_confidence_band = Some(ConfidenceBand::High);
        item.actual_projection_status = Some(ProjectionStatus::Blocked);
        let projection_mismatch = compare_replay_dataset_item(&item);

        assert_eq!(
            verdict_mismatch.comparison_status,
            ReplayComparisonStatus::VerdictKindMismatch
        );
        assert_eq!(verdict_mismatch.severity, ReplayComparisonSeverity::High);
        assert_eq!(
            confidence_mismatch.comparison_status,
            ReplayComparisonStatus::ConfidenceBandMismatch
        );
        assert_eq!(confidence_mismatch.severity, ReplayComparisonSeverity::Low);
        assert_eq!(
            projection_mismatch.comparison_status,
            ReplayComparisonStatus::ProjectionStatusMismatch
        );
        assert_eq!(
            projection_mismatch.severity,
            ReplayComparisonSeverity::Medium
        );
    }

    #[test]
    fn replay_blocks_destructive_effects_and_uses_recorded_or_safe_mock_outcomes_only() {
        let destructive_with_recorded = ToolReplayInput {
            tool_ref: replay_evidence_ref(EvidenceKind::ReplayRecord, "tool-1"),
            effect: ToolEffectKind::Destructive,
            recorded_outcome_ref: Some(replay_evidence_ref(
                EvidenceKind::ReplayResult,
                "outcome-1",
            )),
            safe_mock_outcome_ref: None,
        };
        let destructive_with_mock = ToolReplayInput {
            tool_ref: replay_evidence_ref(EvidenceKind::ReplayRecord, "tool-2"),
            effect: ToolEffectKind::Destructive,
            recorded_outcome_ref: None,
            safe_mock_outcome_ref: Some(replay_evidence_ref(EvidenceKind::ReplayResult, "mock-1")),
        };
        let destructive_without_fixture = ToolReplayInput {
            tool_ref: replay_evidence_ref(EvidenceKind::ReplayRecord, "tool-3"),
            effect: ToolEffectKind::Destructive,
            recorded_outcome_ref: None,
            safe_mock_outcome_ref: None,
        };

        let recorded_plan = replay_tool_plan(
            &ReplaySafeEffectsPolicy::RecordedOutcomeOnly,
            &destructive_with_recorded,
        );
        let mock_plan = replay_tool_plan(
            &ReplaySafeEffectsPolicy::RecordedOrSafeMockOutcome,
            &destructive_with_mock,
        );
        let invalid_plan = replay_tool_plan(
            &ReplaySafeEffectsPolicy::RecordedOrSafeMockOutcome,
            &destructive_without_fixture,
        );

        assert!(!recorded_plan.execute_live_tool);
        assert_eq!(
            recorded_plan.outcome_source,
            ReplayToolOutcomeSource::RecordedOutcome
        );
        assert!(!mock_plan.execute_live_tool);
        assert_eq!(
            mock_plan.outcome_source,
            ReplayToolOutcomeSource::SafeMockOutcome
        );
        assert!(!invalid_plan.execute_live_tool);
        assert_eq!(
            invalid_plan.outcome_source,
            ReplayToolOutcomeSource::InvalidFixture
        );
        assert_eq!(
            invalid_plan.result_classification,
            Some(ReplayResultClassification::InvalidFixture)
        );
    }

    #[test]
    fn regression_case_compares_expected_verdict_and_outcome_to_replay_result() {
        let regression_case = quality_regression_case();

        assert_eq!(
            compare_regression_case(
                &regression_case,
                &replay_result(
                    ReplayResultClassification::Pass,
                    Some(VerdictKind::Pass),
                    Some(TaskOutcomeClass::Verify),
                ),
            ),
            ReplayResultClassification::Pass
        );
        assert_eq!(
            compare_regression_case(
                &regression_case,
                &replay_result(
                    ReplayResultClassification::Pass,
                    Some(VerdictKind::Fail),
                    Some(TaskOutcomeClass::Escalate),
                ),
            ),
            ReplayResultClassification::Regression
        );
        assert_eq!(
            compare_regression_case(
                &regression_case,
                &replay_result(ReplayResultClassification::Pass, None, None),
            ),
            ReplayResultClassification::Inconclusive
        );
        assert_eq!(
            compare_regression_case(
                &regression_case,
                &replay_result(ReplayResultClassification::InvalidFixture, None, None),
            ),
            ReplayResultClassification::InvalidFixture
        );
    }

    #[test]
    fn diagnostics_export_includes_only_redacted_replay_evidence() {
        let redacted_refs = vec![
            replay_evidence_ref(EvidenceKind::TrajectoryRecord, "trajectory-1"),
            replay_evidence_ref(EvidenceKind::ReplayRecord, "replay-1"),
            replay_evidence_ref(EvidenceKind::ReplayResult, "result-1"),
        ];
        let unredacted_ref = EvidenceRef {
            redaction_status: RedactionStatus::AlreadySafe,
            ..replay_evidence_ref(EvidenceKind::ReplayRecord, "unredacted-replay")
        };
        let non_replay_ref = replay_evidence_ref(EvidenceKind::TaskResult, "task-1");

        assert!(diagnostics_export_allowed_for_replay_evidence(
            &redacted_refs
        ));
        assert!(!diagnostics_export_allowed_for_replay_evidence(&[
            redacted_refs[0].clone(),
            unredacted_ref,
        ]));
        assert!(!diagnostics_export_allowed_for_replay_evidence(&[
            redacted_refs[0].clone(),
            non_replay_ref,
        ]));
    }

    #[test]
    fn evaluator_route_is_separate_from_main_assistant_route_and_fallback_is_ledgerable() {
        let main_snapshot = provider_snapshot(
            "provider-main",
            "model-main",
            ProviderRouteRole::MainAssistant,
            None,
            Vec::new(),
        );
        let evaluator_snapshot = provider_snapshot(
            "provider-judge-fallback",
            "model-judge-fallback",
            ProviderRouteRole::AuxiliaryJudge,
            Some(AuxiliaryJudgeRole::TaskOutcome),
            vec![ProviderFallbackStep {
                provider_id: "provider-judge-default".to_owned(),
                model_id: "model-judge-default".to_owned(),
                reason: JudgeFallbackReason::DefaultProviderFailed,
            }],
        );
        let mut decision = judge_routing_decision(Some(JudgeFallbackReason::DefaultProviderFailed));
        decision.judge_role = AuxiliaryJudgeRole::TaskOutcome;
        decision.selected_provider_id = evaluator_snapshot.provider_id.clone();
        decision.selected_model_id = evaluator_snapshot.model_id.clone();
        decision.provider_snapshot_ref = replay_evidence_ref(
            EvidenceKind::ProviderModelSnapshot,
            &evaluator_snapshot.snapshot_id,
        );

        assert!(provider_routes_are_separated_and_ledgerable(
            &main_snapshot,
            &evaluator_snapshot,
            &decision,
        ));
    }

    #[test]
    fn frozen_snapshot_redacts_payload_with_shared_redaction_rules() {
        let snapshot = FrozenEvaluationSnapshot::new_redacted(
            "snapshot-1",
            10,
            "corr-1",
            "default",
            &json!({
                "safe": "visible",
                "api_key": "secret-value",
                "nested": {"authorization": "Bearer sk-hidden"}
            }),
        );

        assert_eq!(snapshot.redacted_payload["safe"], "visible");
        assert_eq!(snapshot.redacted_payload["api_key"], REDACTED);
        assert_eq!(
            snapshot.redacted_payload["nested"]["authorization"],
            REDACTED
        );
    }

    #[test]
    fn frozen_snapshot_digest_is_stable_for_equivalent_json_ordering() {
        let left: Value = serde_json::from_str(r#"{"b":2,"a":{"d":4,"c":3}}"#)
            .expect("fixture JSON should parse");
        let right: Value = serde_json::from_str(r#"{"a":{"c":3,"d":4},"b":2}"#)
            .expect("fixture JSON should parse");
        let left_snapshot =
            FrozenEvaluationSnapshot::new_redacted("snapshot-1", 10, "corr-1", "default", &left);
        let right_snapshot =
            FrozenEvaluationSnapshot::new_redacted("snapshot-1", 10, "corr-1", "default", &right);

        let left_digest = left_snapshot
            .digest()
            .expect("snapshot digest should encode JSON fixture");
        let right_digest = right_snapshot
            .digest()
            .expect("snapshot digest should encode JSON fixture");

        assert_eq!(left_digest, right_digest);
        assert_eq!(left_digest.len(), 64);
        assert!(left_digest
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert_eq!(left_digest, left_digest.to_ascii_lowercase());
    }

    #[test]
    fn evaluator_envelopes_are_advisory_not_execution_authority() {
        let request = EvaluatorRequestEnvelope {
            request_id: "request-1".to_owned(),
            evaluator_kind: EvaluatorKind::SafetyCapability,
            correlation_id: "corr-1".to_owned(),
            session_id: Some("session-1".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            source: EvaluationTriggerSource::SessionTurn,
            snapshot_digest: "abc".to_owned(),
            redaction_profile: "default".to_owned(),
            caller_intent: "check capability before tool use".to_owned(),
        };
        let verdict = EvaluatorVerdictEnvelope {
            verdict_kind: VerdictKind::Pass,
            reason: "safe to consider".to_owned(),
            confidence: 0.91,
            evidence_refs: Vec::new(),
            suggested_next_action: SuggestedNextAction::EscalateToOrchestrator,
            expires_at_ms: Some(20),
            redaction_status: RedactionStatus::AlreadySafe,
            evaluator_version: "foundation-test".to_owned(),
        };

        assert_eq!(
            request.authority_boundary(),
            EvaluatorAuthority::AdvisoryOnly
        );
        assert_eq!(
            verdict.authority_boundary(),
            EvaluatorAuthority::AdvisoryOnly
        );
        assert!(!request.grants_execution_authority());
        assert!(!verdict.grants_execution_authority());
    }

    #[test]
    fn ledger_records_serialize_and_project_evaluation_boundary() {
        let evidence = EvidenceRef {
            kind: EvidenceKind::TaskResult,
            id: "result-1".to_owned(),
            digest: "digest-1".to_owned(),
            summary: "redacted task result".to_owned(),
            redaction_status: RedactionStatus::Redacted,
            owner_spec: Some("018".to_owned()),
            locator: Some("ledger://task/result-1".to_owned()),
            retention_hint: None,
        };
        let request = EvaluatorRequestEnvelope {
            request_id: "request-1".to_owned(),
            evaluator_kind: EvaluatorKind::TaskOutcome,
            correlation_id: "corr-1".to_owned(),
            session_id: Some("session-1".to_owned()),
            turn_id: None,
            source: EvaluationTriggerSource::AppTask,
            snapshot_digest: "snapshot-digest".to_owned(),
            redaction_profile: "default".to_owned(),
            caller_intent: "classify background result".to_owned(),
        };
        let verdict = EvaluatorVerdictEnvelope {
            verdict_kind: VerdictKind::LowConfidence,
            reason: "evidence is incomplete".to_owned(),
            confidence: 0.42,
            evidence_refs: vec![evidence.clone()],
            suggested_next_action: SuggestedNextAction::AskUser,
            expires_at_ms: None,
            redaction_status: RedactionStatus::Redacted,
            evaluator_version: "foundation-test".to_owned(),
        };
        let evaluation = EvaluationLedgerRecord {
            record_id: "eval-record-1".to_owned(),
            evaluator_kind: EvaluatorKind::TaskOutcome,
            request,
            snapshot_id: "snapshot-1".to_owned(),
            snapshot_digest: "snapshot-digest".to_owned(),
            verdict,
            authority_boundary: EvaluatorAuthority::AdvisoryOnly,
            created_at_ms: 30,
        };
        let task = TaskLedgerRecord {
            record_id: "task-record-1".to_owned(),
            task_id: "task-1".to_owned(),
            correlation_id: "corr-1".to_owned(),
            source: EvaluationTriggerSource::AppTask,
            status: TaskLedgerStatus::TimedOut,
            job_id: Some("job-1".to_owned()),
            result_ref: Some(evidence),
            outcome_request_id: Some("request-1".to_owned()),
            delivery_ref: None,
            retry_or_rollback_ref: Some("retry-1".to_owned()),
            created_at_ms: 29,
        };

        let serialized_evaluation =
            serde_json::to_value(EvaluatorLedgerRecord::Evaluation(evaluation.clone()))
                .expect("evaluation ledger record should serialize");
        let serialized_task = serde_json::to_value(EvaluatorLedgerRecord::Task(task))
            .expect("task ledger record should serialize");
        let projection = evaluation.projection();

        assert_eq!(serialized_evaluation["ledger_kind"], "evaluation");
        assert_eq!(serialized_task["ledger_kind"], "task");
        assert_eq!(projection.request_id, "request-1");
        assert_eq!(projection.correlation_id, "corr-1");
        assert_eq!(projection.verdict_kind, VerdictKind::LowConfidence);
        assert_eq!(
            projection.authority_boundary,
            EvaluatorAuthority::AdvisoryOnly
        );
    }

    #[test]
    fn capability_evaluator_request_and_verdict_serialize_with_advisory_boundary() {
        let input = capability_input(PermissionMode::Default);
        let verdict = capability_verdict(CapabilityDecisionHint::AllowCandidate);

        let serialized_input =
            serde_json::to_value(&input).expect("capability evaluation input should serialize");
        let serialized_verdict =
            serde_json::to_value(&verdict).expect("capability verdict should serialize");

        assert_eq!(serialized_input["action_kind"], "destructive_write");
        assert_eq!(serialized_input["target_digest"], "target-digest-1");
        assert_eq!(serialized_input["requested_capabilities"][0], "fs_write");
        assert_eq!(
            serialized_input["permission_mode_snapshot"]["mode"],
            "default"
        );
        assert_eq!(
            serialized_input["approval_context"]["correlation_id"],
            "corr-002"
        );
        assert_eq!(serialized_input["checkpoint_hint"]["required"], true);
        assert_eq!(serialized_input["correlation_id"], "corr-002");
        assert_eq!(serialized_input["snapshot_digest"], "snapshot-digest-1");
        assert_eq!(serialized_verdict["hint"], "allow_candidate");
        assert_eq!(
            verdict.authority_boundary(),
            EvaluatorAuthority::AdvisoryOnly
        );
        assert!(!verdict.grants_execution_authority());
    }

    #[test]
    fn permission_mode_denial_overrides_allow_candidate() {
        let mut input = capability_input(PermissionMode::Default);
        input.permission_mode_snapshot.denied_capabilities = vec!["fs_write".to_owned()];
        let verdict = capability_verdict(CapabilityDecisionHint::AllowCandidate);

        let denied = consume_permission_mode_before_verdict(&input, &verdict)
            .expect_err("permission denial should override allow_candidate");

        assert_eq!(denied.code, "permission_capability_denied");
        assert_eq!(denied.retry_class, DeniedRetryClass::RetryAfterUserAction);
        assert_eq!(denied.correlation_id, "corr-002");
    }

    #[test]
    fn approval_request_decision_correlation_mismatch_is_denied() {
        let request = approval_request();
        let mut decision = approval_decision();
        decision.request_id = "other-request".to_owned();

        let denied = validate_approval_decision_consumption(
            &request,
            &decision,
            "snapshot-digest-1",
            20,
            Some(evidence_ref()),
        )
        .expect_err("mismatched request id should be denied");

        assert_eq!(denied.code, "approval_request_mismatch");
        assert_eq!(denied.redacted_evidence_ref, Some(evidence_ref()));
        assert_eq!(
            denied.required_next_step,
            "Review the current risk summary and approve the matching request again."
        );
    }

    #[test]
    fn expired_approval_rejection_is_denied_outcome() {
        let request = approval_request();
        let decision = approval_decision();

        let denied = validate_approval_decision_consumption(
            &request,
            &decision,
            "snapshot-digest-1",
            100,
            Some(evidence_ref()),
        )
        .expect_err("expired approval should be denied");

        assert_eq!(denied.code, "approval_expired");
        assert_eq!(denied.retry_class, DeniedRetryClass::RetryAfterUserAction);
        assert_eq!(
            denied.required_next_step,
            "Request approval again before executing the action."
        );
    }

    #[test]
    fn stale_snapshot_approval_rejection_is_denied_outcome() {
        let request = approval_request();
        let decision = approval_decision();

        let denied = validate_approval_decision_consumption(
            &request,
            &decision,
            "fresh-snapshot-digest",
            20,
            Some(evidence_ref()),
        )
        .expect_err("stale approval snapshot should be denied");

        assert_eq!(denied.code, "stale_approval_snapshot");
        assert_eq!(denied.retry_class, DeniedRetryClass::RetryWithFreshSnapshot);
        assert_eq!(
            denied.required_next_step,
            "Re-run evaluation with a fresh snapshot before requesting approval again."
        );
    }

    #[test]
    fn mismatched_action_digest_approval_rejection_is_denied_outcome() {
        let request = approval_request();
        let mut decision = approval_decision();
        decision.action_digest = "other-action-digest".to_owned();

        let denied = validate_approval_decision_consumption(
            &request,
            &decision,
            "snapshot-digest-1",
            20,
            Some(evidence_ref()),
        )
        .expect_err("mismatched action digest should be denied");

        assert_eq!(denied.code, "approval_action_mismatch");
        assert_eq!(denied.retry_class, DeniedRetryClass::RetryAfterUserAction);
        assert_eq!(
            denied.required_next_step,
            "Review the current action and approve the matching request again."
        );
    }

    #[test]
    fn checkpoint_required_action_without_inspectable_checkpoint_is_blocked() {
        let missing = CheckpointHint {
            required: false,
            checkpoint_ref: None,
            inspectable: false,
        };
        let not_inspectable = CheckpointHint {
            required: false,
            checkpoint_ref: Some("checkpoint-1".to_owned()),
            inspectable: false,
        };

        let missing_decision =
            decide_checkpoint_gate(&CapabilityActionKind::SelfImprovementApply, &missing);
        let not_inspectable_decision =
            decide_checkpoint_gate(&CapabilityActionKind::DestructiveWrite, &not_inspectable);

        assert_eq!(missing_decision.status, CheckpointGateStatus::Blocked);
        assert!(missing_decision.required);
        assert_eq!(
            not_inspectable_decision.status,
            CheckpointGateStatus::Blocked
        );
        assert_eq!(
            not_inspectable_decision.checkpoint_ref,
            Some("checkpoint-1".to_owned())
        );
    }

    #[test]
    fn denied_outcome_standard_fields_are_reusable() {
        let evidence = evidence_ref();
        let denied = denied_outcome(
            "approval_action_mismatch",
            "Approval decision was for a different action and cannot be consumed.",
            Some(evidence.clone()),
            DeniedRetryClass::RetryAfterUserAction,
            "Review the current action and approve the matching request again.",
            "corr-002",
        );
        let serialized = serde_json::to_value(&denied).expect("denied outcome should serialize");

        assert_eq!(denied.code, "approval_action_mismatch");
        assert_eq!(
            denied.message,
            "Approval decision was for a different action and cannot be consumed."
        );
        assert_eq!(denied.redacted_evidence_ref, Some(evidence));
        assert_eq!(denied.retry_class, DeniedRetryClass::RetryAfterUserAction);
        assert_eq!(
            denied.required_next_step,
            "Review the current action and approve the matching request again."
        );
        assert_eq!(
            serialized["message"],
            "Approval decision was for a different action and cannot be consumed."
        );
        assert_eq!(serialized["retry_class"], "retry_after_user_action");
    }

    #[test]
    fn background_source_kinds_normalize_to_same_result_ref_contract() {
        let sources = [
            BackgroundSourceKind::Heartbeat,
            BackgroundSourceKind::Cron,
            BackgroundSourceKind::Subagent,
            BackgroundSourceKind::App,
            BackgroundSourceKind::Channel,
            BackgroundSourceKind::LocalApi,
        ];

        for source in sources {
            let result_ref = normalize_background_result_ref(
                source,
                "source-1",
                "digest-1",
                BackgroundExitStatus::Success,
                BackgroundResultTiming {
                    started_at_ms: 10,
                    completed_at_ms: Some(20),
                },
                None,
                "corr-003",
            );
            let serialized =
                serde_json::to_value(&result_ref).expect("background result should serialize");

            assert_eq!(serialized["source_id"], "source-1");
            assert_eq!(serialized["redacted_payload_digest"], "digest-1");
            assert_eq!(serialized["exit_status"], "success");
            assert_eq!(serialized["correlation_id"], "corr-003");
        }
    }

    #[test]
    fn outcome_classes_map_to_distinct_next_action_semantics() {
        let cases = [
            (TaskOutcomeClass::Notify, TaskOutcomeNextAction::Delivery),
            (
                TaskOutcomeClass::Suppress,
                TaskOutcomeNextAction::NoDelivery,
            ),
            (
                TaskOutcomeClass::ContinueTask,
                TaskOutcomeNextAction::ContinueWithGates,
            ),
            (
                TaskOutcomeClass::Escalate,
                TaskOutcomeNextAction::UserAttention,
            ),
            (
                TaskOutcomeClass::Verify,
                TaskOutcomeNextAction::VerificationRequired,
            ),
            (
                TaskOutcomeClass::Rollback,
                TaskOutcomeNextAction::RollbackRequired,
            ),
        ];

        for (class, expected_action) in cases {
            let semantics = task_outcome_next_action_semantics(&class);
            assert_eq!(semantics.action, expected_action);
        }

        let serialized_continue = serde_json::to_value(TaskOutcomeClass::ContinueTask)
            .expect("outcome class should serialize");
        assert_eq!(serialized_continue, "continue");

        let continue_semantics =
            task_outcome_next_action_semantics(&TaskOutcomeClass::ContinueTask);
        assert!(continue_semantics.continuation_requires_persistent_goal);
        assert!(continue_semantics.continuation_requires_budget);
        assert!(continue_semantics.continuation_requires_recursion_guard);
        assert!(continue_semantics.continuation_requires_permission_gate);
    }

    #[test]
    fn one_shot_completed_job_is_not_schedulable_while_recurring_remains_schedulable() {
        let mut job = automation_job(AutomationSchedule::OneShot {
            scheduled_at_ms: 10,
        });
        assert!(automation_job_is_schedulable(&job));

        job.status = AutomationJobStatus::Completed;
        assert!(!automation_job_is_schedulable(&job));

        let recurring = AutomationJob {
            schedule: AutomationSchedule::Recurring {
                schedule_ref: "cron://daily".to_owned(),
                next_run_at_ms: Some(100),
            },
            status: AutomationJobStatus::Completed,
            ..automation_job(AutomationSchedule::OneShot {
                scheduled_at_ms: 10,
            })
        };

        assert!(automation_job_is_schedulable(&recurring));
    }

    #[test]
    fn timeout_result_ledgers_independently_of_delivery_failure() {
        let result_ref = background_result_ref(BackgroundExitStatus::TimedOut);
        let run = AutomationRun {
            run_id: "run-1".to_owned(),
            job_id: "job-1".to_owned(),
            trigger: AutomationTriggerKind::ScheduledWake,
            status: AutomationRunStatus::TimedOut,
            started_at_ms: 10,
            timeout_at_ms: Some(20),
            completed_at_ms: Some(20),
            result_ref: Some(result_ref.clone()),
            outcome_verdict_id: Some("verdict-1".to_owned()),
            correlation_id: "corr-003".to_owned(),
        };
        let delivery = delivery_record(result_ref, DeliveryStatus::Failed);

        assert_eq!(task_result_status(&run), AutomationRunStatus::TimedOut);
        assert!(delivery_status_is_independent_from_task_result(
            &run, &delivery
        ));
        assert!(!delivery_failure_poisoned_task_result(&run, &delivery));

        let successful_result_ref = background_result_ref(BackgroundExitStatus::Success);
        let successful_run = AutomationRun {
            status: AutomationRunStatus::Succeeded,
            result_ref: Some(successful_result_ref.clone()),
            ..run.clone()
        };
        let failed_delivery =
            delivery_record(successful_result_ref.clone(), DeliveryStatus::Failed);
        assert!(delivery_status_is_independent_from_task_result(
            &successful_run,
            &failed_delivery
        ));
        assert!(!delivery_failure_poisoned_task_result(
            &successful_run,
            &failed_delivery
        ));

        let poisoned_run = AutomationRun {
            status: AutomationRunStatus::Failed,
            result_ref: Some(successful_result_ref),
            ..run
        };
        assert!(delivery_failure_poisoned_task_result(
            &poisoned_run,
            &failed_delivery
        ));
    }

    #[test]
    fn recursion_guard_blocks_self_triggering_loop_and_depth_overflow() {
        let guard = RecursionGuard {
            root_trigger_id: "evaluator:task_outcome".to_owned(),
            depth: 1,
            source_chain: vec!["heartbeat".to_owned(), "evaluator:task_outcome".to_owned()],
            max_depth: 3,
            blocked_reason: None,
        };
        let loop_decision = guard.evaluate_continuation("evaluator:task_outcome");
        assert!(!loop_decision.allowed);
        assert_eq!(
            loop_decision.blocked_reason,
            Some("self_triggering_loop".to_owned())
        );

        let overflow_guard = RecursionGuard {
            depth: 3,
            source_chain: vec!["heartbeat".to_owned()],
            ..guard
        };
        let overflow_decision = overflow_guard.evaluate_continuation("subagent");
        assert!(!overflow_decision.allowed);
        assert_eq!(
            overflow_decision.blocked_reason,
            Some("max_depth_exceeded".to_owned())
        );
    }

    #[test]
    fn verify_outcome_is_not_automatic_success() {
        let semantics = task_outcome_next_action_semantics(&TaskOutcomeClass::Verify);

        assert_eq!(
            semantics.action,
            TaskOutcomeNextAction::VerificationRequired
        );
        assert!(semantics.verification_required);
        assert!(!semantics.automatic_success);
    }

    #[test]
    fn rollback_outcome_requires_checkpoint_and_rollback_readiness() {
        let verdict = task_outcome_verdict(TaskOutcomeClass::Rollback);
        let semantics = task_outcome_next_action_semantics(&verdict.class);

        assert!(semantics.rollback_requires_checkpoint);
        assert!(semantics.rollback_requires_primitive);
        assert!(!rollback_action_ready(&verdict, true, false));
        assert!(!rollback_action_ready(&verdict, false, true));
        assert!(rollback_action_ready(&verdict, true, true));
    }

    #[test]
    fn bounded_memory_evidence_respects_budget_and_omits_raw_payloads() {
        let refs = vec![
            evidence_ref_with_id("memory-1", "digest-1"),
            evidence_ref_with_id("memory-2", "digest-2"),
            evidence_ref_with_id("memory-3", "digest-3"),
        ];

        let evidence_set = build_bounded_memory_evidence_set(BoundedMemoryEvidenceSetInput {
            evidence_id: "memory-set-1".to_owned(),
            request_id: "memory-request-1".to_owned(),
            query: "recent evaluator facts".to_owned(),
            source_scope: "local_memory".to_owned(),
            cutoff: "2026-05-20T00:00:00Z".to_owned(),
            max_result_refs: 2,
            created_at_ms: 10,
            frozen_at_ms: 20,
            candidate_refs: refs,
            summary_ref: None,
            redaction_profile: "default".to_owned(),
            omitted_reason: MemoryEvidenceOmittedReason::OmittedByBudget,
        })
        .expect("memory evidence set should digest refs");
        let serialized =
            serde_json::to_value(&evidence_set).expect("memory evidence should serialize");

        assert_eq!(evidence_set.result_refs.len(), 2);
        assert_eq!(evidence_set.omitted_refs.len(), 1);
        assert_eq!(evidence_set.redaction_profile, "default");
        assert_eq!(evidence_set.request_id, "memory-request-1");
        assert_eq!(evidence_set.created_at_ms, 10);
        assert_eq!(evidence_set.frozen_at_ms, 20);
        assert_eq!(evidence_set.candidate_count, 3);
        assert_eq!(evidence_set.result_count, 2);
        assert_eq!(evidence_set.omitted_count, 1);
        assert_eq!(
            evidence_set.omitted_reason,
            Some(MemoryEvidenceOmittedReason::OmittedByBudget)
        );
        assert!(serialized.get("raw_payload").is_none());
        assert_eq!(evidence_set.result_digest.len(), 64);
    }

    #[test]
    fn frozen_session_search_snapshot_digest_is_stable_and_stale_when_refs_change() {
        let search_input = json!({"query": "approval", "scope": "session"});
        let refs = vec![
            evidence_ref_with_id("event-1", "digest-1"),
            evidence_ref_with_id("event-2", "digest-2"),
        ];
        let same_refs = refs.clone();
        let changed_refs = vec![evidence_ref_with_id("event-1", "digest-1")];

        let snapshot = frozen_session_search_snapshot("search-snapshot-1", &search_input, refs, 10)
            .expect("search snapshot should digest refs");
        let repeated = frozen_session_search_snapshot(
            "search-snapshot-2",
            &search_input,
            same_refs.clone(),
            20,
        )
        .expect("search snapshot should digest refs");

        assert_eq!(snapshot.search_input_digest, repeated.search_input_digest);
        assert_eq!(snapshot.result_digest, repeated.result_digest);
        assert!(
            frozen_session_search_snapshot_is_fresh(&snapshot, &same_refs)
                .expect("freshness check should digest refs")
        );
        assert!(
            !frozen_session_search_snapshot_is_fresh(&snapshot, &changed_refs)
                .expect("freshness check should digest changed refs")
        );
    }

    #[test]
    fn evaluator_summary_ref_records_sources_omissions_confidence_and_redaction() {
        let source_refs = vec![evidence_ref_with_id("source-1", "digest-1")];
        let omitted_refs = vec![evidence_ref_with_id("source-2", "digest-2")];

        let summary = evaluator_summary_ref(
            "summary-1",
            source_refs.clone(),
            omitted_refs.clone(),
            Some("budget_cut".to_owned()),
            "redacted summary",
            0.82,
            RedactionStatus::Redacted,
        )
        .expect("summary digest should be stable");

        assert_eq!(summary.source_refs, source_refs);
        assert_eq!(summary.omitted_refs, omitted_refs);
        assert_eq!(summary.omitted_reason.as_deref(), Some("budget_cut"));
        assert_eq!(summary.confidence, 0.82);
        assert_eq!(summary.redaction_status, RedactionStatus::Redacted);
        assert_eq!(summary.summary_digest.len(), 64);
    }

    #[test]
    fn skill_disclosure_levels_expose_list_view_and_reference_boundaries() {
        let list = skill_list_disclosure(
            "skill-disclosure-1",
            "review-work",
            "local",
            "active",
            "Reviews implementation work",
            "skill-digest-1",
        );
        let view = skill_view_disclosure(&list, "name: review-work\napi_key=sk-secret")
            .expect("skill body digest should be stable");
        let reference = skill_reference_disclosure(&list);
        let serialized_list =
            serde_json::to_value(&list).expect("list disclosure should serialize");

        assert_eq!(list.level, SkillDisclosureLevel::List);
        assert_eq!(view.level, SkillDisclosureLevel::View);
        assert_eq!(reference.level, SkillDisclosureLevel::Reference);
        assert!(serialized_list.get("redacted_body").is_none());
        assert!(serialized_list.get("body_digest").is_none());
        assert!(view
            .redacted_body
            .as_deref()
            .unwrap_or_default()
            .contains(REDACTED));
        assert!(view.body_digest.is_some());
        assert_eq!(
            reference
                .evidence_ref
                .as_ref()
                .map(|evidence| &evidence.kind),
            Some(&EvidenceKind::SkillDisclosure)
        );
        assert_eq!(
            reference
                .evidence_ref
                .as_ref()
                .map(|evidence| evidence.digest.as_str()),
            Some("skill-digest-1")
        );
    }

    #[test]
    fn authored_skill_becomes_active_only_after_dry_run_and_approval() {
        let mut lifecycle = authored_skill_lifecycle_draft("skill-1", vec![evidence_ref()]);

        assert_eq!(lifecycle.state, AuthoredSkillLifecycleState::Draft);
        assert!(!authored_skill_can_become_active(&lifecycle));

        lifecycle.state = AuthoredSkillLifecycleState::DryRunPending;
        lifecycle.dry_run_passed = true;
        assert!(!authored_skill_can_become_active(&lifecycle));

        lifecycle.state = AuthoredSkillLifecycleState::DryRunFailed;
        assert!(!authored_skill_can_become_active(&lifecycle));

        lifecycle.state = AuthoredSkillLifecycleState::ApprovalPending;
        assert!(!authored_skill_can_become_active(&lifecycle));

        lifecycle.approval_granted = true;
        assert!(!authored_skill_can_become_active(&lifecycle));

        lifecycle.state = AuthoredSkillLifecycleState::ActiveCandidate;
        assert!(authored_skill_can_become_active(&lifecycle));

        lifecycle.state = AuthoredSkillLifecycleState::Active;
        assert!(authored_skill_is_active_injection_candidate(&lifecycle));
    }

    #[test]
    fn stale_and_archived_authored_skills_keep_audit_boundaries() {
        let mut lifecycle = authored_skill_lifecycle_draft("skill-1", vec![evidence_ref()]);

        lifecycle.state = AuthoredSkillLifecycleState::Stale;
        lifecycle.dry_run_passed = true;
        lifecycle.approval_granted = true;
        assert!(authored_skill_is_disable_candidate(&lifecycle));
        assert!(!authored_skill_is_active_injection_candidate(&lifecycle));
        assert!(authored_skill_remains_replay_evidence(&lifecycle));

        lifecycle.state = AuthoredSkillLifecycleState::Archived;
        assert!(!authored_skill_is_disable_candidate(&lifecycle));
        assert!(!authored_skill_is_active_injection_candidate(&lifecycle));
        assert!(authored_skill_remains_replay_evidence(&lifecycle));
    }

    #[test]
    fn curator_recommendation_never_auto_deletes_or_activates_without_approval() {
        let delete_recommendation = CuratorRecommendation {
            recommendation_id: "recommendation-1".to_owned(),
            target_kind: CuratorTargetKind::Memory,
            action_proposed: CuratorActionProposed::DeleteMemory,
            reason: "low value duplicate".to_owned(),
            evidence_refs: vec![evidence_ref()],
            requires_approval: true,
        };
        let activate_recommendation = CuratorRecommendation {
            recommendation_id: "recommendation-2".to_owned(),
            target_kind: CuratorTargetKind::Skill,
            action_proposed: CuratorActionProposed::ActivateSkill,
            reason: "dry run passed".to_owned(),
            evidence_refs: vec![evidence_ref()],
            requires_approval: true,
        };

        assert!(curator_action_requires_approval(
            &delete_recommendation.action_proposed
        ));
        assert!(!curator_recommendation_allows_execution(
            &delete_recommendation,
            false
        ));
        assert!(!curator_recommendation_allows_execution(
            &activate_recommendation,
            false
        ));
        assert!(curator_recommendation_allows_execution(
            &activate_recommendation,
            true
        ));
    }

    #[test]
    fn prd010_memory_omitted_reasons_and_curator_proposal_lineage_are_constrained() {
        let reasons = [
            MemoryEvidenceOmittedReason::OmittedByBudget,
            MemoryEvidenceOmittedReason::OmittedByRedaction,
            MemoryEvidenceOmittedReason::OmittedByCutoff,
            MemoryEvidenceOmittedReason::OmittedByRelevance,
        ];
        let serialized = reasons
            .iter()
            .map(|reason| serde_json::to_value(reason).expect("reason serializes"))
            .collect::<Vec<_>>();

        assert_eq!(
            serialized,
            vec![
                json!("omitted_by_budget"),
                json!("omitted_by_redaction"),
                json!("omitted_by_cutoff"),
                json!("omitted_by_relevance")
            ]
        );

        let proposal = curator_proposal(
            "proposal-1",
            CuratorTargetKind::Memory,
            vec![evidence_ref_with_id("memory-target-1", "memory-digest-1")],
            "duplicate memory",
            vec![evidence_ref()],
            CuratorActionProposed::DeleteMemory,
            Some(evidence_ref_with_id("approval-1", "approval-digest-1")),
        );

        assert_eq!(proposal.proposal_id, "proposal-1");
        assert_eq!(
            proposal.final_status,
            CuratorProposalFinalStatus::ApprovalPending
        );
        assert!(proposal.approval_ref.is_some());
        assert_eq!(proposal.target_refs.len(), 1);
    }

    #[test]
    fn improvement_lifecycle_states_serialize_and_approval_does_not_change_runtime() {
        let serialized = serde_json::to_value(ImprovementProposalStatus::ApprovalPending)
            .expect("status should serialize");
        assert_eq!(serialized, json!("approval_pending"));

        let proposal = improvement_proposal(ImprovementProposalStatus::ApprovalPending);
        let approval = improvement_approval();

        assert!(!improvement_proposal_can_affect_runtime(&proposal));
        assert!(!improvement_approval_changes_runtime_behavior(
            &proposal, &approval
        ));

        let applied = improvement_proposal(ImprovementProposalStatus::Applied);
        assert!(improvement_proposal_can_affect_runtime(&applied));
    }

    #[test]
    fn improvement_apply_order_cannot_skip_approval_or_checkpoint() {
        let approval = improvement_approval();
        let checkpoint = improvement_checkpoint();
        let gate = improvement_checkpoint_gate();

        let approved_without_checkpoint = improvement_proposal(ImprovementProposalStatus::Approved);
        let denied = validate_improvement_apply_readiness(
            &approved_without_checkpoint,
            &approval,
            &checkpoint,
            &gate,
            20,
            Some(evidence_ref()),
        )
        .expect_err("apply before checkpoint should be denied");
        assert_eq!(denied.code, "improvement_not_checkpointed");

        let mut pending_approval = improvement_approval();
        pending_approval.request_ref.status = ApprovalRequestStatus::Pending;
        let checkpointed = improvement_proposal(ImprovementProposalStatus::Checkpointed);
        let denied = validate_improvement_apply_readiness(
            &checkpointed,
            &pending_approval,
            &checkpoint,
            &gate,
            20,
            Some(evidence_ref()),
        )
        .expect_err("apply before approval should be denied");
        assert_eq!(denied.code, "improvement_approval_not_ready");

        let mut mismatched_scope = approval.clone();
        mismatched_scope.approved_scope = vec!["other_scope".to_owned()];
        let denied = validate_improvement_apply_readiness(
            &checkpointed,
            &mismatched_scope,
            &checkpoint,
            &gate,
            20,
            Some(evidence_ref()),
        )
        .expect_err("mismatched approval scope should deny apply");
        assert_eq!(denied.code, "improvement_approval_not_ready");

        let mut broad_target_kind_scope = approval.clone();
        broad_target_kind_scope.approved_scope = vec!["app_manifest".to_owned()];
        let denied = validate_improvement_apply_readiness(
            &checkpointed,
            &broad_target_kind_scope,
            &checkpoint,
            &gate,
            20,
            Some(evidence_ref()),
        )
        .expect_err("target_kind approval must not cover a specific target_ref");
        assert_eq!(denied.code, "improvement_approval_not_ready");

        let denied = validate_improvement_apply_readiness(
            &checkpointed,
            &approval,
            &checkpoint,
            &gate,
            91,
            Some(evidence_ref()),
        )
        .expect_err("expired approval should deny apply");
        assert_eq!(denied.code, "improvement_approval_not_ready");

        let blocked_gate = CheckpointGateDecision {
            status: CheckpointGateStatus::Blocked,
            required: true,
            reason: "missing inspectable checkpoint".to_owned(),
            checkpoint_ref: Some("checkpoint-1".to_owned()),
        };
        let denied = validate_improvement_apply_readiness(
            &checkpointed,
            &approval,
            &checkpoint,
            &blocked_gate,
            20,
            Some(evidence_ref()),
        )
        .expect_err("blocked checkpoint gate should deny apply");
        assert_eq!(denied.code, "improvement_checkpoint_not_ready");

        validate_improvement_apply_readiness(
            &checkpointed,
            &approval,
            &checkpoint,
            &gate,
            20,
            None,
        )
        .expect("approved checkpointed proposal should be apply-ready");
    }

    #[test]
    fn app_task_can_create_proposal_but_cannot_approve_or_apply_alone() {
        assert!(app_task_improvement_authority(
            &ImprovementActorAuthority::AppTask,
            &ImprovementAuthorityAction::CreateProposal
        ));
        assert!(!app_task_improvement_authority(
            &ImprovementActorAuthority::AppTask,
            &ImprovementAuthorityAction::Approve
        ));
        assert!(!app_task_improvement_authority(
            &ImprovementActorAuthority::AppTask,
            &ImprovementAuthorityAction::Apply
        ));
        assert!(app_task_improvement_authority(
            &ImprovementActorAuthority::LocalUser,
            &ImprovementAuthorityAction::Approve
        ));
        assert!(app_task_improvement_authority(
            &ImprovementActorAuthority::OwnerPrimitive,
            &ImprovementAuthorityAction::Apply
        ));
    }

    #[test]
    fn mcp_exposure_projection_defaults_deny_and_requires_proposal_approval_to_widen() {
        let projection = default_mcp_exposure_projection(
            "tool:search",
            "session",
            "deny",
            "mcp exposure is default deny until local approval",
        );

        assert_eq!(projection.current_exposure, "deny");
        assert!(projection.default_deny_reason.is_some());
        assert!(!mcp_exposure_can_widen(&projection));

        let mut proposed = projection.clone();
        proposed.proposal_id = Some("proposal-1".to_owned());
        proposed.correlation_id = Some("corr-005".to_owned());
        assert!(!mcp_exposure_can_widen(&proposed));

        proposed.approval_ref = Some(approval_decision());
        assert!(mcp_exposure_can_widen(&proposed));
    }

    #[test]
    fn failed_improvement_verification_rolls_back_only_when_checkpoint_and_owner_primitive_ready() {
        let mut verification = improvement_verification(false);
        let checkpoint = improvement_checkpoint();

        assert_eq!(
            failed_improvement_verification_next_action(&verification, Some(&checkpoint), true),
            ImprovementVerificationNextAction::Rollback
        );
        assert_eq!(
            failed_improvement_verification_next_action(&verification, Some(&checkpoint), false),
            ImprovementVerificationNextAction::ReportFailed
        );
        assert_eq!(
            failed_improvement_verification_next_action(&verification, None, true),
            ImprovementVerificationNextAction::ReportFailed
        );

        verification.passed = true;
        assert_eq!(
            failed_improvement_verification_next_action(&verification, Some(&checkpoint), true),
            ImprovementVerificationNextAction::RecordSuccess
        );
    }

    #[test]
    fn improvement_records_share_correlation_id_across_ledgers_and_exposure() {
        let proposal = improvement_proposal(ImprovementProposalStatus::Checkpointed);
        let approval = improvement_approval();
        let checkpoint = improvement_checkpoint();
        let apply_record = improvement_apply_record();
        let verification = improvement_verification(true);
        let mut exposure =
            default_mcp_exposure_projection("resource:skill", "session", "deny", "default deny");
        exposure.proposal_id = Some("proposal-1".to_owned());
        exposure.correlation_id = Some("corr-002".to_owned());

        assert!(self_improvement_records_share_correlation_id(
            &proposal,
            Some(&approval),
            Some(&checkpoint),
            Some(&apply_record),
            Some(&verification),
            Some(&exposure),
        ));

        exposure.correlation_id = Some("other-corr".to_owned());
        assert!(!self_improvement_records_share_correlation_id(
            &proposal,
            Some(&approval),
            Some(&checkpoint),
            Some(&apply_record),
            Some(&verification),
            Some(&exposure),
        ));
    }

    #[test]
    fn improvement_apply_record_uses_owner_spec_and_action_refs_not_direct_mutation_fields() {
        let apply_record = improvement_apply_record();
        let serialized =
            serde_json::to_value(&apply_record).expect("apply record should serialize");

        assert_eq!(apply_record.owner_spec, "017-app-owner");
        assert_eq!(apply_record.action_ref.owner_spec, "017-app-owner");
        assert_eq!(
            apply_record.action_ref.primitive_ref,
            "owner-primitive://app/apply-manifest-update"
        );
        assert!(serialized.get("store_mutation").is_none());
        assert!(serialized.get("tool_mutation").is_none());
        assert!(serialized.get("app_mutation").is_none());
    }

    #[test]
    fn prd011_statuses_targets_and_rollback_record_preserve_lineage() {
        assert_eq!(
            serde_json::to_value(ImprovementProposalStatus::BlockedCheckpointUnavailable)
                .expect("status should serialize"),
            json!("blocked_checkpoint_unavailable")
        );
        assert_eq!(
            ImprovementTargetKind::ToolExposure.as_scope(),
            "tool_exposure"
        );

        let rollback = improvement_rollback_record(Some(OwnerPrimitiveRef {
            owner_spec: "017-app-owner".to_owned(),
            primitive_ref: "owner-primitive://app/rollback-manifest".to_owned(),
        }));
        assert_eq!(rollback.proposal_id, "proposal-1");
        assert_eq!(rollback.checkpoint_ref, "checkpoint-1");
        assert_eq!(rollback.verify_failure_ref.id, "verification-result-1");
        assert_eq!(rollback.result, ImprovementRollbackResult::RolledBack);
        assert_eq!(
            rollback.final_state,
            ImprovementRollbackFinalState::RestoredCheckpoint
        );
        assert_eq!(rollback.correlation_id, "corr-002");
    }

    #[test]
    fn prd011_manual_recovery_rollback_record_carries_blocked_state() {
        let rollback = improvement_rollback_record(None);

        assert_eq!(
            rollback.result,
            ImprovementRollbackResult::BlockedManualRecoveryRequired
        );
        assert_eq!(
            rollback.final_state,
            ImprovementRollbackFinalState::ManualRecoveryRequired
        );
        assert_eq!(
            rollback.manual_recovery_hint.as_deref(),
            Some("restore checkpoint-1 through the target owner manually")
        );
        assert!(rollback.owner_rollback_ref.is_none());
    }

    #[test]
    fn prd007_projection_surfaces_share_status_semantics() {
        let statuses = shared_surface_status_semantics(ProjectionStatus::Blocked);
        let event_classes: Vec<_> = statuses
            .iter()
            .map(|status| {
                channel_event_class_for_status(&status.status).expect("status should map")
            })
            .collect();

        assert!(surfaces_share_status_semantics(&statuses));
        assert_eq!(statuses.len(), 4);
        assert!(statuses
            .iter()
            .all(|status| status.status == ProjectionStatus::Blocked));
        assert!(channel_projection_filters_user_visible_events(
            &event_classes
        ));
    }

    #[test]
    fn prd007_channel_projection_filters_to_user_visible_events_only() {
        let statuses = [
            ProjectionStatus::Success,
            ProjectionStatus::Pending,
            ProjectionStatus::Blocked,
            ProjectionStatus::Failed,
            ProjectionStatus::Stale,
            ProjectionStatus::Denied,
            ProjectionStatus::RedactionFailed,
        ];
        let event_classes: Vec<_> = statuses
            .iter()
            .filter_map(channel_event_class_for_status)
            .collect();

        assert_eq!(event_classes.len(), statuses.len());
        assert!(channel_projection_filters_user_visible_events(
            &event_classes
        ));
        assert!(event_classes.contains(&ChannelProjectionEventClass::Notify));
        assert!(event_classes.contains(&ChannelProjectionEventClass::Escalate));
        assert!(event_classes.contains(&ChannelProjectionEventClass::Blocked));
        assert!(event_classes.contains(&ChannelProjectionEventClass::ApprovalRequired));
        assert!(event_classes.contains(&ChannelProjectionEventClass::VerificationFailed));
    }

    #[test]
    fn prd007_ledger_inspect_matches_correlation_and_subject_ids() {
        let task = TaskLedgerRecord {
            record_id: "task-record-1".to_owned(),
            task_id: "goal-1".to_owned(),
            correlation_id: "corr-007".to_owned(),
            source: EvaluationTriggerSource::ScheduledJob,
            status: TaskLedgerStatus::Completed,
            job_id: Some("job-1".to_owned()),
            result_ref: Some(evidence_ref()),
            outcome_request_id: None,
            delivery_ref: None,
            retry_or_rollback_ref: None,
            created_at_ms: 20,
        };
        let evaluation = EvaluationLedgerRecord {
            record_id: "evaluation-record-1".to_owned(),
            evaluator_kind: EvaluatorKind::GoalCompletion,
            request: evaluator_request("corr-007", Some("session-1")),
            snapshot_id: "snapshot-1".to_owned(),
            snapshot_digest: "snapshot-digest-1".to_owned(),
            verdict: evaluator_verdict(VerdictKind::Pass),
            authority_boundary: EvaluatorAuthority::AdvisoryOnly,
            created_at_ms: 30,
        };
        let proposal = ImprovementProposal {
            correlation_id: "corr-007".to_owned(),
            ..improvement_proposal(ImprovementProposalStatus::ApprovalPending)
        };
        let replay = ReplayRecord {
            replay_id: "replay-1".to_owned(),
            trajectory_id: "trajectory-1".to_owned(),
            mode: ReplayMode::Deterministic,
            safe_effects_policy: ReplaySafeEffectsPolicy::RecordedOutcomeOnly,
            started_at_ms: 40,
            result: replay_result(
                ReplayResultClassification::Pass,
                Some(VerdictKind::Pass),
                Some(TaskOutcomeClass::Notify),
            ),
            correlation_id: "corr-007".to_owned(),
        };
        let entries = [
            LedgerInspectEntry::Task(task.clone()),
            LedgerInspectEntry::Evaluation(evaluation.projection()),
            LedgerInspectEntry::Improvement(ledger_subject_for_improvement(&proposal, 35)),
            LedgerInspectEntry::Replay(ledger_subject_for_replay(&replay)),
        ];

        let correlation_query = LedgerInspectQuery {
            correlation_id: Some("corr-007".to_owned()),
            from_ms: Some(10),
            to_ms: Some(50),
            ..LedgerInspectQuery::default()
        };
        assert!(entries
            .iter()
            .all(|entry| ledger_inspect_entry_matches(&correlation_query, entry)));

        assert!(ledger_inspect_entry_matches(
            &LedgerInspectQuery {
                goal_id: Some("goal-1".to_owned()),
                job_id: Some("job-1".to_owned()),
                ..LedgerInspectQuery::default()
            },
            &entries[0]
        ));
        assert!(ledger_inspect_matches_subject(
            &LedgerInspectQuery {
                session_id: Some("session-1".to_owned()),
                ..LedgerInspectQuery::default()
            },
            &ledger_subject_for_record(&EvaluatorLedgerRecord::Evaluation(evaluation))
        ));
        assert!(ledger_inspect_entry_matches(
            &LedgerInspectQuery {
                proposal_id: Some("proposal-1".to_owned()),
                ..LedgerInspectQuery::default()
            },
            &entries[2]
        ));
        assert!(ledger_inspect_entry_matches(
            &LedgerInspectQuery {
                trajectory_id: Some("trajectory-1".to_owned()),
                ..LedgerInspectQuery::default()
            },
            &entries[3]
        ));
    }

    #[test]
    fn prd007_diagnostics_bundle_refs_are_redacted_and_mark_failures() {
        let denied = denied_outcome(
            "approval_bypass",
            "secret token sk-hidden must not leak",
            Some(evidence_ref()),
            DeniedRetryClass::RetryAfterUserAction,
            "ask local user",
            "corr-007",
        );
        let checkpoint = improvement_checkpoint();
        let delivery = delivery_record(
            background_result_ref(BackgroundExitStatus::Failed),
            DeliveryStatus::Failed,
        );
        let replay = replay_result(
            ReplayResultClassification::Regression,
            Some(VerdictKind::Fail),
            Some(TaskOutcomeClass::Escalate),
        );
        let bundle = diagnostics_bundle_evidence_refs(DiagnosticsEvidenceBundleInput {
            frozen_snapshot_digest: Some("snapshot-digest-1".to_owned()),
            evaluator_verdict_summary: Some("verdict summary without payload".to_owned()),
            denied_outcome: Some(&denied),
            checkpoint_ref: Some(&checkpoint),
            delivery_failure: Some(&delivery),
            replay_result: Some(&replay),
            redaction_profile: Some("default".to_owned()),
            redaction_failure: true,
        });
        let serialized = serde_json::to_string(&bundle).expect("bundle refs should serialize");

        assert!(diagnostics_evidence_export_is_redacted(&bundle));
        assert_eq!(
            bundle
                .redaction_failure_status
                .as_ref()
                .map(|evidence_ref| &evidence_ref.redaction_status),
            Some(&RedactionStatus::RedactionFailed)
        );
        assert!(!serialized.contains("sk-hidden"));
        assert!(!serialized.contains("raw_payload"));
    }

    #[test]
    fn prd007_release_gate_blocks_missing_coverage_and_blocker_families() {
        let required_prds = ["000", "001", "002", "003", "004", "005", "006", "007"];
        let full_coverage: Vec<_> = required_prds
            .iter()
            .map(|prd_id| Spec018CoverageEntry {
                prd_id: (*prd_id).to_owned(),
                requirement_id: format!("prd-{prd_id}-contract"),
                test_evidence: vec![evidence_ref_with_id(
                    &format!("test-{prd_id}"),
                    "test-digest",
                )],
                diagnostics_evidence: vec![evidence_ref_with_id(
                    &format!("diagnostics-{prd_id}"),
                    "diagnostics-digest",
                )],
                release_gate_status: ReleaseGateStatus::Pass,
            })
            .collect();

        assert!(!release_gate_blocks_missing_coverage_or_blockers(
            &full_coverage,
            &required_prds,
            &[]
        ));
        assert!(release_gate_blocks_missing_coverage_or_blockers(
            &full_coverage[..full_coverage.len() - 1],
            &required_prds,
            &[]
        ));

        for blocker in [
            ReleaseBlockerFamily::RedactionFailure,
            ReleaseBlockerFamily::ApprovalBypass,
            ReleaseBlockerFamily::StaleVerdictApply,
            ReleaseBlockerFamily::DestructiveReplayEffect,
            ReleaseBlockerFamily::SilentSelfModification,
            ReleaseBlockerFamily::UnboundedContinuationLoop,
        ] {
            assert!(release_gate_blocks_missing_coverage_or_blockers(
                &full_coverage,
                &required_prds,
                &[blocker]
            ));
        }
    }

    #[test]
    fn prd014_manifest_requires_all_categories_redaction_and_owner_without_raw_secret() {
        let skipped = Spec018SkippedEvidence {
            source_ref: evidence_ref_with_id("skipped-stale", "digest"),
            classification: Spec018SkippedEvidenceClassification::Stale,
            redacted_summary: "stale verdict skipped".to_owned(),
        };
        let manifest = Spec018DiagnosticsEvidenceManifest {
            manifest_id: "manifest-018".to_owned(),
            generated_at_ms: 42,
            evaluator_refs: vec![evidence_ref_with_id("evaluator", "digest")],
            ledger_refs: vec![evidence_ref_with_id("ledger", "digest")],
            automation_refs: vec![evidence_ref_with_id("automation", "digest")],
            memory_refs: vec![evidence_ref_with_id("memory", "digest")],
            improvement_refs: vec![evidence_ref_with_id("improvement", "digest")],
            replay_refs: vec![evidence_ref_with_id("replay", "digest")],
            projection_refs: vec![evidence_ref_with_id("projection", "digest")],
            skipped_evidence: vec![skipped],
            diagnostics_artifact_refs: vec![evidence_ref_with_id("diagnostics", "digest")],
            redaction_summary: Spec018DiagnosticsRedactionSummary {
                redaction_profile: "default".to_owned(),
                redacted_ref_count: 9,
                already_safe_ref_count: 0,
                failed_ref_count: 0,
                skipped_ref_count: 1,
            },
        };
        let serialized = serde_json::to_string(&manifest).expect("manifest should serialize");

        assert!(spec018_manifest_includes_all_evidence_categories(&manifest));
        assert!(spec018_manifest_redaction_is_valid(&manifest));
        assert!(!serialized.contains("sk-hidden"));
        assert!(!serialized.contains("raw private file content"));

        let mut missing_owner = evidence_ref_with_id("missing-owner", "digest");
        missing_owner.owner_spec = None;
        assert!(!spec018_evidence_ref_has_owner_and_redaction(
            &missing_owner
        ));

        let mut failed_redaction = evidence_ref_with_id("failed-redaction", "digest");
        failed_redaction.redaction_status = RedactionStatus::RedactionFailed;
        assert!(!spec018_evidence_ref_has_owner_and_redaction(
            &failed_redaction
        ));
    }

    #[test]
    fn prd014_ledger_inspect_links_verdict_to_runtime_projection_and_diagnostics_refs() {
        let query = Spec018LedgerInspectQuery {
            query_kind: Spec018LedgerInspectQueryKind::VerdictId,
            target_ref: "verdict-1".to_owned(),
            include_skipped: true,
            include_diagnostics_refs: true,
            redaction_profile: "default".to_owned(),
        };
        let result = Spec018LedgerInspectResult {
            query,
            source_refs: vec![evidence_ref_with_id("verdict-1", "digest")],
            consumption_record_refs: vec![evidence_ref_with_id("consumption-1", "digest")],
            runtime_decision_refs: vec![evidence_ref_with_id("runtime-decision-1", "digest")],
            projection_item_refs: vec![evidence_ref_with_id("projection-item-1", "digest")],
            diagnostics_artifact_refs: vec![evidence_ref_with_id("diagnostics-1", "digest")],
            skipped_evidence: vec![Spec018SkippedEvidence {
                source_ref: evidence_ref_with_id("expired-verdict", "digest"),
                classification: Spec018SkippedEvidenceClassification::Expired,
                redacted_summary: "expired verdict skipped".to_owned(),
            }],
        };

        assert!(spec018_ledger_inspect_links_runtime_projection_and_diagnostics(&result));
    }

    #[test]
    fn prd014_release_coverage_accepts_any_verification_refs_with_diagnostics() {
        let mut test_only =
            spec018_release_entry(Spec018ClosureCoverageBucket::ReplayRunner, "replay-runner");
        test_only.replay_refs.clear();
        test_only.manual_refs.clear();
        assert!(spec018_release_coverage_entry_passes(&test_only));

        let mut no_verification = test_only.clone();
        no_verification.test_refs.clear();
        assert!(!spec018_release_coverage_entry_passes(&no_verification));

        let skipped = [
            Spec018SkippedEvidenceClassification::Stale,
            Spec018SkippedEvidenceClassification::Expired,
            Spec018SkippedEvidenceClassification::Superseded,
        ];
        for classification in skipped {
            assert!(spec018_skipped_evidence_is_non_blocking(
                &Spec018SkippedEvidence {
                    source_ref: evidence_ref_with_id("skipped", "digest"),
                    classification,
                    redacted_summary: "redacted skipped evidence".to_owned(),
                }
            ));
        }
    }

    #[test]
    fn prd014_release_blockers_cover_unverified_improvement_and_failed_replay() {
        let blockers = vec![
            spec018_blocker(
                Spec018ReleaseBlockerCategory::UnverifiedAppliedImprovement,
                "unverified-improvement",
            ),
            spec018_blocker(
                Spec018ReleaseBlockerCategory::FailedReplayRegression,
                "failed-replay",
            ),
        ];

        assert!(spec018_release_gate_blocks_for(
            &blockers,
            Spec018ReleaseBlockerCategory::UnverifiedAppliedImprovement
        ));
        assert!(spec018_release_gate_blocks_for(
            &blockers,
            Spec018ReleaseBlockerCategory::FailedReplayRegression
        ));
    }

    #[test]
    fn prd014_final_closure_requires_all_integration_buckets() {
        let entries: Vec<_> = spec018_required_closure_buckets()
            .into_iter()
            .enumerate()
            .map(|(index, bucket)| spec018_release_entry(bucket, &format!("bucket-{index}")))
            .collect();

        assert!(spec018_final_closure_passes(&entries, &[]));
        assert!(spec018_release_gate_outcome(entries.clone(), Vec::new()).final_closure_passed);
        assert!(!spec018_final_closure_passes(
            &entries[..entries.len() - 1],
            &[]
        ));
        assert!(!spec018_final_closure_passes(
            &entries,
            &[spec018_blocker(
                Spec018ReleaseBlockerCategory::MissingLedgerConsumptionEvidence,
                "missing-ledger"
            )]
        ));
    }

    #[test]
    fn prd007_stale_denied_blocked_and_failed_never_project_as_success() {
        let verdict_statuses = [
            projection_status_from_verdict(&evaluator_verdict(VerdictKind::Stale)),
            projection_status_from_verdict(&evaluator_verdict(VerdictKind::Denied)),
            projection_status_from_verdict(&evaluator_verdict(VerdictKind::Fail)),
        ];
        let blocked_outcome =
            projection_status_from_task_outcome(&task_outcome_verdict(TaskOutcomeClass::Escalate));
        let failed_run = projection_status_from_run(&automation_run(AutomationRunStatus::Failed));
        let failed_delivery = projection_status_from_delivery(&delivery_record(
            background_result_ref(BackgroundExitStatus::Failed),
            DeliveryStatus::Failed,
        ));
        let projection = evaluation_projection(
            vec![
                evaluator_verdict(VerdictKind::Pass),
                evaluator_verdict(VerdictKind::Stale),
            ],
            vec![denied_outcome(
                "denied",
                "denied outcome",
                Some(evidence_ref()),
                DeniedRetryClass::RetryAfterUserAction,
                "ask local user",
                "corr-007",
            )],
            vec![evidence_ref()],
            RedactionStatus::RedactionFailed,
        );

        for status in
            verdict_statuses
                .into_iter()
                .chain([blocked_outcome, failed_run, failed_delivery])
        {
            assert!(!projection_status_is_success(&status));
        }
        assert_eq!(projection.stale_verdicts.len(), 1);
        assert_eq!(projection.denied_outcomes.len(), 1);
        assert_eq!(
            projection.redaction_status,
            RedactionStatus::RedactionFailed
        );
        assert!(projection.redaction_failure_marker.is_some());
    }

    #[test]
    fn prd007_projection_contract_stays_self_hosted_personal_use() {
        let assumptions = default_projection_assumptions();
        let serialized = serde_json::to_string(&assumptions).expect("assumptions should serialize");

        assert!(projection_assumptions_are_self_hosted_personal_use(
            &assumptions
        ));
        assert_eq!(assumptions.runtime_scope, "self_hosted_personal_use");
        assert_eq!(assumptions.primary_actor, "local_user");
        assert!(!serialized.contains("admin_approval_queue"));
        assert!(!serialized.contains("organization_id"));
        assert!(!serialized.contains("fleet_id"));
    }

    #[test]
    fn prd013_statuses_are_shared_across_surfaces() {
        let statuses =
            spec018_shared_surface_status_semantics(Spec018ProjectionStatusKind::ApprovalRequired);

        assert!(spec018_surfaces_share_status_semantics(&statuses));
        assert_eq!(statuses.len(), 4);
    }

    #[test]
    fn prd013_acknowledgement_does_not_equal_approval_or_rejection() {
        let delivery = Spec018AutomationDeliveryStatus {
            delivery_id: "delivery-013".to_owned(),
            run_id: "run-013".to_owned(),
            target_surface: ProjectionSurface::Channel,
            severity: DeliverySeverity::Warning,
            suppress_reason: None,
            acknowledged: true,
            status: spec018_status(Spec018ProjectionStatusKind::WaitingForUser),
            evidence_refs: vec![evidence_ref()],
        };
        let approval =
            spec018_approval_item(vec!["tool_exposure".to_owned()], "restore checkpoint");

        assert!(!spec018_acknowledgement_is_user_decision(
            &delivery, &approval
        ));
    }

    #[test]
    fn prd013_blocked_status_requires_reason_hint_evidence_and_retry() {
        let valid = Spec018ProjectionStatus {
            kind: Spec018ProjectionStatusKind::Blocked,
            severity: Some(Spec018ReleaseBlockerSeverity::Blocking),
            blocked_reason_class: Some(Spec018BlockedReasonClass::CapabilityDenied),
            user_action_hint: Some("approve local capability or change goal".to_owned()),
            evidence_refs: vec![evidence_ref()],
            retry_eligibility: Some(Spec018RetryEligibility::RetryAfterUserAction),
        };
        let missing_hint = Spec018ProjectionStatus {
            user_action_hint: None,
            ..valid.clone()
        };

        assert!(spec018_blocked_status_is_valid(&valid));
        assert!(!spec018_blocked_status_is_valid(&missing_hint));
    }

    #[test]
    fn prd013_approval_item_requires_scope_and_rollback_summary_for_actions() {
        let valid = spec018_approval_item(vec!["tool_exposure".to_owned()], "restore checkpoint");
        let missing_scope = spec018_approval_item(Vec::new(), "restore checkpoint");
        let missing_rollback = spec018_approval_item(vec!["tool_exposure".to_owned()], " ");

        assert!(spec018_approval_item_can_be_actionable(&valid));
        assert!(!spec018_approval_item_can_be_actionable(&missing_scope));
        assert!(!spec018_approval_item_can_be_actionable(&missing_rollback));
        assert!(valid.allowed_decisions.iter().any(|decision| {
            decision.decision == Spec018ApprovalDecisionKind::InspectEvidence
                && decision.unavailable_reason.is_none()
        }));
    }

    #[test]
    fn prd013_channel_projection_filters_user_visible_statuses() {
        let visible = [
            Spec018ProjectionStatusKind::WaitingForUser,
            Spec018ProjectionStatusKind::ApprovalRequired,
            Spec018ProjectionStatusKind::Blocked,
            Spec018ProjectionStatusKind::VerificationFailed,
            Spec018ProjectionStatusKind::RollbackAvailable,
            Spec018ProjectionStatusKind::Completed,
        ];
        let hidden = [
            Spec018ProjectionStatusKind::Idle,
            Spec018ProjectionStatusKind::Running,
            Spec018ProjectionStatusKind::VerificationPending,
            Spec018ProjectionStatusKind::RolledBack,
            Spec018ProjectionStatusKind::Suppressed,
        ];

        assert!(visible.iter().all(
            |kind| spec018_channel_event_kind_for_status(&spec018_status(kind.clone())).is_some()
        ));
        assert!(hidden.iter().all(
            |kind| spec018_channel_event_kind_for_status(&spec018_status(kind.clone())).is_none()
        ));
    }

    #[test]
    fn prd013_projection_serializes_schema_label_and_redacted_evidence_only() {
        let projection = Spec018Projection {
            schema_label: SPEC018_PROJECTION_SCHEMA_LABEL.to_owned(),
            schema_version: SPEC018_PROJECTION_SCHEMA_VERSION.to_owned(),
            generated_at_ms: 130,
            session_id: "session-013".to_owned(),
            goal_summaries: Vec::new(),
            automation_summaries: Vec::new(),
            approval_summaries: vec![spec018_approval_item(
                vec!["tool_exposure".to_owned()],
                "restore checkpoint",
            )],
            blocked_summaries: Vec::new(),
            verification_summaries: Vec::new(),
            replay_summaries: Vec::new(),
            recent_evaluator_decision_summaries: Vec::new(),
            evidence_refs: vec![evidence_ref()],
        };
        let serialized = serde_json::to_string(&projection).expect("projection serializes");

        assert!(serialized.contains("018Projection"));
        assert!(!serialized.contains("sk-secret"));
        assert!(!serialized.contains("unredacted private payload"));
        assert!(spec018_evidence_refs_are_redacted(
            &projection.evidence_refs
        ));
    }

    fn spec018_status(kind: Spec018ProjectionStatusKind) -> Spec018ProjectionStatus {
        Spec018ProjectionStatus {
            kind,
            severity: None,
            blocked_reason_class: None,
            user_action_hint: None,
            evidence_refs: Vec::new(),
            retry_eligibility: None,
        }
    }

    fn spec018_approval_item(
        requested_scope: Vec<String>,
        rollback_summary: &str,
    ) -> Spec018ApprovalProjectionItem {
        Spec018ApprovalProjectionItem {
            proposal_id: "proposal-013".to_owned(),
            target_kind: "tool_exposure".to_owned(),
            requested_scope,
            risk_summary: "changes local tool exposure without raw payload".to_owned(),
            rollback_summary: rollback_summary.to_owned(),
            allowed_decisions: spec018_allowed_decisions(None, None, None, None),
            status: spec018_status(Spec018ProjectionStatusKind::ApprovalRequired),
            evidence_refs: vec![evidence_ref()],
        }
    }

    fn improvement_proposal(status: ImprovementProposalStatus) -> ImprovementProposal {
        ImprovementProposal {
            proposal_id: "proposal-1".to_owned(),
            target_kind: "app_manifest".to_owned(),
            target_ref: Some("app://local/test".to_owned()),
            target_source: Some("app_task".to_owned()),
            proposed_diff_summary_ref: EvidenceRef {
                kind: EvidenceKind::ImprovementProposal,
                ..evidence_ref_with_id("diff-summary-1", "diff-digest-1")
            },
            risk_summary: "updates app manifest through owner primitive".to_owned(),
            evidence_refs: vec![evidence_ref()],
            expected_benefit: "improves task routing".to_owned(),
            rollback_plan: "restore checkpoint through owner rollback primitive".to_owned(),
            status,
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn improvement_approval() -> ImprovementApproval {
        let mut request = approval_request();
        request.status = ApprovalRequestStatus::Approved;
        ImprovementApproval {
            proposal_id: "proposal-1".to_owned(),
            request_ref: request,
            decision_ref: approval_decision(),
            approved_scope: vec!["app://local/test".to_owned()],
            expires_at_ms: 90,
            actor_local_user: "local-user".to_owned(),
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn improvement_checkpoint() -> ImprovementCheckpoint {
        ImprovementCheckpoint {
            checkpoint_ref: "checkpoint-1".to_owned(),
            target_digest_before: "target-digest-before".to_owned(),
            inspect_ref: EvidenceRef {
                kind: EvidenceKind::ImprovementCheckpoint,
                ..evidence_ref_with_id("checkpoint-inspect-1", "checkpoint-digest-1")
            },
            rollback_capability: Some(OwnerPrimitiveRef {
                owner_spec: "017-app-owner".to_owned(),
                primitive_ref: "owner-primitive://app/rollback-manifest".to_owned(),
            }),
            proposal_id: "proposal-1".to_owned(),
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn improvement_checkpoint_gate() -> CheckpointGateDecision {
        CheckpointGateDecision {
            status: CheckpointGateStatus::Required,
            required: true,
            reason: "inspectable checkpoint is available".to_owned(),
            checkpoint_ref: Some("checkpoint-1".to_owned()),
        }
    }

    fn improvement_apply_record() -> ImprovementApplyRecord {
        ImprovementApplyRecord {
            apply_id: "apply-1".to_owned(),
            proposal_id: "proposal-1".to_owned(),
            owner_spec: "017-app-owner".to_owned(),
            action_ref: OwnerPrimitiveRef {
                owner_spec: "017-app-owner".to_owned(),
                primitive_ref: "owner-primitive://app/apply-manifest-update".to_owned(),
            },
            input_digest: "apply-input-digest-1".to_owned(),
            outcome_ref: EvidenceRef {
                kind: EvidenceKind::ImprovementApplyRecord,
                ..evidence_ref_with_id("apply-outcome-1", "apply-digest-1")
            },
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn improvement_verification(passed: bool) -> ImprovementVerification {
        ImprovementVerification {
            verification_id: "verification-1".to_owned(),
            expected_behavior: "app manifest routes task to updated handler".to_owned(),
            observed_result_ref: EvidenceRef {
                kind: EvidenceKind::ImprovementVerification,
                ..evidence_ref_with_id("verification-result-1", "verification-digest-1")
            },
            passed,
            next_action: if passed {
                ImprovementVerificationNextAction::RecordSuccess
            } else {
                ImprovementVerificationNextAction::ReportFailed
            },
            proposal_id: "proposal-1".to_owned(),
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn improvement_rollback_record(
        owner_rollback_ref: Option<OwnerPrimitiveRef>,
    ) -> ImprovementRollbackRecord {
        let result = if owner_rollback_ref.is_some() {
            ImprovementRollbackResult::RolledBack
        } else {
            ImprovementRollbackResult::BlockedManualRecoveryRequired
        };
        let final_state = if owner_rollback_ref.is_some() {
            ImprovementRollbackFinalState::RestoredCheckpoint
        } else {
            ImprovementRollbackFinalState::ManualRecoveryRequired
        };
        ImprovementRollbackRecord {
            rollback_id: "rollback-1".to_owned(),
            proposal_id: "proposal-1".to_owned(),
            checkpoint_ref: "checkpoint-1".to_owned(),
            verify_failure_ref: EvidenceRef {
                kind: EvidenceKind::ImprovementVerification,
                ..evidence_ref_with_id("verification-result-1", "verification-digest-1")
            },
            owner_rollback_ref,
            result,
            manual_recovery_hint: Some(
                "restore checkpoint-1 through the target owner manually".to_owned(),
            ),
            final_state,
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn capability_input(permission_mode: PermissionMode) -> CapabilityEvaluationInput {
        CapabilityEvaluationInput {
            action_id: "action-1".to_owned(),
            action_kind: CapabilityActionKind::DestructiveWrite,
            action_digest: "action-digest-1".to_owned(),
            target_digest: "target-digest-1".to_owned(),
            requested_capabilities: vec!["fs_write".to_owned()],
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: permission_mode,
                snapshot_id: "permission-snapshot-1".to_owned(),
                snapshot_digest: "permission-snapshot-digest-1".to_owned(),
                denied_capabilities: Vec::new(),
            },
            approval_context: ApprovalContext {
                correlation_id: "corr-002".to_owned(),
                request_ref: Some(approval_request()),
                decision_ref: None,
                redacted_evidence_ref: Some(evidence_ref()),
            },
            checkpoint_hint: CheckpointHint {
                required: true,
                checkpoint_ref: Some("checkpoint-1".to_owned()),
                inspectable: true,
            },
            correlation_id: "corr-002".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            snapshot_digest: "snapshot-digest-1".to_owned(),
        }
    }

    fn capability_verdict(hint: CapabilityDecisionHint) -> CapabilityVerdict {
        CapabilityVerdict {
            hint,
            reason: "evaluator says action is a candidate only".to_owned(),
            risk_level: "high".to_owned(),
            evidence_refs: vec![evidence_ref()],
            expires_at_ms: Some(90),
            checkpoint_recommendation: Some("create inspectable checkpoint".to_owned()),
        }
    }

    fn approval_request() -> ApprovalRequestRef {
        ApprovalRequestRef {
            request_id: "approval-request-1".to_owned(),
            action_digest: "action-digest-1".to_owned(),
            snapshot_digest: "snapshot-digest-1".to_owned(),
            created_at_ms: 10,
            expires_at_ms: 90,
            displayed_risk_summary: "destructive write to workspace file".to_owned(),
            status: ApprovalRequestStatus::Pending,
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn approval_decision() -> ApprovalDecisionRef {
        ApprovalDecisionRef {
            decision_id: "approval-decision-1".to_owned(),
            request_id: "approval-request-1".to_owned(),
            action_digest: "action-digest-1".to_owned(),
            snapshot_digest: "snapshot-digest-1".to_owned(),
            decision: ApprovalDecisionKind::Approved,
            decided_at_ms: 20,
            actor_local_user: "local-user".to_owned(),
            correlation_id: "corr-002".to_owned(),
        }
    }

    fn spec018_release_entry(
        bucket: Spec018ClosureCoverageBucket,
        id: &str,
    ) -> Spec018ReleaseCoverageEntry {
        Spec018ReleaseCoverageEntry {
            entry_id: id.to_owned(),
            capability_area: bucket,
            required_evidence: vec![evidence_ref_with_id(&format!("required-{id}"), "digest")],
            test_refs: vec![evidence_ref_with_id(&format!("test-{id}"), "digest")],
            replay_refs: vec![evidence_ref_with_id(&format!("replay-{id}"), "digest")],
            manual_refs: vec![evidence_ref_with_id(&format!("manual-{id}"), "digest")],
            diagnostics_artifact_refs: vec![evidence_ref_with_id(
                &format!("diagnostics-{id}"),
                "digest",
            )],
            status: Spec018ReleaseCoverageStatus::Pass,
            blocker_refs: Vec::new(),
        }
    }

    fn spec018_blocker(category: Spec018ReleaseBlockerCategory, id: &str) -> Spec018ReleaseBlocker {
        Spec018ReleaseBlocker {
            blocker_id: id.to_owned(),
            category,
            source_ref: evidence_ref_with_id(&format!("blocker-{id}"), "digest"),
            severity: Spec018ReleaseBlockerSeverity::Blocking,
            redacted_summary: "redacted release blocker".to_owned(),
            resolution_hint: "inspect the local redacted ref".to_owned(),
        }
    }

    fn evidence_ref() -> EvidenceRef {
        evidence_ref_with_id("evidence-1", "evidence-digest-1")
    }

    fn evidence_ref_with_id(id: &str, digest: &str) -> EvidenceRef {
        EvidenceRef {
            kind: EvidenceKind::DiagnosticRecord,
            id: id.to_owned(),
            digest: digest.to_owned(),
            summary: "redacted approval evidence".to_owned(),
            redaction_status: RedactionStatus::Redacted,
            owner_spec: Some("018".to_owned()),
            locator: Some("diagnostics://approval/evidence-1".to_owned()),
            retention_hint: Some("short".to_owned()),
        }
    }

    fn replay_evidence_ref(kind: EvidenceKind, id: &str) -> EvidenceRef {
        EvidenceRef {
            kind,
            id: id.to_owned(),
            digest: format!("digest-{id}"),
            summary: "redacted replay evidence".to_owned(),
            redaction_status: RedactionStatus::Redacted,
            owner_spec: Some("018".to_owned()),
            locator: Some(format!("replay://{id}")),
            retention_hint: Some("local".to_owned()),
        }
    }

    fn trajectory_record() -> TrajectoryRecord {
        TrajectoryRecord {
            trajectory_id: "trajectory-1".to_owned(),
            session_ref: replay_evidence_ref(EvidenceKind::SessionEvent, "session-1"),
            event_refs: vec![replay_evidence_ref(EvidenceKind::SessionEvent, "event-1")],
            model_call_refs: vec![replay_evidence_ref(
                EvidenceKind::ProviderSnapshot,
                "model-call-1",
            )],
            tool_refs: vec![replay_evidence_ref(EvidenceKind::ToolPayload, "tool-1")],
            evaluator_refs: vec![replay_evidence_ref(
                EvidenceKind::EvaluatorSummary,
                "evaluator-1",
            )],
            provider_snapshot_refs: vec![replay_evidence_ref(
                EvidenceKind::ProviderModelSnapshot,
                "provider-snapshot-1",
            )],
            redaction_profile: "default".to_owned(),
            stats: TrajectoryStats {
                started_at_ms: 10,
                completed_at_ms: Some(20),
                model_call_count: 1,
                tool_call_count: 1,
                input_tokens: Some(100),
                output_tokens: Some(50),
            },
            correlation_id: "corr-006".to_owned(),
        }
    }

    fn provider_snapshot(
        provider_id: &str,
        model_id: &str,
        role: ProviderRouteRole,
        evaluator_role: Option<AuxiliaryJudgeRole>,
        fallback_chain: Vec<ProviderFallbackStep>,
    ) -> ProviderModelSnapshot {
        ProviderModelSnapshot {
            snapshot_id: format!("snapshot-{provider_id}-{model_id}"),
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
            profile_ref: "profile://default".to_owned(),
            role,
            routing_reason: "redacted routing reason".to_owned(),
            fallback_chain,
            evaluator_role,
        }
    }

    fn judge_routing_decision(
        fallback_reason: Option<JudgeFallbackReason>,
    ) -> JudgeRoutingDecision {
        JudgeRoutingDecision {
            decision_id: "judge-route-1".to_owned(),
            evaluator_kind: EvaluatorKind::Replay,
            judge_role: AuxiliaryJudgeRole::ReplayJudge,
            preferred_provider_id: "provider-judge-default".to_owned(),
            preferred_model_id: "model-judge-default".to_owned(),
            selected_provider_id: "provider-judge-fallback".to_owned(),
            selected_model_id: "model-judge-fallback".to_owned(),
            fallback_reason,
            denied_reason: Some(JudgeDeniedReason::PolicyDenied),
            provider_snapshot_ref: replay_evidence_ref(
                EvidenceKind::ProviderModelSnapshot,
                "snapshot-provider-judge-fallback-model-judge-fallback",
            ),
            correlation_id: "corr-006".to_owned(),
        }
    }

    fn replay_result(
        classification: ReplayResultClassification,
        actual_verdict: Option<VerdictKind>,
        actual_outcome: Option<TaskOutcomeClass>,
    ) -> ReplayResult {
        ReplayResult {
            classification,
            actual_verdict,
            actual_outcome,
            diff_summary_ref: replay_evidence_ref(EvidenceKind::ReplayResult, "diff-1"),
        }
    }

    fn quality_regression_case() -> QualityRegressionCase {
        QualityRegressionCase {
            case_id: "case-1".to_owned(),
            trajectory_ref: replay_evidence_ref(EvidenceKind::TrajectoryRecord, "trajectory-1"),
            expected_verdict: VerdictKind::Pass,
            expected_outcome: TaskOutcomeClass::Verify,
            redacted_evidence_refs: vec![replay_evidence_ref(
                EvidenceKind::ReplayRecord,
                "replay-1",
            )],
            owner_note: "redacted local regression note".to_owned(),
            correlation_id: "corr-006".to_owned(),
        }
    }

    fn replay_dataset_item() -> ReplayDatasetItem {
        ReplayDatasetItem {
            dataset_id: "dataset-1".to_owned(),
            case_id: "case-1".to_owned(),
            trajectory_refs: vec![replay_evidence_ref(
                EvidenceKind::TrajectoryRecord,
                "trajectory-1",
            )],
            expected_verdict: VerdictKind::Pass,
            expected_outcome: TaskOutcomeClass::Verify,
            expected_projection_status: ProjectionStatus::Success,
            expected_confidence_band: ConfidenceBand::High,
            allowed_judge_roles: vec![AuxiliaryJudgeRole::ReplayJudge],
            redaction_profile: "default".to_owned(),
            tool_outcome_policies: Vec::new(),
            actual_verdict: Some(VerdictKind::Pass),
            actual_outcome: Some(TaskOutcomeClass::Verify),
            actual_projection_status: Some(ProjectionStatus::Success),
            actual_confidence_band: Some(ConfidenceBand::High),
            auxiliary_judge_routes: Vec::new(),
            diagnostics_refs: vec![replay_evidence_ref(
                EvidenceKind::DiagnosticRecord,
                "diagnostic-1",
            )],
            coverage_refs: vec![replay_evidence_ref(
                EvidenceKind::ReplayRecord,
                "coverage-1",
            )],
        }
    }

    fn evaluator_request(
        correlation_id: &str,
        session_id: Option<&str>,
    ) -> EvaluatorRequestEnvelope {
        EvaluatorRequestEnvelope {
            request_id: "request-1".to_owned(),
            evaluator_kind: EvaluatorKind::GoalCompletion,
            correlation_id: correlation_id.to_owned(),
            session_id: session_id.map(str::to_owned),
            turn_id: None,
            source: EvaluationTriggerSource::SessionTurn,
            snapshot_digest: "snapshot-digest-1".to_owned(),
            redaction_profile: "default".to_owned(),
            caller_intent: "inspect local evaluation status".to_owned(),
        }
    }

    fn evaluator_verdict(verdict_kind: VerdictKind) -> EvaluatorVerdictEnvelope {
        EvaluatorVerdictEnvelope {
            verdict_kind,
            reason: "redacted evaluator reason".to_owned(),
            confidence: 0.8,
            evidence_refs: vec![evidence_ref()],
            suggested_next_action: SuggestedNextAction::None,
            expires_at_ms: Some(90),
            redaction_status: RedactionStatus::Redacted,
            evaluator_version: "test-evaluator".to_owned(),
        }
    }

    fn automation_run(status: AutomationRunStatus) -> AutomationRun {
        AutomationRun {
            run_id: "run-1".to_owned(),
            job_id: "job-1".to_owned(),
            trigger: AutomationTriggerKind::ScheduledWake,
            status,
            started_at_ms: 10,
            timeout_at_ms: Some(60),
            completed_at_ms: Some(20),
            result_ref: Some(background_result_ref(BackgroundExitStatus::Failed)),
            outcome_verdict_id: Some("verdict-1".to_owned()),
            correlation_id: "corr-003".to_owned(),
        }
    }

    fn automation_job(schedule: AutomationSchedule) -> AutomationJob {
        AutomationJob {
            job_id: "job-1".to_owned(),
            source: AutomationJobSource::User,
            schedule,
            execution_modes: vec![AutomationExecutionMode::SkillBackedAgent],
            capability_requirements: vec!["fs_read".to_owned()],
            owner_ref: "local-user".to_owned(),
            status: AutomationJobStatus::Active,
            correlation_id: "corr-003".to_owned(),
        }
    }

    fn background_result_ref(exit_status: BackgroundExitStatus) -> BackgroundResultRef {
        normalize_background_result_ref(
            BackgroundSourceKind::Cron,
            "cron-1",
            "digest-1",
            exit_status,
            BackgroundResultTiming {
                started_at_ms: 10,
                completed_at_ms: Some(20),
            },
            Some("timeout".to_owned()),
            "corr-003",
        )
    }

    fn delivery_record(result_ref: BackgroundResultRef, status: DeliveryStatus) -> DeliveryRecord {
        DeliveryRecord {
            delivery_id: "delivery-1".to_owned(),
            run_id: "run-1".to_owned(),
            job_id: "job-1".to_owned(),
            result_ref,
            outcome_verdict_id: "verdict-1".to_owned(),
            correlation_id: "corr-003".to_owned(),
            destination: "local://cli".to_owned(),
            rendered_summary_ref: "redacted://summary-1".to_owned(),
            severity: DeliverySeverity::Warning,
            status,
            retry_hint: Some("retry_delivery".to_owned()),
        }
    }

    fn task_outcome_verdict(class: TaskOutcomeClass) -> TaskOutcomeVerdict {
        let next_action_hint = task_outcome_next_action_semantics(&class).action;
        TaskOutcomeVerdict {
            verdict_id: "verdict-1".to_owned(),
            class,
            reason: "fixture verdict".to_owned(),
            severity: DeliverySeverity::Warning,
            delivery_hint: Some("deliver if actionable".to_owned()),
            next_action_hint,
            rollback_hint: Some("checkpoint-1".to_owned()),
            run_id: "run-1".to_owned(),
            job_id: "job-1".to_owned(),
            result_ref: background_result_ref(BackgroundExitStatus::Failed),
            correlation_id: "corr-003".to_owned(),
        }
    }
}
