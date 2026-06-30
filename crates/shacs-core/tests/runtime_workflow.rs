use serde_json::{json, Map};
use shacs_core::runtime::{
    cancel_runtime_workflow, decide_workflow_admission, read_only_child_tool_names,
    run_read_only_runtime_workflow, run_runtime_workflow_admission_branch,
    runtime_workflow_diagnostics, runtime_workflow_execution_handle, ChildResultEnvelope,
    ChildResultStatus, RuntimeWorkflowAdmissionBranchInput, RuntimeWorkflowAdmissionBranchOutcome,
    RuntimeWorkflowInput, Session, WorkflowAdmissionDecision, WorkflowAdmissionInput,
    WorkflowBarrierDecision, WorkflowBudgetPolicy, WorkflowBudgetSlice, WorkflowCheckpointPolicy,
    WorkflowChildSpec, WorkflowContextPolicy, WorkflowHarnessPlan, WorkflowMergePolicy,
    WorkflowModelRoutingPolicy, WorkflowPattern, WorkflowPermissionPolicy,
    WorkflowQuarantinePolicy, WorkflowResumePolicy, WorkflowRunState, WorkflowStep,
    WorkflowStopCondition, WorkflowSynthesisOutcome, WorkflowToolScopePolicy,
    WorkflowVerificationGate, WorkflowVerifierSpec, WorkflowVerifierVerdict,
    WorkflowVerifierVerdictKind, WorkflowWorktreePolicy,
};

#[test]
fn workflow_symbols_remain_available_through_runtime_reexport() {
    assert!(WorkflowRunState::Completed.is_terminal());

    let decision = decide_workflow_admission(&WorkflowAdmissionInput {
        objective_complexity: 1,
        estimated_item_count: 1,
        requires_parallelism: false,
        requires_independent_verification: false,
        requires_adversarial_review: false,
        requires_large_context_partitioning: false,
        requires_write_isolation: false,
        requires_recurring_loop: false,
        risk_level: 1,
        user_requested_workflow: false,
        available_budget_tokens: Some(1_000),
        blocking_reasons: Vec::new(),
        missing_scope_questions: Vec::new(),
    });

    assert_eq!(decision, WorkflowAdmissionDecision::UseRegularLoop);
}

#[test]
fn read_only_runtime_workflow_emits_monitorable_events_and_succeeds(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let outcome = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![child_result(ChildResultStatus::Completed)],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().to_path_buf(),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    let phases = outcome
        .events
        .iter()
        .map(|event| event.phase)
        .collect::<Vec<_>>();

    assert_eq!(
        phases,
        vec![
            "admitted",
            "child_started",
            "child_completed",
            "verifier_completed",
            "synthesizing",
            "terminal",
        ]
    );
    assert_eq!(outcome.run.state, WorkflowRunState::Completed);
    assert_eq!(
        outcome.events.last().map(|event| event.state),
        Some(WorkflowRunState::Completed)
    );
    assert_eq!(
        outcome
            .events
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        vec![
            WorkflowRunState::Admitted,
            WorkflowRunState::Running,
            WorkflowRunState::WaitingForChildren,
            WorkflowRunState::Verifying,
            WorkflowRunState::Synthesizing,
            WorkflowRunState::Completed,
        ]
    );
    assert_eq!(
        outcome.barrier_decision,
        WorkflowBarrierDecision::Ready {
            ready_step_ids: vec!["extract-claims".to_owned()]
        }
    );
    assert_eq!(outcome.verification_gate, WorkflowVerificationGate::Passed);
    assert!(outcome.synthesis_outcome.final_success_allowed);
    assert_eq!(outcome.child_results[0].child_id, "child-1");
    assert_eq!(outcome.child_results[0].step_id, "extract-claims");
    assert_eq!(outcome.child_results[0].summary, "claims extracted");
    assert_read_only_tools(&outcome.child_tool_names);
    assert_eq!(outcome.child_tool_names, vec!["read_file", "grep"]);

    Ok(())
}

#[test]
fn workflow_verifier_fail_and_missing_verifier_fail_closed(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let failed = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![child_result(ChildResultStatus::Completed)],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Fail)],
        child_workspace: workspace.path().join("failed"),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    assert_eq!(
        failed.verification_gate,
        WorkflowVerificationGate::Failed {
            failing_child_ids: vec!["child-1".to_owned()]
        }
    );
    assert_final_success_blocked(&failed.synthesis_outcome);
    assert_eq!(failed.run.state, WorkflowRunState::Failed);

    let missing = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![child_result(ChildResultStatus::Completed)],
        verifier_verdicts: Vec::new(),
        child_workspace: workspace.path().join("missing"),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    assert_eq!(
        missing.verification_gate,
        WorkflowVerificationGate::Blocked {
            missing_verifier_ids: vec!["verifier-1".to_owned()]
        }
    );
    assert_final_success_blocked(&missing.synthesis_outcome);
    assert_eq!(missing.run.state, WorkflowRunState::Failed);

    Ok(())
}

#[test]
fn workflow_rejects_uncorrelated_child_and_verifier_results(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let mut wrong_session = child_result(ChildResultStatus::Completed);
    wrong_session.session_id = "cli:other".to_owned();
    let failed_child = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![wrong_session],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().join("wrong-session"),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    assert!(matches!(
        failed_child.barrier_decision,
        WorkflowBarrierDecision::Blocked { .. }
    ));
    assert_eq!(failed_child.run.state, WorkflowRunState::Failed);
    assert!(!failed_child.synthesis_outcome.final_success_allowed);
    assert_eq!(
        failed_child.synthesis_outcome.rejected_child_ids,
        vec!["child-1".to_owned()]
    );

    let mut wrong_spawn = child_result(ChildResultStatus::Completed);
    wrong_spawn.spawn_effect_id = "spawn:stale-child".to_owned();
    let failed_spawn = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![wrong_spawn],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().join("wrong-spawn"),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    assert!(matches!(
        failed_spawn.barrier_decision,
        WorkflowBarrierDecision::Blocked { .. }
    ));
    assert_eq!(failed_spawn.run.state, WorkflowRunState::Failed);

    let mut unknown_child = child_result(ChildResultStatus::Completed);
    unknown_child.child_task_id = "unknown-child".to_owned();
    unknown_child.spawn_effect_id = "spawn:unknown-child".to_owned();
    unknown_child.summary = "unplanned child output".to_owned();
    let failed_unknown = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![unknown_child],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().join("unknown-child"),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    assert_eq!(failed_unknown.run.state, WorkflowRunState::Failed);
    assert_eq!(failed_unknown.child_results[0].child_id, "unknown-child");
    assert_eq!(failed_unknown.child_results[0].step_id, "");
    assert_eq!(
        failed_unknown.child_results[0].summary,
        "discarded child result with mismatched workflow provenance"
    );

    let mut wrong_target = verdict(WorkflowVerifierVerdictKind::Pass);
    wrong_target.target_child_id = "child-2".to_owned();
    let failed_verifier = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![child_result(ChildResultStatus::Completed)],
        verifier_verdicts: vec![wrong_target],
        child_workspace: workspace.path().join("wrong-verifier"),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    assert_eq!(
        failed_verifier.verification_gate,
        WorkflowVerificationGate::Failed {
            failing_child_ids: vec!["child-1".to_owned()]
        }
    );
    assert_eq!(failed_verifier.run.state, WorkflowRunState::Failed);
    assert_final_success_blocked(&failed_verifier.synthesis_outcome);

    Ok(())
}

#[test]
fn workflow_admission_branch_enters_dynamic_runtime_path() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace = tempfile::tempdir()?;
    let outcome = run_runtime_workflow_admission_branch(RuntimeWorkflowAdmissionBranchInput {
        admission: dynamic_admission(),
        plan: sample_plan(),
        child_results: vec![child_result(ChildResultStatus::Completed)],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().to_path_buf(),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;

    let RuntimeWorkflowAdmissionBranchOutcome::DynamicWorkflow(outcome) = outcome else {
        return Err("dynamic admission should enter runtime workflow branch".into());
    };
    assert_eq!(outcome.run.state, WorkflowRunState::Completed);
    assert_eq!(
        outcome.events.first().map(|event| event.phase),
        Some("admitted")
    );
    assert_eq!(
        outcome.events.last().map(|event| event.phase),
        Some("terminal")
    );

    Ok(())
}

#[test]
fn workflow_admission_branch_preserves_non_dynamic_decisions(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let mut admission = dynamic_admission();
    admission.user_requested_workflow = false;
    admission.requires_parallelism = false;
    admission.requires_independent_verification = false;
    admission.objective_complexity = 1;
    admission.estimated_item_count = 1;
    assert_eq!(
        run_runtime_workflow_admission_branch(RuntimeWorkflowAdmissionBranchInput {
            admission,
            plan: sample_plan(),
            child_results: Vec::new(),
            verifier_verdicts: Vec::new(),
            child_workspace: workspace.path().to_path_buf(),
            child_model: "test-model".to_owned(),
            admitted_at_ms: 100,
        })?,
        RuntimeWorkflowAdmissionBranchOutcome::RegularLoop
    );

    Ok(())
}

#[test]
fn workflow_interrupt_cancels_handle_children_and_terminal_state(
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();
    let handle = runtime_workflow_execution_handle(&plan);
    assert_eq!(handle.workflow_id, "workflow-1");
    assert_eq!(handle.parent_session_key, "cli:direct");
    assert_eq!(handle.child_ids, vec!["child-1".to_owned()]);
    assert!(!handle.cancellation_token.is_cancelled());

    let interrupted = cancel_runtime_workflow(&plan, &handle, "user requested stop", 150)?;
    assert!(handle.cancellation_token.is_cancelled());
    assert!(interrupted.cancellation_requested);
    assert_eq!(interrupted.run.state, WorkflowRunState::Cancelled);
    assert_eq!(interrupted.cancelled_child_ids, vec!["child-1".to_owned()]);
    assert_eq!(interrupted.reason, "user requested stop");
    assert_eq!(
        interrupted
            .events
            .iter()
            .map(|event| event.phase)
            .collect::<Vec<_>>(),
        vec!["child_cancelled", "terminal"]
    );

    Ok(())
}

#[test]
fn workflow_diagnostics_replay_summary_is_redacted_and_non_live(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let plan = sample_plan();
    let outcome = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: plan.clone(),
        child_results: vec![child_result(ChildResultStatus::Completed)],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().to_path_buf(),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;
    let diagnostics = runtime_workflow_diagnostics(&plan, &outcome)?;

    assert_eq!(diagnostics.manifest.workflow_id, "workflow-1");
    assert!(!diagnostics.manifest.harness_plan_digest.is_empty());
    assert!(!diagnostics.manifest.child_graph_digest.is_empty());
    assert!(!diagnostics.manifest.verifier_graph_digest.is_empty());
    assert_eq!(diagnostics.terminal_state, WorkflowRunState::Completed);
    assert_eq!(diagnostics.child_result_count, 1);
    assert_eq!(diagnostics.verifier_status, "passed");
    assert!(!diagnostics.replay_live_actions_allowed);
    assert_eq!(
        diagnostics.event_phases,
        vec![
            "admitted".to_owned(),
            "child_started".to_owned(),
            "child_completed".to_owned(),
            "verifier_completed".to_owned(),
            "synthesizing".to_owned(),
            "terminal".to_owned(),
        ]
    );

    Ok(())
}

#[test]
fn workflow_read_only_child_registry_excludes_write_edit_and_exec(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let tool_names = read_only_child_tool_names(workspace.path(), "test-model");

    assert_read_only_tools(&tool_names);
    assert!(tool_names.contains(&"read_file".to_owned()));
    assert!(tool_names.contains(&"list_dir".to_owned()));
    assert!(tool_names.contains(&"glob".to_owned()));
    assert!(tool_names.contains(&"grep".to_owned()));

    Ok(())
}

#[test]
fn workflow_child_envelope_data_does_not_mutate_parent_session_truth(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let mut parent_session = Session::new("cli:direct");
    parent_session.add_message("user", "original parent truth", Map::new());
    let before = parent_session.clone();
    let mut envelope = child_result(ChildResultStatus::Completed);
    envelope.structured_result = Some(json!({
        "attempted_parent_truth": "child cannot write this through the runtime helper"
    }));

    let outcome = run_read_only_runtime_workflow(RuntimeWorkflowInput {
        plan: sample_plan(),
        child_results: vec![envelope],
        verifier_verdicts: vec![verdict(WorkflowVerifierVerdictKind::Pass)],
        child_workspace: workspace.path().to_path_buf(),
        child_model: "test-model".to_owned(),
        admitted_at_ms: 100,
    })?;

    assert_eq!(parent_session, before);
    assert_eq!(outcome.child_results.len(), 1);
    assert_eq!(outcome.child_results[0].summary, "claims extracted");
    assert!(outcome.child_results[0].evidence_refs.is_empty());

    Ok(())
}

fn assert_read_only_tools(tool_names: &[String]) {
    assert!(!tool_names.contains(&"write_file".to_owned()));
    assert!(!tool_names.contains(&"edit_file".to_owned()));
    assert!(!tool_names.contains(&"exec".to_owned()));
}

fn assert_final_success_blocked(outcome: &WorkflowSynthesisOutcome) {
    assert!(!outcome.final_success_allowed);
    assert_eq!(outcome.accepted_child_ids, vec!["child-1".to_owned()]);
}

fn child_result(status: ChildResultStatus) -> ChildResultEnvelope {
    ChildResultEnvelope {
        session_id: "cli:direct".to_owned(),
        parent_turn_id: "turn-1".to_owned(),
        child_task_id: "child-1".to_owned(),
        spawn_effect_id: "spawn:child-1".to_owned(),
        subagent_kind: "default".to_owned(),
        status,
        started_at_ms: 10,
        finished_at_ms: 20,
        duration_ms: 10,
        summary: "claims extracted".to_owned(),
        structured_result: None,
        error: None,
        observations: None,
        budget_usage: None,
    }
}

fn dynamic_admission() -> WorkflowAdmissionInput {
    WorkflowAdmissionInput {
        objective_complexity: 8,
        estimated_item_count: 8,
        requires_parallelism: true,
        requires_independent_verification: true,
        requires_adversarial_review: false,
        requires_large_context_partitioning: false,
        requires_write_isolation: false,
        requires_recurring_loop: false,
        risk_level: 4,
        user_requested_workflow: true,
        available_budget_tokens: Some(10_000),
        blocking_reasons: Vec::new(),
        missing_scope_questions: Vec::new(),
    }
}

fn verdict(kind: WorkflowVerifierVerdictKind) -> WorkflowVerifierVerdict {
    WorkflowVerifierVerdict {
        verifier_id: "verifier-1".to_owned(),
        target_child_id: "child-1".to_owned(),
        verdict: kind,
        summary: "verifier summary".to_owned(),
        evidence_refs: Vec::new(),
    }
}

fn sample_plan() -> WorkflowHarnessPlan {
    WorkflowHarnessPlan {
        workflow_id: "workflow-1".to_owned(),
        origin_session_id: "cli:direct".to_owned(),
        origin_turn_id: "turn-1".to_owned(),
        objective: "verify every claim".to_owned(),
        constraints: vec!["do not mutate session truth from child".to_owned()],
        pattern: WorkflowPattern::FanOutAndSynthesize,
        steps: vec![WorkflowStep {
            step_id: "extract-claims".to_owned(),
            label: "Extract claims".to_owned(),
            pattern: WorkflowPattern::ClassifyAndAct,
            depends_on: Vec::new(),
            required: true,
            expected_output_schema: Some(json!({ "type": "object" })),
        }],
        child_graph: vec![WorkflowChildSpec {
            child_id: "child-1".to_owned(),
            step_id: "extract-claims".to_owned(),
            goal: "extract factual claims".to_owned(),
            tool_scope_ref: Some("scope-1".to_owned()),
            worktree_policy: WorkflowWorktreePolicy::ReadOnlySnapshot,
            budget: WorkflowBudgetSlice {
                max_tokens: Some(1_000),
                max_wall_clock_ms: Some(30_000),
            },
            verifier_required: true,
        }],
        verifier_graph: vec![WorkflowVerifierSpec {
            verifier_id: "verifier-1".to_owned(),
            target_child_id: "child-1".to_owned(),
            rubric: "claims have source evidence".to_owned(),
            independent_evidence_required: true,
        }],
        context_policy: WorkflowContextPolicy {
            root_objective_snapshot: "verify every claim".to_owned(),
            include_constraints_in_children: true,
            untrusted_input_labels: vec!["blog-draft".to_owned()],
        },
        tool_scope_policy: WorkflowToolScopePolicy {
            scope_digest: "scope-digest".to_owned(),
            allowed_tools: vec!["read_file".to_owned(), "grep".to_owned()],
            deferred_tool_search_allowed: true,
            quarantine: WorkflowQuarantinePolicy::ReadOnlyUntrusted,
        },
        permission_policy: WorkflowPermissionPolicy {
            permission_snapshot_ref: "permission-snapshot".to_owned(),
            denied_capabilities: vec!["proc_exec".to_owned()],
            approval_required_for_privileged_steps: true,
        },
        worktree_policy: WorkflowWorktreePolicy::ReadOnlySnapshot,
        model_routing_policy: WorkflowModelRoutingPolicy {
            classifier_model_hint: None,
            child_model_hint: Some("fast".to_owned()),
            verifier_model_hint: Some("strong".to_owned()),
            synthesis_model_hint: None,
            fallback_model_policy: "use provider default".to_owned(),
        },
        budget_policy: WorkflowBudgetPolicy {
            max_total_tokens: Some(10_000),
            max_child_tokens: Some(2_000),
            max_verifier_tokens: Some(2_000),
            max_iterations: 4,
            max_parallel_children: 2,
            max_wall_clock_ms: Some(120_000),
            max_heavy_commands: Some(0),
        },
        checkpoint_policy: WorkflowCheckpointPolicy {
            checkpoint_required: true,
            checkpoint_before_privileged_steps: true,
        },
        merge_policy: WorkflowMergePolicy {
            require_verifier_pass: true,
            allow_partial_completion: false,
            surface_disagreements: true,
        },
        stop_condition: WorkflowStopCondition {
            description: "all claims verified".to_owned(),
            no_new_findings_threshold: None,
        },
        resume_policy: WorkflowResumePolicy {
            require_plan_digest_match: true,
            allow_completed_resume: false,
        },
    }
}
