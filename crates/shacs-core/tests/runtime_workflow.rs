use serde_json::json;
use shacs_core::runtime::{
    admit_workflow_plan, build_workflow_checkpoint, decide_workflow_admission,
    workflow_barrier_decision, workflow_budget_decision, workflow_diagnostics_manifest,
    workflow_harness_plan_digest, workflow_model_route_snapshot,
    workflow_permission_ceiling_decision, workflow_prd000_release_evidence_checklist,
    workflow_projection, workflow_quarantine_decision, workflow_ready_step_ids,
    workflow_recipe_readiness, workflow_resume_decision,
    workflow_spec024_release_evidence_checklist, workflow_synthesis_outcome,
    workflow_verification_gate, workflow_worktree_decision, WorkflowAdmissionDecision,
    WorkflowAdmissionInput, WorkflowBarrierDecision, WorkflowBudgetDecision, WorkflowBudgetPolicy,
    WorkflowBudgetSlice, WorkflowBudgetUsage, WorkflowCheckpointInput, WorkflowCheckpointPolicy,
    WorkflowChildResult, WorkflowChildRunStatus, WorkflowChildSpec, WorkflowContextPolicy,
    WorkflowHarnessPlan, WorkflowMergePolicy, WorkflowModelRoutingPolicy, WorkflowPattern,
    WorkflowPermissionCeilingDecision, WorkflowPermissionPolicy, WorkflowPrd000ReleaseEvidence,
    WorkflowPrd000ReleaseEvidenceBucket, WorkflowQuarantineDecision, WorkflowQuarantinePolicy,
    WorkflowRecipe, WorkflowRecipeReadiness, WorkflowResumeDecision, WorkflowResumePolicy,
    WorkflowRunState, WorkflowSpec024ReleaseEvidence, WorkflowSpec024ReleaseEvidenceBucket,
    WorkflowStep, WorkflowStepPrivilege, WorkflowStopCondition, WorkflowToolScopePolicy,
    WorkflowVerificationGate, WorkflowVerifierSpec, WorkflowVerifierVerdict,
    WorkflowVerifierVerdictKind, WorkflowWorktreeDecision, WorkflowWorktreePolicy,
    WorkflowWorktreeRequest,
};
use shacs_utils::evaluator::{EvidenceKind, EvidenceRef, RedactionStatus};

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
        evidence_refs: Vec::new(),
    };
    assert_eq!(
        workflow_verification_gate(&plan, &[pass_verdict, failed_verdict]),
        WorkflowVerificationGate::Failed {
            failing_child_ids: vec!["child-1".to_owned()]
        }
    );

    let passed_gate = WorkflowVerificationGate::Passed;
    let synthesis = workflow_synthesis_outcome(&[result], &passed_gate, &plan.merge_policy);
    assert_eq!(synthesis.accepted_child_ids, vec!["child-1".to_owned()]);
    assert!(synthesis.rejected_child_ids.is_empty());
    assert!(synthesis.unresolved_child_ids.is_empty());
    assert!(synthesis.final_success_allowed);

    let unresolved = WorkflowChildResult {
        child_id: "child-2".to_owned(),
        step_id: "extract-claims".to_owned(),
        status: WorkflowChildRunStatus::Running,
        summary: "still running".to_owned(),
        evidence_refs: Vec::new(),
    };
    let blocked = workflow_synthesis_outcome(&[unresolved], &passed_gate, &plan.merge_policy);
    assert!(!blocked.final_success_allowed);
}

#[test]
fn workflow_worktree_budget_and_model_routing_contracts_are_explicit() {
    let plan = sample_plan();

    assert_eq!(
        workflow_worktree_decision(&WorkflowWorktreeRequest {
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
            child_id: "child-1".to_owned(),
            requires_write: true,
            policy: WorkflowWorktreePolicy::IsolatedWorktreeRequired,
            approval_granted: true,
            existing_worktree_ref: None,
        }),
        WorkflowWorktreeDecision::CreateIsolated {
            branch_name: "workflow/child-1".to_owned()
        }
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

    let route = workflow_model_route_snapshot(&plan.model_routing_policy, "verifier");
    assert_eq!(route.role, "verifier");
    assert_eq!(route.selected_model_hint, Some("strong".to_owned()));
    assert_eq!(route.fallback_model_policy, "use provider default");
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
    assert_eq!(manifest.evidence_refs.len(), 1);

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

    let incomplete = [WorkflowSpec024ReleaseEvidence {
        bucket: WorkflowSpec024ReleaseEvidenceBucket::Prd001PatternChildGraph,
        test_names: vec!["workflow_barrier_verifier_and_synthesis_fail_closed".to_owned()],
        manual_qa_refs: Vec::new(),
        evidence_refs: vec![evidence("bad", Some("018"), RedactionStatus::AlreadySafe)],
    }];
    assert!(!workflow_spec024_release_evidence_checklist(&incomplete).passed);

    Ok(())
}
