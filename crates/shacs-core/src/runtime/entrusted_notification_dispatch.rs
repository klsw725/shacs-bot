use shacs_session::SessionMutationGuard;

use super::entrusted_notification_store::{append_notification_fact, read_notifications};
use super::entrusted_observation_surface::check_wake;
use super::entrusted_work_surface::load_owner_session;
use super::{
    DurableWorkDispatcher, EntrustedNotificationEnqueueState as Enqueue,
    EntrustedNotificationStage as Stage, InboundMessage, ObservationError, ObservationRuntime,
    ENTRUSTED_NOTIFICATION_CORRELATION,
};

impl DurableWorkDispatcher {
    pub fn enqueue_entrusted_notifications(
        &mut self,
        runtime: &ObservationRuntime<'_>,
    ) -> Result<(), ObservationError> {
        let root = runtime.data_dir.join("runtime/durable-events");
        for (correlation, candidate) in
            read_notifications(&root)?.into_iter().filter(|(_, entry)| {
                matches!(entry.snapshot.enqueue, Enqueue::Pending | Enqueue::Failed)
            })
        {
            let owner = &candidate.snapshot.intent.owner_session_id;
            let _guard = SessionMutationGuard::acquire(runtime.workspace, owner)?;
            let entries = read_notifications(&root)?;
            let Some(entry) = entries.get(&correlation) else {
                continue;
            };
            if matches!(
                entry.snapshot.enqueue,
                Enqueue::Enqueued | Enqueue::Suppressed | Enqueue::UnsupportedTarget
            ) {
                continue;
            }
            let intent = &entry.snapshot.intent;
            let current = load_owner_session(runtime.workspace, owner);
            if !current.is_ok_and(|(_, session)| {
                session.key == *owner && check_wake(&session, &entry.observation.wake).is_ok()
            }) {
                append_notification_fact(&root, intent, Stage::SuppressedStale)?;
                continue;
            }
            let Some((channel, chat_id)) = owner
                .split_once(':')
                .filter(|(channel, chat)| *channel == "email" && !chat.is_empty())
            else {
                append_notification_fact(&root, intent, Stage::UnsupportedTarget)?;
                continue;
            };
            if entry.last_stage != Some(Stage::EnqueuePending) {
                append_notification_fact(&root, intent, Stage::EnqueuePending)?;
            }
            let mut message = InboundMessage::new(channel, "entrusted-notification", chat_id, "");
            message.timestamp = format!("unix-ms:{}", entry.observation.observed_at_ms);
            message.metadata.insert(
                ENTRUSTED_NOTIFICATION_CORRELATION.to_owned(),
                serde_json::json!(correlation),
            );
            match self.enqueue_inbound(&correlation, &message, Some(correlation.clone()), None) {
                Ok(_) => append_notification_fact(&root, intent, Stage::Enqueued)?,
                Err(error) => {
                    append_notification_fact(&root, intent, Stage::EnqueueFailed)?;
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }
}
