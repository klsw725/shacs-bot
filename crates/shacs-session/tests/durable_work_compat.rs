use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventRecord, DurableEventStore, WORK_ENQUEUED,
    WORK_TERMINAL,
};
use shacs_session::durable_replay::{evaluate_durable_recovery, DurableRecoveryStatus};
use shacs_session::durable_work::{
    ReplayWorkState, WorkEnqueued, WorkPayloadRef, WorkTerminal, WorkTerminalKind,
};
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

fn append(
    events: &mut DurableEventStore,
    kind: &str,
    payload: &impl Serialize,
) -> Result<(), Box<dyn Error>> {
    events.append(DurableEventInput::new(
        "trajectory:local",
        kind,
        DurableEventPayload::inline("durable_work", serde_json::to_value(payload)?),
    ))?;
    Ok(())
}

fn legacy_owner_request() -> Result<WorkEnqueued, Box<dyn Error>> {
    let correlation = "automation-owner:sha256:legacy";
    Ok(WorkEnqueued {
        work_id: "automation-route-suppress-legacy".to_owned(),
        work_kind: "automation.owner_request".to_owned(),
        payload_ref: WorkPayloadRef::inline(
            "shacs.automation_owner_request.v1",
            json!({
                "route": "suppress",
                "result_ref": "automation:check:legacy",
                "correlation_id": correlation,
            }),
        )?,
        dedupe_hint: Some(correlation.to_owned()),
        next_wake_at_ms: None,
        effect_id: Some(correlation.to_owned()),
    })
}

fn terminal(outcome_ref: &str) -> WorkTerminal {
    WorkTerminal {
        work_id: "automation-route-suppress-legacy".to_owned(),
        terminal_kind: WorkTerminalKind::Succeeded,
        outcome_ref: outcome_ref.to_owned(),
        facts: None,
    }
}

fn append_legacy_frame(
    event_path: &Path,
    previous: &DurableEventRecord,
    payload: WorkTerminal,
) -> Result<(), Box<dyn Error>> {
    let sequence = previous.sequence + 1;
    let record = DurableEventRecord {
        schema_version: 1,
        event_id: format!("event-{sequence:020}"),
        sequence,
        session_id: previous.session_id.clone(),
        turn_id: previous.turn_id.clone(),
        causation_id: previous.causation_id.clone(),
        correlation_id: previous.correlation_id.clone(),
        kind: WORK_TERMINAL.to_owned(),
        payload: DurableEventPayload::inline("durable_work", serde_json::to_value(payload)?),
        provenance: previous.provenance.clone(),
        recorded_at: "2026-08-13T12:41:08.795Z".to_owned(),
    };
    let record_bytes = serde_json::to_vec(&serde_json::to_value(&record)?)?;
    let frame = json!({
        "frame_version": 1,
        "record_length": record_bytes.len(),
        "checksum": format!("sha256:{:x}", Sha256::digest(&record_bytes)),
        "record": record,
    });
    let mut file = OpenOptions::new().append(true).open(event_path)?;
    serde_json::to_writer(&mut file, &frame)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

#[test]
fn persisted_v1_suppress_terminal_without_lease_remains_replayable() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &legacy_owner_request()?)?;
    let enqueued = events
        .scan(usize::MAX)?
        .records
        .pop()
        .ok_or("missing enqueued event")?;
    append_legacy_frame(events.path(), &enqueued, terminal("no_notification"))?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);

    assert_eq!(
        replay.status,
        DurableRecoveryStatus::Healthy,
        "recovery issues: {:?}",
        replay.issues
    );
    assert!(replay.writable);
    let state = replay.state.ok_or("missing replay state")?;
    let item = &state.work.items["automation-route-suppress-legacy"];
    assert_eq!(item.state, ReplayWorkState::Terminal);
    assert_eq!(item.terminal_kind, Some(WorkTerminalKind::Succeeded));
    assert_eq!(item.attempt, 0);
    Ok(())
}

#[test]
fn persisted_v1_unleased_terminal_outside_legacy_contract_stays_blocked(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &legacy_owner_request()?)?;
    let enqueued = events
        .scan(usize::MAX)?
        .records
        .pop()
        .ok_or("missing enqueued event")?;
    append_legacy_frame(events.path(), &enqueued, terminal("forged_success"))?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);

    assert_eq!(replay.status, DurableRecoveryStatus::Blocked);
    assert!(!replay.writable);
    let state = replay.state.ok_or("missing valid replay prefix")?;
    assert_eq!(
        state.work.items["automation-route-suppress-legacy"].state,
        ReplayWorkState::Pending
    );
    Ok(())
}

#[test]
fn new_suppress_terminal_without_lease_is_rejected_before_persist() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &legacy_owner_request()?)?;

    let result = append(&mut events, WORK_TERMINAL, &terminal("no_notification"));

    assert!(result.is_err());
    assert_eq!(events.scan(usize::MAX)?.records.len(), 1);
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(replay.status, DurableRecoveryStatus::Healthy);
    assert!(replay.writable);
    Ok(())
}
