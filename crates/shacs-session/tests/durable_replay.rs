use rusqlite::config::DbConfig;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, DURABLE_EVENT_DATABASE_FILE,
    LEGACY_DURABLE_EVENT_LOG_FILE, SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED,
    SESSION_TURN_FAILED, WORKFLOW_COMPLETED, WORKFLOW_PLANNED,
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

/// Rewrites one stored record outside the store, with a matching checksum and schema version
/// column, so that only what `mutate` changed differs from what the store wrote.
fn rewrite_event_record(
    event_root: &Path,
    sequence: i64,
    mutate: impl FnOnce(&mut Value),
) -> Result<(), Box<dyn Error>> {
    let connection = Connection::open(event_root.join(DURABLE_EVENT_DATABASE_FILE))?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false)?;
    let text: String = connection.query_row(
        "SELECT record FROM events WHERE sequence = ?1",
        [sequence],
        |row| row.get(0),
    )?;
    let mut record: Value = serde_json::from_str(&text)?;
    mutate(&mut record);
    connection.execute(
        "UPDATE events SET record = ?1, checksum = ?2, schema_version = ?3 WHERE sequence = ?4",
        params![
            serde_json::to_string(&record)?,
            checksum(&record)?,
            record["schema_version"].as_i64(),
            sequence
        ],
    )?;
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
fn durable_replay_ignores_an_uncommitted_append() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut events = DurableEventStore::open(&event_root)?;
    events.append(input(
        "session-1",
        "turn-1",
        SESSION_TURN_ACCEPTED,
        json!({"content_hash": "sha256:one", "media_count": 0}),
    ))?;
    let wal = events
        .root()
        .join(format!("{DURABLE_EVENT_DATABASE_FILE}-wal"));
    let wal_before = fs::metadata(&wal).map_or(0, |metadata| metadata.len());
    let writer = Connection::open(events.path())?;
    writer.pragma_update(None, "cache_size", 2_i64)?;
    writer.execute_batch("BEGIN IMMEDIATE")?;
    for sequence in 2..64_i64 {
        writer.execute(
            "INSERT INTO events (sequence, event_id, session_id, kind, schema_version, \
             recorded_at, record, checksum) VALUES (?1, ?2, 'session-1', ?3, 1, 'never', ?4, \
             'sha256:never')",
            params![
                sequence,
                format!("event-{sequence:020}"),
                SESSION_TURN_COMPLETED,
                "x".repeat(16 * 1024)
            ],
        )?;
    }
    assert!(fs::metadata(&wal)?.len() > wal_before);

    let admission = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"));
    drop(writer);

    assert_eq!(admission.status, DurableRecoveryStatus::Healthy);
    assert!(admission.writable);
    assert_eq!(
        admission.state.and_then(|state| state.applied_through),
        Some(1)
    );
    assert!(admission.issues.is_empty());
    assert!(admission.recovery_hints.is_empty());
    Ok(())
}

#[test]
fn durable_replay_blocks_a_legacy_event_log_until_it_is_removed() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    fs::create_dir_all(&event_root)?;
    let legacy = event_root.join(LEGACY_DURABLE_EVENT_LOG_FILE);
    fs::write(&legacy, b"{\"frame_version\":1}\n")?;

    let blocked = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(blocked.status, DurableRecoveryStatus::Blocked);
    assert!(!blocked.writable);
    assert!(blocked
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::LegacyEventLog));
    assert_eq!(
        blocked.recovery_hints,
        vec![DurableRecoveryHint::RemoveLegacyEventLog]
    );
    assert_eq!(fs::read(&legacy)?, b"{\"frame_version\":1}\n");
    assert!(!event_root.join(DURABLE_EVENT_DATABASE_FILE).exists());

    fs::remove_file(&legacy)?;
    let healthy = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(healthy.status, DurableRecoveryStatus::Healthy);
    assert!(healthy.writable);
    Ok(())
}

#[test]
fn durable_replay_blocks_sidecars_without_a_database() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    fs::create_dir_all(&event_root)?;
    fs::write(
        event_root.join(format!("{DURABLE_EVENT_DATABASE_FILE}-wal")),
        b"left behind",
    )?;

    let admission = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"));
    assert_eq!(admission.status, DurableRecoveryStatus::Blocked);
    assert!(!admission.writable);
    assert!(admission
        .issues
        .iter()
        .any(|issue| issue.kind == DurableRecoveryIssueKind::EventCorrupt));
    assert!(!event_root.join(DURABLE_EVENT_DATABASE_FILE).exists());
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
    rewrite_event_record(&event_root, 2, |record| record["sequence"] = json!(3))?;
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
    rewrite_event_record(&event_root, 1, |record| record["schema_version"] = json!(2))?;
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
