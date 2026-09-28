use shacs_channels::{ChannelAdapter, ChannelError, ChannelManager, OutboundMessage};
use std::error::Error;

struct DeliveredAdapter;

impl ChannelAdapter for DeliveredAdapter {
    fn name(&self) -> &str {
        "discord"
    }

    fn send(&self, _message: OutboundMessage) -> Result<(), ChannelError> {
        Ok(())
    }
}

#[test]
fn successful_production_dispatch_records_shared_revised_projection() -> Result<(), Box<dyn Error>>
{
    // Given: an enabled production channel adapter.
    let mut manager = ChannelManager::new();
    manager.register_adapter(Box::new(DeliveredAdapter), true)?;

    // When: the normal outbound dispatch path completes delivery.
    manager.dispatch_outbound(OutboundMessage::new("discord", "chat-a", "done"))?;

    // Then: that path exposes the shared owner-faithful projection.
    let projection = manager
        .latest_spec035_revised_projection()
        .ok_or("missing production Spec035 projection")?;
    let value = serde_json::to_value(projection)?;
    assert_eq!(
        value["delivery"]["final_delivery"]["owner_surface"],
        "channel"
    );
    assert_eq!(
        value["delivery"]["final_delivery"]["state"],
        "final_delivered"
    );
    Ok(())
}
