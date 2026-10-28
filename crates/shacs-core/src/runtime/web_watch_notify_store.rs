use std::collections::BTreeMap;
use std::path::Path;

use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, WEB_WATCH_NOTIFICATION_FACT,
    WORK_CANCELLED, WORK_ENQUEUED, WORK_TERMINAL,
};
use shacs_session::durable_work::{WorkEnqueued, WorkTerminal, WorkTerminalKind};

use super::entrusted_notification_store::writer_instance_ref;
use super::{
    DurableDispatchError, ObservationBlock, ObservationError, OutboundMessage,
    WebNotificationSnapshot, WebNotifyDelivery as Delivery, WebNotifyEnqueue as Enqueue,
    WebNotifyFact, WebNotifyIntent, WebNotifyStage as Stage,
};

pub(super) struct WebNotifyEntry {
    pub intent: WebNotifyIntent,
    pub snapshot: WebNotificationSnapshot,
    pub last_stage: Option<Stage>,
    pub last_failed_at_ms: u64,
    pub first_fact_index: usize,
}

/// Every 042 notification by correlation: the first fact carries the intent, later facts and
/// the inbound enqueue of the target session move it on.
pub(super) fn read_web_notifications(
    root: &Path,
) -> Result<BTreeMap<String, WebNotifyEntry>, ObservationError> {
    let mut entries: BTreeMap<String, WebNotifyEntry> = BTreeMap::new();
    if !root.exists() {
        return Ok(entries);
    }
    let store = DurableEventStore::open(root).map_err(DurableDispatchError::from)?;
    let mut failure = None;
    let summary = store
        .visit_from_sequence(0, |event| {
            if failure.is_some() {
                return;
            }
            let DurableEventPayload::Inline { data, .. } = &event.payload else {
                return;
            };
            let result = (|| -> Result<(), serde_json::Error> {
                match event.kind.as_str() {
                    WEB_WATCH_NOTIFICATION_FACT => {
                        let fact: WebNotifyFact = serde_json::from_value(data.clone())?;
                        if event.session_id != fact.intent.owner_session_id {
                            return Ok(());
                        }
                        let first_fact_index = entries.len();
                        let entry = entries
                            .entry(fact.intent.correlation_id.clone())
                            .or_insert_with(|| WebNotifyEntry {
                                snapshot: snapshot(&fact.intent),
                                intent: fact.intent.clone(),
                                last_stage: None,
                                last_failed_at_ms: 0,
                                first_fact_index,
                            });
                        if entry.intent == fact.intent {
                            apply_fact(entry, &fact);
                        }
                    }
                    WORK_ENQUEUED => {
                        let work: WorkEnqueued = serde_json::from_value(data.clone())?;
                        if let Some(entry) = entries.get_mut(&work.work_id) {
                            if event.session_id == entry.intent.target
                                && work.work_kind == "agent.inbound_turn"
                                && work.dedupe_hint.as_deref() == Some(work.work_id.as_str())
                            {
                                entry.snapshot.enqueue = Enqueue::Enqueued;
                                entry.snapshot.delivery = Delivery::Pending;
                            }
                        }
                    }
                    WORK_TERMINAL | WORK_CANCELLED => {
                        let ended_unsent = event.kind == WORK_CANCELLED
                            || serde_json::from_value::<WorkTerminal>(data.clone())?.terminal_kind
                                != WorkTerminalKind::Succeeded;
                        let entry = data
                            .get("work_id")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|work_id| entries.get_mut(work_id));
                        if let Some(entry) = entry.filter(|entry| {
                            ended_unsent
                                && event.session_id == entry.intent.target
                                && !entry.snapshot.consumption_started
                                && !entry.snapshot.suppressed_stale
                        }) {
                            entry.snapshot.delivery = Delivery::Failed;
                        }
                    }
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = result {
                failure = Some(DurableDispatchError::from(error));
            }
        })
        .map_err(DurableDispatchError::from)?;
    if let Some(error) = failure {
        return Err(error.into());
    }
    if summary.incomplete_tail || !summary.compatibility.is_current() {
        return Err(ObservationError::Blocked(ObservationBlock::RecoveryNeeded));
    }
    Ok(entries)
}

fn snapshot(intent: &WebNotifyIntent) -> WebNotificationSnapshot {
    WebNotificationSnapshot {
        correlation_id: intent.correlation_id.clone(),
        kind: intent.kind(),
        event_id: intent.event_id.clone(),
        settings_revision: intent.settings_revision,
        target: intent.target.clone(),
        enqueue: Enqueue::Pending,
        enqueue_failures: 0,
        consumption_started: false,
        suppressed_stale: false,
        delivery: Delivery::NotRequested,
    }
}

fn apply_fact(entry: &mut WebNotifyEntry, fact: &WebNotifyFact) {
    entry.last_stage = Some(fact.stage);
    if fact.stage == Stage::EnqueueFailed {
        entry.last_failed_at_ms = fact.failed_at_ms.unwrap_or_default();
    }
    let snapshot = &mut entry.snapshot;
    match fact.stage {
        Stage::EnqueuePending | Stage::Enqueued => {}
        Stage::EnqueueFailed => {
            snapshot.enqueue_failures = snapshot.enqueue_failures.saturating_add(1);
            if snapshot.enqueue != Enqueue::Enqueued {
                snapshot.enqueue = Enqueue::Failed;
            }
        }
        Stage::SuppressedStale => {
            if snapshot.enqueue != Enqueue::Enqueued {
                snapshot.enqueue = Enqueue::Suppressed;
            }
            snapshot.suppressed_stale = true;
            snapshot.delivery = Delivery::NotRequested;
        }
        Stage::Consuming => {
            snapshot.consumption_started = true;
            snapshot.delivery = Delivery::Unknown;
        }
        Stage::OutboundPending => {
            snapshot.delivery = if fact.writer_instance_ref == writer_instance_ref() {
                Delivery::Pending
            } else {
                Delivery::Unknown
            };
        }
        Stage::DeliveryFailed => snapshot.delivery = Delivery::Failed,
        Stage::SentHint => snapshot.delivery = Delivery::SentHint,
    }
}

/// A transport fact for a consumed notification sent to its own target.
pub(super) fn record_web_notification_delivery(
    root: &Path,
    message: &OutboundMessage,
    correlation: &str,
    stage: Stage,
) -> Result<(), ObservationError> {
    let entries = read_web_notifications(root)?;
    let Some(entry) = entries.get(correlation) else {
        return Ok(());
    };
    if !entry.snapshot.consumption_started
        || entry.snapshot.suppressed_stale
        || entry.intent.target != format!("{}:{}", message.channel, message.chat_id)
    {
        return Ok(());
    }
    append_web_notification_fact(root, &entry.intent, stage)
}

pub(super) fn append_web_notification_fact(
    root: &Path,
    intent: &WebNotifyIntent,
    stage: Stage,
) -> Result<(), ObservationError> {
    append_fact(root, intent, stage, None)
}

pub(super) fn append_web_enqueue_failure(
    root: &Path,
    intent: &WebNotifyIntent,
    failed_at_ms: u64,
) -> Result<(), ObservationError> {
    append_fact(root, intent, Stage::EnqueueFailed, Some(failed_at_ms))
}

fn append_fact(
    root: &Path,
    intent: &WebNotifyIntent,
    stage: Stage,
    failed_at_ms: Option<u64>,
) -> Result<(), ObservationError> {
    let fact = WebNotifyFact {
        intent: intent.clone(),
        stage,
        writer_instance_ref: writer_instance_ref().to_owned(),
        failed_at_ms,
    };
    let mut event = DurableEventInput::new(
        &intent.owner_session_id,
        WEB_WATCH_NOTIFICATION_FACT,
        DurableEventPayload::inline(
            "web_watch_notification.v1",
            serde_json::to_value(fact).map_err(DurableDispatchError::from)?,
        ),
    );
    event.correlation_id = Some(intent.correlation_id.clone());
    event.causation_id = Some(intent.event_id.clone());
    DurableEventStore::open(root)
        .map_err(DurableDispatchError::from)?
        .append(event)
        .map_err(DurableDispatchError::from)?;
    Ok(())
}
