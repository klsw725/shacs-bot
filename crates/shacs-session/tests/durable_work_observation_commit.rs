use serde::Serialize;
use serde_json::json;
use shacs_session::durable_event::*;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::*;

#[test]
fn cancellation_sensitive_terminal_atomically_discards_facts_for_every_outcome(
) -> Result<(), Box<dyn std::error::Error>> {
    for terminal_kind in [
        WorkTerminalKind::Succeeded,
        WorkTerminalKind::Failed,
        WorkTerminalKind::Blocked,
    ] {
        let root = tempfile::tempdir()?;
        let events_root = root.path().join("events");
        let mut events = DurableEventStore::open(&events_root)?;
        append(
            &mut events,
            WORK_ENQUEUED,
            &WorkEnqueued {
                work_id: "observe-1".to_owned(),
                work_kind: "entrusted.observation".to_owned(),
                payload_ref: WorkPayloadRef::inline("observation", json!({"revision": 1}))?,
                dedupe_hint: None,
                next_wake_at_ms: Some(1),
                effect_id: None,
            },
        )?;
        append(
            &mut events,
            WORK_LEASED,
            &WorkLeased {
                work_id: "observe-1".to_owned(),
                lease_id: "lease".to_owned(),
                lease_owner_ref: "owner".to_owned(),
                attempt: 1,
                leased_at_ms: 1,
                lease_expires_at_ms: 100,
            },
        )?;
        let mut other_process = DurableEventStore::open(&events_root)?;
        append(
            &mut other_process,
            WORK_CANCEL_REQUESTED,
            &WorkCancellation {
                work_id: "observe-1".to_owned(),
                reason: "stop".to_owned(),
            },
        )?;

        append(
            &mut events,
            WORK_TERMINAL,
            &WorkTerminalCommit {
                terminal: WorkTerminal {
                    work_id: "observe-1".to_owned(),
                    terminal_kind,
                    outcome_ref: "observation".to_owned(),
                    facts: Some(json!({"observation": "late", "next_wake": 2})),
                },
                cancel_if_requested: true,
            },
        )?;

        let replay = evaluate_durable_recovery(&events_root, root.path().join("checkpoints"));
        let item = replay
            .state
            .ok_or("replay")?
            .work
            .items
            .remove("observe-1")
            .ok_or("work")?;
        assert_eq!(item.state, ReplayWorkState::Cancelled);
        assert!(item.terminal_facts.is_none());
        let scan = events.scan(16)?;
        assert_eq!(scan.records.last().ok_or("event")?.kind, WORK_CANCELLED);
        assert!(!serde_json::to_string(&scan.records)?.contains("\"late\""));
    }
    Ok(())
}

fn append(
    events: &mut DurableEventStore,
    kind: &str,
    value: &impl Serialize,
) -> Result<(), Box<dyn std::error::Error>> {
    events.append(DurableEventInput::new(
        "owner-session",
        kind,
        DurableEventPayload::inline("work", serde_json::to_value(value)?),
    ))?;
    Ok(())
}
