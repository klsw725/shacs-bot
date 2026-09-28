#[path = "spec035_tasks_assembler/support.rs"]
mod support;

use serde_json::json;
use shacs_core::app_lifecycle::{AppLifecycleAction, AppProcessState};
use shacs_core::runtime::{
    assemble_spec035_tasks, Spec035RecoveryOwnerFact, Spec035TasksAssemblerErrorKind,
    Spec035TasksOwnerSource,
};
use shacs_projection::{
    Spec031Freshness, Spec033AutomationJobStatus, Spec033GoalStatus, Spec035TaskActionStatus,
    Spec035TaskCount, Spec035TaskOwnerKind, Spec035TaskState, SPEC035_TASKS_ROWS_MAX,
};
use shacs_session::durable_child::ReplayChildTaskState;
use shacs_session::durable_replay::DurableRecoveryStatus;
use shacs_workflow::WorkflowRunState;
use support::*;

#[test]
fn spec035_tasks_assembler_emits_six_owner_kinds_together() -> Result<(), FixtureError> {
    // Given
    let mut snapshots = unavailable_snapshots();
    snapshots.goal = source(
        10,
        Spec031Freshness::Current,
        vec![located(
            "owner:goal:goal-1",
            goal(Spec033GoalStatus::Blocked),
        )],
    );
    snapshots.child = source(
        11,
        Spec031Freshness::Current,
        vec![located(
            "owner:child:reviewer",
            child("reviewer", ReplayChildTaskState::Completed),
        )],
    );
    snapshots.workflow = source(
        12,
        Spec031Freshness::Stale,
        vec![located(
            "owner:workflow:release",
            workflow(WorkflowRunState::Blocked),
        )],
    );
    snapshots.automation = source(
        13,
        Spec031Freshness::Current,
        vec![located(
            "owner:automation:work-1",
            automation(Spec033AutomationJobStatus::Pending),
        )],
    );
    snapshots.app = source(
        14,
        Spec031Freshness::Current,
        vec![located(
            "owner:app:formatter",
            app(AppLifecycleAction::Stop, AppProcessState::Stopping, false)?,
        )],
    );
    snapshots.recovery = source(
        15,
        Spec031Freshness::Current,
        vec![located(
            "owner:recovery:runtime",
            Spec035RecoveryOwnerFact::Recovered {
                admission: recovery(DurableRecoveryStatus::Healthy),
                outcome: recovered_outcome(),
            },
        )],
    );

    // When
    let projection = assemble_spec035_tasks(snapshots)?;

    // Then
    assert_eq!(projection.rows().len(), 6);
    assert_eq!(
        projection
            .rows()
            .iter()
            .map(|row| row.owner().kind())
            .collect::<Vec<_>>(),
        Spec035TaskOwnerKind::ALL
    );
    assert_eq!(projection.rows()[2].freshness(), Spec031Freshness::Stale);
    assert_eq!(projection.rows()[5].state(), Spec035TaskState::Recovered);
    assert_eq!(
        projection.rows()[5].action().map(|action| action.status),
        Some(Spec035TaskActionStatus::Completed)
    );
    if let Some(path) = std::env::var_os("SPEC035_TASKS_ASSEMBLY_ARTIFACT") {
        write_task4_artifact(path, projection)?;
    }
    Ok(())
}

#[test]
fn spec035_tasks_assembler_preserves_missing_sources_without_zero_detail(
) -> Result<(), FixtureError> {
    // Given
    let mut snapshots = unavailable_snapshots();
    snapshots.goal = Spec035TasksOwnerSource::Unavailable {
        freshness: Spec031Freshness::Unknown,
    };

    // When
    let projection = assemble_spec035_tasks(snapshots)?;
    let value = serde_json::to_value(&projection)?;

    // Then
    assert!(projection.rows().is_empty());
    assert_eq!(projection.coverage().goal, Spec035TaskCount::Unavailable);
    assert_eq!(
        value["coverage"]["goal"],
        json!({"availability":"unavailable"})
    );
    assert!(value.get("detail").is_none());
    Ok(())
}

#[test]
fn spec035_tasks_assembler_rejects_malformed_duplicate_and_oversized_owner_input(
) -> Result<(), FixtureError> {
    // Given / When
    let malformed = {
        let mut snapshots = unavailable_snapshots();
        snapshots.goal = source(
            1,
            Spec031Freshness::Current,
            vec![located(
                "/Users/private/goal",
                goal(Spec033GoalStatus::Active),
            )],
        );
        assemble_spec035_tasks(snapshots)
    };
    let duplicate = {
        let mut snapshots = unavailable_snapshots();
        snapshots.child = source(
            1,
            Spec031Freshness::Current,
            vec![
                located(
                    "owner:child:same",
                    child("one", ReplayChildTaskState::Running),
                ),
                located(
                    "owner:child:same",
                    child("two", ReplayChildTaskState::Running),
                ),
            ],
        );
        assemble_spec035_tasks(snapshots)
    };
    let oversized = {
        let mut snapshots = unavailable_snapshots();
        snapshots.child = source(
            1,
            Spec031Freshness::Current,
            (0..=SPEC035_TASKS_ROWS_MAX)
                .map(|index| {
                    located(
                        &format!("owner:child:{index}"),
                        child(&index.to_string(), ReplayChildTaskState::Running),
                    )
                })
                .collect(),
        );
        assemble_spec035_tasks(snapshots)
    };

    // Then
    assert_eq!(
        malformed.expect_err("malformed locator").kind(),
        Spec035TasksAssemblerErrorKind::Projection
    );
    assert_eq!(
        duplicate.expect_err("duplicate owner").kind(),
        Spec035TasksAssemblerErrorKind::Projection
    );
    assert_eq!(
        oversized.expect_err("oversized input").kind(),
        Spec035TasksAssemblerErrorKind::TooManyOwnerFacts
    );
    Ok(())
}

#[test]
fn spec035_tasks_assembler_fails_closed_for_unknown_rows_and_omits_sensitive_owner_fields(
) -> Result<(), FixtureError> {
    // Given
    let unknown = {
        let mut snapshots = unavailable_snapshots();
        snapshots.automation = source(
            1,
            Spec031Freshness::Unknown,
            vec![located(
                "owner:automation:unknown",
                automation(Spec033AutomationJobStatus::Succeeded),
            )],
        );
        assemble_spec035_tasks(snapshots)
    };
    let stale_terminal = {
        let mut snapshots = unavailable_snapshots();
        snapshots.child = source(
            1,
            Spec031Freshness::Stale,
            vec![located(
                "owner:child:stale-done",
                child("stale-done", ReplayChildTaskState::Completed),
            )],
        );
        assemble_spec035_tasks(snapshots)
    };
    let mut snapshots = unavailable_snapshots();
    snapshots.app = source(
        1,
        Spec031Freshness::Current,
        vec![located(
            "owner:app:formatter",
            app(AppLifecycleAction::Stop, AppProcessState::Stopped, true)?,
        )],
    );

    // When
    let encoded = serde_json::to_string(&assemble_spec035_tasks(snapshots)?)?;

    // Then
    assert_eq!(
        unknown.expect_err("unknown row evidence").kind(),
        Spec035TasksAssemblerErrorKind::InvalidSourceFreshness
    );
    assert_eq!(
        stale_terminal.expect_err("stale terminal evidence").kind(),
        Spec035TasksAssemblerErrorKind::Projection
    );
    for sentinel in [
        "secret-token-must-not-leak",
        "/Users/private/process",
        "API_TOKEN=secret",
        "raw owner detail must not leak",
    ] {
        assert!(!encoded.contains(sentinel));
    }
    Ok(())
}
