use shacs_bus::{InboundMessage, MessageBus, OutboundMessage};

fn main() {
    let bus = MessageBus::bounded(1);

    bus.publish_inbound(InboundMessage::new("telegram", "sender", "chat", "first"));
    let inbound_before = bus.accounting_snapshot();
    let _inbound = bus.consume_inbound();
    let inbound_after = bus.accounting_snapshot_and_reset();

    bus.publish_outbound(OutboundMessage::new("telegram", "chat", "progress"));
    bus.publish_outbound(OutboundMessage::new("telegram", "chat", "final"));
    bus.record_outbound_coalesced(1);
    let outbound_before = bus.accounting_snapshot();
    let _outbound = bus.consume_outbound();
    let outbound_after = bus.accounting_snapshot_and_reset();
    let after_reset = bus.accounting_snapshot();

    println!("inbound_before={inbound_before:#?}");
    println!("inbound_after={inbound_after:#?}");
    println!("outbound_before={outbound_before:#?}");
    println!("outbound_after={outbound_after:#?}");
    println!("after_reset={after_reset:#?}");
}
