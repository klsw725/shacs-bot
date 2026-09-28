use shacs_core::app::{AppError, AppId};
use shacs_core::app_lifecycle::{AppLifecycleAction, AppLifecycleReceipt, AppProcessState};
use shacs_core::runtime::{
    assemble_spec035_tasks, Spec035LocatedOwnerFact, Spec035TasksAssemblerErrorKind,
    Spec035TasksOwnerSnapshots, Spec035TasksOwnerSource, SurfaceActionOutcome,
    SurfaceActionOutcomeKind,
};
use shacs_projection::{
    Spec031Freshness, Spec031ObservedAtUnixMs, Spec033AutomationFact, Spec033AutomationJobStatus,
    Spec033DeliveryStatus, Spec033GoalBudgetFact, Spec033GoalFact, Spec033GoalStatus,
    Spec033GoalUsageSummary, Spec035TaskActionStatus, Spec035TasksProjection,
};
use shacs_session::durable_child::{ReplayChildTask, ReplayChildTaskState};
use shacs_session::durable_replay::{
    DurableRecoveryStatus, DurableReplayAdmission, DurableReplayState,
};
use shacs_workflow::{WorkflowBudgetUsage, WorkflowPattern, WorkflowProjection, WorkflowRunState};
use std::{fs, path::Path};

pub type FixtureError = Box<dyn std::error::Error>;

pub fn located<T>(locator: &str, fact: T) -> Spec035LocatedOwnerFact<T> {
    Spec035LocatedOwnerFact {
        locator: locator.to_owned(),
        fact,
    }
}

pub fn source<T>(
    observed_at: u64,
    freshness: Spec031Freshness,
    facts: Vec<Spec035LocatedOwnerFact<T>>,
) -> Spec035TasksOwnerSource<T> {
    Spec035TasksOwnerSource::Available {
        observed_at_unix_ms: Spec031ObservedAtUnixMs::new(observed_at),
        freshness,
        facts,
    }
}

pub fn goal(status: Spec033GoalStatus) -> Spec033GoalFact {
    Spec033GoalFact {
        goal_id: "goal-1".to_owned(),
        session_id: "cli:direct".to_owned(),
        status,
        turn_budget: 8,
        turns_used: 3,
        last_verdict: None,
        blocked: status == Spec033GoalStatus::Blocked,
        stop_reason: (status == Spec033GoalStatus::Blocked).then(|| "owner_blocked".to_owned()),
        budget: Spec033GoalBudgetFact {
            turn_budget: 8,
            turns_used: 3,
            remaining_turns: 5,
        },
        usage: Spec033GoalUsageSummary {
            turn_limit: 8,
            turns_used: 3,
            remaining_turns: 5,
            exhausted: false,
        },
        user_interrupted: false,
        latest_transition: None,
    }
}

pub fn automation(status: Spec033AutomationJobStatus) -> Spec033AutomationFact {
    Spec033AutomationFact {
        work_id: "work-1".to_owned(),
        job_id: "job-1".to_owned(),
        run_id: "run-1".to_owned(),
        turn_id: None,
        snapshot_id: None,
        snapshot_digest: None,
        checkpoint_id: None,
        artifact_refs: Vec::new(),
        job_status: status,
        delivery_status: Spec033DeliveryStatus::NotRequested,
    }
}

pub fn child(id: &str, state: ReplayChildTaskState) -> ReplayChildTask {
    ReplayChildTask {
        child_task_id: id.to_owned(),
        session_id: "cli:direct".to_owned(),
        parent_turn_id: "turn-1".to_owned(),
        spawn_effect_id: format!("spawn:{id}"),
        correlation_id: format!("correlation:{id}"),
        idempotency_key: format!("idempotency:{id}"),
        run_ref: None,
        state,
        attempt: 2,
        spawned_sequence: 1,
        spawned_at_ms: 10,
        started_sequence: Some(2),
        started_at_ms: Some(20),
        cancellation_requested_sequence: None,
        cancellation_requested_at_ms: None,
        terminal_sequence: state.is_terminal().then_some(3),
        finished_at_ms: state.is_terminal().then_some(30),
        result_ref: state.is_terminal().then(|| format!("result:{id}")),
    }
}

pub fn workflow(state: WorkflowRunState) -> WorkflowProjection {
    WorkflowProjection {
        schema_label: "024WorkflowProjection".to_owned(),
        schema_version: "024WorkflowProjection.v1".to_owned(),
        workflow_id: "release".to_owned(),
        objective_summary: "safe summary".to_owned(),
        pattern: WorkflowPattern::WorkflowSequence,
        state,
        progress_count: 2,
        active_child_count: 1,
        pending_barrier_count: 0,
        verifier_status: "pending".to_owned(),
        budget_usage: WorkflowBudgetUsage {
            known_tokens: 10,
            estimated_tokens: 0,
            child_runs: 1,
            verifier_runs: 0,
            heavy_commands: 0,
        },
        worktree_refs: Vec::new(),
        blocked_reason: None,
        next_action: None,
        resume_available: false,
        evidence_refs: Vec::new(),
    }
}

pub fn app(
    action: AppLifecycleAction,
    state: AppProcessState,
    completed: bool,
) -> Result<AppLifecycleReceipt, AppError> {
    Ok(AppLifecycleReceipt {
        receipt_id: "receipt:formatter".to_owned(),
        request_id: "request:formatter".to_owned(),
        app_id: AppId::parse("formatter")?,
        action,
        previous_state: AppProcessState::Running,
        current_state: state,
        generation: 1,
        completed,
        manifest_digest: "secret-token-must-not-leak".to_owned(),
        trusted_runtime_ref: "/Users/private/process".to_owned(),
        credential_source_statuses: vec!["API_TOKEN=secret".to_owned()],
        blockers: Vec::new(),
        activation_refs: Vec::new(),
        execution_snapshot_ref: None,
        process_outcome: None,
        occurred_at_unix_ms: 30,
    })
}

pub fn recovery(status: DurableRecoveryStatus) -> DurableReplayAdmission {
    DurableReplayAdmission {
        status,
        writable: status == DurableRecoveryStatus::Healthy,
        state: (status == DurableRecoveryStatus::Healthy).then(DurableReplayState::event_zero),
        checkpoint_used: None,
        replayed_event_count: 1,
        issues: Vec::new(),
        recovery_hints: Vec::new(),
    }
}

pub fn recovered_outcome() -> SurfaceActionOutcome {
    SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Completed,
        changed: true,
        detail: "raw owner detail must not leak".to_owned(),
    }
}

pub fn unavailable_snapshots() -> Spec035TasksOwnerSnapshots {
    Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable)
}

pub fn write_task4_artifact(
    path: impl AsRef<Path>,
    mixed: Spec035TasksProjection,
) -> Result<(), FixtureError> {
    let missing = assemble_spec035_tasks(Spec035TasksOwnerSnapshots::all_unavailable(
        Spec031Freshness::Unknown,
    ))?;
    let mut cancelled = child("cancelled", ReplayChildTaskState::Cancelled);
    cancelled.cancellation_requested_sequence = Some(2);
    cancelled.cancellation_requested_at_ms = Some(25);
    let mut cancelled_snapshots = unavailable_snapshots();
    cancelled_snapshots.child = source(
        35,
        Spec031Freshness::Current,
        vec![located("owner:child:cancelled", cancelled)],
    );
    let cancelled_projection = assemble_spec035_tasks(cancelled_snapshots)?;
    let cancelled_stop_completed = cancelled_projection.rows()[0]
        .action()
        .is_some_and(|action| action.status == Spec035TaskActionStatus::Completed);
    let mut invalid_app_snapshots = unavailable_snapshots();
    invalid_app_snapshots.app = source(
        35,
        Spec031Freshness::Current,
        vec![located(
            "owner:app:stopped-incomplete",
            app(AppLifecycleAction::Stop, AppProcessState::Stopped, false)?,
        )],
    );
    let invalid_stopped_incomplete_rejected = assemble_spec035_tasks(invalid_app_snapshots)
        .is_err_and(|error| error.kind() == Spec035TasksAssemblerErrorKind::InvalidOwnerFact);
    fs::write(
        path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "driver": "shacs-core/tests/spec035_tasks_assembler.rs",
            "mixed_projection": mixed,
            "missing_owner_projection": missing,
            "terminal_probe": {
                "cancelled_projection": cancelled_projection,
                "cancelled_stop_completed": cancelled_stop_completed,
                "invalid_stopped_incomplete_rejected": invalid_stopped_incomplete_rejected
            },
            "owner_reads_are_atomic": false
        }))?,
    )?;
    Ok(())
}
