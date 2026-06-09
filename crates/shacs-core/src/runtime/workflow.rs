use serde::{Deserialize, Serialize};
use serde_json::Value;
use shacs_utils::evaluator::stable_sha256_digest;
use shacs_utils::evaluator::{EvidenceRef, RedactionStatus};
use std::collections::BTreeSet;

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
    evidence_ref.owner_spec.as_deref() == Some("023")
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
