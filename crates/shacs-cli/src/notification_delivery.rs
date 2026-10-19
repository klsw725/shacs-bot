use std::path::Path;

use shacs_channels::OutboundMessage;
use shacs_core::runtime::{
    record_entrusted_notification_delivery, EntrustedNotificationTransportFact,
};

pub(super) fn record(
    data_dir: Option<&Path>,
    message: &OutboundMessage,
    fact: EntrustedNotificationTransportFact,
) {
    let Some(data_dir) = data_dir else { return };
    if let Err(error) = record_entrusted_notification_delivery(
        &data_dir.join("runtime/durable-events"),
        message,
        fact,
    ) {
        eprintln!("entrusted notification delivery accounting failed: {error}");
    }
}
