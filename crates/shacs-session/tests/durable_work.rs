use serde::Serialize;
use serde_json::json;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, RUNTIME_RESTART_REQUESTED,
    RUNTIME_STOP_REQUESTED, WORK_CANCELLED, WORK_CANCEL_REQUESTED, WORK_ENQUEUED, WORK_LEASED,
    WORK_REQUEUED, WORK_RETRY_SCHEDULED, WORK_TERMINAL,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::{
    apply_durable_work_event, evaluate_durable_work_recovery,
    evaluate_durable_work_recovery_for_owner, DurableWorkPayloadStore,
    DurableWorkRecoveryIssueKind, DurableWorkRecoveryStatus, DurableWorkReplayState,
    ReplayWorkState, RuntimeControlRequested, WorkCancellation, WorkEnqueued, WorkLeased,
    WorkPayloadRef, WorkRequeued, WorkRetryScheduled, WorkTerminal, WorkTerminalKind,
    MAX_RETAINED_RUNTIME_REQUESTS, MAX_RETAINED_TERMINAL_WORK_ITEMS,
};
use std::error::Error;
use std::fs;
use std::sync::mpsc;
use std::thread;

fn append(
    events: &mut DurableEventStore,
    kind: &str,
    payload: &impl Serialize,
) -> Result<(), Box<dyn Error>> {
    events.append(DurableEventInput::new(
        "session-1",
        kind,
        DurableEventPayload::inline("durable_work", serde_json::to_value(payload)?),
    ))?;
    Ok(())
}

fn enqueued(work_id: &str, dedupe_hint: Option<&str>) -> Result<WorkEnqueued, Box<dyn Error>> {
    Ok(WorkEnqueued {
        work_id: work_id.to_owned(),
        work_kind: "agent.inbound_turn".to_owned(),
        payload_ref: WorkPayloadRef::inline(
            "control",
            json!({"message_ref": format!("message-{work_id}")}),
        )?,
        dedupe_hint: dedupe_hint.map(str::to_owned),
        next_wake_at_ms: None,
        effect_id: None,
    })
}

fn leased(work_id: &str) -> WorkLeased {
    WorkLeased {
        work_id: work_id.to_owned(),
        lease_id: format!("lease-{work_id}"),
        lease_owner_ref: "owner-1".to_owned(),
        attempt: 1,
        leased_at_ms: 100,
        lease_expires_at_ms: 200,
    }
}

#[test]
fn completed_dedupe_lineage_is_superseded_without_reexecution() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(
        &mut events,
        WORK_ENQUEUED,
        &enqueued("completed", Some("same"))?,
    )?;
    append(&mut events, WORK_LEASED, &leased("completed"))?;
    append(
        &mut events,
        WORK_TERMINAL,
        &WorkTerminal {
            work_id: "completed".to_owned(),
            terminal_kind: WorkTerminalKind::Succeeded,
            outcome_ref: "completed".to_owned(),
            facts: Some(json!({"job_result": "succeeded"})),
        },
    )?;
    append(
        &mut events,
        WORK_ENQUEUED,
        &enqueued("duplicate", Some("same"))?,
    )?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing replay state")?;
    let duplicate = &state.work.items["duplicate"];
    assert_eq!(duplicate.terminal_kind, Some(WorkTerminalKind::Superseded));
    assert_eq!(
        duplicate
            .terminal_facts
            .as_ref()
            .and_then(|facts| facts["supersedes_work_id"].as_str()),
        Some("completed")
    );
    Ok(())
}

#[test]
fn durable_work_restores_retry_and_cancellation_without_inventing_outcomes(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &enqueued("work-1", None)?)?;
    append(
        &mut events,
        WORK_LEASED,
        &WorkLeased {
            work_id: "work-1".to_owned(),
            lease_id: "lease-1".to_owned(),
            lease_owner_ref: "owner-1".to_owned(),
            attempt: 1,
            leased_at_ms: 100,
            lease_expires_at_ms: 200,
        },
    )?;
    append(
        &mut events,
        WORK_RETRY_SCHEDULED,
        &WorkRetryScheduled {
            work_id: "work-1".to_owned(),
            attempt: 1,
            next_wake_at_ms: 500,
            backoff_ms: 300,
            reason_ref: "provider_retryable".to_owned(),
        },
    )?;
    append(
        &mut events,
        WORK_CANCEL_REQUESTED,
        &WorkCancellation {
            work_id: "work-1".to_owned(),
            reason: "user_stop".to_owned(),
        },
    )?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing durable replay state")?;
    let item = &state.work.items["work-1"];
    assert_eq!(item.state, ReplayWorkState::WaitingRetry);
    assert_eq!(item.attempt, 1);
    assert_eq!(item.next_wake_at_ms, Some(500));
    assert!(item.cancellation_requested_sequence.is_some());
    assert!(item.terminal_sequence.is_none());
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 600);
    assert_eq!(admission.status, DurableWorkRecoveryStatus::Healthy);
    assert!(admission.writable);
    assert_eq!(admission.waiting_retry_count, 1);
    assert_eq!(admission.cancellation_requested_count, 1);
    assert!(admission.due_work_ids.is_empty());

    let mut reopened = DurableEventStore::open(&event_root)?;
    append(
        &mut reopened,
        WORK_CANCELLED,
        &WorkCancellation {
            work_id: "work-1".to_owned(),
            reason: "cancellation_observed".to_owned(),
        },
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing cancelled replay state")?;
    assert_eq!(state.work.items["work-1"].state, ReplayWorkState::Cancelled);
    Ok(())
}

#[test]
fn durable_work_marks_expired_lease_recoverable_until_requeued() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &enqueued("work-1", None)?)?;
    append(
        &mut events,
        WORK_LEASED,
        &WorkLeased {
            work_id: "work-1".to_owned(),
            lease_id: "lease-1".to_owned(),
            lease_owner_ref: "owner-1".to_owned(),
            attempt: 1,
            leased_at_ms: 100,
            lease_expires_at_ms: 200,
        },
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing leased state")?;
    let stale = evaluate_durable_work_recovery(&state.work, root.path().join("payloads"), 201);
    assert_eq!(stale.status, DurableWorkRecoveryStatus::Recoverable);
    assert!(!stale.writable);
    assert_eq!(stale.stale_lease_work_ids, vec!["work-1"]);
    let active_owner = evaluate_durable_work_recovery_for_owner(
        &state.work,
        root.path().join("payloads"),
        201,
        Some("owner-1"),
    );
    assert_eq!(active_owner.status, DurableWorkRecoveryStatus::Healthy);
    assert!(active_owner.writable);
    assert!(active_owner.stale_lease_work_ids.is_empty());

    append(
        &mut events,
        WORK_REQUEUED,
        &WorkRequeued {
            work_id: "work-1".to_owned(),
            reason: "stale_lease".to_owned(),
        },
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing requeued state")?;
    let recovered = evaluate_durable_work_recovery(&state.work, root.path().join("payloads"), 201);
    assert_eq!(recovered.status, DurableWorkRecoveryStatus::Healthy);
    assert_eq!(recovered.pending_count, 1);
    assert_eq!(recovered.due_work_ids, vec!["work-1"]);
    Ok(())
}

#[test]
fn durable_work_blocks_missing_or_corrupt_artifact_payloads() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let payloads = DurableWorkPayloadStore::open(&payload_root)?;
    let payload_ref = payloads.write_json("agent.inbound", &json!({"token": "secret-value"}))?;
    let artifact_ref = match &payload_ref {
        WorkPayloadRef::Artifact { artifact_ref, .. } => artifact_ref.clone(),
        _ => return Err("expected artifact payload".into()),
    };
    let mut events = DurableEventStore::open(&event_root)?;
    append(
        &mut events,
        WORK_ENQUEUED,
        &WorkEnqueued {
            work_id: "work-1".to_owned(),
            work_kind: "agent.inbound_turn".to_owned(),
            payload_ref,
            dedupe_hint: None,
            next_wake_at_ms: None,
            effect_id: None,
        },
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing payload state")?;
    let healthy = evaluate_durable_work_recovery(&state.work, &payload_root, 1);
    assert_eq!(healthy.status, DurableWorkRecoveryStatus::Healthy);
    let stored = fs::read_to_string(payload_root.join(&artifact_ref))?;
    assert!(!stored.contains("secret-value"));

    fs::write(payload_root.join(&artifact_ref), b"{}")?;
    let corrupt = evaluate_durable_work_recovery(&state.work, &payload_root, 1);
    assert_eq!(corrupt.status, DurableWorkRecoveryStatus::Blocked);
    assert!(corrupt
        .issues
        .iter()
        .any(|issue| issue.kind == DurableWorkRecoveryIssueKind::CorruptPayload));
    fs::remove_file(payload_root.join(&artifact_ref))?;
    let missing = evaluate_durable_work_recovery(&state.work, &payload_root, 1);
    assert!(missing
        .issues
        .iter()
        .any(|issue| issue.kind == DurableWorkRecoveryIssueKind::MissingPayload));
    Ok(())
}

#[test]
fn durable_work_success_before_cancellation_remains_successful() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(
        &mut events,
        WORK_ENQUEUED,
        &enqueued("terminal-first", None)?,
    )?;
    append(&mut events, WORK_LEASED, &leased("terminal-first"))?;
    append(
        &mut events,
        WORK_TERMINAL,
        &WorkTerminal {
            work_id: "terminal-first".to_owned(),
            terminal_kind: WorkTerminalKind::Succeeded,
            outcome_ref: "accepted_outcome".to_owned(),
            facts: None,
        },
    )?;
    append(
        &mut events,
        WORK_CANCEL_REQUESTED,
        &WorkCancellation {
            work_id: "terminal-first".to_owned(),
            reason: "late_cancel".to_owned(),
        },
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing race replay state")?;
    let item = &state.work.items["terminal-first"];
    assert_eq!(item.state, ReplayWorkState::Terminal);
    assert_eq!(item.terminal_kind, Some(WorkTerminalKind::Succeeded));
    assert!(item.cancellation_requested_sequence.is_some());
    Ok(())
}

#[test]
fn concurrent_cancellation_before_success_resolves_cancelled() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &enqueued("cancel-first", None)?)?;
    append(&mut events, WORK_LEASED, &leased("cancel-first"))?;
    let (cancel_committed, terminal_waits) = mpsc::channel();
    let cancel_root = event_root.clone();
    let cancel_writer = thread::spawn(move || -> Result<(), String> {
        let mut events = DurableEventStore::open(cancel_root).map_err(|error| error.to_string())?;
        append(
            &mut events,
            WORK_CANCEL_REQUESTED,
            &WorkCancellation {
                work_id: "cancel-first".to_owned(),
                reason: "user_stop".to_owned(),
            },
        )
        .map_err(|error| error.to_string())?;
        cancel_committed.send(()).map_err(|error| error.to_string())
    });
    let terminal_root = event_root.clone();
    let terminal_writer = thread::spawn(move || -> Result<String, String> {
        terminal_waits.recv().map_err(|error| error.to_string())?;
        let mut events =
            DurableEventStore::open(terminal_root).map_err(|error| error.to_string())?;
        events
            .append(DurableEventInput::new(
                "session-1",
                WORK_TERMINAL,
                DurableEventPayload::inline(
                    "durable_work",
                    serde_json::to_value(WorkTerminal {
                        work_id: "cancel-first".to_owned(),
                        terminal_kind: WorkTerminalKind::Succeeded,
                        outcome_ref: "already_completed".to_owned(),
                        facts: None,
                    })
                    .map_err(|error| error.to_string())?,
                ),
            ))
            .map(|record| record.kind)
            .map_err(|error| error.to_string())
    });
    cancel_writer
        .join()
        .map_err(|_| "cancellation writer panicked")??;
    let terminal_kind = terminal_writer
        .join()
        .map_err(|_| "terminal writer panicked")??;

    assert_eq!(terminal_kind, WORK_CANCELLED);
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing race replay state")?;
    let item = &state.work.items["cancel-first"];
    assert_eq!(item.state, ReplayWorkState::Cancelled);
    assert_eq!(item.terminal_kind, None);
    let scan = events.scan(usize::MAX)?;
    assert!(!scan.records.iter().any(|record| {
        record.kind == WORK_TERMINAL
            && matches!(
                &record.payload,
                DurableEventPayload::Inline { data, .. }
                    if data.get("work_id").and_then(serde_json::Value::as_str)
                        == Some("cancel-first")
            )
    }));
    Ok(())
}

#[test]
fn persisted_cancel_before_success_history_replays_cancelled() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &enqueued("legacy-race", None)?)?;
    append(&mut events, WORK_LEASED, &leased("legacy-race"))?;
    append(
        &mut events,
        WORK_CANCEL_REQUESTED,
        &WorkCancellation {
            work_id: "legacy-race".to_owned(),
            reason: "user_stop".to_owned(),
        },
    )?;
    let mut persisted = events.scan(usize::MAX)?.records;
    let mut legacy_success = persisted
        .last()
        .cloned()
        .ok_or("missing cancellation request")?;
    legacy_success.sequence += 1;
    legacy_success.event_id = format!("event-{:020}", legacy_success.sequence);
    legacy_success.kind = WORK_TERMINAL.to_owned();
    legacy_success.payload = DurableEventPayload::inline(
        "durable_work",
        serde_json::to_value(WorkTerminal {
            work_id: "legacy-race".to_owned(),
            terminal_kind: WorkTerminalKind::Succeeded,
            outcome_ref: "legacy_success".to_owned(),
            facts: None,
        })?,
    );
    persisted.push(legacy_success);

    let mut state = DurableWorkReplayState::default();
    for event in &persisted {
        apply_durable_work_event(&mut state, event)?;
    }

    let item = &state.items["legacy-race"];
    assert_eq!(item.state, ReplayWorkState::Cancelled);
    assert_eq!(item.terminal_kind, None);
    Ok(())
}

#[test]
fn durable_work_dedupe_retention_and_runtime_requests_are_bounded() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(
        &mut events,
        WORK_ENQUEUED,
        &enqueued("pending", Some("same-message"))?,
    )?;
    append(
        &mut events,
        WORK_ENQUEUED,
        &enqueued("duplicate", Some("same-message"))?,
    )?;
    events.append(DurableEventInput::new(
        "session-2",
        WORK_ENQUEUED,
        DurableEventPayload::inline(
            "durable_work",
            serde_json::to_value(enqueued("other-session", Some("same-message"))?)?,
        ),
    ))?;
    let dedupe_replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let dedupe_state = dedupe_replay.state.ok_or("missing dedupe state")?;
    assert_eq!(
        dedupe_state.work.items["duplicate"].terminal_kind,
        Some(WorkTerminalKind::Superseded)
    );
    assert_eq!(
        dedupe_state.work.items["duplicate"]
            .terminal_facts
            .as_ref()
            .and_then(|facts| facts["supersedes_work_id"].as_str()),
        Some("pending")
    );
    assert_eq!(
        dedupe_state.work.items["other-session"].state,
        ReplayWorkState::Pending
    );
    for index in 0..=MAX_RETAINED_TERMINAL_WORK_ITEMS {
        let work_id = format!("terminal-{index}");
        append(&mut events, WORK_ENQUEUED, &enqueued(&work_id, None)?)?;
        append(&mut events, WORK_LEASED, &leased(&work_id))?;
        append(
            &mut events,
            WORK_TERMINAL,
            &WorkTerminal {
                work_id,
                terminal_kind: WorkTerminalKind::Exhausted,
                outcome_ref: "attempt_limit".to_owned(),
                facts: None,
            },
        )?;
    }
    for index in 0..=MAX_RETAINED_RUNTIME_REQUESTS {
        append(
            &mut events,
            if index % 2 == 0 {
                RUNTIME_STOP_REQUESTED
            } else {
                RUNTIME_RESTART_REQUESTED
            },
            &RuntimeControlRequested {
                requested_at_ms: index as u64,
                request_id: Some(format!("request-{index}")),
                target_owner_id: Some("owner-1".to_owned()),
            },
        )?;
    }
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing bounded work state")?;
    assert_eq!(
        state
            .work
            .items
            .values()
            .filter(|item| item.state.is_terminal())
            .count(),
        MAX_RETAINED_TERMINAL_WORK_ITEMS
    );
    assert_eq!(state.work.terminal_evicted_count, 2);
    assert_eq!(
        state.work.runtime_requests.len(),
        MAX_RETAINED_RUNTIME_REQUESTS
    );
    assert_eq!(state.work.items["pending"].state, ReplayWorkState::Pending);
    Ok(())
}

#[test]
fn durable_work_rejects_forged_terminal_and_cancellation_outcomes() -> Result<(), Box<dyn Error>> {
    for (kind, payload) in [
        (
            WORK_CANCELLED,
            serde_json::to_value(WorkCancellation {
                work_id: "work-1".to_owned(),
                reason: "forged_cancel".to_owned(),
            })?,
        ),
        (
            WORK_TERMINAL,
            serde_json::to_value(WorkTerminal {
                work_id: "work-1".to_owned(),
                terminal_kind: WorkTerminalKind::Succeeded,
                outcome_ref: "forged_success".to_owned(),
                facts: None,
            })?,
        ),
    ] {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let checkpoint_root = root.path().join("checkpoints");
        let mut events = DurableEventStore::open(&event_root)?;
        append(&mut events, WORK_ENQUEUED, &enqueued("work-1", None)?)?;
        events.append(DurableEventInput::new(
            "session-1",
            kind,
            DurableEventPayload::inline("durable_work", payload),
        ))?;
        let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
        assert_eq!(
            replay.status,
            shacs_session::durable_replay::DurableRecoveryStatus::Blocked
        );
        let state = replay.state.ok_or("missing valid replay prefix")?;
        assert_eq!(state.work.items["work-1"].state, ReplayWorkState::Pending);
    }
    Ok(())
}

#[test]
fn durable_work_rejects_retry_after_cancellation_request() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let mut events = DurableEventStore::open(&event_root)?;
    append(&mut events, WORK_ENQUEUED, &enqueued("work-1", None)?)?;
    append(&mut events, WORK_LEASED, &leased("work-1"))?;
    append(
        &mut events,
        WORK_CANCEL_REQUESTED,
        &WorkCancellation {
            work_id: "work-1".to_owned(),
            reason: "user_stop".to_owned(),
        },
    )?;
    append(
        &mut events,
        WORK_RETRY_SCHEDULED,
        &WorkRetryScheduled {
            work_id: "work-1".to_owned(),
            attempt: 1,
            next_wake_at_ms: 500,
            backoff_ms: 300,
            reason_ref: "retryable".to_owned(),
        },
    )?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert_eq!(
        replay.status,
        shacs_session::durable_replay::DurableRecoveryStatus::Blocked
    );
    let state = replay.state.ok_or("missing valid replay prefix")?;
    assert_eq!(state.work.items["work-1"].state, ReplayWorkState::Leased);
    assert!(state.work.items["work-1"]
        .cancellation_requested_sequence
        .is_some());
    Ok(())
}
