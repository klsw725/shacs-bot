pub use shacs_core::runtime::{
    own_automation_lifecycle, AdapterSandboxRef, AutomationConfirmationFact,
    AutomationDeliveryResult, AutomationExecutionRequirements, AutomationJobResult,
    AutomationLifecycleInput, AutomationNoDispatchReason, AutomationScheduleKind,
    AutomationSourceEvent, AutomationSourceEventKind, ConfigMigrationState, ConfigSnapshotRef,
    CredentialSnapshotRef, DataDisclosureWarning, ExecutionSnapshot, ExecutionSnapshotInput,
    ProfileSelectionSnapshot, ProviderInputSnapshot, ReplayContract, SandboxMode,
    TokenBudgetSnapshot, TrustedRuntimeFactRef,
};
use shacs_eval::evaluator::{AutomationExecutionMode, AutomationRecursionGuard};
use shacs_projection::HookDenialReason;
use shacs_projection::{
    CredentialFingerprintStatus, CredentialStatus, ProcessAdapterKind, SandboxFallback,
};

fn hook_evidence() -> Vec<shacs_core::runtime::PluginHookDispatchRecord> {
    vec![shacs_core::runtime::PluginHookDispatchRecord {
        plugin_id: "hook-owner-1".to_owned(),
        event: shacs_core::runtime::PluginHookEvent::ToolBefore,
        status: shacs_core::runtime::PluginHookDispatchStatus::Succeeded,
        effect: Some(shacs_core::runtime::PluginHookDispatchEffect::Observed),
        output_evidence: None,
        error: None,
        timeout: None,
    }]
}

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
        timeout_policy_ref: "timeout-1".to_owned(),
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
                .expect("semantic digest")
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
fn all_job_kinds_enter_the_same_queued_lifecycle() {
    let snapshot = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    for (schedule, mode) in [
        (
            AutomationScheduleKind::OneShot,
            AutomationExecutionMode::NoAgentCheck,
        ),
        (
            AutomationScheduleKind::Recurring,
            AutomationExecutionMode::NoAgentCheck,
        ),
        (
            AutomationScheduleKind::OneShot,
            AutomationExecutionMode::SkillBackedAgent,
        ),
        (
            AutomationScheduleKind::OneShot,
            AutomationExecutionMode::ScriptOnly,
        ),
        (
            AutomationScheduleKind::OneShot,
            AutomationExecutionMode::AppTask,
        ),
        (
            AutomationScheduleKind::Recurring,
            AutomationExecutionMode::SkillBackedAgent,
        ),
    ] {
        let evidence = hook_evidence();
        let event = source_event(mode.clone());
        let outcome = own_automation_lifecycle(AutomationLifecycleInput {
            event: &event,
            schedule: schedule.clone(),
            existing_runs: &[],
            durable_work: None,
            execution_snapshot: Some(&snapshot),
            expected_snapshot_digest: &snapshot
                .semantic_compatibility_digest()
                .expect("semantic digest"),
            hook_evidence: Some(&evidence),
            hook_denial: None,
            requirements: requirements(),
            job_result: AutomationJobResult::Pending,
            delivery_result: AutomationDeliveryResult::NotRequested,
        });

        assert!(outcome.dispatch_request.is_some(), "{schedule:?} {mode:?}");
        assert_eq!(outcome.lifecycle.schedule, schedule);
        assert_eq!(outcome.lifecycle.execution_mode, mode);
    }
}

#[test]
fn denied_gates_never_produce_dispatch_requests() {
    let active = snapshot(SandboxMode::Active, SandboxFallback::NotApplicable);
    let denied_cases = [
        (
            None,
            &active,
            Some(HookDenialReason::ExtensionBlocked),
            requirements(),
            AutomationNoDispatchReason::HookVeto,
        ),
        (
            None,
            &active,
            None,
            AutomationExecutionRequirements {
                confirmation: AutomationConfirmationFact::HeadlessDenied,
                ..requirements()
            },
            AutomationNoDispatchReason::HeadlessConfirmationDenied,
        ),
    ];
    for (durable_work, snapshot, hook_denial, requirements, expected) in denied_cases {
        let evidence = hook_evidence();
        let event = source_event(AutomationExecutionMode::ScriptOnly);
        let outcome = own_automation_lifecycle(AutomationLifecycleInput {
            event: &event,
            schedule: AutomationScheduleKind::OneShot,
            existing_runs: &[],
            durable_work,
            execution_snapshot: Some(snapshot),
            expected_snapshot_digest: &snapshot
                .semantic_compatibility_digest()
                .expect("semantic digest"),
            hook_evidence: Some(&evidence),
            hook_denial,
            requirements,
            job_result: AutomationJobResult::Pending,
            delivery_result: AutomationDeliveryResult::NotRequested,
        });
        assert!(outcome.dispatch_request.is_none());
        assert_eq!(outcome.no_dispatch_reason, Some(expected));
    }
}
