use crate::runtime::{
    build_subagent_tool_registry, CancellationToken, ChildResultEnvelope, ChildResultStatus,
    SpawnEnvelope, SubagentExecutionConfig, SubagentRuntime,
};
use std::path::PathBuf;

use shacs_providers::ProviderClient;
use shacs_workflow::{
    admit_workflow_plan, decide_workflow_admission, workflow_barrier_decision,
    workflow_diagnostics_manifest, workflow_permission_ceiling_decision,
    workflow_runtime_enforcement_decision, workflow_synthesis_outcome, workflow_verification_gate,
    WorkflowAdmissionDecision, WorkflowAdmissionInput, WorkflowBarrierDecision,
    WorkflowBudgetPolicy, WorkflowBudgetUsage, WorkflowChildResult, WorkflowChildRunStatus,
    WorkflowDiagnosticsManifest, WorkflowExecutionRole, WorkflowHarnessPlan,
    WorkflowModelRouteSnapshot, WorkflowPermissionCeilingDecision, WorkflowQuarantinePolicy,
    WorkflowRunRecord, WorkflowRunState, WorkflowRuntimeEnforcementDecision,
    WorkflowRuntimeEnforcementInput, WorkflowSynthesisOutcome, WorkflowVerificationGate,
    WorkflowVerifierVerdict, WorkflowVerifierVerdictKind, WorkflowWorktreePolicy,
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone)]
pub struct RuntimeWorkflowLiveInput<'a> {
    pub plan: WorkflowHarnessPlan,
    pub subagent_runtime: &'a SubagentRuntime,
    pub provider_client: &'a dyn ProviderClient,
    pub execution_config: SubagentExecutionConfig,
    pub admitted_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeWorkflowLiveError {
    BudgetBlocked { reason: String },
    ParallelismBlocked { reason: String },
    Serialization(String),
}

impl std::fmt::Display for RuntimeWorkflowLiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BudgetBlocked { reason } => {
                write!(formatter, "workflow budget blocked: {reason}")
            }
            Self::ParallelismBlocked { reason } => {
                write!(formatter, "workflow parallelism blocked: {reason}")
            }
            Self::Serialization(error) => {
                write!(formatter, "workflow serialization failed: {error}")
            }
        }
    }
}

impl std::error::Error for RuntimeWorkflowLiveError {}

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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

pub fn run_live_runtime_workflow(
    input: RuntimeWorkflowLiveInput<'_>,
) -> Result<RuntimeWorkflowOutcome, RuntimeWorkflowLiveError> {
    let RuntimeWorkflowLiveInput {
        plan,
        subagent_runtime,
        provider_client,
        execution_config,
        admitted_at_ms,
    } = input;
    if plan.budget_policy.max_parallel_children == 0 {
        return Err(RuntimeWorkflowLiveError::ParallelismBlocked {
            reason: "workflow max_parallel_children must be at least 1".to_owned(),
        });
    }
    ensure_live_permission_ceiling_allowed(&plan)?;
    ensure_live_read_only_plan_allowed(&plan)?;

    let mut usage = WorkflowBudgetUsage {
        known_tokens: 0,
        estimated_tokens: 0,
        child_runs: 0,
        verifier_runs: 0,
        heavy_commands: 0,
    };
    let mut child_results = Vec::new();
    let mut verifier_verdicts = Vec::new();

    for child in &plan.child_graph {
        let route = ensure_runtime_allowed(
            &plan,
            WorkflowExecutionRole::Child,
            &usage,
            subagent_runtime.running_count(),
            elapsed_since(admitted_at_ms),
            child
                .budget
                .max_tokens
                .or(plan.budget_policy.max_child_tokens),
        )?;
        let mut config = execution_config.clone();
        if let Some(model) = route.selected_model_hint {
            config.model = model;
        }
        config.allowed_tools = Some(scoped_read_only_child_tool_names(
            &plan,
            config.workspace.clone(),
            config.model.clone(),
        ));
        config.max_iterations = plan.budget_policy.max_iterations.max(1) as usize;
        if let Some(max_tokens) = child
            .budget
            .max_tokens
            .or(plan.budget_policy.max_child_tokens)
            .and_then(|tokens| u32::try_from(tokens).ok())
        {
            config.settings.max_tokens = max_tokens;
        }
        let spawn =
            workflow_child_spawn_envelope(&plan, child.child_id.clone(), child.goal.clone());
        subagent_runtime
            .register_spawn(spawn.clone())
            .map_err(|reason| RuntimeWorkflowLiveError::ParallelismBlocked { reason })?;
        let result = subagent_runtime.run_spawn(spawn, provider_client, config);
        usage.child_runs = usage.child_runs.saturating_add(1);
        add_budget_usage(&mut usage, result.budget_usage.as_ref());
        ensure_observed_budget_allowed(&plan, &usage)?;
        child_results.push(result);
    }

    for verifier in &plan.verifier_graph {
        let route = ensure_runtime_allowed(
            &plan,
            WorkflowExecutionRole::Verifier,
            &usage,
            subagent_runtime.running_count(),
            elapsed_since(admitted_at_ms),
            plan.budget_policy.max_verifier_tokens,
        )?;
        let mut config = execution_config.clone();
        if let Some(model) = route.selected_model_hint {
            config.model = model;
        }
        config.allowed_tools = Some(scoped_read_only_child_tool_names(
            &plan,
            config.workspace.clone(),
            config.model.clone(),
        ));
        if let Some(max_tokens) = plan
            .budget_policy
            .max_verifier_tokens
            .and_then(|tokens| u32::try_from(tokens).ok())
        {
            config.settings.max_tokens = max_tokens;
        }
        let target_summary = child_results
            .iter()
            .find(|result| result.child_task_id == verifier.target_child_id)
            .map(|result| result.summary.as_str())
            .unwrap_or("missing child result");
        let verifier_goal = format!(
            "Verify workflow child `{}` with rubric: {}\n\nChild result:\n{}",
            verifier.target_child_id, verifier.rubric, target_summary
        );
        let spawn =
            workflow_child_spawn_envelope(&plan, verifier.verifier_id.clone(), verifier_goal);
        subagent_runtime
            .register_spawn(spawn.clone())
            .map_err(|reason| RuntimeWorkflowLiveError::ParallelismBlocked { reason })?;
        let result = subagent_runtime.run_spawn(spawn, provider_client, config);
        usage.verifier_runs = usage.verifier_runs.saturating_add(1);
        add_budget_usage(&mut usage, result.budget_usage.as_ref());
        ensure_observed_budget_allowed(&plan, &usage)?;
        verifier_verdicts.push(WorkflowVerifierVerdict {
            verifier_id: verifier.verifier_id.clone(),
            target_child_id: verifier.target_child_id.clone(),
            verdict: verifier_verdict_from_summary(&result.summary),
            summary: result.summary,
            evidence_refs: Vec::new(),
        });
    }

    run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan,
        child_results,
        verifier_verdicts,
        child_workspace: execution_config.workspace,
        child_model: execution_config.model,
        admitted_at_ms,
    })
    .map_err(|error| RuntimeWorkflowLiveError::Serialization(error.to_string()))
}

fn ensure_live_read_only_plan_allowed(
    plan: &WorkflowHarnessPlan,
) -> Result<(), RuntimeWorkflowLiveError> {
    if !matches!(
        plan.worktree_policy,
        WorkflowWorktreePolicy::None | WorkflowWorktreePolicy::ReadOnlySnapshot
    ) {
        return Err(RuntimeWorkflowLiveError::BudgetBlocked {
            reason: "live workflow path supports only read-only worktree policies".to_owned(),
        });
    }
    if let Some(child) = plan.child_graph.iter().find(|child| {
        !matches!(
            child.worktree_policy,
            WorkflowWorktreePolicy::None | WorkflowWorktreePolicy::ReadOnlySnapshot
        )
    }) {
        return Err(RuntimeWorkflowLiveError::BudgetBlocked {
            reason: format!(
                "live workflow child `{}` requires isolated worktree handling",
                child.child_id
            ),
        });
    }
    if plan.tool_scope_policy.quarantine == WorkflowQuarantinePolicy::PrivilegedActorSeparated {
        return Err(RuntimeWorkflowLiveError::BudgetBlocked {
            reason: "live workflow path does not open privileged quarantine scopes".to_owned(),
        });
    }
    if let Some(tool_name) = plan
        .tool_scope_policy
        .allowed_tools
        .iter()
        .find(|tool_name| matches!(tool_name.as_str(), "write_file" | "edit_file" | "exec"))
    {
        return Err(RuntimeWorkflowLiveError::BudgetBlocked {
            reason: format!("live workflow path does not allow side-effect tool `{tool_name}`"),
        });
    }
    Ok(())
}

fn ensure_live_permission_ceiling_allowed(
    plan: &WorkflowHarnessPlan,
) -> Result<(), RuntimeWorkflowLiveError> {
    match workflow_permission_ceiling_decision(
        &plan.permission_policy,
        &live_requested_capabilities(plan),
        live_plan_requests_privileged_step(plan),
    ) {
        WorkflowPermissionCeilingDecision::Allowed => Ok(()),
        WorkflowPermissionCeilingDecision::ApprovalRequired { reason } => {
            Err(RuntimeWorkflowLiveError::BudgetBlocked { reason })
        }
        WorkflowPermissionCeilingDecision::Blocked { denied_capability } => {
            Err(RuntimeWorkflowLiveError::BudgetBlocked {
                reason: format!("workflow permission ceiling denies `{denied_capability}`"),
            })
        }
    }
}

fn live_requested_capabilities(plan: &WorkflowHarnessPlan) -> Vec<String> {
    let mut capabilities = Vec::new();
    for tool_name in &plan.tool_scope_policy.allowed_tools {
        push_unique(&mut capabilities, tool_name.clone());
        match tool_name.as_str() {
            "read_file" | "list_dir" | "glob" | "grep" | "notebook_read" => {
                push_unique(&mut capabilities, "fs_read".to_owned())
            }
            "write_file" | "edit_file" | "notebook_edit" => {
                push_unique(&mut capabilities, "fs_write".to_owned())
            }
            "exec" => push_unique(&mut capabilities, "proc_exec".to_owned()),
            "web_fetch" | "web_search" | "image_generate" => {
                push_unique(&mut capabilities, "net_outbound".to_owned())
            }
            "message" => push_unique(&mut capabilities, "external_delivery".to_owned()),
            "cron" => push_unique(&mut capabilities, "automation_schedule".to_owned()),
            "spawn" => push_unique(&mut capabilities, "proc_exec".to_owned()),
            "my" | "self" => push_unique(&mut capabilities, "runtime_config_write".to_owned()),
            tool_name if tool_name.starts_with("mcp_") => {
                push_unique(&mut capabilities, "proc_exec".to_owned())
            }
            _ => {}
        }
    }
    capabilities
}

fn live_plan_requests_privileged_step(plan: &WorkflowHarnessPlan) -> bool {
    plan.tool_scope_policy.quarantine == WorkflowQuarantinePolicy::PrivilegedActorSeparated
        || matches!(
            plan.worktree_policy,
            WorkflowWorktreePolicy::IsolatedWorktreeRequired
        )
        || plan.child_graph.iter().any(|child| {
            matches!(
                child.worktree_policy,
                WorkflowWorktreePolicy::IsolatedWorktreeRequired
            )
        })
        || plan
            .tool_scope_policy
            .allowed_tools
            .iter()
            .any(|tool_name| matches!(tool_name.as_str(), "write_file" | "edit_file" | "exec"))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn ensure_runtime_allowed(
    plan: &WorkflowHarnessPlan,
    role: WorkflowExecutionRole,
    usage: &WorkflowBudgetUsage,
    active_child_count: usize,
    elapsed_wall_clock_ms: u64,
    requested_tokens: Option<u64>,
) -> Result<WorkflowModelRouteSnapshot, RuntimeWorkflowLiveError> {
    match workflow_runtime_enforcement_decision(
        &plan.budget_policy,
        &plan.model_routing_policy,
        &WorkflowRuntimeEnforcementInput {
            role,
            usage: usage.clone(),
            active_child_count,
            elapsed_wall_clock_ms,
            requested_tokens,
            requests_heavy_command: false,
        },
    ) {
        WorkflowRuntimeEnforcementDecision::Allowed { route, .. } => Ok(route),
        WorkflowRuntimeEnforcementDecision::Throttled { reason } => {
            Err(RuntimeWorkflowLiveError::ParallelismBlocked { reason })
        }
        WorkflowRuntimeEnforcementDecision::Blocked { reason } => {
            Err(RuntimeWorkflowLiveError::BudgetBlocked { reason })
        }
    }
}

fn ensure_observed_budget_allowed(
    plan: &WorkflowHarnessPlan,
    usage: &WorkflowBudgetUsage,
) -> Result<(), RuntimeWorkflowLiveError> {
    if let Some(max_total_tokens) = plan.budget_policy.max_total_tokens {
        let used_tokens = usage.known_tokens.saturating_add(usage.estimated_tokens);
        if used_tokens > max_total_tokens {
            return Err(RuntimeWorkflowLiveError::BudgetBlocked {
                reason: "workflow token budget exceeded".to_owned(),
            });
        }
    }
    if let Some(max_heavy_commands) = plan.budget_policy.max_heavy_commands {
        if usage.heavy_commands > max_heavy_commands {
            return Err(RuntimeWorkflowLiveError::BudgetBlocked {
                reason: "workflow heavy command budget exhausted".to_owned(),
            });
        }
    }
    Ok(())
}

fn elapsed_since(start_ms: u64) -> u64 {
    current_unix_ms().saturating_sub(start_ms)
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
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

fn workflow_child_spawn_envelope(
    plan: &WorkflowHarnessPlan,
    child_id: String,
    task_goal: String,
) -> SpawnEnvelope {
    let mut spawn = SpawnEnvelope::new(plan.origin_session_id.clone(), child_id, task_goal);
    spawn.parent_turn_id = plan.origin_turn_id.clone();
    spawn.origin_channel = "workflow".to_owned();
    spawn.origin_chat_id = plan.workflow_id.clone();
    spawn.input_budget_snapshot = serde_json::to_value(&plan.budget_policy).unwrap_or_default();
    spawn.output_budget_snapshot = serde_json::json!({
        "max_child_tokens": plan.budget_policy.max_child_tokens,
        "max_verifier_tokens": plan.budget_policy.max_verifier_tokens,
    });
    spawn.timeout_ms = plan.budget_policy.max_wall_clock_ms;
    spawn.parallelism_group = plan.workflow_id.clone();
    spawn
}

fn add_budget_usage(usage: &mut WorkflowBudgetUsage, raw_usage: Option<&serde_json::Value>) {
    let Some(raw_usage) = raw_usage else {
        return;
    };
    let total_tokens = raw_usage
        .get("total_tokens")
        .and_then(serde_json::Value::as_u64)
        .or_else(|| raw_usage.get("total").and_then(serde_json::Value::as_u64))
        .unwrap_or(0);
    usage.known_tokens = usage.known_tokens.saturating_add(total_tokens);
}

fn verifier_verdict_from_summary(summary: &str) -> WorkflowVerifierVerdictKind {
    let summary = summary.to_ascii_lowercase();
    if summary.trim() == "fail"
        || summary.trim_start().starts_with("fail:")
        || summary.trim_start().starts_with("failed:")
        || summary.contains("verdict: fail")
        || summary.contains("status: fail")
        || summary.contains("verdict: failed")
        || summary.contains("status: failed")
        || summary.contains("reject")
        || summary.contains("not pass")
        || summary.contains("does not pass")
        || summary.contains("doesn't pass")
        || summary.contains("not approved")
    {
        WorkflowVerifierVerdictKind::Fail
    } else if summary.contains("uncertain")
        || summary.contains("inconclusive")
        || summary.contains("unknown")
    {
        WorkflowVerifierVerdictKind::Uncertain
    } else if summary.trim() == "pass"
        || summary.trim_start().starts_with("pass:")
        || summary.contains("verdict: pass")
        || summary.contains("status: pass")
    {
        WorkflowVerifierVerdictKind::Pass
    } else {
        WorkflowVerifierVerdictKind::Uncertain
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
