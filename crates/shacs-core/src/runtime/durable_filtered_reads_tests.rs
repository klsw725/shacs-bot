//! Each consumer that reads only the events its filters select gets what a read of every event
//! gets, from stores that mix in events of other sessions, kinds and work ids.

use super::entrusted_comparison_evaluate::compare;
use super::entrusted_notification_store::{read_notifications, read_notifications_through};
use super::entrusted_observation_store::ObservationLedger;
use super::web_watch_deadline_dispatch::{
    read_web_deadline_records, read_web_deadline_records_through,
};
use super::web_watch_notify_store::{read_web_notifications, read_web_notifications_through};
use super::web_watch_store::WebWatchLedger;
use super::*;
use serde::Serialize;
use serde_json::{json, Value};
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, ENTRUSTED_NOTIFICATION_FACT,
    SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED, WEB_WATCH_NOTIFICATION_FACT, WORKFLOW_PLANNED,
    WORK_CANCELLED, WORK_CANCEL_REQUESTED, WORK_ENQUEUED, WORK_LEASED, WORK_TERMINAL,
};
use shacs_session::durable_work::{
    WorkCancellation, WorkEnqueued, WorkLeased, WorkPayloadRef, WorkTerminal, WorkTerminalKind,
};
use std::error::Error;
use std::path::PathBuf;

struct Events {
    _root: tempfile::TempDir,
    path: PathBuf,
    store: DurableEventStore,
}

impl Events {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("runtime").join("durable-events");
        let store = DurableEventStore::open(&path)?;
        Ok(Self {
            _root: root,
            path,
            store,
        })
    }

    fn fact(&mut self, session_id: &str, kind: &str, data: Value) -> Result<(), Box<dyn Error>> {
        self.store.append(DurableEventInput::new(
            session_id,
            kind,
            DurableEventPayload::inline("test_fact", data),
        ))?;
        Ok(())
    }

    fn work(
        &mut self,
        session_id: &str,
        kind: &str,
        payload: &impl Serialize,
    ) -> Result<(), Box<dyn Error>> {
        self.store.append(DurableEventInput::new(
            session_id,
            kind,
            DurableEventPayload::inline("durable_work", serde_json::to_value(payload)?),
        ))?;
        Ok(())
    }

    fn enqueued(
        &mut self,
        session_id: &str,
        work_id: &str,
        work_kind: &str,
        payload: Value,
    ) -> Result<(), Box<dyn Error>> {
        let dedupe_hint = (work_kind == "agent.inbound_turn").then(|| work_id.to_owned());
        self.work(
            session_id,
            WORK_ENQUEUED,
            &WorkEnqueued {
                work_id: work_id.to_owned(),
                work_kind: work_kind.to_owned(),
                payload_ref: WorkPayloadRef::inline("test.work.v1", payload)?,
                dedupe_hint,
                next_wake_at_ms: None,
                effect_id: None,
            },
        )
    }

    fn leased(&mut self, session_id: &str, work_id: &str) -> Result<(), Box<dyn Error>> {
        self.work(
            session_id,
            WORK_LEASED,
            &WorkLeased {
                work_id: work_id.to_owned(),
                lease_id: format!("lease-{work_id}"),
                lease_owner_ref: "owner-1".to_owned(),
                attempt: 1,
                leased_at_ms: 100,
                lease_expires_at_ms: 200,
            },
        )
    }

    fn terminal(
        &mut self,
        session_id: &str,
        work_id: &str,
        terminal_kind: WorkTerminalKind,
        outcome_ref: &str,
        facts: Option<Value>,
    ) -> Result<(), Box<dyn Error>> {
        self.work(
            session_id,
            WORK_TERMINAL,
            &WorkTerminal {
                work_id: work_id.to_owned(),
                terminal_kind,
                outcome_ref: outcome_ref.to_owned(),
                facts,
            },
        )
    }

    fn finished(
        &mut self,
        session_id: &str,
        work_id: &str,
        terminal_kind: WorkTerminalKind,
        outcome_ref: &str,
        facts: Option<Value>,
    ) -> Result<(), Box<dyn Error>> {
        self.enqueued(session_id, work_id, "test.work", json!({"work": work_id}))?;
        self.leased(session_id, work_id)?;
        self.terminal(session_id, work_id, terminal_kind, outcome_ref, facts)
    }

    /// Events no consumer reacts to: other sessions and kinds, payloads without a string work
    /// id, and work ids that only look like `prefix`.
    fn unrelated(&mut self, prefix: &str) -> Result<(), Box<dyn Error>> {
        for work_id in [
            format!("{}x", prefix.trim_end_matches(':')),
            format!("x{prefix}"),
            "unrelated:1".to_owned(),
        ] {
            self.fact(
                "intruder",
                SESSION_TURN_ACCEPTED,
                json!({"work_id": work_id}),
            )?;
            self.fact("owner", WORKFLOW_PLANNED, json!({"work_id": work_id}))?;
        }
        self.fact("owner", SESSION_TURN_COMPLETED, json!({"work_id": 7}))?;
        self.fact("owner", SESSION_TURN_COMPLETED, json!({"message": prefix}))
    }

    fn decoded_by<T>(
        &self,
        read: impl FnOnce() -> Result<T, Box<dyn Error>>,
    ) -> Result<(T, u64), Box<dyn Error>> {
        let before = self.store.decoded_event_count()?;
        let value = read()?;
        Ok((value, self.store.decoded_event_count()? - before))
    }
}

fn web_wake(work_id: &str, due_at_ms: u64) -> WebWatchWake {
    WebWatchWake {
        entrusted_work_id: work_id.to_owned(),
        owner_session_id: "owner".to_owned(),
        goal_id: "goal-1".to_owned(),
        condition_revision: 1,
        goal_transition_id: "transition-1".to_owned(),
        plan_digest: "sha256:plan".to_owned(),
        due_at_ms,
    }
}

fn web_record(work_id: &str, at_ms: u64, failed: bool) -> WebObservationRecord {
    let response = WebResponseFacts {
        final_url: "https://example.test/".to_owned(),
        final_url_digest: "sha256:url".to_owned(),
        http_status: 200,
        text_digest: format!("sha256:{at_ms}"),
        truncated: false,
    };
    WebObservationRecord {
        wake: web_wake(work_id, at_ms),
        observed_at_ms: at_ms,
        request_url: "https://example.test/".to_owned(),
        request_url_digest: "sha256:url".to_owned(),
        text_version: WEB_TEXT_VERSION.to_owned(),
        result: if failed {
            WebCheckResult::Failed {
                failure: WebCheckFailure::HttpError,
                response: None,
                retry_after_ms: None,
            }
        } else {
            WebCheckResult::Observed {
                response,
                value: WebObservedValue::Number {
                    value: at_ms.to_string(),
                },
            }
        },
        runtime_gap: None,
        next_wake_at_ms: Some(at_ms + 600_000),
        execution_snapshot_ref: None,
        decision: WebDecision {
            decision_id: format!("decision-{work_id}-{at_ms}"),
            verdict: ObservationVerdict::Suppress,
            reason: WebDecisionReason::EquivalentValue,
            previous: None,
            current: None,
        },
        notify_revision: Some(1),
    }
}

fn observation_record(
    work_id: &str,
    due_at_ms: u64,
    verdict: ObservationVerdict,
) -> ObservationRecord {
    let mut record = ObservationRecord {
        wake: ObservationWake {
            entrusted_work_id: work_id.to_owned(),
            owner_session_id: "owner".to_owned(),
            goal_id: "goal-1".to_owned(),
            condition_revision: 1,
            goal_transition_id: "transition-1".to_owned(),
            plan: EntrustedObservationPlan {
                source: ObservationSource::LocalFile {
                    relative_path: "notes.txt".to_owned(),
                },
                cadence_seconds: 600,
                first_wake_at_ms: 1_000,
                comparison: None,
            },
            due_at_ms,
        },
        observed_at_ms: due_at_ms,
        result: ObservationResult::Observed {
            evidence_ref: format!("sha256:{due_at_ms}"),
            byte_count: 10,
            summary: None,
        },
        execution_snapshot_ref: None,
        next_wake_at_ms: Some(due_at_ms + 600_000),
        decision: None,
    };
    let mut decision = compare(&record, None, None);
    decision.verdict = verdict;
    record.decision = Some(decision);
    record
}

fn web_intent(event_id: &str) -> WebNotifyIntent {
    WebNotifyIntent {
        correlation_id: WebNotifyIntent::correlation(WebNotifyKind::Gap, event_id, 1),
        owner_session_id: "owner".to_owned(),
        entrusted_work_id: "watch-1".to_owned(),
        target: "target".to_owned(),
        settings_revision: 1,
        event_id: event_id.to_owned(),
        wake: web_wake("watch-1", 1_000),
        url: "https://example.test/".to_owned(),
        url_digest: "sha256:url".to_owned(),
        content: WebNotifyContent::Gap {
            from_ms: 1_000,
            to_ms: 2_000,
            next_check_at_ms: None,
        },
        deadline_revision: None,
    }
}

fn web_fact(intent: &WebNotifyIntent, stage: WebNotifyStage) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::to_value(WebNotifyFact {
        intent: intent.clone(),
        stage,
        writer_instance_ref: "writer-1".to_owned(),
        failed_at_ms: (stage == WebNotifyStage::EnqueueFailed).then_some(1_500),
    })?)
}

fn deadline_record(work_id: &str, revision: u64) -> WebDeadlineRecord {
    WebDeadlineRecord {
        wake: WebDeadlineWake {
            entrusted_work_id: work_id.to_owned(),
            owner_session_id: "owner".to_owned(),
            goal_id: "goal-1".to_owned(),
            goal_transition_id: "transition-1".to_owned(),
            deadline_revision: revision,
            due_at_ms: 5_000 + revision,
        },
        reached_at_ms: 5_000 + revision,
        deadline: WebDeadline {
            at_ms: 9_000,
            at_text: "tomorrow".to_owned(),
            remind_at_ms: 5_000,
            remind_before_seconds: None,
            remind_now: false,
        },
        goal: "watch the price".to_owned(),
        url: "https://example.test/".to_owned(),
        url_digest: "sha256:url".to_owned(),
        condition_revision: 1,
        condition: WebCondition::TextPresent,
        phrase: None,
        latest: None,
    }
}

fn web_ledger_view(ledger: &WebWatchLedger) -> String {
    format!(
        "{:?}",
        (
            &ledger.state,
            &ledger.history,
            &ledger.comparable,
            &ledger.last_success,
            &ledger.intervals,
            &ledger.gaps,
            &ledger.notify_points,
            &ledger.open_failure,
        )
    )
}

#[test]
fn a_web_watch_ledger_read_through_its_filters_equals_a_full_read() -> Result<(), Box<dyn Error>> {
    // Given
    let mut events = Events::new()?;
    for (index, failed) in [false, true, true, false, true].into_iter().enumerate() {
        for (session_id, watch, offset) in [
            ("owner", "watch-1", 0),
            ("owner", "watch-10", 100),
            ("intruder", "watch-1", 200),
        ] {
            let at_ms = 1_000 + offset + index as u64;
            events.finished(
                session_id,
                &web_wake(watch, at_ms).durable_work_id(),
                if failed {
                    WorkTerminalKind::Failed
                } else {
                    WorkTerminalKind::Succeeded
                },
                WEB_WATCH_OUTCOME,
                Some(serde_json::to_value(web_record(watch, at_ms, failed))?),
            )?;
        }
        events.unrelated("web-watch:watch-1:")?;
    }

    // When
    let (full, full_decoded) = events.decoded_by(|| {
        Ok(WebWatchLedger::read_through(
            &events.path,
            "owner",
            "watch-1",
            None,
        )?)
    })?;
    let (filtered, filtered_decoded) =
        events.decoded_by(|| Ok(WebWatchLedger::read(&events.path, "owner", "watch-1")?))?;

    // Then
    assert_eq!(full.history.len(), 5);
    assert_eq!(web_ledger_view(&filtered), web_ledger_view(&full));
    assert!(filtered_decoded < full_decoded);
    Ok(())
}

#[test]
fn web_notifications_read_through_their_filters_equal_a_full_read() -> Result<(), Box<dyn Error>> {
    // Given
    let mut events = Events::new()?;
    let enqueued = web_intent("gap-1");
    let failed = web_intent("gap-2");
    let foreign = web_intent("gap-3");
    events.fact(
        "owner",
        WEB_WATCH_NOTIFICATION_FACT,
        web_fact(&enqueued, WebNotifyStage::EnqueuePending)?,
    )?;
    events.fact(
        "owner",
        WEB_WATCH_NOTIFICATION_FACT,
        web_fact(&failed, WebNotifyStage::EnqueueFailed)?,
    )?;
    events.fact(
        "intruder",
        WEB_WATCH_NOTIFICATION_FACT,
        web_fact(&foreign, WebNotifyStage::EnqueuePending)?,
    )?;
    for intent in [&enqueued, &failed] {
        events.unrelated(WEB_NOTIFY_PREFIX)?;
        events.enqueued(
            "target",
            &intent.correlation_id,
            "agent.inbound_turn",
            json!({"message": intent.event_id}),
        )?;
    }
    events.fact(
        "owner",
        WEB_WATCH_NOTIFICATION_FACT,
        web_fact(&enqueued, WebNotifyStage::Consuming)?,
    )?;
    events.leased("target", &failed.correlation_id)?;
    events.terminal(
        "target",
        &failed.correlation_id,
        WorkTerminalKind::Failed,
        "delivery_failed",
        None,
    )?;
    events.finished(
        "target",
        "inbound-unrelated",
        WorkTerminalKind::Failed,
        "delivery_failed",
        None,
    )?;

    // When
    let view = |entries: &std::collections::BTreeMap<
        String,
        super::web_watch_notify_store::WebNotifyEntry,
    >| {
        format!(
            "{:?}",
            entries
                .iter()
                .map(|(correlation, entry)| (
                    correlation,
                    &entry.intent,
                    &entry.snapshot,
                    entry.last_stage,
                    entry.last_failed_at_ms,
                    entry.first_fact_index,
                ))
                .collect::<Vec<_>>()
        )
    };
    let (full, full_decoded) =
        events.decoded_by(|| Ok(read_web_notifications_through(&events.path, None)?))?;
    let (filtered, filtered_decoded) =
        events.decoded_by(|| Ok(read_web_notifications(&events.path)?))?;

    // Then
    assert_eq!(full.len(), 2);
    assert!(!full.contains_key(&foreign.correlation_id));
    assert_eq!(
        full[&enqueued.correlation_id].snapshot.enqueue,
        WebNotifyEnqueue::Enqueued
    );
    assert_eq!(
        full[&failed.correlation_id].snapshot.delivery,
        WebNotifyDelivery::Failed
    );
    assert_eq!(view(&filtered), view(&full));
    assert!(filtered_decoded < full_decoded);
    Ok(())
}

#[test]
fn entrusted_notifications_read_through_their_filters_equal_a_full_read(
) -> Result<(), Box<dyn Error>> {
    // Given
    let mut events = Events::new()?;
    let mut intents = Vec::new();
    for (index, verdict) in [
        ObservationVerdict::NotifyCandidate,
        ObservationVerdict::Unknown,
        ObservationVerdict::NotifyCandidate,
    ]
    .into_iter()
    .enumerate()
    {
        let record = observation_record("work-1", 1_000 + index as u64, verdict);
        let facts = serde_json::to_value(&record)?;
        events.finished(
            "owner",
            &record.wake.durable_work_id(),
            WorkTerminalKind::Succeeded,
            ENTRUSTED_OBSERVATION_OUTCOME,
            Some(facts.clone()),
        )?;
        events.finished(
            "owner",
            &format!("copied-{index}"),
            WorkTerminalKind::Succeeded,
            ENTRUSTED_OBSERVATION_OUTCOME,
            Some(facts),
        )?;
        intents.extend(EntrustedNotificationIntent::from_record(&record));
        events.unrelated(ENTRUSTED_NOTIFICATION_PREFIX)?;
    }
    for intent in &intents {
        events.enqueued(
            "owner",
            &intent.correlation_id,
            "agent.inbound_turn",
            json!({"message": intent.decision_id}),
        )?;
        for session_id in ["owner", "intruder"] {
            events.fact(
                session_id,
                ENTRUSTED_NOTIFICATION_FACT,
                serde_json::to_value(EntrustedNotificationFact {
                    intent: intent.clone(),
                    stage: EntrustedNotificationStage::Consuming,
                    writer_instance_ref: "writer-1".to_owned(),
                })?,
            )?;
        }
    }

    // When
    let view = |entries: &std::collections::BTreeMap<
        String,
        super::entrusted_notification_store::NotificationEntry,
    >| {
        format!(
            "{:?}",
            entries
                .iter()
                .map(|(correlation, entry)| (
                    correlation,
                    &entry.observation,
                    &entry.snapshot,
                    entry.last_stage,
                ))
                .collect::<Vec<_>>()
        )
    };
    let (full, full_decoded) =
        events.decoded_by(|| Ok(read_notifications_through(&events.path, None)?))?;
    let (filtered, filtered_decoded) =
        events.decoded_by(|| Ok(read_notifications(&events.path)?))?;

    // Then
    assert_eq!(full.len(), 2);
    assert!(full.values().all(|entry| entry.snapshot.enqueue
        == EntrustedNotificationEnqueueState::Enqueued
        && entry.snapshot.consumption_started));
    assert_eq!(view(&filtered), view(&full));
    assert!(filtered_decoded < full_decoded);
    Ok(())
}

#[test]
fn an_observation_ledger_read_through_its_filters_equals_a_full_read() -> Result<(), Box<dyn Error>>
{
    // Given
    let mut events = Events::new()?;
    for (session_id, work_id, offset) in [
        ("owner", "work-1", 0),
        ("owner", "work-10", 100),
        ("intruder", "work-1", 200),
    ] {
        for index in 0..3 {
            let mut record = observation_record(
                work_id,
                1_000 + offset + index,
                ObservationVerdict::Suppress,
            );
            if index == 1 {
                record.decision = None;
            }
            events.finished(
                session_id,
                &record.wake.durable_work_id(),
                WorkTerminalKind::Succeeded,
                ENTRUSTED_OBSERVATION_OUTCOME,
                Some(serde_json::to_value(&record)?),
            )?;
        }
        let stopped = observation_record(work_id, 5_000 + offset, ObservationVerdict::Suppress);
        let stopped_work_id = stopped.wake.durable_work_id();
        events.enqueued(
            session_id,
            &stopped_work_id,
            ENTRUSTED_OBSERVATION_WORK_KIND,
            serde_json::to_value(&stopped.wake)?,
        )?;
        for kind in [WORK_CANCEL_REQUESTED, WORK_CANCELLED] {
            events.work(
                session_id,
                kind,
                &WorkCancellation {
                    work_id: stopped_work_id.clone(),
                    reason: "observation:GoalInactive".to_owned(),
                },
            )?;
        }
        events.unrelated("observation:work-1:")?;
    }
    let request = EntrustedWorkReadRequest {
        owner_session_id: "owner".to_owned(),
        requester_target: "owner".to_owned(),
        work_id: "work-1".to_owned(),
    };

    // When
    let view = |ledger: &ObservationLedger| {
        format!(
            "{:?}",
            (
                &ledger.state,
                &ledger.history,
                &ledger.baseline,
                &ledger.decisions
            )
        )
    };
    let (full, full_decoded) = events.decoded_by(|| {
        Ok(ObservationLedger::read_through(
            &events.path,
            &request,
            None,
        )?)
    })?;
    let (filtered, filtered_decoded) =
        events.decoded_by(|| Ok(ObservationLedger::read(&events.path, &request)?))?;

    // Then
    assert_eq!(full.history.len(), 3);
    assert_eq!(full.decisions.len(), 4);
    assert_eq!(view(&filtered), view(&full));
    assert!(filtered_decoded < full_decoded);
    Ok(())
}

#[test]
fn web_deadline_records_read_through_their_filters_equal_a_full_read() -> Result<(), Box<dyn Error>>
{
    // Given
    let mut events = Events::new()?;
    for revision in 1..=3 {
        for (session_id, work_id, shift) in [
            ("owner", "watch-1", 0),
            ("owner", "watch-2", 10),
            ("intruder", "watch-1", 20),
        ] {
            let record = deadline_record(work_id, revision + shift);
            events.finished(
                session_id,
                &record.wake.durable_work_id(),
                WorkTerminalKind::Succeeded,
                WEB_DEADLINE_OUTCOME,
                Some(serde_json::to_value(&record)?),
            )?;
        }
        events.finished(
            "owner",
            &format!("other-{revision}"),
            WorkTerminalKind::Succeeded,
            "other_outcome",
            None,
        )?;
        events.unrelated("web-deadline:watch-1:")?;
    }

    // When
    let (full, full_decoded) = events.decoded_by(|| {
        Ok(read_web_deadline_records_through(
            &events.path,
            "owner",
            "watch-1",
            None,
        )?)
    })?;
    let (filtered, filtered_decoded) =
        events.decoded_by(|| Ok(read_web_deadline_records(&events.path, "owner", "watch-1")?))?;

    // Then
    assert_eq!(full.len(), 3);
    assert_eq!(filtered, full);
    assert!(filtered_decoded < full_decoded);
    Ok(())
}
