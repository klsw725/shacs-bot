use serde::{Deserialize, Serialize};
use serde_json::Value;
use shacs_eval::evaluator::stable_sha256_digest;
use shacs_eval::evaluator::{EvidenceRef, RedactionStatus};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunState {
    Planned,
    Admitted,
    Running,
    WaitingForChildren,
    Verifying,
    Synthesizing,
    WaitingForUser,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    Stale,
}

impl WorkflowRunState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Stale
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPattern {
    ClassifyAndAct,
    FanOutAndSynthesize,
    AdversarialVerification,
    GenerateAndFilter,
    Tournament,
    LoopUntilDone,
    WorkflowSequence,
    Hybrid,
}

impl WorkflowPattern {
    fn requires_verifier(self) -> bool {
        matches!(
            self,
            Self::AdversarialVerification | Self::GenerateAndFilter | Self::Tournament
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowQuarantinePolicy {
    None,
    ReadOnlyUntrusted,
    PrivilegedActorSeparated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowWorktreePolicy {
    None,
    ReadOnlySnapshot,
    IsolatedWorktreeRequired,
    IsolatedWorktreeOptional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowAdmissionDecision {
    UseRegularLoop,
    UseQuickWorkflow { reason: String },
    UseDynamicWorkflow { reason: String },
    AskUserForScope { question: String },
    BlockedByPolicy { reasons: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowAdmissionInput {
    pub objective_complexity: u8,
    pub estimated_item_count: usize,
    pub requires_parallelism: bool,
    pub requires_independent_verification: bool,
    pub requires_adversarial_review: bool,
    pub requires_large_context_partitioning: bool,
    pub requires_write_isolation: bool,
    pub requires_recurring_loop: bool,
    pub risk_level: u8,
    pub user_requested_workflow: bool,
    pub available_budget_tokens: Option<u64>,
    #[serde(default)]
    pub blocking_reasons: Vec<String>,
    #[serde(default)]
    pub missing_scope_questions: Vec<String>,
}

pub fn decide_workflow_admission(input: &WorkflowAdmissionInput) -> WorkflowAdmissionDecision {
    if !input.blocking_reasons.is_empty() {
        return WorkflowAdmissionDecision::BlockedByPolicy {
            reasons: input.blocking_reasons.clone(),
        };
    }

    if let Some(question) = input.missing_scope_questions.first() {
        return WorkflowAdmissionDecision::AskUserForScope {
            question: question.clone(),
        };
    }

    let dynamic_required = input.user_requested_workflow
        || input.requires_parallelism
        || input.requires_large_context_partitioning
        || input.requires_write_isolation
        || input.requires_recurring_loop
        || input.estimated_item_count >= 8
        || input.objective_complexity >= 8
        || input.risk_level >= 8;

    if dynamic_required {
        return WorkflowAdmissionDecision::UseDynamicWorkflow {
            reason: dynamic_reason(input),
        };
    }

    let quick_required = input.requires_independent_verification
        || input.requires_adversarial_review
        || input.estimated_item_count >= 3
        || input.objective_complexity >= 5
        || input.risk_level >= 5;

    if quick_required {
        return WorkflowAdmissionDecision::UseQuickWorkflow {
            reason: quick_reason(input),
        };
    }

    WorkflowAdmissionDecision::UseRegularLoop
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowHarnessPlan {
    pub workflow_id: String,
    pub origin_session_id: String,
    pub origin_turn_id: String,
    pub objective: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub pattern: WorkflowPattern,
    #[serde(default)]
    pub steps: Vec<WorkflowStep>,
    #[serde(default)]
    pub child_graph: Vec<WorkflowChildSpec>,
    #[serde(default)]
    pub verifier_graph: Vec<WorkflowVerifierSpec>,
    pub context_policy: WorkflowContextPolicy,
    pub tool_scope_policy: WorkflowToolScopePolicy,
    pub permission_policy: WorkflowPermissionPolicy,
    pub worktree_policy: WorkflowWorktreePolicy,
    pub model_routing_policy: WorkflowModelRoutingPolicy,
    pub budget_policy: WorkflowBudgetPolicy,
    pub checkpoint_policy: WorkflowCheckpointPolicy,
    pub merge_policy: WorkflowMergePolicy,
    pub stop_condition: WorkflowStopCondition,
    pub resume_policy: WorkflowResumePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub step_id: String,
    pub label: String,
    pub pattern: WorkflowPattern,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_output_schema: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowChildSpec {
    pub child_id: String,
    pub step_id: String,
    pub goal: String,
    #[serde(default)]
    pub tool_scope_ref: Option<String>,
    pub worktree_policy: WorkflowWorktreePolicy,
    pub budget: WorkflowBudgetSlice,
    pub verifier_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVerifierSpec {
    pub verifier_id: String,
    pub target_child_id: String,
    pub rubric: String,
    pub independent_evidence_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowContextPolicy {
    pub root_objective_snapshot: String,
    pub include_constraints_in_children: bool,
    #[serde(default)]
    pub untrusted_input_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowToolScopePolicy {
    pub scope_digest: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    pub deferred_tool_search_allowed: bool,
    pub quarantine: WorkflowQuarantinePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowToolScopeRole {
    Sanitizer,
    Child,
    Verifier,
    Synthesis,
    PrivilegedActor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSanitizedHandoffContract {
    pub sanitizer_step_id: String,
    pub privileged_step_id: String,
    pub sanitizer_tools: Vec<String>,
    pub privileged_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum WorkflowSanitizedHandoffStatus {
    NotRequired,
    Validated {
        contract: WorkflowSanitizedHandoffContract,
    },
    Blocked {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPermissionPolicy {
    pub permission_snapshot_ref: String,
    #[serde(default)]
    pub denied_capabilities: Vec<String>,
    pub approval_required_for_privileged_steps: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowModelRoutingPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier_model_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_model_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verifier_model_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesis_model_hint: Option<String>,
    pub fallback_model_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBudgetPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_child_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_verifier_tokens: Option<u64>,
    pub max_iterations: u32,
    pub max_parallel_children: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_wall_clock_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_heavy_commands: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBudgetSlice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_wall_clock_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowCheckpointPolicy {
    pub checkpoint_required: bool,
    pub checkpoint_before_privileged_steps: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowMergePolicy {
    pub require_verifier_pass: bool,
    pub allow_partial_completion: bool,
    pub surface_disagreements: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStopCondition {
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_new_findings_threshold: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowResumePolicy {
    pub require_plan_digest_match: bool,
    pub allow_completed_resume: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub workflow_id: String,
    pub origin_session_id: String,
    pub origin_turn_id: String,
    pub state: WorkflowRunState,
    pub harness_plan_digest: String,
    pub admitted_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<WorkflowCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    pub workflow_id: String,
    pub root_objective_snapshot: String,
    pub harness_plan_digest: String,
    pub state: WorkflowRunState,
    #[serde(default)]
    pub completed_steps: Vec<String>,
    #[serde(default)]
    pub active_children: Vec<String>,
    #[serde(default)]
    pub pending_barriers: Vec<String>,
    pub budget_usage: WorkflowBudgetUsage,
    #[serde(default)]
    pub worktree_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub last_safe_resume_point: String,
    pub recorded_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRuntimeCheckpointPayload {
    pub checkpoint: WorkflowCheckpoint,
    pub completed_step_id: Option<String>,
    #[serde(default)]
    pub completed_child_ids: Vec<String>,
    pub ready_step_ids: Vec<String>,
    pub pending_step_ids: Vec<String>,
    #[serde(default)]
    pub worktree_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub resume_step_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPatternContractEvidence {
    pub pattern: WorkflowPattern,
    pub bounded: bool,
    pub static_dag: bool,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum WorkflowPatternContractStatus {
    Satisfied,
    Blocked { reasons: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSanitizedHandoffEvidence {
    pub sanitizer_step_id: String,
    pub privileged_step_id: String,
    pub sanitizer_output_digest: String,
    pub privileged_input_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_untrusted_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum WorkflowSanitizedHandoffEvidenceStatus {
    Validated,
    Blocked { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBudgetUsage {
    pub known_tokens: u64,
    pub estimated_tokens: u64,
    pub child_runs: u32,
    pub verifier_runs: u32,
    pub heavy_commands: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowResumeDecision {
    ResumeAllowed { resume_point: String },
    AlreadyTerminal { state: WorkflowRunState },
    Blocked { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPrd000ReleaseEvidenceBucket {
    StateModel,
    HarnessPlanSchema,
    Admission,
    CheckpointResume,
    Diagnostics,
    Documentation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowPrd000ReleaseEvidence {
    pub bucket: WorkflowPrd000ReleaseEvidenceBucket,
    #[serde(default)]
    pub test_names: Vec<String>,
    #[serde(default)]
    pub manual_qa_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPrd000ReleaseEvidenceChecklist {
    pub required_buckets: Vec<WorkflowPrd000ReleaseEvidenceBucket>,
    pub covered_buckets: Vec<WorkflowPrd000ReleaseEvidenceBucket>,
    pub missing_buckets: Vec<WorkflowPrd000ReleaseEvidenceBucket>,
    pub passed: bool,
}

impl WorkflowPrd000ReleaseEvidenceBucket {
    pub fn required_buckets() -> Vec<Self> {
        vec![
            Self::StateModel,
            Self::HarnessPlanSchema,
            Self::Admission,
            Self::CheckpointResume,
            Self::Diagnostics,
            Self::Documentation,
        ]
    }
}

pub fn workflow_harness_plan_digest(
    plan: &WorkflowHarnessPlan,
) -> Result<String, serde_json::Error> {
    stable_sha256_digest(&serde_json::to_value(plan)?)
}

pub fn admit_workflow_plan(
    plan: &WorkflowHarnessPlan,
    admitted_at_ms: u64,
) -> Result<WorkflowRunRecord, serde_json::Error> {
    Ok(WorkflowRunRecord {
        workflow_id: plan.workflow_id.clone(),
        origin_session_id: plan.origin_session_id.clone(),
        origin_turn_id: plan.origin_turn_id.clone(),
        state: WorkflowRunState::Admitted,
        harness_plan_digest: workflow_harness_plan_digest(plan)?,
        admitted_at_ms,
        updated_at_ms: admitted_at_ms,
        checkpoint: None,
    })
}

pub fn build_workflow_checkpoint(
    plan: &WorkflowHarnessPlan,
    run: &WorkflowRunRecord,
    input: WorkflowCheckpointInput,
) -> WorkflowCheckpoint {
    WorkflowCheckpoint {
        workflow_id: run.workflow_id.clone(),
        root_objective_snapshot: plan.context_policy.root_objective_snapshot.clone(),
        harness_plan_digest: run.harness_plan_digest.clone(),
        state: input.state,
        completed_steps: input.completed_steps,
        active_children: input.active_children,
        pending_barriers: input.pending_barriers,
        budget_usage: input.budget_usage,
        worktree_refs: input.worktree_refs,
        evidence_refs: input.evidence_refs,
        last_safe_resume_point: input.last_safe_resume_point,
        recorded_at_ms: input.recorded_at_ms,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowCheckpointInput {
    pub state: WorkflowRunState,
    #[serde(default)]
    pub completed_steps: Vec<String>,
    #[serde(default)]
    pub active_children: Vec<String>,
    #[serde(default)]
    pub pending_barriers: Vec<String>,
    pub budget_usage: WorkflowBudgetUsage,
    #[serde(default)]
    pub worktree_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub last_safe_resume_point: String,
    pub recorded_at_ms: u64,
}

pub fn workflow_resume_decision(
    checkpoint: &WorkflowCheckpoint,
    resume_policy: &WorkflowResumePolicy,
    current_harness_plan_digest: &str,
) -> WorkflowResumeDecision {
    if checkpoint.state.is_terminal() && !resume_policy.allow_completed_resume {
        return WorkflowResumeDecision::AlreadyTerminal {
            state: checkpoint.state,
        };
    }

    if resume_policy.require_plan_digest_match
        && checkpoint.harness_plan_digest != current_harness_plan_digest
    {
        return WorkflowResumeDecision::Blocked {
            reason: "harness plan digest mismatch".to_owned(),
        };
    }

    if checkpoint.last_safe_resume_point.trim().is_empty() {
        return WorkflowResumeDecision::Blocked {
            reason: "checkpoint lacks last safe resume point".to_owned(),
        };
    }

    WorkflowResumeDecision::ResumeAllowed {
        resume_point: checkpoint.last_safe_resume_point.clone(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowResumeValidationInput {
    pub checkpoint: WorkflowCheckpoint,
    pub resume_policy: WorkflowResumePolicy,
    pub current_harness_plan_digest: String,
    #[serde(default)]
    pub required_completed_steps: Vec<String>,
    #[serde(default)]
    pub required_worktree_refs: Vec<String>,
    #[serde(default)]
    pub required_evidence_refs: Vec<String>,
}

pub fn workflow_resume_validation_decision(
    input: &WorkflowResumeValidationInput,
) -> WorkflowResumeDecision {
    match workflow_resume_decision(
        &input.checkpoint,
        &input.resume_policy,
        &input.current_harness_plan_digest,
    ) {
        WorkflowResumeDecision::ResumeAllowed { resume_point } => {
            if let Some(missing_step) = input
                .required_completed_steps
                .iter()
                .find(|step_id| !input.checkpoint.completed_steps.contains(step_id))
            {
                return WorkflowResumeDecision::Blocked {
                    reason: format!("checkpoint missing completed step `{missing_step}`"),
                };
            }
            if let Some(missing_ref) = input
                .required_worktree_refs
                .iter()
                .find(|worktree_ref| !input.checkpoint.worktree_refs.contains(worktree_ref))
            {
                return WorkflowResumeDecision::Blocked {
                    reason: format!("checkpoint missing worktree ref `{missing_ref}`"),
                };
            }
            if let Some(missing_ref) = input
                .required_evidence_refs
                .iter()
                .find(|evidence_ref| !input.checkpoint.evidence_refs.contains(evidence_ref))
            {
                return WorkflowResumeDecision::Blocked {
                    reason: format!("checkpoint missing evidence ref `{missing_ref}`"),
                };
            }
            WorkflowResumeDecision::ResumeAllowed { resume_point }
        }
        decision => decision,
    }
}

pub fn workflow_prd000_release_evidence_checklist(
    evidence: &[WorkflowPrd000ReleaseEvidence],
) -> WorkflowPrd000ReleaseEvidenceChecklist {
    let required_buckets = WorkflowPrd000ReleaseEvidenceBucket::required_buckets();
    let covered = evidence
        .iter()
        .filter(|entry| {
            (!entry.test_names.is_empty() || !entry.manual_qa_refs.is_empty())
                && entry.evidence_refs.iter().any(workflow_evidence_ref_valid)
        })
        .map(|entry| entry.bucket)
        .collect::<BTreeSet<_>>();
    let covered_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| covered.contains(bucket))
        .collect::<Vec<_>>();
    let missing_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| !covered.contains(bucket))
        .collect::<Vec<_>>();
    let passed = missing_buckets.is_empty();

    WorkflowPrd000ReleaseEvidenceChecklist {
        required_buckets,
        covered_buckets,
        missing_buckets,
        passed,
    }
}

fn workflow_evidence_ref_valid(evidence_ref: &EvidenceRef) -> bool {
    evidence_ref.owner_spec.as_deref() == Some("024")
        && !evidence_ref.id.trim().is_empty()
        && !evidence_ref.digest.trim().is_empty()
        && matches!(
            evidence_ref.redaction_status,
            RedactionStatus::AlreadySafe | RedactionStatus::Redacted
        )
}

fn dynamic_reason(input: &WorkflowAdmissionInput) -> String {
    if input.user_requested_workflow {
        return "user explicitly requested workflow".to_owned();
    }
    if input.requires_write_isolation {
        return "write isolation required".to_owned();
    }
    if input.requires_parallelism {
        return "parallel child execution required".to_owned();
    }
    if input.requires_large_context_partitioning {
        return "large context partitioning required".to_owned();
    }
    if input.requires_recurring_loop {
        return "recurring loop required".to_owned();
    }
    if input.estimated_item_count >= 8 {
        return "estimated item count requires fan-out".to_owned();
    }
    if input.risk_level >= 8 {
        return "risk level requires full workflow controls".to_owned();
    }
    "objective complexity requires dynamic workflow".to_owned()
}

fn quick_reason(input: &WorkflowAdmissionInput) -> String {
    if input.requires_adversarial_review {
        return "adversarial review required".to_owned();
    }
    if input.requires_independent_verification {
        return "independent verification required".to_owned();
    }
    if input.estimated_item_count >= 3 {
        return "small multi-item task benefits from quick workflow".to_owned();
    }
    if input.risk_level >= 5 {
        return "moderate risk requires lightweight workflow verification".to_owned();
    }
    "moderate complexity requires quick workflow".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowChildRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Stale,
}

impl WorkflowChildRunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut | Self::Stale
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowChildResult {
    pub child_id: String,
    pub step_id: String,
    pub status: WorkflowChildRunStatus,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowBarrierDecision {
    Ready { ready_step_ids: Vec<String> },
    Waiting { pending_step_ids: Vec<String> },
    Blocked { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum WorkflowPlanValidationStatus {
    Valid,
    Invalid { reasons: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowReadyScheduleDecision {
    Ready {
        ready_step_ids: Vec<String>,
        ready_child_ids: Vec<String>,
        deferred_child_ids: Vec<String>,
    },
    Waiting {
        pending_step_ids: Vec<String>,
    },
    Blocked {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSynthesisOutcome {
    pub accepted_child_ids: Vec<String>,
    pub rejected_child_ids: Vec<String>,
    pub unresolved_child_ids: Vec<String>,
    pub evidence_refs: Vec<EvidenceRef>,
    pub final_success_allowed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowVerifierVerdictKind {
    Pass,
    Fail,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowVerifierVerdict {
    pub verifier_id: String,
    pub target_child_id: String,
    pub verdict: WorkflowVerifierVerdictKind,
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVerifierEvidenceContract {
    pub verifier_id: String,
    pub target_child_id: String,
    pub independent_evidence_required: bool,
    pub required_owner_spec: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum WorkflowVerifierEvidenceStatus {
    Satisfied,
    Missing { verifier_id: String },
    Invalid { verifier_id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowVerificationGate {
    Passed,
    Failed { failing_child_ids: Vec<String> },
    Blocked { missing_verifier_ids: Vec<String> },
}

pub fn workflow_ready_step_ids(
    plan: &WorkflowHarnessPlan,
    completed_step_ids: &[String],
) -> Vec<String> {
    let completed = completed_step_ids.iter().collect::<BTreeSet<_>>();
    plan.steps
        .iter()
        .filter(|step| {
            !completed.contains(&step.step_id)
                && step
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency))
        })
        .map(|step| step.step_id.clone())
        .collect()
}

pub fn validate_workflow_plan(plan: &WorkflowHarnessPlan) -> WorkflowPlanValidationStatus {
    let mut reasons = Vec::new();
    if plan.workflow_id.trim().is_empty() {
        reasons.push("workflow id is empty".to_owned());
    }
    if plan.objective.trim().is_empty() {
        reasons.push("workflow objective is empty".to_owned());
    }
    if plan
        .context_policy
        .root_objective_snapshot
        .trim()
        .is_empty()
    {
        reasons.push("root objective snapshot is empty".to_owned());
    }
    if plan.steps.is_empty() {
        reasons.push("workflow plan has no steps".to_owned());
    }
    if plan.budget_policy.max_parallel_children == 0 {
        reasons.push("max_parallel_children must be at least 1".to_owned());
    }

    let step_ids = collect_unique_ids(plan.steps.iter().map(|step| step.step_id.as_str()));
    if let Some(duplicate) = first_duplicate(plan.steps.iter().map(|step| step.step_id.as_str())) {
        reasons.push(format!("duplicate workflow step id `{duplicate}`"));
    }
    if let Some(duplicate) =
        first_duplicate(plan.child_graph.iter().map(|child| child.child_id.as_str()))
    {
        reasons.push(format!("duplicate workflow child id `{duplicate}`"));
    }
    if let Some(duplicate) = first_duplicate(
        plan.verifier_graph
            .iter()
            .map(|verifier| verifier.verifier_id.as_str()),
    ) {
        reasons.push(format!("duplicate workflow verifier id `{duplicate}`"));
    }

    for step in &plan.steps {
        if step.step_id.trim().is_empty() {
            reasons.push("workflow step id is empty".to_owned());
        }
        for dependency in &step.depends_on {
            if !step_ids.contains(dependency.as_str()) {
                reasons.push(format!(
                    "workflow step `{}` depends on unknown step `{dependency}`",
                    step.step_id
                ));
            }
        }
    }
    if has_step_cycle(plan) {
        reasons.push("workflow step dependency graph contains a cycle".to_owned());
    }

    for child in &plan.child_graph {
        if !step_ids.contains(child.step_id.as_str()) {
            reasons.push(format!(
                "workflow child `{}` references unknown step `{}`",
                child.child_id, child.step_id
            ));
        }
    }
    for step in plan.steps.iter().filter(|step| step.required) {
        if !plan
            .child_graph
            .iter()
            .any(|child| child.step_id == step.step_id)
        {
            reasons.push(format!(
                "required workflow step `{}` has no child",
                step.step_id
            ));
        }
    }

    for verifier in &plan.verifier_graph {
        if !plan
            .child_graph
            .iter()
            .any(|child| child.child_id == verifier.target_child_id)
        {
            reasons.push(format!(
                "workflow verifier `{}` targets unknown child `{}`",
                verifier.verifier_id, verifier.target_child_id
            ));
        }
    }
    if plan.pattern.requires_verifier() && plan.verifier_graph.is_empty() {
        reasons.push(format!(
            "workflow pattern {:?} requires verifier graph",
            plan.pattern
        ));
    }
    if let WorkflowSanitizedHandoffStatus::Blocked { reason } =
        workflow_sanitized_handoff_status(plan)
    {
        reasons.push(reason);
    }
    if let WorkflowPatternContractStatus::Blocked {
        reasons: pattern_reasons,
    } = workflow_pattern_contract_status(plan)
    {
        reasons.extend(pattern_reasons);
    }

    if reasons.is_empty() {
        WorkflowPlanValidationStatus::Valid
    } else {
        reasons.sort();
        reasons.dedup();
        WorkflowPlanValidationStatus::Invalid { reasons }
    }
}

pub fn workflow_pattern_contract_status(
    plan: &WorkflowHarnessPlan,
) -> WorkflowPatternContractStatus {
    let mut reasons = Vec::new();
    let has_static_dag = !plan.steps.is_empty()
        && plan
            .child_graph
            .iter()
            .all(|child| plan.steps.iter().any(|step| step.step_id == child.step_id));
    let bounded = plan.budget_policy.max_iterations > 0;
    match plan.pattern {
        WorkflowPattern::ClassifyAndAct | WorkflowPattern::FanOutAndSynthesize => {
            if !has_static_dag {
                reasons.push("workflow pattern requires a static child DAG".to_owned());
            }
        }
        WorkflowPattern::AdversarialVerification | WorkflowPattern::GenerateAndFilter => {
            if !has_static_dag {
                reasons.push("workflow pattern requires a static child DAG".to_owned());
            }
            if plan.verifier_graph.is_empty() {
                reasons.push("workflow pattern requires verifier evidence".to_owned());
            }
        }
        WorkflowPattern::Tournament => {
            if !has_static_dag {
                reasons.push("tournament workflow requires a pre-expanded static DAG".to_owned());
            }
            if !bounded || plan.stop_condition.no_new_findings_threshold.is_none() {
                reasons.push(
                    "tournament workflow requires bounded rounds via stop condition".to_owned(),
                );
            }
            if plan.verifier_graph.is_empty() {
                reasons.push("tournament workflow requires verifier evidence".to_owned());
            }
        }
        WorkflowPattern::LoopUntilDone => {
            if !bounded || plan.stop_condition.no_new_findings_threshold.is_none() {
                reasons.push("loop workflow requires bounded stop condition".to_owned());
            }
        }
        WorkflowPattern::WorkflowSequence => {
            if plan.steps.iter().any(|step| step.depends_on.len() > 1) {
                reasons.push("workflow_sequence steps must have at most one dependency".to_owned());
            }
        }
        WorkflowPattern::Hybrid => {
            if plan
                .steps
                .iter()
                .any(|step| step.pattern == WorkflowPattern::Hybrid)
            {
                reasons.push("hybrid workflow must decompose into non-hybrid steps".to_owned());
            }
        }
    }
    if reasons.is_empty() {
        WorkflowPatternContractStatus::Satisfied
    } else {
        reasons.sort();
        reasons.dedup();
        WorkflowPatternContractStatus::Blocked { reasons }
    }
}

pub fn workflow_pattern_contract_evidence(
    plan: &WorkflowHarnessPlan,
) -> WorkflowPatternContractEvidence {
    WorkflowPatternContractEvidence {
        pattern: plan.pattern,
        bounded: plan.budget_policy.max_iterations > 0
            && (plan.pattern != WorkflowPattern::LoopUntilDone
                || plan.stop_condition.no_new_findings_threshold.is_some()),
        static_dag: !plan.steps.is_empty()
            && plan
                .child_graph
                .iter()
                .all(|child| plan.steps.iter().any(|step| step.step_id == child.step_id)),
        evidence_refs: vec![format!(
            "workflow://{}/pattern/{:?}",
            plan.workflow_id, plan.pattern
        )],
    }
}

pub fn workflow_ready_schedule_decision(
    plan: &WorkflowHarnessPlan,
    completed_step_ids: &[String],
    completed_child_ids: &[String],
    active_child_ids: &[String],
) -> WorkflowReadyScheduleDecision {
    if let WorkflowPlanValidationStatus::Invalid { reasons } = validate_workflow_plan(plan) {
        return WorkflowReadyScheduleDecision::Blocked {
            reason: reasons.join("; "),
        };
    }
    let completed_steps = completed_step_ids.iter().collect::<BTreeSet<_>>();
    let completed_children = completed_child_ids.iter().collect::<BTreeSet<_>>();
    let active_children = active_child_ids.iter().collect::<BTreeSet<_>>();
    if active_children.len() >= plan.budget_policy.max_parallel_children {
        return WorkflowReadyScheduleDecision::Waiting {
            pending_step_ids: workflow_ready_step_ids(plan, completed_step_ids),
        };
    }
    let capacity = plan
        .budget_policy
        .max_parallel_children
        .saturating_sub(active_children.len());
    let mut ready_step_ids = Vec::new();
    let mut candidate_child_ids = Vec::new();
    for step in &plan.steps {
        if completed_steps.contains(&step.step_id) {
            continue;
        }
        if !step
            .depends_on
            .iter()
            .all(|dependency| completed_steps.contains(dependency))
        {
            continue;
        }
        ready_step_ids.push(step.step_id.clone());
        for child in plan
            .child_graph
            .iter()
            .filter(|child| child.step_id == step.step_id)
        {
            if !completed_children.contains(&child.child_id)
                && !active_children.contains(&child.child_id)
            {
                candidate_child_ids.push(child.child_id.clone());
            }
        }
    }
    if ready_step_ids.is_empty() {
        let pending_step_ids = plan
            .steps
            .iter()
            .filter(|step| !completed_steps.contains(&step.step_id))
            .map(|step| step.step_id.clone())
            .collect::<Vec<_>>();
        return WorkflowReadyScheduleDecision::Waiting { pending_step_ids };
    }
    let ready_child_ids = candidate_child_ids
        .iter()
        .take(capacity)
        .cloned()
        .collect::<Vec<_>>();
    let deferred_child_ids = candidate_child_ids
        .iter()
        .skip(capacity)
        .cloned()
        .collect::<Vec<_>>();
    WorkflowReadyScheduleDecision::Ready {
        ready_step_ids,
        ready_child_ids,
        deferred_child_ids,
    }
}

pub fn workflow_role_scoped_tool_names(
    policy: &WorkflowToolScopePolicy,
    role: WorkflowToolScopeRole,
    available_tool_names: &[String],
) -> Vec<String> {
    policy
        .allowed_tools
        .iter()
        .filter(|tool_name| available_tool_names.contains(tool_name))
        .filter(|tool_name| role_allows_tool(policy.quarantine, role, tool_name))
        .cloned()
        .collect()
}

pub fn workflow_sanitized_handoff_status(
    plan: &WorkflowHarnessPlan,
) -> WorkflowSanitizedHandoffStatus {
    if plan.tool_scope_policy.quarantine != WorkflowQuarantinePolicy::PrivilegedActorSeparated {
        return WorkflowSanitizedHandoffStatus::NotRequired;
    }
    if plan.context_policy.untrusted_input_labels.is_empty() {
        return WorkflowSanitizedHandoffStatus::Blocked {
            reason: "privileged actor separation requires labeled untrusted inputs".to_owned(),
        };
    }
    let Some(privileged_step) = plan
        .steps
        .iter()
        .find(|step| step_has_privileged_child(plan, &step.step_id))
    else {
        return WorkflowSanitizedHandoffStatus::Blocked {
            reason: "privileged actor separation requires an isolated privileged step".to_owned(),
        };
    };
    let Some(sanitizer_step_id) = privileged_step
        .depends_on
        .iter()
        .find(|step_id| !step_has_privileged_child(plan, step_id))
    else {
        return WorkflowSanitizedHandoffStatus::Blocked {
            reason:
                "privileged actor separation requires a sanitizer dependency before privileged step"
                    .to_owned(),
        };
    };
    let sanitizer_tools = plan
        .tool_scope_policy
        .allowed_tools
        .iter()
        .filter(|tool_name| {
            role_allows_tool(
                plan.tool_scope_policy.quarantine,
                WorkflowToolScopeRole::Sanitizer,
                tool_name,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let privileged_tools = plan
        .tool_scope_policy
        .allowed_tools
        .iter()
        .filter(|tool_name| {
            role_allows_tool(
                plan.tool_scope_policy.quarantine,
                WorkflowToolScopeRole::PrivilegedActor,
                tool_name,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if sanitizer_tools.is_empty() || privileged_tools.is_empty() {
        return WorkflowSanitizedHandoffStatus::Blocked {
            reason: "privileged actor separation requires sanitizer and privileged tool scopes"
                .to_owned(),
        };
    }
    WorkflowSanitizedHandoffStatus::Validated {
        contract: WorkflowSanitizedHandoffContract {
            sanitizer_step_id: sanitizer_step_id.clone(),
            privileged_step_id: privileged_step.step_id.clone(),
            sanitizer_tools,
            privileged_tools,
        },
    }
}

pub fn workflow_sanitized_handoff_evidence_status(
    contract: &WorkflowSanitizedHandoffContract,
    evidence: &WorkflowSanitizedHandoffEvidence,
) -> WorkflowSanitizedHandoffEvidenceStatus {
    if evidence.sanitizer_step_id != contract.sanitizer_step_id
        || evidence.privileged_step_id != contract.privileged_step_id
    {
        return WorkflowSanitizedHandoffEvidenceStatus::Blocked {
            reason: "sanitized handoff evidence step mismatch".to_owned(),
        };
    }
    if evidence.sanitizer_output_digest.trim().is_empty()
        || evidence.privileged_input_digest.trim().is_empty()
    {
        return WorkflowSanitizedHandoffEvidenceStatus::Blocked {
            reason: "sanitized handoff evidence lacks digest".to_owned(),
        };
    }
    if evidence.sanitizer_output_digest != evidence.privileged_input_digest {
        return WorkflowSanitizedHandoffEvidenceStatus::Blocked {
            reason: "privileged input must match sanitizer output digest".to_owned(),
        };
    }
    if evidence
        .raw_untrusted_digest
        .as_ref()
        .is_some_and(|raw_digest| raw_digest == &evidence.privileged_input_digest)
    {
        return WorkflowSanitizedHandoffEvidenceStatus::Blocked {
            reason: "privileged input digest must not be raw untrusted input".to_owned(),
        };
    }
    WorkflowSanitizedHandoffEvidenceStatus::Validated
}

pub fn workflow_barrier_decision(
    plan: &WorkflowHarnessPlan,
    results: &[WorkflowChildResult],
) -> WorkflowBarrierDecision {
    let result_child_ids = results
        .iter()
        .map(|result| result.child_id.as_str())
        .collect::<BTreeSet<_>>();
    let failed_required = plan
        .child_graph
        .iter()
        .filter(|child| {
            plan.steps
                .iter()
                .any(|step| step.step_id == child.step_id && step.required)
        })
        .filter(|child| {
            results.iter().any(|result| {
                result.child_id == child.child_id
                    && !matches!(result.status, WorkflowChildRunStatus::Completed)
                    && result.status.is_terminal()
            })
        })
        .map(|child| child.child_id.clone())
        .collect::<Vec<_>>();
    if !failed_required.is_empty() {
        return WorkflowBarrierDecision::Blocked {
            reason: format!("required child failed: {}", failed_required.join(", ")),
        };
    }

    let pending_step_ids = plan
        .steps
        .iter()
        .filter(|step| step.required)
        .filter(|step| {
            plan.child_graph
                .iter()
                .filter(|child| child.step_id == step.step_id)
                .any(|child| {
                    !result_child_ids.contains(child.child_id.as_str())
                        || results.iter().any(|result| {
                            result.child_id == child.child_id
                                && result.status != WorkflowChildRunStatus::Completed
                        })
                })
        })
        .map(|step| step.step_id.clone())
        .collect::<Vec<_>>();
    if !pending_step_ids.is_empty() {
        return WorkflowBarrierDecision::Waiting { pending_step_ids };
    }

    WorkflowBarrierDecision::Ready {
        ready_step_ids: plan.steps.iter().map(|step| step.step_id.clone()).collect(),
    }
}

pub fn workflow_verification_gate(
    plan: &WorkflowHarnessPlan,
    verdicts: &[WorkflowVerifierVerdict],
) -> WorkflowVerificationGate {
    let mut missing = Vec::new();
    let mut failing = Vec::new();

    for verifier in &plan.verifier_graph {
        let matching = verdicts
            .iter()
            .filter(|verdict| verdict.verifier_id == verifier.verifier_id)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            missing.push(verifier.verifier_id.clone());
        } else if matching.iter().any(|verdict| {
            workflow_verifier_evidence_status(&verifier.evidence_contract(), verdict)
                != WorkflowVerifierEvidenceStatus::Satisfied
                || verdict.verdict != WorkflowVerifierVerdictKind::Pass
        }) {
            failing.push(verifier.target_child_id.clone());
        }
    }

    for child in plan
        .child_graph
        .iter()
        .filter(|child| child.verifier_required)
    {
        let verifier_specs = plan
            .verifier_graph
            .iter()
            .filter(|verifier| verifier.target_child_id == child.child_id)
            .collect::<Vec<_>>();
        if verifier_specs.is_empty() {
            missing.push(child.child_id.clone());
            continue;
        }
        if !verifier_specs.iter().any(|verifier| {
            verdicts.iter().any(|verdict| {
                verdict.verifier_id == verifier.verifier_id
                    && verdict.target_child_id == child.child_id
            })
        }) && !missing.iter().any(|missing_id| {
            verifier_specs
                .iter()
                .any(|verifier| missing_id == &verifier.verifier_id)
        }) {
            missing.push(child.child_id.clone());
        }
    }

    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        return WorkflowVerificationGate::Blocked {
            missing_verifier_ids: missing,
        };
    }
    if !failing.is_empty() {
        failing.sort();
        failing.dedup();
        return WorkflowVerificationGate::Failed {
            failing_child_ids: failing,
        };
    }
    WorkflowVerificationGate::Passed
}

pub fn workflow_verifier_evidence_status(
    contract: &WorkflowVerifierEvidenceContract,
    verdict: &WorkflowVerifierVerdict,
) -> WorkflowVerifierEvidenceStatus {
    if verdict.verifier_id != contract.verifier_id
        || verdict.target_child_id != contract.target_child_id
    {
        return WorkflowVerifierEvidenceStatus::Invalid {
            verifier_id: contract.verifier_id.clone(),
            reason: "verifier verdict identity mismatch".to_owned(),
        };
    }
    if !contract.independent_evidence_required {
        return WorkflowVerifierEvidenceStatus::Satisfied;
    }
    if verdict.evidence_refs.is_empty() {
        return WorkflowVerifierEvidenceStatus::Missing {
            verifier_id: contract.verifier_id.clone(),
        };
    }
    if verdict.evidence_refs.iter().any(|evidence_ref| {
        evidence_ref.owner_spec.as_deref() != Some(contract.required_owner_spec.as_str())
    }) {
        return WorkflowVerifierEvidenceStatus::Invalid {
            verifier_id: contract.verifier_id.clone(),
            reason: "verifier evidence owner mismatch".to_owned(),
        };
    }
    if !verdict
        .evidence_refs
        .iter()
        .all(workflow_evidence_ref_valid)
    {
        return WorkflowVerifierEvidenceStatus::Invalid {
            verifier_id: contract.verifier_id.clone(),
            reason: "verifier evidence is not redaction-safe".to_owned(),
        };
    }
    WorkflowVerifierEvidenceStatus::Satisfied
}

pub fn workflow_synthesis_outcome(
    plan: &WorkflowHarnessPlan,
    results: &[WorkflowChildResult],
    verification_gate: &WorkflowVerificationGate,
    merge_policy: &WorkflowMergePolicy,
) -> WorkflowSynthesisOutcome {
    let accepted_child_ids = results
        .iter()
        .filter(|result| result.status == WorkflowChildRunStatus::Completed)
        .map(|result| result.child_id.clone())
        .collect::<Vec<_>>();
    let rejected_child_ids = results
        .iter()
        .filter(|result| {
            result.status.is_terminal() && result.status != WorkflowChildRunStatus::Completed
        })
        .map(|result| result.child_id.clone())
        .collect::<Vec<_>>();
    let unresolved_child_ids = results
        .iter()
        .filter(|result| !result.status.is_terminal())
        .map(|result| result.child_id.clone())
        .collect::<Vec<_>>();
    let evidence_refs = results
        .iter()
        .flat_map(|result| result.evidence_refs.iter().cloned())
        .filter(workflow_evidence_ref_valid)
        .collect::<Vec<_>>();
    let verifier_required = plan.child_graph.iter().any(|child| child.verifier_required);
    let verifier_allows_success = !(merge_policy.require_verifier_pass || verifier_required)
        || matches!(verification_gate, WorkflowVerificationGate::Passed);
    let final_success_allowed = verifier_allows_success
        && unresolved_child_ids.is_empty()
        && (rejected_child_ids.is_empty() || merge_policy.allow_partial_completion);

    WorkflowSynthesisOutcome {
        accepted_child_ids,
        rejected_child_ids,
        unresolved_child_ids,
        evidence_refs,
        final_success_allowed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowWorktreeRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    pub child_id: String,
    pub requires_write: bool,
    pub policy: WorkflowWorktreePolicy,
    pub approval_granted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_worktree_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowWorktreeDecision {
    NotRequired,
    UseExisting { worktree_ref: String },
    CreateIsolated { branch_name: String },
    Blocked { reason: String },
}

pub fn workflow_worktree_decision(request: &WorkflowWorktreeRequest) -> WorkflowWorktreeDecision {
    if !request.requires_write && request.policy == WorkflowWorktreePolicy::None {
        return WorkflowWorktreeDecision::NotRequired;
    }
    if let Some(worktree_ref) = request.existing_worktree_ref.as_ref() {
        if !worktree_ref.trim().is_empty() {
            return WorkflowWorktreeDecision::UseExisting {
                worktree_ref: worktree_ref.clone(),
            };
        }
    }
    match request.policy {
        WorkflowWorktreePolicy::None | WorkflowWorktreePolicy::ReadOnlySnapshot => {
            if request.requires_write {
                WorkflowWorktreeDecision::Blocked {
                    reason: "write-capable child lacks isolated worktree policy".to_owned(),
                }
            } else {
                WorkflowWorktreeDecision::NotRequired
            }
        }
        WorkflowWorktreePolicy::IsolatedWorktreeRequired => {
            if request.approval_granted {
                WorkflowWorktreeDecision::CreateIsolated {
                    branch_name: workflow_worktree_branch_name(request),
                }
            } else {
                WorkflowWorktreeDecision::Blocked {
                    reason: "isolated worktree requires orchestrator approval".to_owned(),
                }
            }
        }
        WorkflowWorktreePolicy::IsolatedWorktreeOptional => {
            if request.approval_granted {
                WorkflowWorktreeDecision::CreateIsolated {
                    branch_name: workflow_worktree_branch_name(request),
                }
            } else {
                WorkflowWorktreeDecision::NotRequired
            }
        }
    }
}

pub fn workflow_worktree_branch_name(request: &WorkflowWorktreeRequest) -> String {
    match request
        .workflow_id
        .as_deref()
        .filter(|workflow_id| !workflow_id.trim().is_empty())
    {
        Some(workflow_id) => format!(
            "workflow/{}/{}",
            sanitize_branch_component(workflow_id),
            sanitize_branch_component(&request.child_id)
        ),
        None => format!("workflow/{}", sanitize_branch_component(&request.child_id)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowBudgetDecision {
    Allowed { remaining_tokens: Option<u64> },
    Blocked { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowExecutionRole {
    Classifier,
    Child,
    Verifier,
    Synthesis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRuntimeEnforcementInput {
    pub role: WorkflowExecutionRole,
    pub usage: WorkflowBudgetUsage,
    pub active_child_count: usize,
    pub elapsed_wall_clock_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_tokens: Option<u64>,
    pub requests_heavy_command: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowRuntimeEnforcementDecision {
    Allowed {
        route: WorkflowModelRouteSnapshot,
        remaining_tokens: Option<u64>,
    },
    Throttled {
        reason: String,
    },
    Blocked {
        reason: String,
    },
}

pub fn workflow_budget_decision(
    policy: &WorkflowBudgetPolicy,
    usage: &WorkflowBudgetUsage,
) -> WorkflowBudgetDecision {
    let used_tokens = usage.known_tokens.saturating_add(usage.estimated_tokens);
    if let Some(max_total_tokens) = policy.max_total_tokens {
        if used_tokens >= max_total_tokens {
            return WorkflowBudgetDecision::Blocked {
                reason: "workflow token budget exhausted".to_owned(),
            };
        }
    }
    if usage.child_runs > policy.max_iterations {
        return WorkflowBudgetDecision::Blocked {
            reason: "workflow iteration budget exhausted".to_owned(),
        };
    }
    if let Some(max_heavy_commands) = policy.max_heavy_commands {
        if usage.heavy_commands > max_heavy_commands {
            return WorkflowBudgetDecision::Blocked {
                reason: "workflow heavy command budget exhausted".to_owned(),
            };
        }
    }

    WorkflowBudgetDecision::Allowed {
        remaining_tokens: policy
            .max_total_tokens
            .map(|max_total_tokens| max_total_tokens.saturating_sub(used_tokens)),
    }
}

pub fn workflow_runtime_enforcement_decision(
    budget_policy: &WorkflowBudgetPolicy,
    model_policy: &WorkflowModelRoutingPolicy,
    input: &WorkflowRuntimeEnforcementInput,
) -> WorkflowRuntimeEnforcementDecision {
    if let Some(max_wall_clock_ms) = budget_policy.max_wall_clock_ms {
        if input.elapsed_wall_clock_ms >= max_wall_clock_ms {
            return WorkflowRuntimeEnforcementDecision::Blocked {
                reason: "workflow wall-clock budget exhausted".to_owned(),
            };
        }
    }

    if input.role == WorkflowExecutionRole::Child
        && input.active_child_count >= budget_policy.max_parallel_children
    {
        return WorkflowRuntimeEnforcementDecision::Throttled {
            reason: "workflow parallel child limit reached".to_owned(),
        };
    }
    if input.role == WorkflowExecutionRole::Child
        && input.usage.child_runs >= budget_policy.max_iterations
    {
        return WorkflowRuntimeEnforcementDecision::Blocked {
            reason: "workflow iteration budget exhausted".to_owned(),
        };
    }
    if let Some(max_heavy_commands) = budget_policy.max_heavy_commands {
        if input.requests_heavy_command && input.usage.heavy_commands >= max_heavy_commands {
            return WorkflowRuntimeEnforcementDecision::Blocked {
                reason: "workflow heavy command budget exhausted".to_owned(),
            };
        }
    }

    let mut usage = input.usage.clone();
    if input.requests_heavy_command {
        usage.heavy_commands = usage.heavy_commands.saturating_add(1);
    }
    if let Some(requested_tokens) = input.requested_tokens {
        if let Some(reason) =
            per_role_token_block_reason(budget_policy, input.role, requested_tokens)
        {
            return WorkflowRuntimeEnforcementDecision::Blocked { reason };
        }
        usage.estimated_tokens = usage.estimated_tokens.saturating_add(requested_tokens);
    }

    match workflow_budget_decision(budget_policy, &usage) {
        WorkflowBudgetDecision::Allowed { remaining_tokens } => {
            WorkflowRuntimeEnforcementDecision::Allowed {
                route: workflow_model_route_snapshot(model_policy, input.role.as_route_role()),
                remaining_tokens,
            }
        }
        WorkflowBudgetDecision::Blocked { reason } => {
            WorkflowRuntimeEnforcementDecision::Blocked { reason }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowModelRouteSnapshot {
    pub role: String,
    pub selected_model_hint: Option<String>,
    pub fallback_model_policy: String,
}

pub fn workflow_model_route_snapshot(
    policy: &WorkflowModelRoutingPolicy,
    role: &str,
) -> WorkflowModelRouteSnapshot {
    let selected_model_hint = match role {
        "classifier" => policy.classifier_model_hint.clone(),
        "child" => policy.child_model_hint.clone(),
        "verifier" => policy.verifier_model_hint.clone(),
        "synthesis" => policy.synthesis_model_hint.clone(),
        _ => None,
    };
    WorkflowModelRouteSnapshot {
        role: role.to_owned(),
        selected_model_hint,
        fallback_model_policy: policy.fallback_model_policy.clone(),
    }
}

impl WorkflowExecutionRole {
    fn as_route_role(self) -> &'static str {
        match self {
            Self::Classifier => "classifier",
            Self::Child => "child",
            Self::Verifier => "verifier",
            Self::Synthesis => "synthesis",
        }
    }
}

impl WorkflowVerifierSpec {
    pub fn evidence_contract(&self) -> WorkflowVerifierEvidenceContract {
        WorkflowVerifierEvidenceContract {
            verifier_id: self.verifier_id.clone(),
            target_child_id: self.target_child_id.clone(),
            independent_evidence_required: self.independent_evidence_required,
            required_owner_spec: "024".to_owned(),
        }
    }
}

fn sanitize_branch_component(component: &str) -> String {
    let sanitized = component
        .trim()
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    if sanitized.is_empty() {
        "unnamed".to_owned()
    } else {
        sanitized
    }
}

fn per_role_token_block_reason(
    policy: &WorkflowBudgetPolicy,
    role: WorkflowExecutionRole,
    requested_tokens: u64,
) -> Option<String> {
    match role {
        WorkflowExecutionRole::Child => policy.max_child_tokens.and_then(|max_child_tokens| {
            (requested_tokens > max_child_tokens)
                .then(|| "workflow child token slice exceeded".to_owned())
        }),
        WorkflowExecutionRole::Verifier => {
            policy.max_verifier_tokens.and_then(|max_verifier_tokens| {
                (requested_tokens > max_verifier_tokens)
                    .then(|| "workflow verifier token slice exceeded".to_owned())
            })
        }
        WorkflowExecutionRole::Classifier | WorkflowExecutionRole::Synthesis => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRecipe {
    pub recipe_id: String,
    pub source_ref: String,
    pub pattern: WorkflowPattern,
    pub prompt_template_ref: String,
    pub rubric_ref: Option<String>,
    pub output_schema_ref: Option<String>,
    pub suggested_budget_tokens: Option<u64>,
    pub suggested_tool_scope_ref: Option<String>,
    pub safety_notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum WorkflowRecipeReadiness {
    Ready,
    Malformed { reasons: Vec<String> },
}

pub fn workflow_recipe_readiness(recipe: &WorkflowRecipe) -> WorkflowRecipeReadiness {
    let mut reasons = Vec::new();
    if recipe.recipe_id.trim().is_empty() {
        reasons.push("recipe id is empty".to_owned());
    }
    if recipe.source_ref.trim().is_empty() {
        reasons.push("source ref is empty".to_owned());
    }
    if recipe.prompt_template_ref.trim().is_empty() {
        reasons.push("prompt template ref is empty".to_owned());
    }
    if reasons.is_empty() {
        WorkflowRecipeReadiness::Ready
    } else {
        WorkflowRecipeReadiness::Malformed { reasons }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepPrivilege {
    ReadTrusted,
    ReadUntrusted,
    PrivilegedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowQuarantineDecision {
    Allowed,
    Blocked { reason: String },
    RequiresSanitizedHandoff,
}

pub fn workflow_quarantine_decision(
    policy: WorkflowQuarantinePolicy,
    privilege: WorkflowStepPrivilege,
) -> WorkflowQuarantineDecision {
    match (policy, privilege) {
        (WorkflowQuarantinePolicy::None, _) => WorkflowQuarantineDecision::Allowed,
        (WorkflowQuarantinePolicy::ReadOnlyUntrusted, WorkflowStepPrivilege::PrivilegedAction) => {
            WorkflowQuarantineDecision::Blocked {
                reason: "read-only untrusted workflow cannot perform privileged action".to_owned(),
            }
        }
        (
            WorkflowQuarantinePolicy::PrivilegedActorSeparated,
            WorkflowStepPrivilege::PrivilegedAction,
        ) => WorkflowQuarantineDecision::RequiresSanitizedHandoff,
        _ => WorkflowQuarantineDecision::Allowed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowPermissionCeilingDecision {
    Allowed,
    ApprovalRequired { reason: String },
    Blocked { denied_capability: String },
}

pub fn workflow_permission_ceiling_decision(
    policy: &WorkflowPermissionPolicy,
    requested_capabilities: &[String],
    privileged_step: bool,
) -> WorkflowPermissionCeilingDecision {
    if let Some(denied) = requested_capabilities
        .iter()
        .find(|capability| policy.denied_capabilities.contains(capability))
    {
        return WorkflowPermissionCeilingDecision::Blocked {
            denied_capability: denied.clone(),
        };
    }
    if privileged_step && policy.approval_required_for_privileged_steps {
        return WorkflowPermissionCeilingDecision::ApprovalRequired {
            reason: "privileged workflow step requires approval".to_owned(),
        };
    }
    WorkflowPermissionCeilingDecision::Allowed
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowProjection {
    pub schema_label: String,
    pub schema_version: String,
    pub workflow_id: String,
    pub objective_summary: String,
    pub pattern: WorkflowPattern,
    pub state: WorkflowRunState,
    pub progress_count: usize,
    pub active_child_count: usize,
    pub pending_barrier_count: usize,
    pub verifier_status: String,
    pub budget_usage: WorkflowBudgetUsage,
    pub worktree_refs: Vec<String>,
    pub blocked_reason: Option<String>,
    pub next_action: Option<String>,
    pub resume_available: bool,
    pub evidence_refs: Vec<EvidenceRef>,
}

pub fn workflow_projection(
    run: &WorkflowRunRecord,
    plan: &WorkflowHarnessPlan,
    checkpoint: Option<&WorkflowCheckpoint>,
    verifier_gate: &WorkflowVerificationGate,
    evidence_refs: &[EvidenceRef],
) -> WorkflowProjection {
    let evidence_refs = evidence_refs
        .iter()
        .filter(|evidence_ref| workflow_evidence_ref_valid(evidence_ref))
        .cloned()
        .collect::<Vec<_>>();
    let budget_usage = checkpoint
        .map(|checkpoint| checkpoint.budget_usage.clone())
        .unwrap_or(WorkflowBudgetUsage {
            known_tokens: 0,
            estimated_tokens: 0,
            child_runs: 0,
            verifier_runs: 0,
            heavy_commands: 0,
        });
    WorkflowProjection {
        schema_label: "024WorkflowProjection".to_owned(),
        schema_version: "024WorkflowProjection.v1".to_owned(),
        workflow_id: run.workflow_id.clone(),
        objective_summary: plan.objective.clone(),
        pattern: plan.pattern,
        state: run.state,
        progress_count: checkpoint
            .map(|checkpoint| checkpoint.completed_steps.len())
            .unwrap_or(0),
        active_child_count: checkpoint
            .map(|checkpoint| checkpoint.active_children.len())
            .unwrap_or(0),
        pending_barrier_count: checkpoint
            .map(|checkpoint| checkpoint.pending_barriers.len())
            .unwrap_or(0),
        verifier_status: verifier_status_label(verifier_gate).to_owned(),
        budget_usage,
        worktree_refs: checkpoint
            .map(|checkpoint| checkpoint.worktree_refs.clone())
            .unwrap_or_default(),
        blocked_reason: blocked_reason_for_resume(run),
        next_action: next_action_for_state(run.state).map(str::to_owned),
        resume_available: checkpoint
            .map(|checkpoint| !checkpoint.last_safe_resume_point.trim().is_empty())
            .unwrap_or(false),
        evidence_refs,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDiagnosticsManifest {
    pub workflow_id: String,
    pub harness_plan_digest: String,
    pub child_graph_digest: String,
    pub verifier_graph_digest: String,
    pub merge_decision_ref: Option<String>,
    pub stale_result_refs: Vec<String>,
    pub runtime_diagnostic_refs: Vec<String>,
    pub replay_live_actions_allowed: bool,
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRuntimeDiagnosticsInput {
    pub merge_decision_ref: Option<String>,
    #[serde(default)]
    pub stale_result_refs: Vec<String>,
    #[serde(default)]
    pub recipe_source_refs: Vec<String>,
    #[serde(default)]
    pub barrier_refs: Vec<String>,
    #[serde(default)]
    pub tool_scope_refs: Vec<String>,
    #[serde(default)]
    pub verifier_refs: Vec<String>,
    #[serde(default)]
    pub merge_refs: Vec<String>,
    #[serde(default)]
    pub synthesis_refs: Vec<String>,
    #[serde(default)]
    pub cleanup_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

pub fn workflow_diagnostics_manifest(
    plan: &WorkflowHarnessPlan,
    stale_result_refs: Vec<String>,
    evidence_refs: Vec<EvidenceRef>,
) -> Result<WorkflowDiagnosticsManifest, serde_json::Error> {
    Ok(WorkflowDiagnosticsManifest {
        workflow_id: plan.workflow_id.clone(),
        harness_plan_digest: workflow_harness_plan_digest(plan)?,
        child_graph_digest: stable_sha256_digest(&serde_json::to_value(&plan.child_graph)?)?,
        verifier_graph_digest: stable_sha256_digest(&serde_json::to_value(&plan.verifier_graph)?)?,
        merge_decision_ref: None,
        stale_result_refs,
        runtime_diagnostic_refs: Vec::new(),
        replay_live_actions_allowed: false,
        evidence_refs: evidence_refs
            .into_iter()
            .filter(workflow_evidence_ref_valid)
            .collect(),
    })
}

pub fn workflow_runtime_diagnostics_manifest(
    plan: &WorkflowHarnessPlan,
    input: WorkflowRuntimeDiagnosticsInput,
) -> Result<WorkflowDiagnosticsManifest, serde_json::Error> {
    let mut runtime_diagnostic_refs = Vec::new();
    runtime_diagnostic_refs.extend(input.recipe_source_refs);
    runtime_diagnostic_refs.extend(input.barrier_refs);
    runtime_diagnostic_refs.extend(input.tool_scope_refs);
    runtime_diagnostic_refs.extend(input.verifier_refs);
    runtime_diagnostic_refs.extend(input.merge_refs);
    runtime_diagnostic_refs.extend(input.synthesis_refs);
    runtime_diagnostic_refs.extend(input.cleanup_refs);
    runtime_diagnostic_refs.sort();
    runtime_diagnostic_refs.dedup();

    let mut manifest =
        workflow_diagnostics_manifest(plan, input.stale_result_refs, input.evidence_refs)?;
    manifest.merge_decision_ref = input
        .merge_decision_ref
        .filter(|reference| !reference.trim().is_empty());
    manifest.runtime_diagnostic_refs = runtime_diagnostic_refs;
    Ok(manifest)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSpec024ReleaseEvidenceBucket {
    Prd000StateHarnessPlan,
    Prd001PatternChildGraph,
    Prd002VerifierReview,
    Prd003WorktreeMerge,
    Prd004BudgetModelRouting,
    Prd005SkillRecipes,
    Prd006QuarantinePermissions,
    Prd007ResumeReplayDiagnostics,
    Prd008ProjectionReleaseGate,
    Prd009RuntimeExecution,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSpec024ReleaseEvidence {
    pub bucket: WorkflowSpec024ReleaseEvidenceBucket,
    #[serde(default)]
    pub test_names: Vec<String>,
    #[serde(default)]
    pub manual_qa_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSpec024ReleaseEvidenceChecklist {
    pub required_buckets: Vec<WorkflowSpec024ReleaseEvidenceBucket>,
    pub covered_buckets: Vec<WorkflowSpec024ReleaseEvidenceBucket>,
    pub missing_buckets: Vec<WorkflowSpec024ReleaseEvidenceBucket>,
    pub passed: bool,
}

impl WorkflowSpec024ReleaseEvidenceBucket {
    pub fn required_buckets() -> Vec<Self> {
        vec![
            Self::Prd000StateHarnessPlan,
            Self::Prd001PatternChildGraph,
            Self::Prd002VerifierReview,
            Self::Prd003WorktreeMerge,
            Self::Prd004BudgetModelRouting,
            Self::Prd005SkillRecipes,
            Self::Prd006QuarantinePermissions,
            Self::Prd007ResumeReplayDiagnostics,
            Self::Prd008ProjectionReleaseGate,
            Self::Prd009RuntimeExecution,
        ]
    }
}

pub fn workflow_spec024_release_evidence_checklist(
    evidence: &[WorkflowSpec024ReleaseEvidence],
) -> WorkflowSpec024ReleaseEvidenceChecklist {
    let required_buckets = WorkflowSpec024ReleaseEvidenceBucket::required_buckets();
    let covered = evidence
        .iter()
        .filter(|entry| {
            (!entry.test_names.is_empty() || !entry.manual_qa_refs.is_empty())
                && entry.evidence_refs.iter().any(workflow_evidence_ref_valid)
        })
        .map(|entry| entry.bucket)
        .collect::<BTreeSet<_>>();
    let covered_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| covered.contains(bucket))
        .collect::<Vec<_>>();
    let missing_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| !covered.contains(bucket))
        .collect::<Vec<_>>();
    let passed = missing_buckets.is_empty();

    WorkflowSpec024ReleaseEvidenceChecklist {
        required_buckets,
        covered_buckets,
        missing_buckets,
        passed,
    }
}

fn verifier_status_label(gate: &WorkflowVerificationGate) -> &'static str {
    match gate {
        WorkflowVerificationGate::Passed => "passed",
        WorkflowVerificationGate::Failed { .. } => "failed",
        WorkflowVerificationGate::Blocked { .. } => "blocked",
    }
}

fn blocked_reason_for_resume(run: &WorkflowRunRecord) -> Option<String> {
    if run.state == WorkflowRunState::Blocked {
        Some("workflow is blocked; inspect checkpoint and evidence refs".to_owned())
    } else {
        None
    }
}

fn next_action_for_state(state: WorkflowRunState) -> Option<&'static str> {
    match state {
        WorkflowRunState::WaitingForUser => Some("wait_for_user"),
        WorkflowRunState::Blocked => Some("inspect_blocker"),
        WorkflowRunState::Failed => Some("inspect_failure"),
        WorkflowRunState::Completed => None,
        _ => Some("continue_workflow"),
    }
}

fn collect_unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> BTreeSet<&'a str> {
    ids.filter(|id| !id.trim().is_empty()).collect()
}

fn first_duplicate<'a>(ids: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Some(id.to_owned());
        }
    }
    None
}

fn has_step_cycle(plan: &WorkflowHarnessPlan) -> bool {
    let dependencies = plan
        .steps
        .iter()
        .map(|step| (step.step_id.as_str(), step.depends_on.as_slice()))
        .collect::<BTreeMap<_, _>>();
    plan.steps.iter().any(|step| {
        let mut visiting = BTreeSet::new();
        step_visits_cycle(step.step_id.as_str(), &dependencies, &mut visiting)
    })
}

fn step_visits_cycle<'a>(
    step_id: &'a str,
    dependencies: &BTreeMap<&'a str, &'a [String]>,
    visiting: &mut BTreeSet<&'a str>,
) -> bool {
    if !visiting.insert(step_id) {
        return true;
    }
    let has_cycle = dependencies.get(step_id).is_some_and(|step_dependencies| {
        step_dependencies.iter().any(|dependency| {
            dependencies.contains_key(dependency.as_str())
                && step_visits_cycle(dependency.as_str(), dependencies, visiting)
        })
    });
    visiting.remove(step_id);
    has_cycle
}

fn step_has_privileged_child(plan: &WorkflowHarnessPlan, step_id: &str) -> bool {
    plan.child_graph.iter().any(|child| {
        child.step_id == step_id
            && matches!(
                child.worktree_policy,
                WorkflowWorktreePolicy::IsolatedWorktreeRequired
                    | WorkflowWorktreePolicy::IsolatedWorktreeOptional
            )
    })
}

fn role_allows_tool(
    quarantine: WorkflowQuarantinePolicy,
    role: WorkflowToolScopeRole,
    tool_name: &str,
) -> bool {
    match role {
        WorkflowToolScopeRole::Sanitizer => !tool_is_privileged(tool_name),
        WorkflowToolScopeRole::Verifier | WorkflowToolScopeRole::Synthesis => {
            !tool_is_privileged(tool_name)
        }
        WorkflowToolScopeRole::Child => {
            quarantine != WorkflowQuarantinePolicy::PrivilegedActorSeparated
                || !tool_is_privileged(tool_name)
        }
        WorkflowToolScopeRole::PrivilegedActor => tool_is_privileged(tool_name),
    }
}

fn tool_is_privileged(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write_file" | "edit_file" | "exec" | "notebook_edit" | "message" | "cron" | "spawn"
    ) || tool_name.starts_with("mcp_")
}
