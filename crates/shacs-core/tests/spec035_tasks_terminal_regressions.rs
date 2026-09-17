use shacs_core::app::AppId;
use shacs_core::app_lifecycle::{AppLifecycleAction, AppLifecycleReceipt, AppProcessState};
use shacs_core::runtime::{
    assemble_spec035_tasks, Spec035LocatedOwnerFact, Spec035TasksAssemblerErrorKind,
    Spec035TasksOwnerSnapshots, Spec035TasksOwnerSource,
};
use shacs_projection::{
    Spec031Freshness, Spec031ObservedAtUnixMs, Spec035TaskActionStatus, Spec035TaskState,
};
use shacs_session::durable_child::{ReplayChildTask, ReplayChildTaskState};

fn source<T>(locator: &str, fact: T) -> Spec035TasksOwnerSource<T> {
    Spec035TasksOwnerSource::Available {
        observed_at_unix_ms: Spec031ObservedAtUnixMs::new(35),
        freshness: Spec031Freshness::Current,
        facts: vec![Spec035LocatedOwnerFact {
            locator: locator.to_owned(),
            fact,
        }],
    }
}

fn cancelled_child() -> ReplayChildTask {
    ReplayChildTask {
        child_task_id: "cancelled".to_owned(),
        session_id: "cli:direct".to_owned(),
        parent_turn_id: "turn-1".to_owned(),
        spawn_effect_id: "spawn:cancelled".to_owned(),
        correlation_id: "correlation:cancelled".to_owned(),
        idempotency_key: "idempotency:cancelled".to_owned(),
        run_ref: None,
        state: ReplayChildTaskState::Cancelled,
        attempt: 1,
        spawned_sequence: 1,
        spawned_at_ms: 10,
        started_sequence: Some(2),
        started_at_ms: Some(20),
        cancellation_requested_sequence: Some(3),
        cancellation_requested_at_ms: Some(25),
        terminal_sequence: Some(4),
        finished_at_ms: Some(30),
        result_ref: Some("result:cancelled".to_owned()),
    }
}

fn incomplete_stopped_app() -> Result<AppLifecycleReceipt, Box<dyn std::error::Error>> {
    Ok(AppLifecycleReceipt {
        receipt_id: "receipt:stopped-incomplete".to_owned(),
        request_id: "request:stopped-incomplete".to_owned(),
        app_id: AppId::parse("formatter")?,
        action: AppLifecycleAction::Stop,
        previous_state: AppProcessState::Running,
        current_state: AppProcessState::Stopped,
        generation: 1,
        completed: false,
        manifest_digest: String::new(),
        trusted_runtime_ref: String::new(),
        credential_source_statuses: Vec::new(),
        blockers: Vec::new(),
        activation_refs: Vec::new(),
        execution_snapshot_ref: None,
        process_outcome: None,
        occurred_at_unix_ms: 35,
    })
}

#[test]
fn cancelled_child_with_stop_request_reports_completed_action(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let mut snapshots = Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable);
    snapshots.child = source("owner:child:cancelled", cancelled_child());

    // When
    let projection = assemble_spec035_tasks(snapshots)?;

    // Then
    assert_eq!(projection.rows()[0].state(), Spec035TaskState::Cancelled);
    assert_eq!(
        projection.rows()[0].action().map(|action| action.status),
        Some(Spec035TaskActionStatus::Completed)
    );
    Ok(())
}

#[test]
fn stopped_app_with_incomplete_receipt_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let mut snapshots = Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable);
    snapshots.app = source("owner:app:formatter", incomplete_stopped_app()?);

    // When
    let result = assemble_spec035_tasks(snapshots);

    // Then
    assert_eq!(
        result.expect_err("incomplete stopped receipt").kind(),
        Spec035TasksAssemblerErrorKind::InvalidOwnerFact
    );
    Ok(())
}
