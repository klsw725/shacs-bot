use shacs_core::app::{AppError, AppId};
use shacs_core::app_lifecycle::{AppLifecycleAction, AppLifecycleReceipt, AppProcessState};
use shacs_core::runtime::{
    accept_spec035_surface_action_outcome, assemble_spec035_tasks, Spec035RecoveryOwnerFact,
    Spec035TasksOwnerSnapshots, Spec035TasksOwnerSource, SurfaceActionOutcome,
    SurfaceActionOutcomeKind,
};
use shacs_projection::{
    Spec031Freshness, Spec031ObservedAtUnixMs, Spec033GoalBudgetFact, Spec033GoalFact,
    Spec033GoalStatus, Spec033GoalUsageSummary, Spec035TaskActionKind, Spec035TaskActionStatus,
    Spec035TaskOwnerKind,
};
use shacs_session::durable_replay::{
    DurableRecoveryStatus, DurableReplayAdmission, DurableReplayState,
};

type FixtureError = Box<dyn std::error::Error>;

#[test]
fn current_goal_and_app_rows_advertise_owner_supported_actions() -> Result<(), FixtureError> {
    // Given
    let mut snapshots = unavailable_snapshots();
    snapshots.goal = source(
        1,
        Spec031Freshness::Current,
        vec![located("goal-1", goal(Spec033GoalStatus::Active))],
    );
    snapshots.app = source(
        2,
        Spec031Freshness::Current,
        vec![located(
            "formatter",
            app(AppLifecycleAction::Start, AppProcessState::Running, true)?,
        )],
    );

    // When
    let projection = assemble_spec035_tasks(snapshots)?;

    // Then
    let goal = projection
        .rows()
        .iter()
        .find(|row| row.owner().kind() == Spec035TaskOwnerKind::Goal)
        .and_then(|row| row.action())
        .ok_or("goal action")?;
    let app = projection
        .rows()
        .iter()
        .find(|row| row.owner().kind() == Spec035TaskOwnerKind::App)
        .and_then(|row| row.action())
        .ok_or("app action")?;
    assert_eq!(goal.kind, Spec035TaskActionKind::Pause);
    assert_eq!(goal.status, Spec035TaskActionStatus::Available);
    assert_eq!(app.kind, Spec035TaskActionKind::Stop);
    assert_eq!(app.status, Spec035TaskActionStatus::Available);
    Ok(())
}

fn unavailable_snapshots() -> Spec035TasksOwnerSnapshots {
    Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable)
}

fn located<T>(locator: &str, fact: T) -> shacs_core::runtime::Spec035LocatedOwnerFact<T> {
    shacs_core::runtime::Spec035LocatedOwnerFact {
        locator: locator.to_owned(),
        fact,
    }
}

fn source<T>(
    observed_at: u64,
    freshness: Spec031Freshness,
    facts: Vec<shacs_core::runtime::Spec035LocatedOwnerFact<T>>,
) -> Spec035TasksOwnerSource<T> {
    Spec035TasksOwnerSource::Available {
        observed_at_unix_ms: Spec031ObservedAtUnixMs::new(observed_at),
        freshness,
        facts,
    }
}

fn goal(status: Spec033GoalStatus) -> Spec033GoalFact {
    Spec033GoalFact {
        goal_id: "goal-1".to_owned(),
        session_id: "cli:direct".to_owned(),
        status,
        turn_budget: 8,
        turns_used: 3,
        last_verdict: None,
        blocked: status == Spec033GoalStatus::Blocked,
        stop_reason: None,
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

fn app(
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
        manifest_digest: String::new(),
        trusted_runtime_ref: String::new(),
        credential_source_statuses: Vec::new(),
        blockers: Vec::new(),
        activation_refs: Vec::new(),
        execution_snapshot_ref: None,
        process_outcome: None,
        occurred_at_unix_ms: 30,
    })
}

fn recovery(status: DurableRecoveryStatus) -> DurableReplayAdmission {
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

#[test]
fn recoverable_runtime_row_does_not_advertise_unaccepted_recover() -> Result<(), FixtureError> {
    // Given
    let mut snapshots = unavailable_snapshots();
    snapshots.recovery = source(
        1,
        Spec031Freshness::Current,
        vec![located(
            "runtime:recovery",
            Spec035RecoveryOwnerFact::Admission(recovery(DurableRecoveryStatus::Recoverable)),
        )],
    );

    // When
    let projection = assemble_spec035_tasks(snapshots)?;

    // Then
    assert!(projection.rows()[0].action().is_none());
    Ok(())
}

#[test]
fn unavailable_and_stale_owner_outcomes_are_not_accepted() {
    // Given
    let unavailable = SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Unavailable,
        changed: false,
        detail: "owner detail".to_owned(),
    };
    let stale = SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::StaleLineage,
        changed: false,
        detail: "owner detail".to_owned(),
    };

    // When
    let unavailable_result = accept_spec035_surface_action_outcome(unavailable);
    let stale_result = accept_spec035_surface_action_outcome(stale);

    // Then
    assert_eq!(
        unavailable_result
            .expect_err("unavailable must fail")
            .to_string(),
        "tasks owner action is unavailable"
    );
    assert_eq!(
        stale_result.expect_err("stale must fail").to_string(),
        "tasks owner action lineage is stale"
    );
}
