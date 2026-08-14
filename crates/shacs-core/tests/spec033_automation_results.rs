pub use shacs_core::runtime::{
    own_automation_lifecycle, AdapterSandboxRef, AutomationConfirmationFact,
    AutomationDeliveryResult, AutomationExecutionRequirements, AutomationJobResult,
    AutomationLifecycleInput, AutomationScheduleKind, AutomationSourceEvent,
    AutomationSourceEventKind, ConfigMigrationState, ConfigSnapshotRef, CredentialSnapshotRef,
    DataDisclosureWarning, ExecutionSnapshot, ExecutionSnapshotInput, ProfileSelectionSnapshot,
    ProviderInputSnapshot, ReplayContract, SandboxMode, TokenBudgetSnapshot, TrustedRuntimeFactRef,
};
use shacs_eval::evaluator::{AutomationExecutionMode, AutomationRecursionGuard, ProjectionSurface};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialStatus, ProcessAdapterKind, SandboxFallback,
};

pub fn source_event(mode: AutomationExecutionMode) -> AutomationSourceEvent {
    AutomationSourceEvent {
        runtime_service_event_id: "event-1".to_owned(),
        source_owner: "runtime-service".to_owned(),
        received_at_ms: 100,
        job_id: "job-1".to_owned(),
        session_id: Some("session-1".to_owned()),
        goal_id: Some("goal-1".to_owned()),
        active_goal: true,
        pending_automation: true,
        execution_mode: mode,
        timeout_policy_ref: "runtime-default".to_owned(),
        retry_policy_ref: "retry-1".to_owned(),
        delivery_policy_ref: "delivery-1".to_owned(),
        recursion_guard: AutomationRecursionGuard {
            token: "guard-1".to_owned(),
            source_run_id: None,
            depth: 0,
            max_depth: 3,
            parent_refs: Vec::new(),
            blocked_reason: None,
        },
        prd008_goal_gate_ref: Some("goal-gate-1".to_owned()),
        source: AutomationSourceEventKind::ManualResume {
            resume_ref: "resume-1".to_owned(),
        },
    }
}

pub fn snapshot(mode: SandboxMode, fallback: SandboxFallback) -> ExecutionSnapshot {
    ExecutionSnapshot::create(ExecutionSnapshotInput {
        snapshot_id: "snapshot-1".to_owned(),
        created_at_unix_ms: 100,
        config: ConfigSnapshotRef {
            source_ref: "config:1".to_owned(),
            schema_version: 1,
            migration_state: ConfigMigrationState::Current,
        },
        profiles: ProfileSelectionSnapshot {
            provider: None,
            trusted_runtime: Some("trusted:1".to_owned()),
            context: None,
        },
        trusted_runtime: TrustedRuntimeFactRef {
            schema_version: 1,
            profile_ref: "trusted:1".to_owned(),
            projection_digest: "sha256:trusted".to_owned(),
        },
        sandbox: vec![AdapterSandboxRef {
            adapter: ProcessAdapterKind::GenericExec,
            mode,
            fallback,
        }],
        credential: CredentialSnapshotRef {
            source_kind: None,
            status: CredentialStatus::Resolved,
            fingerprint_status: CredentialFingerprintStatus::Current,
        },
        context_sources: Vec::new(),
        selected_tools: Vec::new(),
        selected_resources: Vec::new(),
        provider: ProviderInputSnapshot {
            provider: "provider".to_owned(),
            model: "model".to_owned(),
            shaping_version: "v1".to_owned(),
            messages_digest: "sha256:messages".to_owned(),
            tools_digest: "sha256:tools".to_owned(),
        },
        token_budget: TokenBudgetSnapshot {
            tokenizer: "estimate".to_owned(),
            estimator_uncertainty_percent: 0,
            budget_tokens: 100,
            reserved_tokens: 10,
            used_context_tokens: 10,
            estimated_input_tokens: 10,
        },
        disclosure: DataDisclosureWarning {
            raw_content_possible: false,
            surfaces: Vec::new(),
        },
        replay: ReplayContract::diagnostic_only(),
    })
    .expect("valid snapshot fixture")
}

pub fn requirements() -> AutomationExecutionRequirements {
    AutomationExecutionRequirements {
        execution_sensitive: true,
        credential_required: false,
        sandbox_required: false,
        confirmation: AutomationConfirmationFact::NotRequired,
    }
}

pub fn lifecycle_input<'a>(
    event: &'a AutomationSourceEvent,
    snapshot: &'a ExecutionSnapshot,
) -> AutomationLifecycleInput<'a> {
    AutomationLifecycleInput {
        event,
        schedule: AutomationScheduleKind::OneShot,
        existing_runs: &[],
        durable_work: None,
        execution_snapshot: Some(snapshot),
        expected_snapshot_digest: Box::leak(
            snapshot
                .semantic_compatibility_digest()
                .expect("semantic snapshot digest")
                .into_boxed_str(),
        ),
        hook_evidence: None,
        hook_denial: None,
        requirements: requirements(),
        job_result: AutomationJobResult::Pending,
        delivery_result: AutomationDeliveryResult::NotRequested,
    }
}

#[test]
fn timeout_is_a_job_result_without_implying_delivery_failure() {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::SkillBackedAgent);
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        job_result: AutomationJobResult::TimedOut {
            timeout_ref: "timeout:1".to_owned(),
        },
        delivery_result: AutomationDeliveryResult::Succeeded {
            target: ProjectionSurface::LocalApi,
        },
        ..lifecycle_input(&event, &snapshot)
    });
    assert!(outcome.dispatch_request.is_none());
    assert!(matches!(
        outcome.job_result,
        AutomationJobResult::TimedOut { .. }
    ));
    assert!(matches!(
        outcome.delivery_result,
        AutomationDeliveryResult::Succeeded { .. }
    ));
}

#[test]
fn job_and_delivery_results_are_independent_records() {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let event = source_event(AutomationExecutionMode::AppTask);
    let outcome = own_automation_lifecycle(AutomationLifecycleInput {
        job_result: AutomationJobResult::Succeeded {
            result_ref: "job-result:1".to_owned(),
        },
        delivery_result: AutomationDeliveryResult::Failed {
            target: ProjectionSurface::Channel,
            reason_ref: "delivery-failure:1".to_owned(),
        },
        ..lifecycle_input(&event, &snapshot)
    });
    assert!(matches!(
        outcome.job_result,
        AutomationJobResult::Succeeded { .. }
    ));
    assert!(matches!(
        outcome.delivery_result,
        AutomationDeliveryResult::Failed { .. }
    ));
}
