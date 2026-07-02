use crate::runtime::{
    build_subagent_tool_registry, CancellationToken, ChildResultEnvelope, ChildResultStatus,
    SubagentExecutionConfig,
};
use std::path::PathBuf;

use shacs_workflow::{
    admit_workflow_plan, decide_workflow_admission, workflow_barrier_decision,
    workflow_diagnostics_manifest, workflow_synthesis_outcome, workflow_verification_gate,
    WorkflowAdmissionDecision, WorkflowAdmissionInput, WorkflowBarrierDecision,
    WorkflowBudgetPolicy, WorkflowChildResult, WorkflowChildRunStatus, WorkflowDiagnosticsManifest,
    WorkflowHarnessPlan, WorkflowRunRecord, WorkflowRunState, WorkflowSynthesisOutcome,
    WorkflowVerificationGate, WorkflowVerifierVerdict,
};

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeWorkflowInput {
    pub plan: WorkflowHarnessPlan,
    pub child_results: Vec<ChildResultEnvelope>,
    pub verifier_verdicts: Vec<WorkflowVerifierVerdict>,
    pub child_workspace: PathBuf,
    pub child_model: String,
    pub admitted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeWorkflowEvent {
    pub phase: &'static str,
    pub workflow_id: String,
    pub child_id: Option<String>,
    pub verifier_id: Option<String>,
    pub state: WorkflowRunState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeWorkflowOutcome {
    pub run: WorkflowRunRecord,
    pub events: Vec<RuntimeWorkflowEvent>,
    pub child_tool_names: Vec<String>,
    pub child_results: Vec<WorkflowChildResult>,
    pub barrier_decision: WorkflowBarrierDecision,
    pub verification_gate: WorkflowVerificationGate,
    pub synthesis_outcome: WorkflowSynthesisOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeWorkflowAdmissionBranchInput {
    pub admission: WorkflowAdmissionInput,
    pub plan: WorkflowHarnessPlan,
    pub child_results: Vec<ChildResultEnvelope>,
    pub verifier_verdicts: Vec<WorkflowVerifierVerdict>,
    pub child_workspace: PathBuf,
    pub child_model: String,
    pub admitted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeWorkflowAdmissionBranchOutcome {
    RegularLoop,
    QuickWorkflow { reason: String },
    AskUser { question: String },
    Blocked { reasons: Vec<String> },
    DynamicWorkflow(Box<RuntimeWorkflowOutcome>),
}

#[derive(Debug, Clone)]
pub struct RuntimeWorkflowExecutionHandle {
    pub workflow_id: String,
    pub parent_session_key: String,
    pub child_ids: Vec<String>,
    pub budget_snapshot: WorkflowBudgetPolicy,
    pub cancellation_token: CancellationToken,
}

#[derive(Debug, Clone)]
pub struct RuntimeWorkflowInterruptOutcome {
    pub run: WorkflowRunRecord,
    pub events: Vec<RuntimeWorkflowEvent>,
    pub cancelled_child_ids: Vec<String>,
    pub cancellation_requested: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeWorkflowDiagnostics {
    pub manifest: WorkflowDiagnosticsManifest,
    pub event_phases: Vec<String>,
    pub terminal_state: WorkflowRunState,
    pub child_result_count: usize,
    pub verifier_status: String,
    pub replay_live_actions_allowed: bool,
}

pub fn run_runtime_workflow_admission_branch(
    input: RuntimeWorkflowAdmissionBranchInput,
) -> Result<RuntimeWorkflowAdmissionBranchOutcome, serde_json::Error> {
    match decide_workflow_admission(&input.admission) {
        WorkflowAdmissionDecision::UseRegularLoop => {
            Ok(RuntimeWorkflowAdmissionBranchOutcome::RegularLoop)
        }
        WorkflowAdmissionDecision::UseQuickWorkflow { reason } => {
            Ok(RuntimeWorkflowAdmissionBranchOutcome::QuickWorkflow { reason })
        }
        WorkflowAdmissionDecision::AskUserForScope { question } => {
            Ok(RuntimeWorkflowAdmissionBranchOutcome::AskUser { question })
        }
        WorkflowAdmissionDecision::BlockedByPolicy { reasons } => {
            Ok(RuntimeWorkflowAdmissionBranchOutcome::Blocked { reasons })
        }
        WorkflowAdmissionDecision::UseDynamicWorkflow { .. } => {
            let outcome = run_read_only_runtime_workflow(RuntimeWorkflowInput {
                plan: input.plan,
                child_results: input.child_results,
                verifier_verdicts: input.verifier_verdicts,
                child_workspace: input.child_workspace,
                child_model: input.child_model,
                admitted_at_ms: input.admitted_at_ms,
            })?;
            Ok(RuntimeWorkflowAdmissionBranchOutcome::DynamicWorkflow(
                Box::new(outcome),
            ))
        }
    }
}

pub fn run_read_only_runtime_workflow(
    input: RuntimeWorkflowInput,
) -> Result<RuntimeWorkflowOutcome, serde_json::Error> {
    let RuntimeWorkflowInput {
        plan,
        child_results,
        verifier_verdicts,
        child_workspace,
        child_model,
        admitted_at_ms,
    } = input;
    let mut run = admit_workflow_plan(&plan, admitted_at_ms)?;
    let mut events = vec![workflow_event(&run, "admitted", None, None)];

    let child_tool_names = scoped_read_only_child_tool_names(&plan, child_workspace, child_model);
    let workflow_child_results = child_results
        .iter()
        .map(|result| {
            run.state = WorkflowRunState::Running;
            events.push(workflow_event(
                &run,
                "child_started",
                Some(result.child_task_id.clone()),
                None,
            ));
            run.state = WorkflowRunState::WaitingForChildren;
            events.push(workflow_event(
                &run,
                "child_completed",
                Some(result.child_task_id.clone()),
                None,
            ));
            workflow_child_result_from_envelope(&plan, result)
        })
        .collect::<Vec<_>>();
    let verifier_verdicts = verifier_verdicts
        .into_iter()
        .map(|verdict| normalize_verifier_verdict(&plan, verdict))
        .collect::<Vec<_>>();

    let barrier_decision = workflow_barrier_decision(&plan, &workflow_child_results);
    run.state = WorkflowRunState::Verifying;
    run.updated_at_ms = admitted_at_ms;
    for verdict in &verifier_verdicts {
        events.push(workflow_event(
            &run,
            "verifier_completed",
            Some(verdict.target_child_id.clone()),
            Some(verdict.verifier_id.clone()),
        ));
    }
    let verification_gate = workflow_verification_gate(&plan, &verifier_verdicts);

    run.state = WorkflowRunState::Synthesizing;
    events.push(workflow_event(&run, "synthesizing", None, None));
    let synthesis_outcome = workflow_synthesis_outcome(
        &workflow_child_results,
        &verification_gate,
        &plan.merge_policy,
    );

    run.state = terminal_state(&barrier_decision, &synthesis_outcome);
    events.push(workflow_event(&run, "terminal", None, None));

    Ok(RuntimeWorkflowOutcome {
        run,
        events,
        child_tool_names,
        child_results: workflow_child_results,
        barrier_decision,
        verification_gate,
        synthesis_outcome,
    })
}

pub fn read_only_child_tool_names(
    child_workspace: impl Into<PathBuf>,
    child_model: impl Into<String>,
) -> Vec<String> {
    let mut config = SubagentExecutionConfig::new(child_workspace, child_model);
    config.allow_side_effect_tools = false;
    config.enable_exec = false;
    build_subagent_tool_registry(&config).tool_names()
}

pub fn runtime_workflow_execution_handle(
    plan: &WorkflowHarnessPlan,
) -> RuntimeWorkflowExecutionHandle {
    RuntimeWorkflowExecutionHandle {
        workflow_id: plan.workflow_id.clone(),
        parent_session_key: plan.origin_session_id.clone(),
        child_ids: plan
            .child_graph
            .iter()
            .map(|child| child.child_id.clone())
            .collect(),
        budget_snapshot: plan.budget_policy.clone(),
        cancellation_token: CancellationToken::new(),
    }
}

pub fn cancel_runtime_workflow(
    plan: &WorkflowHarnessPlan,
    handle: &RuntimeWorkflowExecutionHandle,
    reason: impl Into<String>,
    cancelled_at_ms: u64,
) -> Result<RuntimeWorkflowInterruptOutcome, serde_json::Error> {
    handle.cancellation_token.cancel();
    let reason = reason.into();
    let mut run = admit_workflow_plan(plan, cancelled_at_ms)?;
    run.state = WorkflowRunState::Cancelled;
    run.updated_at_ms = cancelled_at_ms;
    let mut events = handle
        .child_ids
        .iter()
        .map(|child_id| workflow_event(&run, "child_cancelled", Some(child_id.clone()), None))
        .collect::<Vec<_>>();
    events.push(workflow_event(&run, "terminal", None, None));

    Ok(RuntimeWorkflowInterruptOutcome {
        run,
        events,
        cancelled_child_ids: handle.child_ids.clone(),
        cancellation_requested: handle.cancellation_token.is_cancelled(),
        reason,
    })
}

pub fn runtime_workflow_diagnostics(
    plan: &WorkflowHarnessPlan,
    outcome: &RuntimeWorkflowOutcome,
) -> Result<RuntimeWorkflowDiagnostics, serde_json::Error> {
    let stale_result_refs = outcome
        .child_results
        .iter()
        .filter(|result| result.status == WorkflowChildRunStatus::Stale)
        .map(|result| result.child_id.clone())
        .collect::<Vec<_>>();
    let manifest = workflow_diagnostics_manifest(plan, stale_result_refs, Vec::new())?;
    Ok(RuntimeWorkflowDiagnostics {
        manifest,
        event_phases: outcome
            .events
            .iter()
            .map(|event| event.phase.to_owned())
            .collect(),
        terminal_state: outcome.run.state,
        child_result_count: outcome.child_results.len(),
        verifier_status: verifier_status(&outcome.verification_gate).to_owned(),
        replay_live_actions_allowed: false,
    })
}

fn workflow_child_result_from_envelope(
    plan: &WorkflowHarnessPlan,
    result: &ChildResultEnvelope,
) -> WorkflowChildResult {
    let child = plan
        .child_graph
        .iter()
        .find(|child| child.child_id == result.child_task_id);
    let status = child
        .filter(|_| child_result_matches_plan(plan, result))
        .map(|_| workflow_child_status(result.status.clone()))
        .unwrap_or(WorkflowChildRunStatus::Failed);
    let summary =
        if status == WorkflowChildRunStatus::Failed && !child_result_matches_plan(plan, result) {
            "discarded child result with mismatched workflow provenance".to_owned()
        } else {
            result.summary.clone()
        };

    WorkflowChildResult {
        child_id: result.child_task_id.clone(),
        step_id: step_id_for_child(plan, &result.child_task_id),
        status,
        summary,
        evidence_refs: Vec::new(),
    }
}

fn child_result_matches_plan(plan: &WorkflowHarnessPlan, result: &ChildResultEnvelope) -> bool {
    let spawn_effect_id = format!("spawn:{}", result.child_task_id);
    plan.child_graph
        .iter()
        .any(|child| child.child_id == result.child_task_id)
        && result.session_id == plan.origin_session_id
        && result.parent_turn_id == plan.origin_turn_id
        && result.spawn_effect_id == spawn_effect_id
}

fn scoped_read_only_child_tool_names(
    plan: &WorkflowHarnessPlan,
    child_workspace: PathBuf,
    child_model: String,
) -> Vec<String> {
    let read_only_tool_names = read_only_child_tool_names(child_workspace, child_model);
    plan.tool_scope_policy
        .allowed_tools
        .iter()
        .filter(|tool_name| read_only_tool_names.contains(tool_name))
        .cloned()
        .collect()
}

fn normalize_verifier_verdict(
    plan: &WorkflowHarnessPlan,
    mut verdict: WorkflowVerifierVerdict,
) -> WorkflowVerifierVerdict {
    let Some(spec) = plan
        .verifier_graph
        .iter()
        .find(|spec| spec.verifier_id == verdict.verifier_id)
    else {
        return verdict;
    };
    if spec.target_child_id == verdict.target_child_id {
        return verdict;
    }

    let actual_target = verdict.target_child_id;
    verdict.target_child_id = spec.target_child_id.clone();
    verdict.verdict = shacs_workflow::WorkflowVerifierVerdictKind::Fail;
    verdict.summary = format!(
        "verifier target mismatch: expected {}, got {}",
        spec.target_child_id, actual_target
    );
    verdict
}

fn step_id_for_child(plan: &WorkflowHarnessPlan, child_id: &str) -> String {
    plan.child_graph
        .iter()
        .find(|child| child.child_id == child_id)
        .map(|child| child.step_id.clone())
        .unwrap_or_default()
}

fn workflow_child_status(status: ChildResultStatus) -> WorkflowChildRunStatus {
    match status {
        ChildResultStatus::Completed => WorkflowChildRunStatus::Completed,
        ChildResultStatus::Failed => WorkflowChildRunStatus::Failed,
        ChildResultStatus::Cancelled => WorkflowChildRunStatus::Cancelled,
        ChildResultStatus::TimedOut => WorkflowChildRunStatus::TimedOut,
    }
}

fn terminal_state(
    barrier_decision: &WorkflowBarrierDecision,
    synthesis_outcome: &WorkflowSynthesisOutcome,
) -> WorkflowRunState {
    if matches!(barrier_decision, WorkflowBarrierDecision::Ready { .. })
        && synthesis_outcome.final_success_allowed
    {
        WorkflowRunState::Completed
    } else {
        WorkflowRunState::Failed
    }
}

fn verifier_status(gate: &WorkflowVerificationGate) -> &'static str {
    match gate {
        WorkflowVerificationGate::Passed => "passed",
        WorkflowVerificationGate::Failed { .. } => "failed",
        WorkflowVerificationGate::Blocked { .. } => "blocked",
    }
}

fn workflow_event(
    run: &WorkflowRunRecord,
    phase: &'static str,
    child_id: Option<String>,
    verifier_id: Option<String>,
) -> RuntimeWorkflowEvent {
    RuntimeWorkflowEvent {
        phase,
        workflow_id: run.workflow_id.clone(),
        child_id,
        verifier_id,
        state: run.state,
    }
}
