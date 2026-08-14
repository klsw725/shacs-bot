use fs2::FileExt;
use shacs_core::runtime::{
    DurableWorkDispatcher, DurableWorkEnqueueInput, InboundMessage, MessageBus,
};
use shacs_session::durable_child::DurableChildRecorder;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, WORK_ENQUEUED,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_trace::DurableTraceStore;
use shacs_session::durable_work::{
    evaluate_durable_work_recovery, DurableWorkPayloadStore, DurableWorkRecoveryStatus,
    ReplayWorkState, WorkPayloadRef, WorkTerminalKind, MAX_DURABLE_WORK_OPEN_ITEMS,
};
use std::collections::BTreeSet;
use std::error::Error;
use std::sync::{Arc, Barrier};

fn payload_artifacts(root: &std::path::Path) -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    std::fs::read_dir(root)?
        .filter_map(|entry| match entry {
            Ok(entry) if entry.path().extension().is_some_and(|ext| ext == "json") => {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn seed_open_work(event_root: &std::path::Path, count: usize) -> Result<(), Box<dyn Error>> {
    let mut events = DurableEventStore::open(event_root)?;
    for index in 0..count {
        let work_id = format!("seed-{index}");
        events.append(DurableEventInput::new(
            "session",
            WORK_ENQUEUED,
            DurableEventPayload::inline(
                "durable_work",
                serde_json::json!({
                    "work_id": work_id,
                    "work_kind": "test.seed",
                    "payload_ref": WorkPayloadRef::inline(
                        "test.seed.v1",
                        serde_json::json!({"index": index}),
                    )?,
                }),
            ),
        ))?;
    }
    Ok(())
}

#[test]
fn concurrent_dispatchers_cannot_lease_the_same_work_twice() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let mut producer = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        MessageBus::new(),
        "producer",
        100,
    )?;
    producer.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing pending state")?;
    let item = pending.work.items["work-1"].clone();
    let barrier = Arc::new(Barrier::new(2));

    let handles = ["owner-1", "owner-2"].map(|owner| {
        let event_root = event_root.clone();
        let payload_root = payload_root.clone();
        let item = item.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            let mut dispatcher = DurableWorkDispatcher::open(
                event_root,
                payload_root,
                MessageBus::new(),
                owner,
                100,
            )?;
            barrier.wait();
            dispatcher.lease_work(&item, 100).map(|_| ())
        })
    });
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().map_err(|_| "lease thread panicked"))
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing leased state")?;
    assert_eq!(leased.work.items["work-1"].state, ReplayWorkState::Leased);
    assert_eq!(leased.work.items["work-1"].attempt, 1);
    Ok(())
}

#[test]
fn durable_dispatch_restores_due_inbound_and_requeues_stale_lease() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner-1", 100)?;
    let message = InboundMessage::new("cli", "user", "direct", "hello")
        .with_session_key_override("cli:restored");
    dispatcher.enqueue_inbound("work-1", &message, Some("message-1".to_owned()), None)?;
    let traces = DurableTraceStore::scan_existing(event_root.join("durable-diagnostics"), 10)?;
    let channel_trace = traces
        .records
        .iter()
        .find(|record| record.kind == "durable_channel.inbound_committed")
        .ok_or("missing channel-correlated diagnostics evidence")?;
    assert_eq!(channel_trace.correlation.channel_id.as_deref(), Some("cli"));
    assert!(channel_trace
        .correlation
        .service_correlation_id
        .as_deref()
        .is_some_and(|value| value.starts_with("service:")));
    assert!(!format!("{channel_trace:?}").contains("work-1"));
    let persisted_traces = std::fs::read_to_string(
        event_root
            .join("durable-diagnostics")
            .join("diagnostics.log"),
    )?;
    assert!(!persisted_traces.contains("work-1"));
    assert!(!persisted_traces.contains("hello"));

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing pending state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 100);
    assert_eq!(admission.due_work_ids, vec!["work-1"]);
    let dispatched = dispatcher.dispatch_due(&state.work, &admission, 100)?;
    assert_eq!(dispatched.leased_work_ids, vec!["work-1"]);
    let restored = bus.consume_inbound().ok_or("missing restored inbound")?;
    assert_eq!(restored.content, "hello");
    assert_eq!(restored.session_key(), "cli:restored");
    assert_eq!(restored.metadata["durable_work_id"], "work-1");

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing leased state")?;
    assert_eq!(leased.work.items["work-1"].state, ReplayWorkState::Leased);
    let stale = evaluate_durable_work_recovery(&leased.work, &payload_root, 201);
    assert_eq!(stale.status, DurableWorkRecoveryStatus::Recoverable);

    let restart_bus = MessageBus::new();
    let mut restarted = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        restart_bus.clone(),
        "owner-2",
        100,
    )?;
    restarted.requeue_stale(&leased.work, &stale)?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing requeued state")?;
    let due = evaluate_durable_work_recovery(&pending.work, &payload_root, 201);
    restarted.dispatch_due(&pending.work, &due, 201)?;
    assert!(restart_bus.consume_inbound().is_some());
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing second lease state")?;
    assert_eq!(state.work.items["work-1"].attempt, 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn durable_dispatch_event_commit_survives_trace_append_unavailable() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    std::fs::create_dir_all(&event_root)?;
    std::os::unix::fs::symlink(
        root.path().join("missing-trace-target"),
        event_root.join("durable-diagnostics"),
    )?;
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus, "owner-1", 100)?;

    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    assert!(replay.writable);
    let state = replay.state.ok_or("missing committed event state")?;
    assert!(state.work.items.contains_key("work-1"));
    assert!(DurableTraceStore::open(event_root.join("durable-diagnostics")).is_err());
    Ok(())
}

#[test]
fn durable_dispatch_cancels_stale_lease_with_prior_cancellation_request(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus, "owner-1", 100)?;
    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing pending state")?;
    let admission = evaluate_durable_work_recovery(&pending.work, &payload_root, 100);
    dispatcher.dispatch_due(&pending.work, &admission, 100)?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing leased state")?;
    dispatcher.request_cancellation(&leased.work.items["work-1"], "runtime_shutdown")?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let requested = replay.state.ok_or("missing requested state")?;
    let stale = evaluate_durable_work_recovery(&requested.work, &payload_root, 201);
    let recovered = dispatcher.requeue_stale(&requested.work, &stale)?;
    assert_eq!(recovered.cancelled_work_ids, vec!["work-1"]);
    assert!(recovered.requeued_work_ids.is_empty());
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let cancelled = replay.state.ok_or("missing cancelled state")?;
    assert_eq!(
        cancelled.work.items["work-1"].state,
        ReplayWorkState::Cancelled
    );
    Ok(())
}

#[test]
fn durable_dispatch_rejects_retry_after_cancellation_request() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus, "owner-1", 100)?;
    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing pending state")?;
    let admission = evaluate_durable_work_recovery(&pending.work, &payload_root, 100);
    dispatcher.dispatch_due(&pending.work, &admission, 100)?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing leased state")?;
    dispatcher.request_cancellation(&leased.work.items["work-1"], "user_stop")?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let requested = replay.state.ok_or("missing requested state")?;

    let error = dispatcher
        .schedule_retry(&requested.work.items["work-1"], 500, 300, "retryable")
        .expect_err("cancelled work must not enter retry waiting");
    assert!(error
        .to_string()
        .contains("cannot retry after cancellation was requested"));
    Ok(())
}

#[test]
fn durable_dispatch_does_not_spend_retry_attempt_while_session_is_busy(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner-1", 100)?;
    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("discord", "user", "channel-1", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing pending state")?;
    let due = evaluate_durable_work_recovery(&pending.work, &payload_root, 100);
    dispatcher.dispatch_due(&pending.work, &due, 100)?;
    let _ = bus.consume_inbound().ok_or("missing first attempt")?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing leased state")?;
    dispatcher.schedule_retry(&leased.work.items["work-1"], 200, 100, "session_busy")?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let waiting = replay.state.ok_or("missing waiting retry state")?;
    let due = evaluate_durable_work_recovery(&waiting.work, &payload_root, 200);
    let busy_sessions = BTreeSet::from(["discord:channel-1".to_owned()]);
    let summary =
        dispatcher.dispatch_due_excluding_sessions(&waiting.work, &due, 200, &busy_sessions)?;

    assert!(summary.leased_work_ids.is_empty());
    assert!(bus.consume_inbound().is_none());
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let still_waiting = replay.state.ok_or("missing gated retry state")?;
    assert_eq!(
        still_waiting.work.items["work-1"].state,
        ReplayWorkState::WaitingRetry
    );
    assert_eq!(still_waiting.work.items["work-1"].attempt, 1);
    Ok(())
}

#[test]
fn durable_dispatch_retries_then_exhausts_when_process_local_bus_is_full(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::bounded(1);
    bus.try_publish_inbound(InboundMessage::new("cli", "user", "occupied", "busy"))?;
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus, "owner-1", 100)?;
    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing pending state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 100);
    let result = dispatcher.dispatch_due(&state.work, &admission, 100)?;
    assert_eq!(result.retry_scheduled_work_ids, vec!["work-1"]);
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let mut state = replay.state.ok_or("missing retry state")?;
    assert_eq!(
        state.work.items["work-1"].state,
        ReplayWorkState::WaitingRetry
    );
    assert_eq!(state.work.items["work-1"].attempt, 1);
    for attempt in 2..=shacs_session::durable_work::MAX_DURABLE_WORK_ATTEMPTS {
        let now_ms = 100 + u64::from(attempt - 1) * 250;
        let admission = evaluate_durable_work_recovery(&state.work, &payload_root, now_ms);
        let result = dispatcher.dispatch_due(&state.work, &admission, now_ms)?;
        let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
        state = replay.state.ok_or("missing bus-full retry state")?;
        if attempt < shacs_session::durable_work::MAX_DURABLE_WORK_ATTEMPTS {
            assert_eq!(result.retry_scheduled_work_ids, vec!["work-1"]);
            assert_eq!(
                state.work.items["work-1"].state,
                ReplayWorkState::WaitingRetry
            );
        } else {
            assert_eq!(result.exhausted_work_ids, vec!["work-1"]);
            assert_eq!(
                state.work.items["work-1"].terminal_kind,
                Some(WorkTerminalKind::Exhausted)
            );
        }
    }
    Ok(())
}

#[test]
fn durable_dispatch_persists_retry_request_and_cancellation_outcome() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus, "owner-1", 100)?;
    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing pending state")?;
    let due = evaluate_durable_work_recovery(&pending.work, &payload_root, 100);
    dispatcher.dispatch_due(&pending.work, &due, 100)?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing leased state")?;
    dispatcher.schedule_retry(&leased.work.items["work-1"], 500, 300, "retryable")?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let waiting = replay.state.ok_or("missing retry state")?;
    assert_eq!(
        waiting.work.items["work-1"].state,
        ReplayWorkState::WaitingRetry
    );
    dispatcher.request_cancellation(&waiting.work.items["work-1"], "user_stop")?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let requested = replay.state.ok_or("missing cancel request state")?;
    assert!(requested.work.items["work-1"]
        .cancellation_requested_sequence
        .is_some());
    dispatcher.record_cancelled(&requested.work.items["work-1"], "token_observed")?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let cancelled = replay.state.ok_or("missing cancelled state")?;
    assert_eq!(
        cancelled.work.items["work-1"].state,
        ReplayWorkState::Cancelled
    );

    dispatcher.enqueue_inbound(
        "work-2",
        &InboundMessage::new("cli", "user", "second", "hello"),
        None,
        None,
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let pending = replay.state.ok_or("missing second pending state")?;
    let due = evaluate_durable_work_recovery(&pending.work, &payload_root, 100);
    dispatcher.dispatch_due(&pending.work, &due, 100)?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let leased = replay.state.ok_or("missing second leased state")?;
    dispatcher.record_terminal(
        &leased.work.items["work-2"],
        WorkTerminalKind::Exhausted,
        "attempt_limit",
    )?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let terminal = replay.state.ok_or("missing terminal state")?;
    assert_eq!(
        terminal.work.items["work-2"].terminal_kind,
        Some(WorkTerminalKind::Exhausted)
    );
    Ok(())
}

#[test]
fn durable_dispatch_leases_only_one_work_per_session() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner-1", 100)?;
    for work_id in ["work-1", "work-2"] {
        dispatcher.enqueue_inbound(
            work_id,
            &InboundMessage::new("cli", "user", "direct", work_id)
                .with_session_key_override("cli:same"),
            None,
            None,
        )?;
    }
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing pending state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 100);
    let dispatched = dispatcher.dispatch_due(&state.work, &admission, 100)?;
    assert_eq!(dispatched.leased_work_ids.len(), 1);
    assert!(bus.consume_inbound().is_some());
    assert!(bus.consume_inbound().is_none());
    Ok(())
}

#[test]
fn durable_dispatch_allows_explicit_priority_work_for_active_session() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner-1", 100)?;
    for (work_id, content) in [("active", "hello"), ("stop", "/stop")] {
        dispatcher.enqueue_inbound(
            work_id,
            &InboundMessage::new("cli", "user", "direct", content)
                .with_session_key_override("cli:same"),
            None,
            None,
        )?;
    }
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing pending state")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 100);
    dispatcher.dispatch_due(&state.work, &admission, 100)?;
    let _ = bus.consume_inbound().ok_or("missing active inbound")?;
    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing active lease state")?;
    dispatcher.dispatch_priority(&state.work.items["stop"], 101)?;
    let stop = bus
        .consume_inbound()
        .ok_or("missing priority stop inbound")?;
    assert_eq!(stop.content, "/stop");
    Ok(())
}

#[test]
fn durable_dispatch_rejects_oversized_inbound_without_poisoning_dispatcher(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    let mut dispatcher = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        MessageBus::new(),
        "owner-1",
        100,
    )?;
    let oversized = InboundMessage::new("cli", "user", "direct", "x".repeat(1024 * 1024));
    let error = dispatcher
        .enqueue_inbound("oversized", &oversized, None, None)
        .expect_err("oversized durable payload must be rejected");
    assert!(error.to_string().contains("exceeds"));
    dispatcher.enqueue_inbound(
        "valid",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;
    Ok(())
}

#[test]
fn generic_enqueue_fails_closed_when_event_log_reaches_quota() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    let mut dispatcher = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        MessageBus::new(),
        "owner-1",
        100,
    )?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(event_root.join("events.log"))?
        .set_len(512 * 1024 * 1024)?;

    // When
    let error = dispatcher
        .enqueue_work(DurableWorkEnqueueInput {
            work_id: "generic-at-quota".to_owned(),
            work_kind: "test.generic".to_owned(),
            session_key: "session".to_owned(),
            turn_id: None,
            effect_id: None,
            payload_ref: WorkPayloadRef::inline(
                "test.generic.v1",
                serde_json::json!({"value": "bounded"}),
            )?,
            dedupe_hint: None,
            next_wake_at_ms: None,
        })
        .expect_err("generic work must be rejected at the event-log quota");

    // Then
    assert!(error.to_string().contains("durable event log exceeds"));
    Ok(())
}

#[test]
fn generic_enqueue_rejects_new_work_when_open_work_limit_is_reached() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    seed_open_work(&event_root, MAX_DURABLE_WORK_OPEN_ITEMS)?;
    let mut dispatcher = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        MessageBus::new(),
        "owner-1",
        100,
    )?;

    // When
    let error = dispatcher
        .enqueue_work(DurableWorkEnqueueInput {
            work_id: "over-limit".to_owned(),
            work_kind: "test.generic".to_owned(),
            session_key: "session".to_owned(),
            turn_id: None,
            effect_id: None,
            payload_ref: WorkPayloadRef::inline(
                "test.generic.v1",
                serde_json::json!({"value": "rejected"}),
            )?,
            dedupe_hint: None,
            next_wake_at_ms: None,
        })
        .expect_err("new work must be rejected at the open-work limit");

    // Then
    assert!(error.to_string().contains("open durable work limit"));
    let state = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), MAX_DURABLE_WORK_OPEN_ITEMS);
    assert!(!state.work.items.contains_key("over-limit"));
    Ok(())
}

#[test]
fn idempotent_generic_enqueue_succeeds_when_open_work_limit_is_reached(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    seed_open_work(&event_root, MAX_DURABLE_WORK_OPEN_ITEMS)?;
    let mut dispatcher = DurableWorkDispatcher::open(
        &event_root,
        &payload_root,
        MessageBus::new(),
        "owner-1",
        100,
    )?;

    // When
    let record = dispatcher.enqueue_work(DurableWorkEnqueueInput {
        work_id: "seed-0".to_owned(),
        work_kind: "test.seed".to_owned(),
        session_key: "session".to_owned(),
        turn_id: None,
        effect_id: None,
        payload_ref: WorkPayloadRef::inline("test.seed.v1", serde_json::json!({"index": 0}))?,
        dedupe_hint: None,
        next_wake_at_ms: None,
    })?;

    // Then
    assert_eq!(record.sequence, 1);
    let state = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), MAX_DURABLE_WORK_OPEN_ITEMS);
    Ok(())
}

#[test]
fn concurrent_generic_enqueue_admits_only_one_final_open_work_slot() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    seed_open_work(&event_root, MAX_DURABLE_WORK_OPEN_ITEMS - 1)?;
    let barrier = Arc::new(Barrier::new(3));
    let handles = (0..2)
        .map(|index| {
            let event_root = event_root.clone();
            let payload_root = payload_root.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || -> Result<(), String> {
                let mut dispatcher = DurableWorkDispatcher::open(
                    event_root,
                    payload_root,
                    MessageBus::new(),
                    format!("owner-{index}"),
                    100,
                )
                .map_err(|error| error.to_string())?;
                barrier.wait();
                dispatcher
                    .enqueue_work(DurableWorkEnqueueInput {
                        work_id: format!("concurrent-{index}"),
                        work_kind: "test.generic".to_owned(),
                        session_key: "session".to_owned(),
                        turn_id: None,
                        effect_id: None,
                        payload_ref: WorkPayloadRef::inline(
                            "test.generic.v1",
                            serde_json::json!({"index": index}),
                        )
                        .map_err(|error| error.to_string())?,
                        dedupe_hint: None,
                        next_wake_at_ms: None,
                    })
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
        })
        .collect::<Vec<_>>();

    // When
    barrier.wait();
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().map_err(|_| "enqueue thread panicked"))
        .collect::<Result<Vec<_>, _>>()?;

    // Then
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    assert!(outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .any(|error| error.contains("open durable work limit")));
    let state = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), MAX_DURABLE_WORK_OPEN_ITEMS);
    Ok(())
}

#[test]
fn child_and_dispatch_enqueue_admit_only_one_final_open_work_slot() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    seed_open_work(&event_root, MAX_DURABLE_WORK_OPEN_ITEMS - 1)?;
    let event_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(event_root.join("events.lock"))?;
    event_lock.lock_exclusive()?;
    let dispatcher_event_root = event_root.clone();
    let dispatcher_payload_root = payload_root.clone();
    let dispatcher = std::thread::spawn(move || -> Result<(), String> {
        let mut dispatcher = DurableWorkDispatcher::open(
            dispatcher_event_root,
            dispatcher_payload_root,
            MessageBus::new(),
            "dispatcher",
            100,
        )
        .map_err(|error| error.to_string())?;
        dispatcher
            .enqueue_work(DurableWorkEnqueueInput {
                work_id: "dispatcher-final-slot".to_owned(),
                work_kind: "test.dispatcher".to_owned(),
                session_key: "session".to_owned(),
                turn_id: None,
                effect_id: None,
                payload_ref: WorkPayloadRef::inline(
                    "test.dispatcher.v1",
                    serde_json::json!({"source": "dispatcher"}),
                )
                .map_err(|error| error.to_string())?,
                dedupe_hint: None,
                next_wake_at_ms: None,
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    let child_event_root = event_root.clone();
    let child_payload_root = payload_root.clone();
    let child = std::thread::spawn(move || -> Result<(), String> {
        let recorder =
            DurableChildRecorder::open_with_payload_root(child_event_root, child_payload_root)
                .map_err(|error| error.to_string())?;
        recorder.ensure_child_run_work(
            "session",
            "turn:parent",
            "quota-race",
            "spawn:quota-race",
            WorkPayloadRef::inline(
                "subagent.child_run.v1",
                serde_json::json!({"source": "child"}),
            )
            .map_err(|error| error.to_string())?,
        )
    });

    // When
    event_lock.unlock()?;
    let outcomes = [
        dispatcher
            .join()
            .map_err(|_| "dispatcher thread panicked")?,
        child.join().map_err(|_| "child thread panicked")?,
    ];

    // Then
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
    assert!(outcomes
        .iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .any(|error| error.contains("open durable work limit")));
    let state = evaluate_durable_recovery(&event_root, root.path().join("checkpoints"))
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), MAX_DURABLE_WORK_OPEN_ITEMS);
    Ok(())
}

#[test]
fn append_failure_removes_only_new_work_payload_artifact() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, MessageBus::new(), "owner", 100)?;
    let message =
        InboundMessage::new("cli", "user", "direct", "orphan").with_session_key_override("");

    // When
    let error = dispatcher
        .enqueue_inbound("append-fails", &message, None, None)
        .expect_err("invalid session identity must fail event append");

    // Then
    assert!(error.to_string().contains("session_id"));
    assert!(payload_artifacts(&payload_root)?.is_empty());
    Ok(())
}

#[test]
fn append_failure_preserves_preexisting_idempotent_payload_artifact() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let payload_root = root.path().join("payloads");
    let message =
        InboundMessage::new("cli", "user", "direct", "existing").with_session_key_override("");
    let value = serde_json::to_value(&message)?;
    let payloads = DurableWorkPayloadStore::open(&payload_root)?;
    let existing = payloads.write_json("shacs.inbound_message.v1", &value)?;
    let before = payload_artifacts(&payload_root)?;
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, MessageBus::new(), "owner", 100)?;

    // When
    dispatcher
        .enqueue_inbound("append-fails-existing", &message, None, None)
        .expect_err("invalid session identity must fail event append");

    // Then
    assert_eq!(payload_artifacts(&payload_root)?, before);
    payloads.verify(&existing)?;
    assert_eq!(payloads.read_json(&existing)?, value);
    Ok(())
}

#[test]
fn durable_dispatch_skips_child_run_coordination_work_without_failing() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let bus = MessageBus::new();
    let mut dispatcher =
        DurableWorkDispatcher::open(&event_root, &payload_root, bus.clone(), "owner-1", 100)?;
    let payloads = DurableWorkPayloadStore::open(&payload_root)?;
    let child_payload = payloads.write_json(
        "shacs.child_run.v1",
        &serde_json::json!({"child_task_id": "child-1"}),
    )?;
    dispatcher.enqueue_work(DurableWorkEnqueueInput {
        work_id: "child-run-child-1".to_owned(),
        work_kind: "subagent.child_run".to_owned(),
        session_key: "session".to_owned(),
        turn_id: Some("turn:session".to_owned()),
        effect_id: Some("spawn:child-1".to_owned()),
        payload_ref: child_payload,
        dedupe_hint: Some("subagent.child_run:session:child-1".to_owned()),
        next_wake_at_ms: None,
    })?;
    dispatcher.enqueue_inbound(
        "work-1",
        &InboundMessage::new("cli", "user", "direct", "hello"),
        None,
        None,
    )?;

    let replay = evaluate_durable_recovery(&event_root, &checkpoint_root);
    let state = replay.state.ok_or("missing replay")?;
    let admission = evaluate_durable_work_recovery(&state.work, &payload_root, 100);
    assert!(admission
        .due_work_ids
        .contains(&"child-run-child-1".to_owned()));
    let dispatched = dispatcher.dispatch_due(&state.work, &admission, 100)?;
    assert_eq!(dispatched.leased_work_ids, vec!["work-1"]);
    assert_eq!(
        bus.consume_inbound().ok_or("missing inbound")?.content,
        "hello"
    );
    Ok(())
}

#[test]
fn concurrent_same_identity_enqueue_is_linearizable() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let event_root = root.path().join("events");
    let checkpoint_root = root.path().join("checkpoints");
    let payload_root = root.path().join("payloads");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let message =
        InboundMessage::new("cli", "user", "direct", "same").with_session_key_override("cli:race");
    let handles = (0..2)
        .map(|index| {
            let event_root = event_root.clone();
            let payload_root = payload_root.clone();
            let barrier = barrier.clone();
            let message = message.clone();
            std::thread::spawn(move || -> Result<u64, String> {
                let mut dispatcher = DurableWorkDispatcher::open(
                    event_root,
                    payload_root,
                    MessageBus::new(),
                    format!("owner-{index}"),
                    100,
                )
                .map_err(|error| error.to_string())?;
                barrier.wait();
                dispatcher
                    .enqueue_inbound("work-race", &message, Some("same".to_owned()), None)
                    .map(|record| record.sequence)
                    .map_err(|error| error.to_string())
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let sequences = handles
        .into_iter()
        .map(|handle| handle.join().map_err(|_| "enqueue thread panicked")?)
        .collect::<Result<Vec<_>, _>>()?;

    assert_eq!(sequences[0], sequences[1]);
    let state = evaluate_durable_recovery(&event_root, &checkpoint_root)
        .state
        .ok_or("missing replay")?;
    assert_eq!(state.work.items.len(), 1);
    assert_eq!(
        state.work.items["work-race"].enqueued_sequence,
        sequences[0]
    );
    Ok(())
}
