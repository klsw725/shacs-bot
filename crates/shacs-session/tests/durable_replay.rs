use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, SESSION_TURN_ACCEPTED,
    SESSION_TURN_COMPLETED, SESSION_TURN_FAILED, WORKFLOW_COMPLETED, WORKFLOW_PLANNED,
};
use shacs_session::durable_replay::{
    apply_durable_event, evaluate_durable_recovery, DurableCheckpointStore, DurableRecoveryHint,
    DurableRecoveryIssueKind, DurableRecoveryStatus, DurableReplayState, ReplayTurnStatus,
    ReplayWorkflowStatus,
};
use std::error::Error;
use std::fs;
use std::path::Path;

fn input(session_id: &str, turn_id: &str, kind: &str, data: Value) -> DurableEventInput {
    let mut input = DurableEventInput::new(
        session_id,
        kind,
        DurableEventPayload::inline("orchestrator_fact", data),
    );
    input.turn_id = Some(turn_id.to_owned());
    input
}

fn checksum(value: &Value) -> Result<String, Box<dyn Error>> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}

fn rewrite_event_frame(
    path: &Path,
    index: usize,
    mutate: impl FnOnce(&mut Value),
) -> Result<(), Box<dyn Error>> {
    let mut frames = fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let frame = frames.get_mut(index).ok_or("missing event frame")?;
    mutate(frame);
    let record = frame["record"].clone();
    frame["record_length"] = json!(serde_json::to_vec(&record)?.len());
    frame["checksum"] = Value::String(checksum(&record)?);
    let mut text = frames
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    text.push('\n');
    fs::write(path, text)?;
    Ok(())
}

fn rewrite_checkpoint_frame(
    path: &Path,
    mutate: impl FnOnce(&mut Value),
    refresh_checksum: bool,
) -> Result<(), Box<dyn Error>> {
    let mut frame = serde_json::from_slice::<Value>(&fs::read(path)?)?;
    mutate(&mut frame);
    if refresh_checksum {
        let body = frame["body"].clone();
        frame["checksum"] = Value::String(checksum(&body)?);
    }
    fs::write(path, serde_json::to_vec(&frame)?)?;
    Ok(())
}

fn single_event_checkpoint_fixture() -> Result<
    (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ),
    Box<dyn Error>,
> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing fixture state")?;
    let checkpoints = DurableCheckpointStore::open(&checkpoint_root)?;
    checkpoints.write(&state)?;
    let path = checkpoints
        .candidate_paths()?
        .into_iter()
        .next()
        .ok_or("missing fixture checkpoint")?;
    Ok((root, event_root, checkpoint_root, path))
}

#[test]
fn durable_replay_event_zero_is_healthy_without_creating_live_effects() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let admission =
        evaluate_durable_recovery(root.path().join("events"), root.path().join("checkpoints"));
    assert_eq!(admission.status, DurableRecoveryStatus::Healthy);
    assert!(admission.writable);
    assert_eq!(admission.state, Some(DurableReplayState::event_zero()));
    assert!(!root.path().join("events").exists());
    assert!(!root.path().join("checkpoints").exists());
    Ok(())
}

#[test]
fn durable_replay_reduces_turn_command_and_workflow_lifecycles() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 2}),
    ))?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_COMPLETED,
        json!({
            "stop_reason": "completed",
            "tool_count": 1,
            "outcome_count": 2,
            "pending_effect_count": 0
        }),
    ))?;
    events.append(input(
        "session-1",
        "turn-command",
        SESSION_TURN_COMPLETED,
        json!({"command": "status", "stop_reason": "status"}),
    ))?;
    events.append(input(
        "session-1",
        "turn-workflow",
        WORKFLOW_PLANNED,
        json!({"workflow_id": "workflow-1", "harness_plan_digest": "sha256:plan"}),
    ))?;
    events.append(input(
        "session-1",
        "turn-workflow",
        WORKFLOW_COMPLETED,
        json!({"workflow_id": "workflow-1", "state": "completed", "child_result_count": 3}),
    ))?;

    let admission = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(admission.status, DurableRecoveryStatus::Healthy);
    assert!(admission.writable);
    let state = admission.state.ok_or("missing replay state")?;
    assert_eq!(state.applied_through, Some(5));
    assert_eq!(
        state.sessions["session-1"].turns["turn-1"].status,
        ReplayTurnStatus::Completed
    );
    assert_eq!(
        state.sessions["session-1"].turns["turn-command"].status,
        ReplayTurnStatus::ResponseCompletedWithoutAccepted
    );
    assert_eq!(
        state.workflows["workflow-1"].status,
        ReplayWorkflowStatus::Completed
    );
    Ok(())
}

#[test]
fn durable_replay_uses_checkpoint_then_applies_only_the_tail() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_COMPLETED,
        json!({"stop_reason": "completed"}),
    ))?;
    let first = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = first.state.ok_or("missing initial state")?;
    let checkpoint = DurableCheckpointStore::open(&checkpoint_root)?.write(&state)?;
    events.append(input(
        "session-1",
        "turn-command",
        SESSION_TURN_COMPLETED,
        json!({"command": "status", "stop_reason": "status"}),
    ))?;

    let replayed = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(replayed.status, DurableRecoveryStatus::Healthy);
    assert_eq!(
        replayed.checkpoint_used.as_deref(),
        Some(checkpoint.checkpoint_id.as_str())
    );
    assert_eq!(replayed.replayed_event_count, 1);
    assert_eq!(
        replayed.state.and_then(|state| state.applied_through),
        Some(3)
    );
    Ok(())
}

#[test]
fn durable_replay_falls_back_to_previous_checkpoint_and_event_zero() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    let state_one = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing state one")?;
    let checkpoints = DurableCheckpointStore::open(&checkpoint_root)?;
    let first = checkpoints.write(&state_one)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_FAILED,
        json!({"stop_reason": "provider_error"}),
    ))?;
    let state_two = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing state two")?;
    checkpoints.write(&state_two)?;
    let latest = checkpoints
        .candidate_paths()?
        .into_iter()
        .next()
        .ok_or("missing latest checkpoint")?;
    fs::write(&latest, b"{malformed")?;

    let previous = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(previous.status, DurableRecoveryStatus::Recoverable);
    assert!(!previous.writable);
    assert_eq!(
        previous.checkpoint_used.as_deref(),
        Some(first.checkpoint_id.as_str())
    );
    assert_eq!(
        previous.state.and_then(|state| state.applied_through),
        Some(2)
    );
    assert!(previous
        .recovery_hints
        .contains(&DurableRecoveryHint::RewriteCheckpoint));

    fs::write(
        checkpoints
            .candidate_paths()?
            .into_iter()
            .last()
            .ok_or("missing previous checkpoint")?,
        b"{}",
    )?;
    let event_zero = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(event_zero.status, DurableRecoveryStatus::Recoverable);
    assert_eq!(
        event_zero.state.and_then(|state| state.applied_through),
        Some(2)
    );
    assert!(event_zero.checkpoint_used.is_none());
    Ok(())
}

#[test]
fn durable_replay_rejects_checkpoint_checksum_digest_and_missing_sequence(
) -> Result<(), Box<dyn Error>> {
    let (_root, event_root, checkpoint_root, checkpoint) = single_event_checkpoint_fixture()?;
    rewrite_checkpoint_frame(
        &checkpoint,
        |frame| frame["body"]["recorded_at"] = json!("changed"),
        false,
    )?;
    let checksum_admission = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(
        checksum_admission.status,
        DurableRecoveryStatus::Recoverable
    );
    assert!(checksum_admission
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::CheckpointCorrupt));

    let (_root, event_root, checkpoint_root, checkpoint) = single_event_checkpoint_fixture()?;
    rewrite_checkpoint_frame(
        &checkpoint,
        |frame| frame["body"]["state"]["sessions"]["forged"] = json!({"turns": {}}),
        true,
    )?;
    let digest = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(digest.status, DurableRecoveryStatus::Recoverable);
    assert!(digest
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::CheckpointCorrupt));

    let (_root, event_root, checkpoint_root, checkpoint) = single_event_checkpoint_fixture()?;
    let mut frame = serde_json::from_slice::<Value>(&fs::read(&checkpoint)?)?;
    frame["body"]["state"]["sessions"]["session-1"]["turns"]["turn-1"]["content_hash"] =
        json!("sha256:forged");
    frame["body"]["state_digest"] = Value::String(checksum(&frame["body"]["state"])?);
    frame["checksum"] = Value::String(checksum(&frame["body"])?);
    fs::write(&checkpoint, serde_json::to_vec(&frame)?)?;
    let forged = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(forged.status, DurableRecoveryStatus::Recoverable);
    assert!(!forged.writable);
    assert_eq!(
        forged
            .state
            .as_ref()
            .and_then(|state| state.sessions["session-1"].turns["turn-1"]
                .content_hash
                .as_deref()),
        Some("sha256:one")
    );
    assert!(forged
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::CheckpointCorrupt));

    let (_root, event_root, checkpoint_root, checkpoint) = single_event_checkpoint_fixture()?;
    rewrite_checkpoint_frame(
        &checkpoint,
        |frame| {
            frame["body"]
                .as_object_mut()
                .map(|body| body.remove("included_sequence"));
        },
        true,
    )?;
    let missing = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(missing.status, DurableRecoveryStatus::Recoverable);
    assert!(missing
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::CheckpointCorrupt));
    Ok(())
}

#[test]
fn durable_replay_falls_back_when_checkpoint_is_ahead_of_events() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    let mut ahead = DurableReplayState::event_zero();
    ahead.applied_through = Some(10);
    DurableCheckpointStore::open(&checkpoint_root)?.write(&ahead)?;

    let admission = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(admission.status, DurableRecoveryStatus::Recoverable);
    assert_eq!(
        admission.state.and_then(|state| state.applied_through),
        Some(1)
    );
    assert!(admission
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::CheckpointAheadOfEvents));
    Ok(())
}

#[test]
fn durable_replay_classifies_incomplete_tail_as_recoverable() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_COMPLETED,
        json!({"stop_reason": "completed"}),
    ))?;
    let mut bytes = fs::read(events.path())?;
    bytes.truncate(bytes.len().saturating_sub(8));
    fs::write(events.path(), bytes)?;

    let admission = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"));
    assert_eq!(admission.status, DurableRecoveryStatus::Recoverable);
    assert!(!admission.writable);
    assert_eq!(
        admission.state.and_then(|state| state.applied_through),
        Some(1)
    );
    assert!(admission
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::IncompleteTail));
    assert!(admission
        .recovery_hints
        .contains(&DurableRecoveryHint::DiscardIncompleteTail));
    Ok(())
}

#[test]
fn durable_replay_blocks_middle_corruption_and_preserves_failed_reducer_state(
) -> Result<(), Box<dyn Error>> {
    let corruption_root = tempfile::tempdir()?;
    let event_root = corruption_root.path().join("events");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_COMPLETED,
        json!({"stop_reason": "completed"}),
    ))?;
    rewrite_event_frame(events.path(), 1, |frame| {
        frame["record"]["sequence"] = json!(3)
    })?;
    let corrupt =
        evaluate_durable_recovery(&event_root, corruption_root.path().join("checkpoints"));
    assert_eq!(corrupt.status, DurableRecoveryStatus::Blocked);
    assert!(corrupt
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::EventCorrupt));

    let response_root = tempfile::tempdir()?;
    let event_root = response_root.path().join("events");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_COMPLETED,
        json!({"stop_reason": "completed"}),
    ))?;
    let response = evaluate_durable_recovery(&event_root, response_root.path().join("checkpoints"));
    assert_eq!(response.status, DurableRecoveryStatus::Healthy);
    assert!(response.writable);

    let failure_root = tempfile::tempdir()?;
    let mut failure_events = DurableEventStore::open(failure_root.path())?;
    let record = failure_events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_FAILED,
        json!({"stop_reason": "provider_error"}),
    ))?;
    let mut state = DurableReplayState::event_zero();
    let before = state.clone();
    assert!(apply_durable_event(&mut state, &record).is_err());
    assert_eq!(state, before);
    Ok(())
}

#[test]
fn durable_replay_marks_unknown_event_schema_inspect_only() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    rewrite_event_frame(events.path(), 0, |frame| {
        frame["record"]["schema_version"] = json!(2)
    })?;
    let admission = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"));
    assert_eq!(admission.status, DurableRecoveryStatus::InspectOnly);
    assert!(!admission.writable);
    assert!(admission
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::EventIncompatible));
    Ok(())
}

#[test]
fn durable_replay_reducer_is_a_pure_record_to_state_transition() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let mut events = DurableEventStore::open(root.path())?;
    let record = events.append(input(
        "session-1",
        "turn-1",
        WORKFLOW_PLANNED,
        json!({"workflow_id": "workflow-1", "harness_plan_digest": "sha256:plan"}),
    ))?;
    let mut state = DurableReplayState::event_zero();
    apply_durable_event(&mut state, &record)?;
    assert_eq!(state.applied_through, Some(1));
    assert_eq!(
        state.workflows["workflow-1"].status,
        ReplayWorkflowStatus::Planned
    );
    Ok(())
}
