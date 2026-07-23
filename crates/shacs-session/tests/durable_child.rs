use shacs_session::durable_child::{
    ChildCancelRequested, ChildResultDecisionKind, ChildResultRecorded, ChildRunning, ChildSpawned,
    DurableChildRecorder, ReplayChildTaskState,
};
use shacs_session::durable_event::DurableEventStore;
use shacs_session::durable_replay::{apply_durable_event, DurableReplayState};
use std::error::Error;

fn spawn(child_task_id: &str) -> ChildSpawned {
    ChildSpawned {
        child_task_id: child_task_id.to_owned(),
        parent_turn_id: "turn:parent".to_owned(),
        spawn_effect_id: format!("spawn:{child_task_id}"),
        correlation_id: format!("correlation:{child_task_id}"),
        idempotency_key: format!("idempotency:{child_task_id}"),
        run_ref: None,
        attempt: 1,
        spawned_at_ms: 10,
    }
}

fn result_ref(hex_suffix: &str) -> String {
    format!("child-result:{hex_suffix:0>64}")
}

#[test]
fn durable_child_replays_cancellation_request_and_terminal_outcome_separately(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    let spawned = spawn("child-1");
    recorder.record_spawned("session", &spawned)?;
    recorder.record_running(
        "session",
        &spawned.parent_turn_id,
        &spawned.spawn_effect_id,
        &spawned.correlation_id,
        &ChildRunning {
            child_task_id: spawned.child_task_id.clone(),
            started_at_ms: 20,
        },
    )?;
    recorder.record_cancel_requested(
        "session",
        &spawned.parent_turn_id,
        &spawned.spawn_effect_id,
        &spawned.correlation_id,
        &ChildCancelRequested {
            child_task_id: spawned.child_task_id.clone(),
            requested_at_ms: 30,
        },
    )?;
    recorder.record_result(
        "session",
        &ChildResultRecorded {
            child_task_id: spawned.child_task_id.clone(),
            parent_turn_id: spawned.parent_turn_id.clone(),
            spawn_effect_id: spawned.spawn_effect_id.clone(),
            correlation_id: spawned.correlation_id.clone(),
            idempotency_key: spawned.idempotency_key.clone(),
            decision: ChildResultDecisionKind::Accepted,
            terminal_state: Some(ReplayChildTaskState::Cancelled),
            result_ref: result_ref("deadbeef"),
            finished_at_ms: 40,
        },
    )?;

    let scan = DurableEventStore::open(root.path())?.scan(usize::MAX)?;
    let mut state = DurableReplayState::event_zero();
    for event in &scan.records {
        apply_durable_event(&mut state, event)?;
    }
    let child = state.children.items.get("child-1").ok_or("missing child")?;
    assert_eq!(child.state, ReplayChildTaskState::Cancelled);
    assert!(child.cancellation_requested_sequence.is_some());
    assert!(child.terminal_sequence.is_some());
    assert!(child.cancellation_requested_sequence < child.terminal_sequence);
    assert_eq!(
        child.result_ref.as_deref(),
        Some(result_ref("deadbeef").as_str())
    );
    Ok(())
}

#[test]
fn durable_child_running_restart_remains_nonterminal_and_stale_result_is_inspectable(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    let spawned = spawn("child-active");
    recorder.record_spawned("session", &spawned)?;
    recorder.record_running(
        "session",
        &spawned.parent_turn_id,
        &spawned.spawn_effect_id,
        &spawned.correlation_id,
        &ChildRunning {
            child_task_id: spawned.child_task_id.clone(),
            started_at_ms: 20,
        },
    )?;
    recorder.record_result(
        "wrong-session",
        &ChildResultRecorded {
            child_task_id: spawned.child_task_id.clone(),
            parent_turn_id: "turn:wrong".to_owned(),
            spawn_effect_id: spawned.spawn_effect_id.clone(),
            correlation_id: "correlation:wrong".to_owned(),
            idempotency_key: spawned.idempotency_key.clone(),
            decision: ChildResultDecisionKind::Stale,
            terminal_state: None,
            result_ref: result_ref("badc0de"),
            finished_at_ms: 30,
        },
    )?;

    let scan = DurableEventStore::open(root.path())?.scan(usize::MAX)?;
    let serialized = serde_json::to_string(&scan.records)?;
    assert!(!serialized.contains("raw child result"));
    let mut state = DurableReplayState::event_zero();
    for event in &scan.records {
        apply_durable_event(&mut state, event)?;
    }
    assert_eq!(
        state.children.items["child-active"].state,
        ReplayChildTaskState::Running
    );
    assert_eq!(state.children.decisions.len(), 1);
    assert_eq!(
        state.children.decisions[0].decision,
        ChildResultDecisionKind::Stale
    );
    Ok(())
}

#[test]
fn durable_child_spawned_restart_remains_pending() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    recorder.record_spawned("session", &spawn("child-pending"))?;

    let scan = DurableEventStore::open(root.path())?.scan(usize::MAX)?;
    let mut state = DurableReplayState::event_zero();
    for event in &scan.records {
        apply_durable_event(&mut state, event)?;
    }
    assert_eq!(
        state.children.items["child-pending"].state,
        ReplayChildTaskState::Spawned
    );
    assert!(state.children.items["child-pending"]
        .terminal_sequence
        .is_none());
    Ok(())
}

#[test]
fn durable_child_replays_every_terminal_state_and_late_success_cannot_overwrite_cancelled(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    for (index, terminal_state) in [
        ReplayChildTaskState::Completed,
        ReplayChildTaskState::Failed,
        ReplayChildTaskState::TimedOut,
        ReplayChildTaskState::Cancelled,
    ]
    .into_iter()
    .enumerate()
    {
        let child_task_id = format!("child-{index}");
        let spawned = spawn(&child_task_id);
        recorder.record_spawned("session", &spawned)?;
        recorder.record_result(
            "session",
            &ChildResultRecorded {
                child_task_id: spawned.child_task_id.clone(),
                parent_turn_id: spawned.parent_turn_id.clone(),
                spawn_effect_id: spawned.spawn_effect_id.clone(),
                correlation_id: spawned.correlation_id.clone(),
                idempotency_key: spawned.idempotency_key.clone(),
                decision: ChildResultDecisionKind::Accepted,
                terminal_state: Some(terminal_state),
                result_ref: result_ref(&index.to_string()),
                finished_at_ms: 20,
            },
        )?;
        if terminal_state == ReplayChildTaskState::Cancelled {
            recorder.record_result(
                "session",
                &ChildResultRecorded {
                    child_task_id: spawned.child_task_id,
                    parent_turn_id: spawned.parent_turn_id,
                    spawn_effect_id: spawned.spawn_effect_id,
                    correlation_id: spawned.correlation_id,
                    idempotency_key: spawned.idempotency_key,
                    decision: ChildResultDecisionKind::Late,
                    terminal_state: None,
                    result_ref: result_ref("cafe"),
                    finished_at_ms: 30,
                },
            )?;
        }
    }

    let scan = DurableEventStore::open(root.path())?.scan(usize::MAX)?;
    let mut state = DurableReplayState::event_zero();
    for event in &scan.records {
        apply_durable_event(&mut state, event)?;
    }
    assert_eq!(
        state.children.items["child-0"].state,
        ReplayChildTaskState::Completed
    );
    assert_eq!(
        state.children.items["child-1"].state,
        ReplayChildTaskState::Failed
    );
    assert_eq!(
        state.children.items["child-2"].state,
        ReplayChildTaskState::TimedOut
    );
    assert_eq!(
        state.children.items["child-3"].state,
        ReplayChildTaskState::Cancelled
    );
    assert!(state
        .children
        .decisions
        .iter()
        .any(|decision| decision.decision == ChildResultDecisionKind::Late));
    Ok(())
}

#[test]
fn invalid_accepted_result_does_not_mutate_replay_decisions() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    let spawned = spawn("child-invalid-accepted");
    recorder.record_spawned("session", &spawned)?;
    recorder.record_result(
        "session",
        &ChildResultRecorded {
            child_task_id: spawned.child_task_id,
            parent_turn_id: spawned.parent_turn_id,
            spawn_effect_id: spawned.spawn_effect_id,
            correlation_id: spawned.correlation_id,
            idempotency_key: spawned.idempotency_key,
            decision: ChildResultDecisionKind::Accepted,
            terminal_state: None,
            result_ref: result_ref("bad0"),
            finished_at_ms: 30,
        },
    )?;

    let scan = DurableEventStore::open(root.path())?.scan(usize::MAX)?;
    let mut state = DurableReplayState::event_zero();
    apply_durable_event(&mut state, &scan.records[0])?;
    assert!(apply_durable_event(&mut state, &scan.records[1]).is_err());
    assert!(
        state.children.decisions.is_empty(),
        "an invalid accepted result must not leave an inspectable decision"
    );
    assert_eq!(
        state.children.items["child-invalid-accepted"].state,
        ReplayChildTaskState::Spawned
    );
    Ok(())
}

#[test]
fn running_and_cancel_events_require_full_spawn_correlation() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let recorder = DurableChildRecorder::open(root.path())?;
    let spawned = spawn("child-transition-identity");
    recorder.record_spawned("session", &spawned)?;
    recorder.record_running(
        "wrong-session",
        &spawned.parent_turn_id,
        &spawned.spawn_effect_id,
        &spawned.correlation_id,
        &ChildRunning {
            child_task_id: spawned.child_task_id.clone(),
            started_at_ms: 20,
        },
    )?;

    let scan = DurableEventStore::open(root.path())?.scan(usize::MAX)?;
    let mut state = DurableReplayState::event_zero();
    apply_durable_event(&mut state, &scan.records[0])?;
    assert!(apply_durable_event(&mut state, &scan.records[1]).is_err());
    assert_eq!(
        state.children.items["child-transition-identity"].state,
        ReplayChildTaskState::Spawned
    );
    Ok(())
}
