use std::path::Path;

use super::entrusted_notification_store::{append_notification_fact, read_notifications};
use super::web_watch_notify_store::record_web_notification_delivery;
use super::{
    EntrustedNotificationStage, ObservationError, OutboundMessage, WebNotifyStage,
    ENTRUSTED_NOTIFICATION_CORRELATION, WEB_NOTIFY_PREFIX,
};

#[derive(Debug, Clone, Copy)]
pub enum EntrustedNotificationTransportFact {
    Pending,
    Failed,
    SentHint,
}

pub fn record_entrusted_notification_delivery(
    event_root: &Path,
    message: &OutboundMessage,
    fact: EntrustedNotificationTransportFact,
) -> Result<(), ObservationError> {
    let Some(correlation) = message
        .metadata
        .get(ENTRUSTED_NOTIFICATION_CORRELATION)
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    if correlation.starts_with(WEB_NOTIFY_PREFIX) {
        let stage = match fact {
            EntrustedNotificationTransportFact::Pending => WebNotifyStage::OutboundPending,
            EntrustedNotificationTransportFact::Failed => WebNotifyStage::DeliveryFailed,
            EntrustedNotificationTransportFact::SentHint => WebNotifyStage::SentHint,
        };
        return record_web_notification_delivery(event_root, message, correlation, stage);
    }
    let entries = read_notifications(event_root)?;
    let Some(entry) = entries.get(correlation) else {
        return Ok(());
    };
    if !entry.snapshot.consumption_started
        || entry.snapshot.suppressed_stale
        || entry.snapshot.intent.owner_session_id
            != format!("{}:{}", message.channel, message.chat_id)
    {
        return Ok(());
    }
    append_notification_fact(
        event_root,
        &entry.snapshot.intent,
        match fact {
            EntrustedNotificationTransportFact::Pending => {
                EntrustedNotificationStage::OutboundPending
            }
            EntrustedNotificationTransportFact::Failed => {
                EntrustedNotificationStage::DeliveryFailed
            }
            EntrustedNotificationTransportFact::SentHint => EntrustedNotificationStage::SentHint,
        },
    )
}
