use shacs_core::runtime::{
    assemble_spec035_tasks, Spec035LocatedOwnerFact, Spec035TasksOwnerSnapshots,
    Spec035TasksOwnerSource,
};
use shacs_projection::{Spec031Freshness, Spec031ObservedAtUnixMs};
use shacs_session::durable_child::{ReplayChildTask, ReplayChildTaskState};

fn child(id: &str) -> ReplayChildTask {
    ReplayChildTask {
        child_task_id: id.to_owned(),
        session_id: "cli:direct".to_owned(),
        parent_turn_id: "turn-1".to_owned(),
        spawn_effect_id: format!("spawn:{id}"),
        correlation_id: format!("correlation:{id}"),
        idempotency_key: format!("idempotency:{id}"),
        run_ref: None,
        state: ReplayChildTaskState::Running,
        attempt: 1,
        spawned_sequence: 1,
        spawned_at_ms: 1,
        started_sequence: Some(2),
        started_at_ms: Some(2),
        cancellation_requested_sequence: None,
        cancellation_requested_at_ms: None,
        terminal_sequence: None,
        finished_at_ms: None,
        result_ref: None,
    }
}

fn snapshots(facts: Vec<Spec035LocatedOwnerFact<ReplayChildTask>>) -> Spec035TasksOwnerSnapshots {
    let mut snapshots = Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable);
    snapshots.child = Spec035TasksOwnerSource::Available {
        observed_at_unix_ms: Spec031ObservedAtUnixMs::new(1),
        freshness: Spec031Freshness::Current,
        facts,
    };
    snapshots
}

#[test]
fn spec035_tasks_assembler_is_deterministic_for_reordered_owner_facts(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let facts = vec![
        Spec035LocatedOwnerFact {
            locator: "owner:child:z".to_owned(),
            fact: child("z"),
        },
        Spec035LocatedOwnerFact {
            locator: "owner:child:a".to_owned(),
            fact: child("a"),
        },
    ];
    let reverse = facts.iter().cloned().rev().collect();

    // When
    let forward_json = serde_json::to_string(&assemble_spec035_tasks(snapshots(facts))?)?;
    let reverse_json = serde_json::to_string(&assemble_spec035_tasks(snapshots(reverse))?)?;

    // Then
    assert_eq!(forward_json, reverse_json);
    Ok(())
}
