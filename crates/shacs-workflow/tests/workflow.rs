use serde_json::json;
use shacs_eval::evaluator::{EvidenceKind, EvidenceRef, RedactionStatus};
use shacs_workflow::{
    admit_workflow_plan, build_workflow_checkpoint, decide_workflow_admission,
    validate_workflow_plan, workflow_barrier_decision, workflow_budget_decision,
    workflow_diagnostics_manifest, workflow_harness_plan_digest, workflow_model_route_snapshot,
    workflow_pattern_contract_evidence, workflow_pattern_contract_status,
    workflow_permission_ceiling_decision, workflow_prd000_release_evidence_checklist,
    workflow_projection, workflow_quarantine_decision, workflow_ready_schedule_decision,
    workflow_ready_step_ids, workflow_recipe_readiness, workflow_resume_decision,
    workflow_resume_validation_decision, workflow_role_scoped_tool_names,
    workflow_runtime_diagnostics_manifest, workflow_sanitized_handoff_evidence_status,
    workflow_sanitized_handoff_status, workflow_spec024_release_evidence_checklist,
    workflow_synthesis_outcome, workflow_verification_gate, workflow_verifier_evidence_status,
    workflow_worktree_branch_name, workflow_worktree_decision, WorkflowAdmissionDecision,
    WorkflowAdmissionInput, WorkflowBarrierDecision, WorkflowBudgetDecision, WorkflowBudgetPolicy,
    WorkflowBudgetSlice, WorkflowBudgetUsage, WorkflowCheckpointInput, WorkflowCheckpointPolicy,
    WorkflowChildResult, WorkflowChildRunStatus, WorkflowChildSpec, WorkflowContextPolicy,
    WorkflowExecutionRole, WorkflowHarnessPlan, WorkflowMergePolicy, WorkflowModelRoutingPolicy,
    WorkflowPattern, WorkflowPatternContractStatus, WorkflowPermissionCeilingDecision,
    WorkflowPermissionPolicy, WorkflowPlanValidationStatus, WorkflowPrd000ReleaseEvidence,
    WorkflowPrd000ReleaseEvidenceBucket, WorkflowQuarantineDecision, WorkflowQuarantinePolicy,
    WorkflowReadyScheduleDecision, WorkflowRecipe, WorkflowRecipeReadiness, WorkflowResumeDecision,
    WorkflowResumePolicy, WorkflowResumeValidationInput, WorkflowRunState,
    WorkflowRuntimeDiagnosticsInput, WorkflowRuntimeEnforcementDecision,
    WorkflowRuntimeEnforcementInput, WorkflowSanitizedHandoffEvidence,
    WorkflowSanitizedHandoffEvidenceStatus, WorkflowSanitizedHandoffStatus,
    WorkflowSpec024ReleaseEvidence, WorkflowSpec024ReleaseEvidenceBucket, WorkflowStep,
    WorkflowStepPrivilege, WorkflowStopCondition, WorkflowToolScopePolicy, WorkflowToolScopeRole,
    WorkflowVerificationGate, WorkflowVerifierEvidenceStatus, WorkflowVerifierSpec,
    WorkflowVerifierVerdict, WorkflowVerifierVerdictKind, WorkflowWorktreeDecision,
    WorkflowWorktreePolicy, WorkflowWorktreeRequest,
};

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

fn evidence(id: &str, owner_spec: Option<&str>, redaction_status: RedactionStatus) -> EvidenceRef {
    EvidenceRef {
        kind: EvidenceKind::DiagnosticRecord,
        id: id.to_owned(),
        digest: format!("digest-{id}"),
        summary: format!("summary-{id}"),
        redaction_status,
        owner_spec: owner_spec.map(str::to_owned),
        locator: Some(format!("workflow://{id}")),
        retention_hint: Some("release_evidence".to_owned()),
    }
}

#[test]
fn workflow_admission_routes_regular_quick_and_dynamic_tasks() {
    let regular = WorkflowAdmissionInput {
        objective_complexity: 2,
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
    };
    assert_eq!(
        decide_workflow_admission(&regular),
        WorkflowAdmissionDecision::UseRegularLoop
    );

    let mut quick = regular.clone();
    quick.requires_independent_verification = true;
    assert!(matches!(
        decide_workflow_admission(&quick),
        WorkflowAdmissionDecision::UseQuickWorkflow { .. }
    ));

    let mut dynamic = regular.clone();
    dynamic.requires_write_isolation = true;
    assert!(matches!(
        decide_workflow_admission(&dynamic),
        WorkflowAdmissionDecision::UseDynamicWorkflow { .. }
    ));

    let mut blocked = regular.clone();
    blocked.blocking_reasons = vec!["budget unavailable".to_owned()];
    assert!(matches!(
        decide_workflow_admission(&blocked),
        WorkflowAdmissionDecision::BlockedByPolicy { .. }
    ));
}

#[test]
fn workflow_harness_plan_digest_is_stable_and_objective_sensitive(
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();
    let first = workflow_harness_plan_digest(&plan)?;
    let second = workflow_harness_plan_digest(&plan)?;
    assert_eq!(first, second);

    let mut changed = plan;
    changed.objective = "verify every claim and cite every source".to_owned();
    assert_ne!(first, workflow_harness_plan_digest(&changed)?);
    Ok(())
}

#[test]
fn workflow_projection_no_checkpoint_uses_owner_defined_zero_defaults(
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();
    let run = admit_workflow_plan(&plan, 100)?;

    let projection = workflow_projection(&run, &plan, None, &WorkflowVerificationGate::Passed, &[]);

    assert_eq!(projection.progress_count, 0);
    assert_eq!(projection.active_child_count, 0);
    assert_eq!(projection.pending_barrier_count, 0);
    assert_eq!(projection.budget_usage.known_tokens, 0);
    assert_eq!(projection.budget_usage.estimated_tokens, 0);
    assert_eq!(projection.budget_usage.child_runs, 0);
    assert_eq!(projection.budget_usage.verifier_runs, 0);
    assert_eq!(projection.budget_usage.heavy_commands, 0);
    assert!(projection.worktree_refs.is_empty());
    assert!(!projection.resume_available);
    Ok(())
}

#[test]
fn workflow_checkpoint_resume_requires_matching_plan_digest_and_nonterminal_state(
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();
    let run = admit_workflow_plan(&plan, 100)?;
    let checkpoint = build_workflow_checkpoint(
        &plan,
        &run,
        WorkflowCheckpointInput {
            state: WorkflowRunState::WaitingForChildren,
            completed_steps: vec!["extract-claims".to_owned()],
            active_children: vec!["child-1".to_owned()],
            pending_barriers: vec!["barrier-1".to_owned()],
            budget_usage: WorkflowBudgetUsage {
                known_tokens: 100,
                estimated_tokens: 150,
                child_runs: 1,
                verifier_runs: 0,
                heavy_commands: 0,
            },
            worktree_refs: Vec::new(),
            evidence_refs: vec!["evidence-1".to_owned()],
            last_safe_resume_point: "after-extract-claims".to_owned(),
            recorded_at_ms: 200,
        },
    );

    assert_eq!(
        workflow_resume_decision(&checkpoint, &plan.resume_policy, &run.harness_plan_digest),
        WorkflowResumeDecision::ResumeAllowed {
            resume_point: "after-extract-claims".to_owned()
        }
    );
    assert!(matches!(
        workflow_resume_decision(&checkpoint, &plan.resume_policy, "different-digest"),
        WorkflowResumeDecision::Blocked { reason } if reason.contains("digest mismatch")
    ));

    assert_eq!(
        workflow_resume_validation_decision(&WorkflowResumeValidationInput {
            checkpoint: checkpoint.clone(),
            resume_policy: plan.resume_policy.clone(),
            current_harness_plan_digest: run.harness_plan_digest.clone(),
            required_completed_steps: vec!["extract-claims".to_owned()],
            required_worktree_refs: Vec::new(),
            required_evidence_refs: vec!["evidence-1".to_owned()],
        }),
        WorkflowResumeDecision::ResumeAllowed {
            resume_point: "after-extract-claims".to_owned()
        }
    );
    assert!(matches!(
        workflow_resume_validation_decision(&WorkflowResumeValidationInput {
            checkpoint: checkpoint.clone(),
            resume_policy: plan.resume_policy.clone(),
            current_harness_plan_digest: run.harness_plan_digest.clone(),
            required_completed_steps: vec!["missing-step".to_owned()],
            required_worktree_refs: Vec::new(),
            required_evidence_refs: Vec::new(),
        }),
        WorkflowResumeDecision::Blocked { reason } if reason.contains("missing completed step")
    ));

    let mut terminal = checkpoint;
    terminal.state = WorkflowRunState::Completed;
    assert_eq!(
        workflow_resume_decision(&terminal, &plan.resume_policy, &run.harness_plan_digest),
        WorkflowResumeDecision::AlreadyTerminal {
            state: WorkflowRunState::Completed
        }
    );
    Ok(())
}

#[test]
fn workflow_prd000_release_evidence_requires_all_buckets_and_valid_owner_redaction() {
    let complete = WorkflowPrd000ReleaseEvidenceBucket::required_buckets()
        .into_iter()
        .map(|bucket| WorkflowPrd000ReleaseEvidence {
            bucket,
            test_names: vec![format!("test-{bucket:?}")],
            manual_qa_refs: Vec::new(),
            evidence_refs: vec![evidence("ok", Some("024"), RedactionStatus::AlreadySafe)],
        })
        .collect::<Vec<_>>();
    let checklist = workflow_prd000_release_evidence_checklist(&complete);
    assert!(checklist.passed);
    assert!(checklist.missing_buckets.is_empty());

    let invalid_owner = [WorkflowPrd000ReleaseEvidence {
        bucket: WorkflowPrd000ReleaseEvidenceBucket::StateModel,
        test_names: vec!["workflow_state_model".to_owned()],
        manual_qa_refs: Vec::new(),
        evidence_refs: vec![evidence(
            "wrong-owner",
            Some("018"),
            RedactionStatus::AlreadySafe,
        )],
    }];
    let checklist = workflow_prd000_release_evidence_checklist(&invalid_owner);
    assert!(!checklist.passed);
    assert!(checklist
        .missing_buckets
        .contains(&WorkflowPrd000ReleaseEvidenceBucket::StateModel));
}

#[test]
fn workflow_barrier_verifier_and_synthesis_fail_closed() {
    let plan = sample_plan();
    assert_eq!(
        workflow_ready_step_ids(&plan, &[]),
        vec!["extract-claims".to_owned()]
    );

    assert_eq!(
        workflow_barrier_decision(&plan, &[]),
        WorkflowBarrierDecision::Waiting {
            pending_step_ids: vec!["extract-claims".to_owned()]
        }
    );

    let result = WorkflowChildResult {
        child_id: "child-1".to_owned(),
        step_id: "extract-claims".to_owned(),
        status: WorkflowChildRunStatus::Completed,
        summary: "claims extracted".to_owned(),
        evidence_refs: vec![evidence("child", Some("024"), RedactionStatus::AlreadySafe)],
    };
    assert_eq!(
        workflow_barrier_decision(&plan, std::slice::from_ref(&result)),
        WorkflowBarrierDecision::Ready {
            ready_step_ids: vec!["extract-claims".to_owned()]
        }
    );

    let running = WorkflowChildResult {
        child_id: "child-1".to_owned(),
        step_id: "extract-claims".to_owned(),
        status: WorkflowChildRunStatus::Running,
        summary: "still running".to_owned(),
        evidence_refs: Vec::new(),
    };
    assert_eq!(
        workflow_barrier_decision(&plan, &[running]),
        WorkflowBarrierDecision::Waiting {
            pending_step_ids: vec!["extract-claims".to_owned()]
        }
    );

    let failed_child = WorkflowChildResult {
        child_id: "child-1".to_owned(),
        step_id: "extract-claims".to_owned(),
        status: WorkflowChildRunStatus::Failed,
        summary: "failed after duplicate completion".to_owned(),
        evidence_refs: Vec::new(),
    };
    assert!(matches!(
        workflow_barrier_decision(&plan, &[result.clone(), failed_child]),
        WorkflowBarrierDecision::Blocked { reason } if reason.contains("child-1")
    ));

    assert!(matches!(
        workflow_verification_gate(&plan, &[]),
        WorkflowVerificationGate::Blocked { missing_verifier_ids }
            if missing_verifier_ids == vec!["verifier-1".to_owned()]
    ));

    let mut missing_verifier_spec = plan.clone();
    missing_verifier_spec.verifier_graph.clear();
    assert!(matches!(
        workflow_verification_gate(&missing_verifier_spec, &[]),
        WorkflowVerificationGate::Blocked { missing_verifier_ids }
            if missing_verifier_ids == vec!["child-1".to_owned()]
    ));

    let failed_verdict = WorkflowVerifierVerdict {
        verifier_id: "verifier-1".to_owned(),
        target_child_id: "child-1".to_owned(),
        verdict: WorkflowVerifierVerdictKind::Fail,
        summary: "missing evidence".to_owned(),
        evidence_refs: Vec::new(),
    };
    assert_eq!(
        workflow_verification_gate(&plan, std::slice::from_ref(&failed_verdict)),
        WorkflowVerificationGate::Failed {
            failing_child_ids: vec!["child-1".to_owned()]
        }
    );

    let pass_verdict = WorkflowVerifierVerdict {
        verifier_id: "verifier-1".to_owned(),
        target_child_id: "child-1".to_owned(),
        verdict: WorkflowVerifierVerdictKind::Pass,
        summary: "looks good".to_owned(),
        evidence_refs: vec![evidence("verifier", Some("024"), RedactionStatus::Redacted)],
    };
    assert_eq!(
        workflow_verifier_evidence_status(
            &plan.verifier_graph[0].evidence_contract(),
            &pass_verdict
        ),
        WorkflowVerifierEvidenceStatus::Satisfied
    );

    let missing_evidence_verdict = WorkflowVerifierVerdict {
        evidence_refs: Vec::new(),
        ..pass_verdict.clone()
    };
    assert_eq!(
        workflow_verification_gate(&plan, std::slice::from_ref(&missing_evidence_verdict)),
        WorkflowVerificationGate::Failed {
            failing_child_ids: vec!["child-1".to_owned()]
        }
    );

    let wrong_owner_verdict = WorkflowVerifierVerdict {
        evidence_refs: vec![evidence(
            "wrong-owner",
            Some("018"),
            RedactionStatus::Redacted,
        )],
        ..pass_verdict.clone()
    };
    assert!(matches!(
        workflow_verifier_evidence_status(
            &plan.verifier_graph[0].evidence_contract(),
            &wrong_owner_verdict
        ),
        WorkflowVerifierEvidenceStatus::Invalid { reason, .. } if reason.contains("owner")
    ));
    assert_eq!(
        workflow_verification_gate(&plan, &[pass_verdict, failed_verdict]),
        WorkflowVerificationGate::Failed {
            failing_child_ids: vec!["child-1".to_owned()]
        }
    );

    let passed_gate = WorkflowVerificationGate::Passed;
    let synthesis = workflow_synthesis_outcome(
        &plan,
        std::slice::from_ref(&result),
        &passed_gate,
        &plan.merge_policy,
    );
    assert_eq!(synthesis.accepted_child_ids, vec!["child-1".to_owned()]);
    assert!(synthesis.rejected_child_ids.is_empty());
    assert!(synthesis.unresolved_child_ids.is_empty());
    assert!(synthesis.final_success_allowed);

    let mut permissive_merge = plan.merge_policy.clone();
    permissive_merge.require_verifier_pass = false;
    let blocked_gate = WorkflowVerificationGate::Blocked {
        missing_verifier_ids: vec!["verifier-1".to_owned()],
    };
    let blocked_required_verifier = workflow_synthesis_outcome(
        &plan,
        std::slice::from_ref(&result),
        &blocked_gate,
        &permissive_merge,
    );
    assert!(!blocked_required_verifier.final_success_allowed);

    let mut no_required_verifier = plan.clone();
    no_required_verifier.child_graph[0].verifier_required = false;
    let permissive_without_required_verifier = workflow_synthesis_outcome(
        &no_required_verifier,
        std::slice::from_ref(&result),
        &blocked_gate,
        &permissive_merge,
    );
    assert!(permissive_without_required_verifier.final_success_allowed);

    let unresolved = WorkflowChildResult {
        child_id: "child-2".to_owned(),
        step_id: "extract-claims".to_owned(),
        status: WorkflowChildRunStatus::Running,
        summary: "still running".to_owned(),
        evidence_refs: Vec::new(),
    };
    let blocked =
        workflow_synthesis_outcome(&plan, &[unresolved], &passed_gate, &plan.merge_policy);
    assert!(!blocked.final_success_allowed);
}

#[test]
fn workflow_ready_schedule_validates_dag_and_bounds_fan_out() {
    let mut plan = sample_plan();
    plan.steps.push(WorkflowStep {
        step_id: "verify-claims".to_owned(),
        label: "Verify claims".to_owned(),
        pattern: WorkflowPattern::AdversarialVerification,
        depends_on: vec!["extract-claims".to_owned()],
        required: true,
        expected_output_schema: None,
    });
    plan.child_graph.push(WorkflowChildSpec {
        child_id: "child-2".to_owned(),
        step_id: "verify-claims".to_owned(),
        goal: "verify factual claims".to_owned(),
        tool_scope_ref: Some("scope-1".to_owned()),
        worktree_policy: WorkflowWorktreePolicy::ReadOnlySnapshot,
        budget: WorkflowBudgetSlice {
            max_tokens: Some(1_000),
            max_wall_clock_ms: Some(30_000),
        },
        verifier_required: false,
    });
    plan.budget_policy.max_parallel_children = 1;

    assert_eq!(
        validate_workflow_plan(&plan),
        WorkflowPlanValidationStatus::Valid
    );
    assert_eq!(
        workflow_ready_schedule_decision(&plan, &[], &[], &[]),
        WorkflowReadyScheduleDecision::Ready {
            ready_step_ids: vec!["extract-claims".to_owned()],
            ready_child_ids: vec!["child-1".to_owned()],
            deferred_child_ids: Vec::new(),
        }
    );
    assert_eq!(
        workflow_ready_schedule_decision(
            &plan,
            &["extract-claims".to_owned()],
            &["child-1".to_owned()],
            &[],
        ),
        WorkflowReadyScheduleDecision::Ready {
            ready_step_ids: vec!["verify-claims".to_owned()],
            ready_child_ids: vec!["child-2".to_owned()],
            deferred_child_ids: Vec::new(),
        }
    );

    let mut cyclic = plan;
    cyclic.steps[0].depends_on = vec!["verify-claims".to_owned()];
    assert!(matches!(
        validate_workflow_plan(&cyclic),
        WorkflowPlanValidationStatus::Invalid { reasons }
            if reasons.iter().any(|reason| reason.contains("cycle"))
    ));
}

#[test]
fn workflow_pattern_contracts_cover_supported_and_blocked_semantics() {
    let mut generate_filter = sample_plan();
    generate_filter.pattern = WorkflowPattern::GenerateAndFilter;
    assert_eq!(
        workflow_pattern_contract_status(&generate_filter),
        WorkflowPatternContractStatus::Satisfied
    );
    let evidence = workflow_pattern_contract_evidence(&generate_filter);
    assert_eq!(evidence.pattern, WorkflowPattern::GenerateAndFilter);
    assert!(evidence.static_dag);

    let mut tournament = sample_plan();
    tournament.pattern = WorkflowPattern::Tournament;
    tournament.stop_condition.no_new_findings_threshold = None;
    assert!(matches!(
        workflow_pattern_contract_status(&tournament),
        WorkflowPatternContractStatus::Blocked { reasons }
            if reasons.iter().any(|reason| reason.contains("bounded rounds"))
    ));
    tournament.stop_condition.no_new_findings_threshold = Some(2);
    assert_eq!(
        workflow_pattern_contract_status(&tournament),
        WorkflowPatternContractStatus::Satisfied
    );

    let mut loop_plan = sample_plan();
    loop_plan.pattern = WorkflowPattern::LoopUntilDone;
    assert!(matches!(
        workflow_pattern_contract_status(&loop_plan),
        WorkflowPatternContractStatus::Blocked { reasons }
            if reasons.iter().any(|reason| reason.contains("bounded stop"))
    ));
    loop_plan.stop_condition.no_new_findings_threshold = Some(1);
    assert_eq!(
        workflow_pattern_contract_status(&loop_plan),
        WorkflowPatternContractStatus::Satisfied
    );

    let mut hybrid = sample_plan();
    hybrid.pattern = WorkflowPattern::Hybrid;
    hybrid.steps[0].pattern = WorkflowPattern::Hybrid;
    assert!(matches!(
        workflow_pattern_contract_status(&hybrid),
        WorkflowPatternContractStatus::Blocked { reasons }
            if reasons.iter().any(|reason| reason.contains("decompose"))
    ));
}

#[test]
fn workflow_privileged_actor_separation_requires_sanitizer_handoff_and_scopes_tools() {
    let mut plan = sample_plan();
    plan.tool_scope_policy.quarantine = WorkflowQuarantinePolicy::PrivilegedActorSeparated;
    plan.tool_scope_policy.allowed_tools = vec![
        "read_file".to_owned(),
        "grep".to_owned(),
        "write_file".to_owned(),
    ];
    plan.worktree_policy = WorkflowWorktreePolicy::IsolatedWorktreeRequired;
    plan.steps.push(WorkflowStep {
        step_id: "privileged-write".to_owned(),
        label: "Privileged write".to_owned(),
        pattern: WorkflowPattern::WorkflowSequence,
        depends_on: vec!["extract-claims".to_owned()],
        required: true,
        expected_output_schema: None,
    });
    plan.child_graph.push(WorkflowChildSpec {
        child_id: "child-2".to_owned(),
        step_id: "privileged-write".to_owned(),
        goal: "apply sanitized change".to_owned(),
        tool_scope_ref: Some("scope-privileged".to_owned()),
        worktree_policy: WorkflowWorktreePolicy::IsolatedWorktreeRequired,
        budget: WorkflowBudgetSlice {
            max_tokens: Some(1_000),
            max_wall_clock_ms: Some(30_000),
        },
        verifier_required: false,
    });

    let available_tools = vec![
        "read_file".to_owned(),
        "grep".to_owned(),
        "write_file".to_owned(),
    ];
    assert_eq!(
        workflow_role_scoped_tool_names(
            &plan.tool_scope_policy,
            WorkflowToolScopeRole::Sanitizer,
            &available_tools,
        ),
        vec!["read_file".to_owned(), "grep".to_owned()]
    );
    assert_eq!(
        workflow_role_scoped_tool_names(
            &plan.tool_scope_policy,
            WorkflowToolScopeRole::PrivilegedActor,
            &available_tools,
        ),
        vec!["write_file".to_owned()]
    );
    let WorkflowSanitizedHandoffStatus::Validated { contract } =
        workflow_sanitized_handoff_status(&plan)
    else {
        panic!("expected sanitized handoff contract");
    };
    assert_eq!(contract.sanitizer_step_id, "extract-claims");
    assert_eq!(contract.privileged_step_id, "privileged-write");
    assert_eq!(
        workflow_sanitized_handoff_evidence_status(
            &contract,
            &WorkflowSanitizedHandoffEvidence {
                sanitizer_step_id: "extract-claims".to_owned(),
                privileged_step_id: "privileged-write".to_owned(),
                sanitizer_output_digest: "safe-digest".to_owned(),
                privileged_input_digest: "safe-digest".to_owned(),
                raw_untrusted_digest: Some("raw-digest".to_owned()),
            },
        ),
        WorkflowSanitizedHandoffEvidenceStatus::Validated
    );
    assert!(matches!(
        workflow_sanitized_handoff_evidence_status(
            &contract,
            &WorkflowSanitizedHandoffEvidence {
                sanitizer_step_id: "extract-claims".to_owned(),
                privileged_step_id: "privileged-write".to_owned(),
                sanitizer_output_digest: "raw-digest".to_owned(),
                privileged_input_digest: "raw-digest".to_owned(),
                raw_untrusted_digest: Some("raw-digest".to_owned()),
            },
        ),
        WorkflowSanitizedHandoffEvidenceStatus::Blocked { reason }
            if reason.contains("raw untrusted")
    ));

    plan.steps[1].depends_on.clear();
    assert!(matches!(
        workflow_sanitized_handoff_status(&plan),
        WorkflowSanitizedHandoffStatus::Blocked { reason }
            if reason.contains("sanitizer dependency")
    ));
}

#[test]
fn workflow_worktree_budget_and_model_routing_contracts_are_explicit() {
    let plan = sample_plan();

    assert_eq!(
        workflow_worktree_decision(&WorkflowWorktreeRequest {
            workflow_id: Some("workflow-1".to_owned()),
            child_id: "child-1".to_owned(),
            requires_write: true,
            policy: WorkflowWorktreePolicy::IsolatedWorktreeRequired,
            approval_granted: false,
            existing_worktree_ref: None,
        }),
        WorkflowWorktreeDecision::Blocked {
            reason: "isolated worktree requires orchestrator approval".to_owned()
        }
    );
    assert_eq!(
        workflow_worktree_decision(&WorkflowWorktreeRequest {
            workflow_id: Some("workflow-1".to_owned()),
            child_id: "child-1".to_owned(),
            requires_write: true,
            policy: WorkflowWorktreePolicy::IsolatedWorktreeRequired,
            approval_granted: true,
            existing_worktree_ref: None,
        }),
        WorkflowWorktreeDecision::CreateIsolated {
            branch_name: "workflow/workflow-1/child-1".to_owned()
        }
    );
    assert_eq!(
        workflow_worktree_branch_name(&WorkflowWorktreeRequest {
            workflow_id: Some("workflow 1".to_owned()),
            child_id: "child/one".to_owned(),
            requires_write: true,
            policy: WorkflowWorktreePolicy::IsolatedWorktreeRequired,
            approval_granted: true,
            existing_worktree_ref: None,
        }),
        "workflow/workflow-1/child-one"
    );

    assert_eq!(
        workflow_budget_decision(
            &plan.budget_policy,
            &WorkflowBudgetUsage {
                known_tokens: 10_000,
                estimated_tokens: 0,
                child_runs: 1,
                verifier_runs: 0,
                heavy_commands: 0,
            }
        ),
        WorkflowBudgetDecision::Blocked {
            reason: "workflow token budget exhausted".to_owned()
        }
    );

    let mut exact_policy = plan.budget_policy.clone();
    exact_policy.max_iterations = 1;
    exact_policy.max_heavy_commands = Some(1);
    assert!(matches!(
        workflow_budget_decision(
            &exact_policy,
            &WorkflowBudgetUsage {
                known_tokens: 1_000,
                estimated_tokens: 0,
                child_runs: 1,
                verifier_runs: 0,
                heavy_commands: 1,
            },
        ),
        WorkflowBudgetDecision::Allowed { .. }
    ));

    let route = workflow_model_route_snapshot(&plan.model_routing_policy, "verifier");
    assert_eq!(route.role, "verifier");
    assert_eq!(route.selected_model_hint, Some("strong".to_owned()));
    assert_eq!(route.fallback_model_policy, "use provider default");
}

#[test]
fn workflow_runtime_enforcement_applies_budget_timeout_parallelism_and_route() {
    let mut plan = sample_plan();
    plan.budget_policy.max_heavy_commands = Some(2);
    let usage = WorkflowBudgetUsage {
        known_tokens: 1_000,
        estimated_tokens: 500,
        child_runs: 1,
        verifier_runs: 0,
        heavy_commands: 0,
    };

    assert_eq!(
        shacs_workflow::workflow_runtime_enforcement_decision(
            &plan.budget_policy,
            &plan.model_routing_policy,
            &WorkflowRuntimeEnforcementInput {
                role: WorkflowExecutionRole::Child,
                usage: usage.clone(),
                active_child_count: 1,
                elapsed_wall_clock_ms: 1_000,
                requested_tokens: Some(250),
                requests_heavy_command: false,
            },
        ),
        WorkflowRuntimeEnforcementDecision::Allowed {
            route: shacs_workflow::workflow_model_route_snapshot(
                &plan.model_routing_policy,
                "child"
            ),
            remaining_tokens: Some(8_250),
        }
    );

    assert_eq!(
        shacs_workflow::workflow_runtime_enforcement_decision(
            &plan.budget_policy,
            &plan.model_routing_policy,
            &WorkflowRuntimeEnforcementInput {
                role: WorkflowExecutionRole::Child,
                usage: usage.clone(),
                active_child_count: 2,
                elapsed_wall_clock_ms: 1_000,
                requested_tokens: Some(250),
                requests_heavy_command: false,
            },
        ),
        WorkflowRuntimeEnforcementDecision::Throttled {
            reason: "workflow parallel child limit reached".to_owned(),
        }
    );

    assert_eq!(
        shacs_workflow::workflow_runtime_enforcement_decision(
            &plan.budget_policy,
            &plan.model_routing_policy,
            &WorkflowRuntimeEnforcementInput {
                role: WorkflowExecutionRole::Verifier,
                usage: usage.clone(),
                active_child_count: 0,
                elapsed_wall_clock_ms: 1_000,
                requested_tokens: Some(2_001),
                requests_heavy_command: false,
            },
        ),
        WorkflowRuntimeEnforcementDecision::Blocked {
            reason: "workflow verifier token slice exceeded".to_owned(),
        }
    );

    assert_eq!(
        shacs_workflow::workflow_runtime_enforcement_decision(
            &plan.budget_policy,
            &plan.model_routing_policy,
            &WorkflowRuntimeEnforcementInput {
                role: WorkflowExecutionRole::Synthesis,
                usage,
                active_child_count: 0,
                elapsed_wall_clock_ms: 120_000,
                requested_tokens: None,
                requests_heavy_command: false,
            },
        ),
        WorkflowRuntimeEnforcementDecision::Blocked {
            reason: "workflow wall-clock budget exhausted".to_owned(),
        }
    );

    let mut exhausted = plan.budget_policy.clone();
    exhausted.max_iterations = 1;
    assert_eq!(
        shacs_workflow::workflow_runtime_enforcement_decision(
            &exhausted,
            &plan.model_routing_policy,
            &WorkflowRuntimeEnforcementInput {
                role: WorkflowExecutionRole::Child,
                usage: WorkflowBudgetUsage {
                    known_tokens: 1_000,
                    estimated_tokens: 0,
                    child_runs: 1,
                    verifier_runs: 0,
                    heavy_commands: 0,
                },
                active_child_count: 0,
                elapsed_wall_clock_ms: 1_000,
                requested_tokens: None,
                requests_heavy_command: false,
            },
        ),
        WorkflowRuntimeEnforcementDecision::Blocked {
            reason: "workflow iteration budget exhausted".to_owned(),
        }
    );

    let mut heavy_exhausted = plan.budget_policy.clone();
    heavy_exhausted.max_heavy_commands = Some(1);
    assert_eq!(
        shacs_workflow::workflow_runtime_enforcement_decision(
            &heavy_exhausted,
            &plan.model_routing_policy,
            &WorkflowRuntimeEnforcementInput {
                role: WorkflowExecutionRole::Child,
                usage: WorkflowBudgetUsage {
                    known_tokens: 1_000,
                    estimated_tokens: 0,
                    child_runs: 0,
                    verifier_runs: 0,
                    heavy_commands: 1,
                },
                active_child_count: 0,
                elapsed_wall_clock_ms: 1_000,
                requested_tokens: None,
                requests_heavy_command: true,
            },
        ),
        WorkflowRuntimeEnforcementDecision::Blocked {
            reason: "workflow heavy command budget exhausted".to_owned(),
        }
    );
}

#[test]
fn workflow_recipe_quarantine_and_permission_ceiling_preserve_safety_boundaries() {
    let plan = sample_plan();
    assert_eq!(
        workflow_recipe_readiness(&WorkflowRecipe {
            recipe_id: "review".to_owned(),
            source_ref: "skill://workflow-review".to_owned(),
            pattern: WorkflowPattern::AdversarialVerification,
            prompt_template_ref: "prompt://review".to_owned(),
            rubric_ref: Some("rubric://claims".to_owned()),
            output_schema_ref: None,
            suggested_budget_tokens: Some(4_000),
            suggested_tool_scope_ref: Some("scope://read-only".to_owned()),
            safety_notes: vec!["keep untrusted input read-only".to_owned()],
        }),
        WorkflowRecipeReadiness::Ready
    );
    assert!(matches!(
        workflow_recipe_readiness(&WorkflowRecipe {
            recipe_id: "".to_owned(),
            source_ref: "".to_owned(),
            pattern: WorkflowPattern::FanOutAndSynthesize,
            prompt_template_ref: "".to_owned(),
            rubric_ref: None,
            output_schema_ref: None,
            suggested_budget_tokens: None,
            suggested_tool_scope_ref: None,
            safety_notes: Vec::new(),
        }),
        WorkflowRecipeReadiness::Malformed { reasons } if reasons.len() == 3
    ));

    assert_eq!(
        workflow_quarantine_decision(
            WorkflowQuarantinePolicy::ReadOnlyUntrusted,
            WorkflowStepPrivilege::PrivilegedAction,
        ),
        WorkflowQuarantineDecision::Blocked {
            reason: "read-only untrusted workflow cannot perform privileged action".to_owned()
        }
    );
    assert_eq!(
        workflow_quarantine_decision(
            WorkflowQuarantinePolicy::PrivilegedActorSeparated,
            WorkflowStepPrivilege::PrivilegedAction,
        ),
        WorkflowQuarantineDecision::RequiresSanitizedHandoff
    );

    assert_eq!(
        workflow_permission_ceiling_decision(
            &plan.permission_policy,
            &["proc_exec".to_owned()],
            false,
        ),
        WorkflowPermissionCeilingDecision::Blocked {
            denied_capability: "proc_exec".to_owned()
        }
    );
    assert_eq!(
        workflow_permission_ceiling_decision(&plan.permission_policy, &[], true),
        WorkflowPermissionCeilingDecision::ApprovalRequired {
            reason: "privileged workflow step requires approval".to_owned()
        }
    );
}

#[test]
fn workflow_projection_diagnostics_and_spec024_release_gate_are_evidence_backed(
) -> Result<(), Box<dyn std::error::Error>> {
    let plan = sample_plan();
    let run = admit_workflow_plan(&plan, 100)?;
    let checkpoint = build_workflow_checkpoint(
        &plan,
        &run,
        WorkflowCheckpointInput {
            state: WorkflowRunState::WaitingForChildren,
            completed_steps: vec!["extract-claims".to_owned()],
            active_children: vec!["child-1".to_owned()],
            pending_barriers: vec!["barrier-1".to_owned()],
            budget_usage: WorkflowBudgetUsage {
                known_tokens: 1_000,
                estimated_tokens: 500,
                child_runs: 1,
                verifier_runs: 0,
                heavy_commands: 0,
            },
            worktree_refs: vec!["worktree://child-1".to_owned()],
            evidence_refs: vec!["evidence://checkpoint".to_owned()],
            last_safe_resume_point: "after-child".to_owned(),
            recorded_at_ms: 200,
        },
    );

    let projection = workflow_projection(
        &run,
        &plan,
        Some(&checkpoint),
        &WorkflowVerificationGate::Passed,
        &[
            evidence("projection", Some("024"), RedactionStatus::AlreadySafe),
            evidence("wrong-owner", Some("018"), RedactionStatus::AlreadySafe),
        ],
    );
    assert_eq!(projection.schema_version, "024WorkflowProjection.v1");
    assert_eq!(projection.progress_count, 1);
    assert_eq!(projection.active_child_count, 1);
    assert_eq!(projection.verifier_status, "passed");
    assert!(projection.resume_available);
    assert_eq!(projection.evidence_refs.len(), 1);

    let manifest = workflow_diagnostics_manifest(
        &plan,
        vec!["stale-child".to_owned()],
        vec![
            evidence("diag", Some("024"), RedactionStatus::Redacted),
            evidence("unsafe", Some("024"), RedactionStatus::RedactionFailed),
        ],
    )?;
    assert_eq!(manifest.workflow_id, "workflow-1");
    assert_eq!(manifest.stale_result_refs, vec!["stale-child".to_owned()]);
    assert!(manifest.runtime_diagnostic_refs.is_empty());
    assert!(!manifest.replay_live_actions_allowed);
    assert_eq!(manifest.evidence_refs.len(), 1);

    let runtime_manifest = workflow_runtime_diagnostics_manifest(
        &plan,
        WorkflowRuntimeDiagnosticsInput {
            merge_decision_ref: Some("workflow://workflow-1/synthesis/final".to_owned()),
            stale_result_refs: vec!["stale-child".to_owned()],
            recipe_source_refs: vec!["workflow://workflow-1/recipe/source".to_owned()],
            barrier_refs: vec!["workflow://workflow-1/barrier/waiting".to_owned()],
            tool_scope_refs: vec!["workflow://workflow-1/tool-scope/scope-digest".to_owned()],
            verifier_refs: vec!["workflow://workflow-1/verifier/verifier-1".to_owned()],
            merge_refs: vec!["workflow://workflow-1/merge/final".to_owned()],
            synthesis_refs: vec!["workflow://workflow-1/synthesis/accepted=1".to_owned()],
            cleanup_refs: vec!["workflow://workflow-1/cleanup/worktree".to_owned()],
            evidence_refs: vec![evidence(
                "runtime-diag",
                Some("024"),
                RedactionStatus::Redacted,
            )],
        },
    )?;
    assert_eq!(
        runtime_manifest.merge_decision_ref.as_deref(),
        Some("workflow://workflow-1/synthesis/final")
    );
    assert_eq!(
        runtime_manifest.stale_result_refs,
        vec!["stale-child".to_owned()]
    );
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/recipe/")));
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/barrier/")));
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/tool-scope/")));
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/verifier/")));
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/merge/")));
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/synthesis/")));
    assert!(runtime_manifest
        .runtime_diagnostic_refs
        .iter()
        .any(|reference| reference.contains("/cleanup/")));
    assert_eq!(runtime_manifest.evidence_refs.len(), 1);

    let complete = WorkflowSpec024ReleaseEvidenceBucket::required_buckets()
        .into_iter()
        .map(|bucket| WorkflowSpec024ReleaseEvidence {
            bucket,
            test_names: vec![format!("test-{bucket:?}")],
            manual_qa_refs: Vec::new(),
            evidence_refs: vec![evidence(
                "release",
                Some("024"),
                RedactionStatus::AlreadySafe,
            )],
        })
        .collect::<Vec<_>>();
    assert!(workflow_spec024_release_evidence_checklist(&complete).passed);
    assert!(complete
        .iter()
        .any(|entry| entry.bucket == WorkflowSpec024ReleaseEvidenceBucket::Prd009RuntimeExecution));

    let incomplete = [WorkflowSpec024ReleaseEvidence {
        bucket: WorkflowSpec024ReleaseEvidenceBucket::Prd001PatternChildGraph,
        test_names: vec!["workflow_barrier_verifier_and_synthesis_fail_closed".to_owned()],
        manual_qa_refs: Vec::new(),
        evidence_refs: vec![evidence("bad", Some("018"), RedactionStatus::AlreadySafe)],
    }];
    assert!(!workflow_spec024_release_evidence_checklist(&incomplete).passed);

    Ok(())
}
