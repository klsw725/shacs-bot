use std::path::Path;

use super::entrusted_notification_store::{append_notification_fact, read_notifications};
use super::{
    EntrustedNotificationStage, ObservationError, OutboundMessage,
    ENTRUSTED_NOTIFICATION_CORRELATION,
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
