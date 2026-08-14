use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_eval::evaluator::{
    EvaluationTriggerSource, EvaluatorKind, EvaluatorRequestEnvelope, EvidenceRef,
    FrozenEvaluationSnapshot, RedactionStatus, SuggestedNextAction, TaskOutcomeClass,
};
use shacs_session::Session;

use super::goal_accounting::{
    ensure_legal_transition, transition_fact, GoalObservedState, GoalStopReason,
    GoalTransitionError, GoalTransitionFact, GoalTransitionKind,
};

pub const PERSISTENT_GOAL_METADATA_KEY: &str = "persistent_goal";
pub const GOAL_TRANSITION_HISTORY_METADATA_KEY: &str = "goal_transition_history";
pub const DEFAULT_GOAL_TURN_BUDGET: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistentGoal {
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub status: PersistentGoalStatus,
    pub created_at: String,
    pub updated_at: String,
    pub turn_budget: u32,
    pub turns_used: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verdict: Option<GoalCompletionVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition: Option<GoalTransitionFact>,
    #[serde(default)]
    pub transitions: Vec<GoalTransitionFact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentGoalStatus {
    Active,
    Paused,
    Blocked,
    Done,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalCompletionVerdict {
    Done,
    Continue,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalContinuationDecision {
    Continue { remaining_turns: u32 },
    Stop(GoalContinuationStopReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalContinuationStopReason {
    NoGoal,
    UserInterrupted,
    NotActive(PersistentGoalStatus),
    TurnBudgetExhausted,
    LastVerdictDone,
    LastVerdictBlocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalEvaluationRequest {
    pub snapshot: FrozenEvaluationSnapshot,
    pub request: EvaluatorRequestEnvelope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluatorDecisionInput {
    pub verdict_id: String,
    pub evaluator_kind: EvaluatorKind,
    pub evaluator_version: String,
    pub source_ledger_ref: String,
    pub frozen_snapshot_digest: String,
    pub current_target_snapshot_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    pub suggested_action: SuggestedNextAction,
    pub confidence: f32,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub redaction_status: RedactionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_goal_completion_verdict: Option<GoalCompletionVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unblock_hint: Option<String>,
    pub created_at_ms: u64,
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseding_verdict_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_outcome_class: Option<TaskOutcomeClass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerConsumptionStatus {
    Pending,
    Consumed,
    DiscardedStale,
    DiscardedExpired,
    Superseded,
    BlockedByPolicy,
    FailedToApply,
}

impl LedgerConsumptionStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerConsumptionRecord {
    pub consumption_id: String,
    pub ledger_ref: String,
    pub consumer_id: String,
    pub idempotency_key: String,
    pub verdict_id: String,
    pub status: LedgerConsumptionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDecisionKind {
    GoalCompletion,
    GoalContinuation,
    Capability,
    TaskOutcome,
    NoEffect,
    FailedToApply,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSelectedAction {
    None,
    ContinueGoal,
    CompleteGoal,
    BlockGoal,
    ApplyCapability,
    VerifyTaskOutcome,
    RollbackTaskOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePolicyGateResults {
    pub now_ms: u64,
    pub permission_gate_passed: bool,
    pub approval_gate_passed: bool,
    pub recursion_guard_passed: bool,
    pub user_interrupted: bool,
    pub runtime_cancelled: bool,
    pub owner_primitive_ready: bool,
    pub checkpoint_ready: bool,
}

impl RuntimePolicyGateResults {
    pub fn all_passed() -> Self {
        Self {
            now_ms: 0,
            permission_gate_passed: true,
            approval_gate_passed: true,
            recursion_guard_passed: true,
            user_interrupted: false,
            runtime_cancelled: false,
            owner_primitive_ready: true,
            checkpoint_ready: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeContinuationDecision {
    pub goal_id: String,
    pub source_verdict_id: String,
    pub source_turn_id: String,
    pub expected_turns_used: u32,
    pub remaining_turns: u32,
    pub user_interrupted: bool,
    pub recursion_guard_passed: bool,
    pub permission_gate_passed: bool,
    pub runtime_cancelled: bool,
    pub final_action: RuntimeSelectedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleVerdictRecord {
    pub verdict_id: String,
    pub expected_digest: String,
    pub current_digest: String,
    pub discard_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseding_verdict_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeDecisionRecord {
    pub decision_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub decision_kind: RuntimeDecisionKind,
    pub policy_gate_results: RuntimePolicyGateResults,
    pub selected_action: RuntimeSelectedAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unblock_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_goal_state: Option<PersistentGoal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<RuntimeContinuationDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_verdict: Option<StaleVerdictRecord>,
    pub source_ledger_ref: String,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
    pub created_at_ms: u64,
    pub correlation_id: String,
}

#[derive(Debug)]
pub enum GoalMetadataError {
    Serialize(serde_json::Error),
    SnapshotDigest(serde_json::Error),
}

impl std::fmt::Display for GoalMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialize(error) => {
                write!(formatter, "goal metadata could not serialize: {error}")
            }
            Self::SnapshotDigest(error) => {
                write!(formatter, "goal snapshot digest failed: {error}")
            }
        }
    }
}

impl std::error::Error for GoalMetadataError {}

impl PersistentGoal {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            PersistentGoalStatus::Done | PersistentGoalStatus::Cleared
        )
    }
}

pub fn evaluator_consumption_idempotency_key(input: &EvaluatorDecisionInput) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.verdict_id.as_bytes());
    let digest = hasher.finalize();
    format!(
        "eval-consume-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
    )
}

pub fn consume_evaluator_decision(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    existing_records: &[LedgerConsumptionRecord],
    gates: &RuntimePolicyGateResults,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    let idempotency_key = evaluator_consumption_idempotency_key(input);
    if let Some(record) = existing_records
        .iter()
        .find(|record| record.idempotency_key == idempotency_key && record.status.is_terminal())
    {
        return (
            runtime_decision(
                input,
                current_goal,
                gates,
                RuntimeDecisionKind::NoEffect,
                RuntimeSelectedAction::None,
                Some("verdict already terminal-consumed".to_owned()),
            ),
            record.clone(),
        );
    }

    if input.source_ledger_ref.is_empty() && input.evidence_refs.is_empty() {
        return terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::FailedToApply,
            RuntimeDecisionKind::FailedToApply,
            RuntimeSelectedAction::None,
            Some("runtime decision requires ledger or session evidence".to_owned()),
        );
    }

    if let Some(superseding_verdict_ref) = &input.superseding_verdict_ref {
        let mut decision = runtime_decision(
            input,
            current_goal,
            gates,
            RuntimeDecisionKind::NoEffect,
            RuntimeSelectedAction::None,
            Some("verdict superseded before consumption".to_owned()),
        );
        decision.stale_verdict = Some(StaleVerdictRecord {
            verdict_id: input.verdict_id.clone(),
            expected_digest: input.frozen_snapshot_digest.clone(),
            current_digest: input.current_target_snapshot_digest.clone(),
            discard_reason: "superseded".to_owned(),
            superseding_verdict_ref: Some(superseding_verdict_ref.clone()),
        });
        return with_record(input, decision, LedgerConsumptionStatus::Superseded);
    }

    if input.frozen_snapshot_digest != input.current_target_snapshot_digest {
        let mut decision = runtime_decision(
            input,
            current_goal,
            gates,
            RuntimeDecisionKind::NoEffect,
            RuntimeSelectedAction::None,
            Some("verdict target snapshot digest is stale".to_owned()),
        );
        decision.stale_verdict = Some(StaleVerdictRecord {
            verdict_id: input.verdict_id.clone(),
            expected_digest: input.frozen_snapshot_digest.clone(),
            current_digest: input.current_target_snapshot_digest.clone(),
            discard_reason: "snapshot_digest_mismatch".to_owned(),
            superseding_verdict_ref: None,
        });
        return with_record(input, decision, LedgerConsumptionStatus::DiscardedStale);
    }

    if input
        .expires_at_ms
        .is_some_and(|expires_at_ms| gates.now_ms > expires_at_ms)
    {
        return terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::DiscardedExpired,
            RuntimeDecisionKind::NoEffect,
            RuntimeSelectedAction::None,
            Some("verdict expired before runtime consumption".to_owned()),
        );
    }

    match input.evaluator_kind {
        EvaluatorKind::GoalCompletion => consume_goal_completion(input, current_goal, gates),
        EvaluatorKind::SafetyCapability => consume_capability(input, current_goal, gates),
        EvaluatorKind::TaskOutcome => consume_task_outcome(input, current_goal, gates),
        EvaluatorKind::Replay | EvaluatorKind::RedactionCheck => terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::Consumed,
            RuntimeDecisionKind::NoEffect,
            RuntimeSelectedAction::None,
            None,
        ),
    }
}

fn consume_goal_completion(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    let Some(verdict) = input.explicit_goal_completion_verdict else {
        return terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::FailedToApply,
            RuntimeDecisionKind::FailedToApply,
            RuntimeSelectedAction::None,
            Some("goal completion verdict must be explicit runtime input".to_owned()),
        );
    };

    match verdict {
        GoalCompletionVerdict::Continue => consume_goal_continue(input, current_goal, gates),
        GoalCompletionVerdict::Done => consume_goal_terminal(
            input,
            current_goal,
            gates,
            GoalCompletionVerdict::Done,
            RuntimeSelectedAction::CompleteGoal,
        ),
        GoalCompletionVerdict::Blocked => consume_goal_terminal(
            input,
            current_goal,
            gates,
            GoalCompletionVerdict::Blocked,
            RuntimeSelectedAction::BlockGoal,
        ),
    }
}

fn consume_goal_continue(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    let Some(goal) = current_goal else {
        return blocked_by_policy(input, current_goal, gates, "no active persistent goal");
    };
    if input
        .goal_id
        .as_deref()
        .is_some_and(|goal_id| goal_id != goal.id)
    {
        return blocked_by_policy(
            input,
            current_goal,
            gates,
            "verdict goal id does not match active goal",
        );
    }
    if gates.runtime_cancelled {
        return blocked_by_policy(input, current_goal, gates, "runtime is cancelled");
    }
    if !gates.permission_gate_passed {
        return blocked_by_policy(
            input,
            current_goal,
            gates,
            "permission gate blocked continuation",
        );
    }
    if !gates.recursion_guard_passed {
        return blocked_by_policy(
            input,
            current_goal,
            gates,
            "recursion guard blocked continuation",
        );
    }

    match continuation_decision(Some(goal), gates.user_interrupted) {
        GoalContinuationDecision::Continue { remaining_turns } => {
            let mut decision = runtime_decision(
                input,
                current_goal,
                gates,
                RuntimeDecisionKind::GoalContinuation,
                RuntimeSelectedAction::ContinueGoal,
                None,
            );
            decision.next_goal_state = apply_completion_verdict(
                goal,
                GoalCompletionVerdict::Continue,
                None,
                gates.now_ms.to_string(),
            )
            .ok();
            decision.continuation = Some(RuntimeContinuationDecision {
                goal_id: goal.id.clone(),
                source_verdict_id: input.verdict_id.clone(),
                source_turn_id: input.turn_id.clone().unwrap_or_default(),
                expected_turns_used: goal.turns_used.saturating_add(1),
                remaining_turns,
                user_interrupted: gates.user_interrupted,
                recursion_guard_passed: gates.recursion_guard_passed,
                permission_gate_passed: gates.permission_gate_passed,
                runtime_cancelled: gates.runtime_cancelled,
                final_action: RuntimeSelectedAction::ContinueGoal,
            });
            with_record(input, decision, LedgerConsumptionStatus::Consumed)
        }
        GoalContinuationDecision::Stop(reason) => blocked_by_policy(
            input,
            current_goal,
            gates,
            &format!("continuation blocked: {reason:?}"),
        ),
    }
}

fn consume_goal_terminal(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
    verdict: GoalCompletionVerdict,
    action: RuntimeSelectedAction,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    if gates.user_interrupted || gates.runtime_cancelled {
        return blocked_by_policy(input, current_goal, gates, "runtime is interrupted");
    }
    let Some(goal) = current_goal else {
        return blocked_by_policy(input, current_goal, gates, "no persistent goal to update");
    };
    if input
        .goal_id
        .as_deref()
        .is_some_and(|goal_id| goal_id != goal.id)
    {
        return blocked_by_policy(
            input,
            current_goal,
            gates,
            "verdict goal id does not match active goal",
        );
    }

    let mut decision = runtime_decision(
        input,
        current_goal,
        gates,
        RuntimeDecisionKind::GoalCompletion,
        action,
        input.blocked_reason.clone(),
    );
    let Ok(next_goal) = apply_completion_verdict(
        goal,
        verdict,
        input.blocked_reason.clone(),
        gates.now_ms.to_string(),
    ) else {
        return blocked_by_policy(input, current_goal, gates, "illegal goal transition");
    };
    decision.next_goal_state = Some(next_goal);
    if verdict == GoalCompletionVerdict::Blocked {
        decision.unblock_hint = input
            .unblock_hint
            .clone()
            .or_else(|| Some("Resolve the blocked reason, then resume the goal.".to_owned()));
    }
    with_record(input, decision, LedgerConsumptionStatus::Consumed)
}

fn consume_capability(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    if gates.approval_gate_passed && gates.permission_gate_passed {
        terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::Consumed,
            RuntimeDecisionKind::Capability,
            RuntimeSelectedAction::ApplyCapability,
            None,
        )
    } else {
        blocked_by_policy(
            input,
            current_goal,
            gates,
            "capability requires approval and permission gates",
        )
    }
}

fn consume_task_outcome(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    match input.task_outcome_class {
        Some(TaskOutcomeClass::Verify) if gates.owner_primitive_ready => terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::Consumed,
            RuntimeDecisionKind::TaskOutcome,
            RuntimeSelectedAction::VerifyTaskOutcome,
            None,
        ),
        Some(TaskOutcomeClass::Rollback)
            if gates.owner_primitive_ready && gates.checkpoint_ready =>
        {
            terminal_decision(
                input,
                current_goal,
                gates,
                LedgerConsumptionStatus::Consumed,
                RuntimeDecisionKind::TaskOutcome,
                RuntimeSelectedAction::RollbackTaskOutcome,
                None,
            )
        }
        Some(TaskOutcomeClass::Verify | TaskOutcomeClass::Rollback) => blocked_by_policy(
            input,
            current_goal,
            gates,
            "task outcome owner primitive is not ready",
        ),
        _ => terminal_decision(
            input,
            current_goal,
            gates,
            LedgerConsumptionStatus::Consumed,
            RuntimeDecisionKind::TaskOutcome,
            RuntimeSelectedAction::None,
            None,
        ),
    }
}

fn blocked_by_policy(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
    reason: &str,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    terminal_decision(
        input,
        current_goal,
        gates,
        LedgerConsumptionStatus::BlockedByPolicy,
        RuntimeDecisionKind::NoEffect,
        RuntimeSelectedAction::None,
        Some(reason.to_owned()),
    )
}

fn terminal_decision(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
    status: LedgerConsumptionStatus,
    kind: RuntimeDecisionKind,
    action: RuntimeSelectedAction,
    reason: Option<String>,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    let decision = runtime_decision(input, current_goal, gates, kind, action, reason);
    with_record(input, decision, status)
}

fn with_record(
    input: &EvaluatorDecisionInput,
    decision: RuntimeDecisionRecord,
    status: LedgerConsumptionStatus,
) -> (RuntimeDecisionRecord, LedgerConsumptionRecord) {
    let record = LedgerConsumptionRecord {
        consumption_id: format!(
            "consumption-{}",
            evaluator_consumption_idempotency_key(input)
        ),
        ledger_ref: input.source_ledger_ref.clone(),
        consumer_id: "shacs-runtime".to_owned(),
        idempotency_key: evaluator_consumption_idempotency_key(input),
        verdict_id: input.verdict_id.clone(),
        status,
        decision_ref: Some(decision.decision_id.clone()),
        reason: decision.blocked_reason.clone(),
        created_at_ms: input.created_at_ms,
        completed_at_ms: Some(input.created_at_ms),
    };
    (decision, record)
}

fn runtime_decision(
    input: &EvaluatorDecisionInput,
    current_goal: Option<&PersistentGoal>,
    gates: &RuntimePolicyGateResults,
    kind: RuntimeDecisionKind,
    action: RuntimeSelectedAction,
    blocked_reason: Option<String>,
) -> RuntimeDecisionRecord {
    RuntimeDecisionRecord {
        decision_id: format!(
            "runtime-decision-{}",
            evaluator_consumption_idempotency_key(input)
        ),
        session_id: current_goal
            .map(|goal| goal.session_id.clone())
            .unwrap_or_default(),
        goal_id: input
            .goal_id
            .clone()
            .or_else(|| current_goal.map(|goal| goal.id.clone())),
        turn_id: input.turn_id.clone(),
        decision_kind: kind,
        policy_gate_results: gates.clone(),
        selected_action: action,
        blocked_reason,
        unblock_hint: input.unblock_hint.clone(),
        projection_ref: Some(format!("projection:{}", input.verdict_id)),
        next_goal_state: None,
        continuation: None,
        stale_verdict: None,
        source_ledger_ref: input.source_ledger_ref.clone(),
        evidence_refs: input.evidence_refs.clone(),
        created_at_ms: input.created_at_ms,
        correlation_id: input.correlation_id.clone(),
    }
}

pub fn create_persistent_goal(
    session_id: impl Into<String>,
    text: impl Into<String>,
    now: impl Into<String>,
    turn_budget: u32,
) -> PersistentGoal {
    let session_id = session_id.into();
    let text = text.into();
    let now = now.into();
    let id = goal_id(&session_id, &text, &now);
    let mut goal = PersistentGoal {
        id,
        session_id,
        text,
        status: PersistentGoalStatus::Active,
        created_at: now.clone(),
        updated_at: now.clone(),
        turn_budget,
        turns_used: 0,
        last_verdict: None,
        blocked_reason: None,
        last_transition: None,
        transitions: Vec::new(),
    };
    let fact = transition_fact(
        &goal,
        GoalObservedState::Unavailable,
        GoalStopReason::GoalSet,
        now,
    );
    goal.last_transition = Some(fact.clone());
    goal.transitions.push(fact);
    goal
}

pub fn pause_goal(
    goal: &PersistentGoal,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    ensure_legal_transition(goal.status, GoalTransitionKind::Pause)?;
    let now = now.into();
    let mut next = goal.clone();
    next.status = PersistentGoalStatus::Paused;
    next.updated_at = now.clone();
    append_transition(
        &mut next,
        goal.status.into(),
        GoalStopReason::PausedByUser,
        now,
    );
    Ok(next)
}

pub fn resume_goal(
    goal: &PersistentGoal,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    ensure_legal_transition(goal.status, GoalTransitionKind::Resume)?;
    let now = now.into();
    let mut next = goal.clone();
    next.status = PersistentGoalStatus::Active;
    next.blocked_reason = None;
    next.updated_at = now.clone();
    append_transition(
        &mut next,
        goal.status.into(),
        GoalStopReason::ResumedByUser,
        now,
    );
    Ok(next)
}

pub fn clear_goal(
    goal: &PersistentGoal,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    ensure_legal_transition(goal.status, GoalTransitionKind::Clear)?;
    let now = now.into();
    let mut next = goal.clone();
    next.status = PersistentGoalStatus::Cleared;
    next.updated_at = now.clone();
    append_transition(
        &mut next,
        goal.status.into(),
        GoalStopReason::ClearedByUser,
        now,
    );
    Ok(next)
}

pub fn mark_goal_done(
    goal: &PersistentGoal,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    ensure_legal_transition(goal.status, GoalTransitionKind::MarkDoneByUser)?;
    terminal_goal(
        goal,
        GoalCompletionVerdict::Done,
        None,
        GoalStopReason::MarkedDoneByUser,
        now,
    )
}

pub fn mark_goal_blocked(
    goal: &PersistentGoal,
    reason: impl Into<String>,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    ensure_legal_transition(goal.status, GoalTransitionKind::BlockByUser)?;
    terminal_goal(
        goal,
        GoalCompletionVerdict::Blocked,
        Some(reason.into()),
        GoalStopReason::BlockedByUser,
        now,
    )
}

fn append_transition(
    goal: &mut PersistentGoal,
    prior_state: GoalObservedState,
    stop_reason: GoalStopReason,
    observed_at: String,
) {
    let fact = transition_fact(goal, prior_state, stop_reason, observed_at);
    goal.last_transition = Some(fact.clone());
    goal.transitions.push(fact);
}

fn terminal_goal(
    goal: &PersistentGoal,
    verdict: GoalCompletionVerdict,
    blocked_reason: Option<String>,
    stop_reason: GoalStopReason,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    let now = now.into();
    let mut next = goal.clone();
    next.status = match verdict {
        GoalCompletionVerdict::Done => PersistentGoalStatus::Done,
        GoalCompletionVerdict::Blocked => PersistentGoalStatus::Blocked,
        GoalCompletionVerdict::Continue => PersistentGoalStatus::Active,
    };
    next.last_verdict = Some(verdict);
    next.blocked_reason = blocked_reason;
    next.updated_at = now.clone();
    append_transition(&mut next, goal.status.into(), stop_reason, now);
    Ok(next)
}

pub fn record_goal_stop(
    goal: &PersistentGoal,
    stop_reason: GoalStopReason,
    user_interrupted: bool,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    let kind = match stop_reason {
        GoalStopReason::UserInterrupted => GoalTransitionKind::UserInterrupted,
        GoalStopReason::ContinuationBudgetExhausted => {
            GoalTransitionKind::ContinuationBudgetExhausted
        }
        _ => GoalTransitionKind::EvaluatorContinue,
    };
    ensure_legal_transition(goal.status, kind)?;
    let mut next = goal.clone();
    let mut fact = transition_fact(&next, goal.status.into(), stop_reason, now.into());
    fact.user_interrupted = user_interrupted;
    next.last_transition = Some(fact.clone());
    next.transitions.push(fact);
    Ok(next)
}

pub fn apply_completion_verdict(
    goal: &PersistentGoal,
    verdict: GoalCompletionVerdict,
    blocked_reason: Option<String>,
    now: impl Into<String>,
) -> Result<PersistentGoal, GoalTransitionError> {
    let now = now.into();
    match verdict {
        GoalCompletionVerdict::Done => {
            ensure_legal_transition(goal.status, GoalTransitionKind::EvaluatorDone)?;
            terminal_goal(
                goal,
                verdict,
                None,
                GoalStopReason::EvaluatorCompletionAccepted,
                now,
            )
        }
        GoalCompletionVerdict::Blocked => {
            ensure_legal_transition(goal.status, GoalTransitionKind::EvaluatorBlocked)?;
            terminal_goal(
                goal,
                verdict,
                Some(
                    blocked_reason.unwrap_or_else(|| {
                        "Goal completion evaluator reported blocked.".to_owned()
                    }),
                ),
                GoalStopReason::EvaluatorBlocked,
                now,
            )
        }
        GoalCompletionVerdict::Continue => {
            ensure_legal_transition(goal.status, GoalTransitionKind::EvaluatorContinue)?;
            let mut next = goal.clone();
            next.status = PersistentGoalStatus::Active;
            next.turns_used = next.turns_used.saturating_add(1);
            next.last_verdict = Some(GoalCompletionVerdict::Continue);
            next.blocked_reason = None;
            next.updated_at = now.clone();
            append_transition(
                &mut next,
                goal.status.into(),
                GoalStopReason::EvaluatorContinuationAccepted,
                now,
            );
            Ok(next)
        }
    }
}

pub fn continuation_decision(
    goal: Option<&PersistentGoal>,
    user_interrupted: bool,
) -> GoalContinuationDecision {
    let Some(goal) = goal else {
        return GoalContinuationDecision::Stop(GoalContinuationStopReason::NoGoal);
    };
    if user_interrupted {
        return GoalContinuationDecision::Stop(GoalContinuationStopReason::UserInterrupted);
    }
    if goal.status != PersistentGoalStatus::Active {
        return GoalContinuationDecision::Stop(GoalContinuationStopReason::NotActive(goal.status));
    }
    match goal.last_verdict {
        Some(GoalCompletionVerdict::Done) => {
            return GoalContinuationDecision::Stop(GoalContinuationStopReason::LastVerdictDone);
        }
        Some(GoalCompletionVerdict::Blocked) => {
            return GoalContinuationDecision::Stop(GoalContinuationStopReason::LastVerdictBlocked);
        }
        _ => {}
    }
    if goal.turns_used >= goal.turn_budget {
        return GoalContinuationDecision::Stop(GoalContinuationStopReason::TurnBudgetExhausted);
    }
    GoalContinuationDecision::Continue {
        remaining_turns: goal.turn_budget.saturating_sub(goal.turns_used),
    }
}

pub fn persistent_goal_from_session(session: &Session) -> Option<PersistentGoal> {
    session
        .metadata
        .get(PERSISTENT_GOAL_METADATA_KEY)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub fn store_persistent_goal(
    session: &mut Session,
    goal: &PersistentGoal,
) -> Result<(), GoalMetadataError> {
    let value = serde_json::to_value(goal).map_err(GoalMetadataError::Serialize)?;
    session
        .metadata
        .insert(PERSISTENT_GOAL_METADATA_KEY.to_owned(), value);
    let history = session
        .metadata
        .entry(GOAL_TRANSITION_HISTORY_METADATA_KEY.to_owned())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if let Some(history) = history.as_array_mut() {
        for transition in &goal.transitions {
            let value = serde_json::to_value(transition).map_err(GoalMetadataError::Serialize)?;
            if !history.contains(&value) {
                history.push(value);
            }
        }
    }
    Ok(())
}

pub fn remove_persistent_goal(session: &mut Session) {
    session.metadata.remove(PERSISTENT_GOAL_METADATA_KEY);
}

pub fn build_goal_completion_evaluation_request(
    goal: &PersistentGoal,
    source: EvaluationTriggerSource,
    created_at_ms: u64,
) -> Result<GoalEvaluationRequest, GoalMetadataError> {
    let correlation_id = format!("goal:{}:{}", goal.session_id, goal.id);
    let payload = json!({
        "goal": goal,
        "allowed_verdicts": ["done", "continue", "blocked"],
    });
    let mut snapshot = FrozenEvaluationSnapshot::new_redacted(
        format!("goal-snapshot-{}-{}", goal.id, created_at_ms),
        created_at_ms,
        correlation_id.clone(),
        "default",
        &payload,
    );
    snapshot.session_id = Some(goal.session_id.clone());
    let snapshot_digest = snapshot
        .digest()
        .map_err(GoalMetadataError::SnapshotDigest)?;
    let request = EvaluatorRequestEnvelope {
        request_id: format!("goal-eval-{}-{}", goal.id, created_at_ms),
        evaluator_kind: EvaluatorKind::GoalCompletion,
        correlation_id,
        session_id: Some(goal.session_id.clone()),
        turn_id: None,
        source,
        snapshot_digest,
        redaction_profile: snapshot.redaction_profile.clone(),
        caller_intent: "Advisory goal completion evaluation; do not execute follow-up turns."
            .to_owned(),
    };
    Ok(GoalEvaluationRequest { snapshot, request })
}

fn safe_goal_id_part(input: &str) -> String {
    input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn goal_id(session_id: &str, text: &str, created_at: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    hasher.update([0]);
    hasher.update(created_at.as_bytes());
    let digest = hasher.finalize();
    format!(
        "goal-{}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        safe_goal_id_part(session_id),
        digest[0],
        digest[1],
        digest[2],
        digest[3],
        digest[4],
        digest[5]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_lifecycle_updates_state_without_losing_goal_identity() {
        let goal = create_persistent_goal("session-1", "ship it", "now", 2);
        let paused = pause_goal(&goal, "later").expect("active goal pauses");
        assert_eq!(paused.id, goal.id);
        assert_eq!(paused.status, PersistentGoalStatus::Paused);

        let resumed = resume_goal(&paused, "later-2").expect("paused goal resumes");
        assert_eq!(resumed.status, PersistentGoalStatus::Active);

        let continued =
            apply_completion_verdict(&resumed, GoalCompletionVerdict::Continue, None, "later-3")
                .expect("active goal continues");
        assert_eq!(continued.turns_used, 1);
        assert_eq!(
            continued.last_verdict,
            Some(GoalCompletionVerdict::Continue)
        );

        let blocked =
            mark_goal_blocked(&continued, "needs input", "later-4").expect("active goal blocks");
        assert_eq!(blocked.status, PersistentGoalStatus::Blocked);
        assert_eq!(blocked.blocked_reason.as_deref(), Some("needs input"));
    }

    #[test]
    fn continuation_decision_respects_user_interruption_and_budget() {
        let mut goal = create_persistent_goal("session-1", "ship it", "now", 1);
        assert_eq!(
            continuation_decision(Some(&goal), true),
            GoalContinuationDecision::Stop(GoalContinuationStopReason::UserInterrupted)
        );
        assert_eq!(
            continuation_decision(Some(&goal), false),
            GoalContinuationDecision::Continue { remaining_turns: 1 }
        );
        goal.turns_used = 1;
        assert_eq!(
            continuation_decision(Some(&goal), false),
            GoalContinuationDecision::Stop(GoalContinuationStopReason::TurnBudgetExhausted)
        );
    }

    #[test]
    fn evaluator_request_is_advisory_goal_completion() -> Result<(), Box<dyn std::error::Error>> {
        let goal = create_persistent_goal("session-1", "ship it", "now", 2);
        let request = build_goal_completion_evaluation_request(
            &goal,
            EvaluationTriggerSource::SessionTurn,
            42,
        )?;
        assert_eq!(
            request.request.evaluator_kind,
            EvaluatorKind::GoalCompletion
        );
        assert!(!request.request.grants_execution_authority());
        assert_eq!(request.snapshot.session_id.as_deref(), Some("session-1"));
        Ok(())
    }

    #[test]
    fn stores_goal_in_session_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let mut session = Session::new("session-1");
        let goal = create_persistent_goal("session-1", "ship it", "now", 2);
        store_persistent_goal(&mut session, &goal)?;
        assert_eq!(persistent_goal_from_session(&session), Some(goal));
        remove_persistent_goal(&mut session);
        assert_eq!(persistent_goal_from_session(&session), None);
        Ok(())
    }
}
