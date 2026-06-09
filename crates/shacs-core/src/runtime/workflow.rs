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
        } else if matching
            .iter()
            .any(|verdict| verdict.verdict != WorkflowVerifierVerdictKind::Pass)
        {
            failing.push(verifier.target_child_id.clone());
        }
    }

    if !missing.is_empty() {
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

pub fn workflow_synthesis_outcome(
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
    let verifier_allows_success = !merge_policy.require_verifier_pass
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
                    branch_name: format!("workflow/{}", request.child_id),
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
                    branch_name: format!("workflow/{}", request.child_id),
                }
            } else {
                WorkflowWorktreeDecision::NotRequired
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum WorkflowBudgetDecision {
    Allowed { remaining_tokens: Option<u64> },
    Blocked { reason: String },
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
    if usage.child_runs >= policy.max_iterations {
        return WorkflowBudgetDecision::Blocked {
            reason: "workflow iteration budget exhausted".to_owned(),
        };
    }
    if let Some(max_heavy_commands) = policy.max_heavy_commands {
        if usage.heavy_commands >= max_heavy_commands {
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
        schema_label: "023WorkflowProjection".to_owned(),
        schema_version: "023WorkflowProjection.v1".to_owned(),
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
        evidence_refs: evidence_refs
            .into_iter()
            .filter(workflow_evidence_ref_valid)
            .collect(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSpec023ReleaseEvidenceBucket {
    Prd000StateHarnessPlan,
    Prd001PatternChildGraph,
    Prd002VerifierReview,
    Prd003WorktreeMerge,
    Prd004BudgetModelRouting,
    Prd005SkillRecipes,
    Prd006QuarantinePermissions,
    Prd007ResumeReplayDiagnostics,
    Prd008ProjectionReleaseGate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSpec023ReleaseEvidence {
    pub bucket: WorkflowSpec023ReleaseEvidenceBucket,
    #[serde(default)]
    pub test_names: Vec<String>,
    #[serde(default)]
    pub manual_qa_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSpec023ReleaseEvidenceChecklist {
    pub required_buckets: Vec<WorkflowSpec023ReleaseEvidenceBucket>,
    pub covered_buckets: Vec<WorkflowSpec023ReleaseEvidenceBucket>,
    pub missing_buckets: Vec<WorkflowSpec023ReleaseEvidenceBucket>,
    pub passed: bool,
}

impl WorkflowSpec023ReleaseEvidenceBucket {
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
        ]
    }
}

pub fn workflow_spec023_release_evidence_checklist(
    evidence: &[WorkflowSpec023ReleaseEvidence],
) -> WorkflowSpec023ReleaseEvidenceChecklist {
    let required_buckets = WorkflowSpec023ReleaseEvidenceBucket::required_buckets();
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

    WorkflowSpec023ReleaseEvidenceChecklist {
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
