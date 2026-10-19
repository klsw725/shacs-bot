use std::{collections::BTreeMap, path::Path, sync::OnceLock};

use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, ENTRUSTED_NOTIFICATION_FACT,
    WORK_ENQUEUED, WORK_TERMINAL,
};
use shacs_session::durable_work::{WorkEnqueued, WorkTerminal, WorkTerminalKind};

use super::{
    DurableDispatchError, EntrustedNotificationDeliveryState as Delivery,
    EntrustedNotificationEnqueueState as Enqueue, EntrustedNotificationFact,
    EntrustedNotificationIntent, EntrustedNotificationSnapshot,
    EntrustedNotificationStage as Stage, ObservationBlock, ObservationError, ObservationRecord,
    ENTRUSTED_OBSERVATION_OUTCOME,
};

pub(super) struct NotificationEntry {
    pub observation: ObservationRecord,
    pub snapshot: EntrustedNotificationSnapshot,
    pub last_stage: Option<Stage>,
}

pub(super) fn read_notifications(
    root: &Path,
) -> Result<BTreeMap<String, NotificationEntry>, ObservationError> {
    let mut entries: BTreeMap<String, NotificationEntry> = BTreeMap::new();
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
                    WORK_TERMINAL => {
                        let terminal: WorkTerminal = serde_json::from_value(data.clone())?;
                        if terminal.outcome_ref != ENTRUSTED_OBSERVATION_OUTCOME
                            || terminal.terminal_kind != WorkTerminalKind::Succeeded
                        {
                            return Ok(());
                        }
                        let Some(facts) = terminal.facts else {
                            return Ok(());
                        };
                        let observation: ObservationRecord = serde_json::from_value(facts)?;
                        if observation.wake.owner_session_id != event.session_id
                            || observation.wake.durable_work_id() != terminal.work_id
                        {
                            return Ok(());
                        }
                        if let Some(intent) = EntrustedNotificationIntent::from_record(&observation)
                        {
                            entries.entry(intent.correlation_id.clone()).or_insert(
                                NotificationEntry {
                                    observation,
                                    snapshot: EntrustedNotificationSnapshot {
                                        intent,
                                        enqueue: Enqueue::Pending,
                                        consumption_started: false,
                                        suppressed_stale: false,
                                        delivery: Delivery::NotRequested,
                                    },
                                    last_stage: None,
                                },
                            );
                        }
                    }
                    WORK_ENQUEUED => {
                        let work: WorkEnqueued = serde_json::from_value(data.clone())?;
                        if let Some(entry) = entries.get_mut(&work.work_id) {
                            if event.session_id == entry.snapshot.intent.owner_session_id
                                && work.work_kind == "agent.inbound_turn"
                                && work.dedupe_hint.as_deref() == Some(work.work_id.as_str())
                            {
                                entry.snapshot.enqueue = Enqueue::Enqueued;
                                entry.snapshot.delivery = Delivery::Pending;
                            }
                        }
                    }
                    ENTRUSTED_NOTIFICATION_FACT => {
                        let fact: EntrustedNotificationFact = serde_json::from_value(data.clone())?;
                        if let Some(entry) = entries.get_mut(&fact.intent.correlation_id) {
                            if fact.intent == entry.snapshot.intent
                                && event.session_id == fact.intent.owner_session_id
                            {
                                apply_fact(entry, &fact);
                            }
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

fn apply_fact(entry: &mut NotificationEntry, fact: &EntrustedNotificationFact) {
    let snapshot = &mut entry.snapshot;
    entry.last_stage = Some(fact.stage);
    match fact.stage {
        Stage::EnqueuePending => {}
        Stage::EnqueueFailed => {
            if snapshot.enqueue != Enqueue::Enqueued {
                snapshot.enqueue = Enqueue::Failed;
            }
        }
        Stage::Enqueued => {}
        Stage::UnsupportedTarget => snapshot.enqueue = Enqueue::UnsupportedTarget,
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

pub(super) fn append_notification_fact(
    root: &Path,
    intent: &EntrustedNotificationIntent,
    stage: Stage,
) -> Result<(), ObservationError> {
    let fact = EntrustedNotificationFact {
        intent: intent.clone(),
        stage,
        writer_instance_ref: writer_instance_ref().to_owned(),
    };
    let mut event = DurableEventInput::new(
        &intent.owner_session_id,
        ENTRUSTED_NOTIFICATION_FACT,
        DurableEventPayload::inline(
            "entrusted_notification.v1",
            serde_json::to_value(fact).map_err(DurableDispatchError::from)?,
        ),
    );
    event.correlation_id = Some(intent.correlation_id.clone());
    event.causation_id = Some(intent.decision_id.clone());
    DurableEventStore::open(root)
        .map_err(DurableDispatchError::from)?
        .append(event)
        .map_err(DurableDispatchError::from)?;
    Ok(())
}

fn writer_instance_ref() -> &'static str {
    static INSTANCE: OnceLock<String> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        use sha2::{Digest, Sha256};
        format!(
            "sha256:{:x}",
            Sha256::digest(
                format!("{}:{:?}", std::process::id(), std::time::SystemTime::now()).as_bytes()
            )
        )
    })
}
