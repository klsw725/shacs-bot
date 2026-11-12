use crate::spec042_web_watch::support::{hand_out, titles, Fixture, TestResult, T0};
use chrono::DateTime;
use serde_json::json;
use shacs_core::runtime::{InboundMessage, AUTOMATION_WORK_KIND};
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, WORK_ENQUEUED,
};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::{ReplayWorkState, WorkPayloadRef, MAX_PROJECTED_WORK_IDS};

const DAY: u64 = 86_400_000;
const BUSY_SESSION: &str = "telegram:busy";

/// Automation runs due for a session whose turn is taken: the worker leaves them queued.
fn queue_waiting_runs(fixture: &Fixture, count: usize) -> TestResult {
    let mut events = DurableEventStore::open(fixture.data_dir.join("runtime/durable-events"))?;
    for index in 0..count {
        events.append(DurableEventInput::new(
            BUSY_SESSION,
            WORK_ENQUEUED,
            DurableEventPayload::inline(
                "durable_work",
                json!({
                    "work_id": format!("waiting-{index}"),
                    "work_kind": AUTOMATION_WORK_KIND,
                    "payload_ref": WorkPayloadRef::inline(
                        "test.waiting.v1",
                        json!({"index": index}),
                    )?,
                }),
            ),
        ))?;
    }
    Ok(())
}

#[test]
fn work_behind_a_full_report_of_waiting_runs_is_still_taken() -> TestResult {
    // Given: a watched price with a mail target, and as many waiting automation runs as the
    // admission report lists, all enqueued before anything else.
    let fixture = Fixture::new()?;
    fixture.watch_price("12,900", "11000")?;
    fixture.notify(Some("email:user@example.test"), None)?;
    queue_waiting_runs(&fixture, MAX_PROJECTED_WORK_IDS)?;
    let deadline = i64::try_from(T0 + 3 * DAY)
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .ok_or("deadline time")?
        .to_rfc3339();
    fixture.deadline(Some(&deadline), None, false, 0, T0)?;
    let remind_at = T0 + 2 * DAY;
    let checks_before = fixture.status()?.history.len();

    // When: the worker's deadline step and its notification step run at the reminder time.
    fixture.remind(remind_at)?;
    let sent = fixture.deliver(remind_at)?;

    // Then: the deadline wake and the notification inbound, both queued behind the waiting
    // runs, were taken.
    assert_eq!(titles(&sent), vec!["Decision deadline reminder"]);

    // When: the worker's web watch step runs, and a person's message arrives.
    fixture.tick(remind_at)?;
    let said = InboundMessage::new("cli", "user", "direct", "hello");
    fixture
        .dispatcher()?
        .enqueue_inbound("work-said", &said, None, None)?;
    let handed_out = hand_out(&fixture.data_dir, &fixture.bus, remind_at)?;

    // Then: the watch was checked and the message was handed out.
    assert_eq!(fixture.status()?.history.len(), checks_before + 1);
    assert_eq!(
        handed_out
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>(),
        ["hello"]
    );

    // And: the waiting runs are still queued, untouched.
    let state = evaluate_durable_recovery(
        fixture.data_dir.join("runtime/durable-events"),
        fixture.data_dir.join("checkpoints"),
    )
    .state
    .ok_or("durable replay")?;
    let waiting = state
        .work
        .items
        .values()
        .filter(|item| item.session_key == BUSY_SESSION)
        .map(|item| (item.state, item.attempt))
        .collect::<Vec<_>>();
    assert_eq!(
        waiting,
        vec![(ReplayWorkState::Pending, 0); MAX_PROJECTED_WORK_IDS]
    );
    Ok(())
}
