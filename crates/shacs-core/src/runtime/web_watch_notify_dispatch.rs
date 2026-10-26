use std::path::Path;

use shacs_session::{Session, SessionManager, SessionMutationGuard};

use super::entrusted_work_surface::load_owner_session;
use super::web_watch_gate::check_web_wake;
use super::web_watch_notify_events::derive_intents;
use super::web_watch_notify_store::{
    append_web_enqueue_failure, append_web_notification_fact, read_web_notifications,
    WebNotifyEntry,
};
use super::web_watch_store::{web_notify_settings, web_watches, WebWatchLedger};
use super::{
    DurableWorkDispatcher, EntrustedWorkError, InboundMessage, ObservationError,
    WebNotifyEnqueue as Enqueue, WebNotifyIntent, WebNotifyStage as Stage, WebWatchRuntime,
    ENTRUSTED_NOTIFICATION_CORRELATION, WEB_NOTIFY_ENQUEUE_ATTEMPTS, WEB_NOTIFY_ENQUEUE_RETRY_MS,
};

impl DurableWorkDispatcher {
    /// Enqueues each notification the recorded checks call for once, to its `email:` target,
    /// under the owner session lock; one whose lineage or settings changed is suppressed, and
    /// so is a queued one not yet being sent before new notifications are derived.
    /// A failed enqueue is recorded without stopping the worker or the other notifications,
    /// and retried from its stored intent a minute later, `WEB_NOTIFY_ENQUEUE_ATTEMPTS` times
    /// in all, in the order the notifications were first recorded.
    pub fn enqueue_web_watch_notifications(
        &mut self,
        runtime: &WebWatchRuntime<'_>,
    ) -> Result<(), ObservationError> {
        let Some(manager) = SessionManager::open_existing(runtime.workspace)? else {
            return Ok(());
        };
        let root = runtime.data_dir.join("runtime/durable-events");
        for owner in manager.list_sessions()? {
            let _guard = SessionMutationGuard::acquire(runtime.workspace, &owner.key)?;
            let (_, session) = load_owner_session(runtime.workspace, &owner.key)?;
            let mut facts = read_web_notifications(&root)?;
            let mut retries: Vec<&WebNotifyEntry> = facts
                .values()
                .filter(|fact| {
                    fact.intent.owner_session_id == owner.key && retry_due(fact, runtime.now_ms)
                })
                .collect();
            retries.sort_by_key(|fact| fact.first_fact_index);
            let mut changed = !retries.is_empty();
            for fact in retries {
                if current(&session, &fact.intent)? {
                    let pending = fact.last_stage == Some(Stage::EnqueuePending);
                    self.enqueue_web_notification(&root, &fact.intent, pending, runtime.now_ms)?;
                } else {
                    append_web_notification_fact(&root, &fact.intent, Stage::SuppressedStale)?;
                }
            }
            for fact in facts.values().filter(|fact| {
                fact.intent.owner_session_id == owner.key
                    && fact.snapshot.enqueue == Enqueue::Enqueued
                    && !fact.snapshot.consumption_started
                    && !fact.snapshot.suppressed_stale
            }) {
                if !current(&session, &fact.intent)? {
                    append_web_notification_fact(&root, &fact.intent, Stage::SuppressedStale)?;
                    changed = true;
                }
            }
            if changed {
                facts = read_web_notifications(&root)?;
            }
            for watch in web_watches(&session).map_err(EntrustedWorkError::Metadata)? {
                let Some(settings) = web_notify_settings(&session, &watch.work_id)
                    .map_err(EntrustedWorkError::Metadata)?
                else {
                    continue;
                };
                let ledger = WebWatchLedger::read(&root, &owner.key, &watch.work_id)?;
                for intent in derive_intents(&ledger, &watch, &settings, &facts) {
                    if facts.contains_key(&intent.correlation_id) {
                        continue;
                    }
                    if !current(&session, &intent)? {
                        append_web_notification_fact(&root, &intent, Stage::SuppressedStale)?;
                        continue;
                    }
                    self.enqueue_web_notification(&root, &intent, false, runtime.now_ms)?;
                }
            }
        }
        Ok(())
    }

    fn enqueue_web_notification(
        &mut self,
        root: &Path,
        intent: &WebNotifyIntent,
        already_pending: bool,
        now_ms: u64,
    ) -> Result<(), ObservationError> {
        let Some(address) = intent.target.strip_prefix("email:") else {
            return append_web_notification_fact(root, intent, Stage::SuppressedStale);
        };
        if !already_pending {
            append_web_notification_fact(root, intent, Stage::EnqueuePending)?;
        }
        let mut message = InboundMessage::new("email", "web-watch-notification", address, "");
        message.timestamp = format!("unix-ms:{}", intent.wake.due_at_ms);
        message.metadata.insert(
            ENTRUSTED_NOTIFICATION_CORRELATION.to_owned(),
            serde_json::json!(intent.correlation_id),
        );
        match self.enqueue_inbound(
            &intent.correlation_id,
            &message,
            Some(intent.correlation_id.clone()),
            None,
        ) {
            Ok(_) => append_web_notification_fact(root, intent, Stage::Enqueued),
            Err(_) => append_web_enqueue_failure(root, intent, now_ms),
        }
    }
}

/// A recorded notification that is not queued yet: at once after an interrupted attempt, and
/// after a failed one when the retry delay has passed and attempts remain.
fn retry_due(entry: &WebNotifyEntry, now_ms: u64) -> bool {
    match entry.snapshot.enqueue {
        Enqueue::Pending => true,
        Enqueue::Failed => {
            let retry_at_ms = entry
                .last_failed_at_ms
                .saturating_add(WEB_NOTIFY_ENQUEUE_RETRY_MS);
            entry.snapshot.enqueue_failures < WEB_NOTIFY_ENQUEUE_ATTEMPTS && now_ms >= retry_at_ms
        }
        Enqueue::Enqueued | Enqueue::Suppressed => false,
    }
}

/// Whether `intent` still matches the owner's current lineage, plan and notification settings.
pub(super) fn current(
    session: &Session,
    intent: &WebNotifyIntent,
) -> Result<bool, ObservationError> {
    let settings = web_notify_settings(session, &intent.entrusted_work_id)
        .map_err(EntrustedWorkError::Metadata)?;
    Ok(check_web_wake(session, &intent.wake).is_ok()
        && settings.is_some_and(|settings| {
            settings.revision == intent.settings_revision
                && settings.target.as_deref() == Some(intent.target.as_str())
        }))
}
