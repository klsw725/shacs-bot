use shacs_bus::{
    AccountingFreshness, BusMeasurement, InboundMessage, MessageBus, MessageBusError,
    OutboundMessage, QueueAccountingSnapshot,
};

fn inbound(content: &str) -> InboundMessage {
    InboundMessage::new("telegram", "sender", "chat", content)
}

fn outbound(content: &str) -> OutboundMessage {
    OutboundMessage::new("telegram", "chat", content)
}

#[test]
fn accounting_reports_normal_accept_emit_depth_and_capacity() {
    let bus = MessageBus::bounded(4);

    bus.publish_inbound(inbound("inbound"));
    bus.publish_outbound(outbound("outbound"));
    assert_eq!(
        bus.consume_inbound().map(|message| message.content),
        Some("inbound".to_owned())
    );

    let snapshot = bus.accounting_snapshot();

    assert_eq!(snapshot.inbound.depth, BusMeasurement::Available(0));
    assert_eq!(snapshot.inbound.capacity, BusMeasurement::Available(4));
    assert_eq!(snapshot.inbound.accepted, BusMeasurement::Available(1));
    assert_eq!(snapshot.inbound.emitted, BusMeasurement::Available(1));
    assert_eq!(snapshot.inbound.dropped, BusMeasurement::Available(0));
    assert_eq!(snapshot.outbound.depth, BusMeasurement::Available(1));
    assert_eq!(snapshot.outbound.accepted, BusMeasurement::Available(1));
    assert_eq!(snapshot.outbound.emitted, BusMeasurement::Available(0));
}

#[test]
fn accounting_reports_full_inbound_without_fabricated_drop() {
    let bus = MessageBus::bounded(1);

    assert_eq!(bus.try_publish_inbound(inbound("first")), Ok(()));
    assert_eq!(
        bus.try_publish_inbound(inbound("second")),
        Err(MessageBusError::QueueFull { capacity: 1 })
    );

    let snapshot = bus.accounting_snapshot();

    assert_eq!(snapshot.inbound.depth, BusMeasurement::Available(1));
    assert_eq!(snapshot.inbound.capacity, BusMeasurement::Available(1));
    assert_eq!(snapshot.inbound.accepted, BusMeasurement::Available(1));
    assert_eq!(snapshot.inbound.emitted, BusMeasurement::Available(0));
    assert_eq!(snapshot.inbound.dropped, BusMeasurement::Available(0));
}

#[test]
fn accounting_reports_capacity_one_lossy_outbound_drop_at_actual_drop_path() {
    let bus = MessageBus::bounded(1);

    bus.publish_outbound(outbound("progress"));
    bus.publish_outbound(outbound("final"));

    let snapshot = bus.accounting_snapshot();
    assert_eq!(snapshot.outbound.depth, BusMeasurement::Available(1));
    assert_eq!(snapshot.outbound.accepted, BusMeasurement::Available(2));
    assert_eq!(snapshot.outbound.dropped, BusMeasurement::Available(1));
    assert_eq!(
        bus.consume_outbound().map(|message| message.content),
        Some("final".to_owned())
    );
    assert_eq!(
        bus.accounting_snapshot().outbound.emitted,
        BusMeasurement::Available(1)
    );
}

#[test]
fn accounting_reports_coalesced_progress_without_counting_it_as_dropped() {
    let bus = MessageBus::bounded(1);

    bus.record_outbound_coalesced(2);

    let snapshot = bus.accounting_snapshot();
    assert_eq!(snapshot.outbound.coalesced, BusMeasurement::Available(2));
    assert_eq!(snapshot.outbound.dropped, BusMeasurement::Available(0));
    assert_eq!(snapshot.outbound.accepted, BusMeasurement::Available(0));
}

#[test]
fn accounting_unavailable_counter_is_typed_unavailable_not_zero() {
    let snapshot = QueueAccountingSnapshot::unavailable();

    assert_eq!(snapshot.depth, BusMeasurement::Unavailable);
    assert_eq!(snapshot.capacity, BusMeasurement::Unavailable);
    assert_eq!(snapshot.accepted, BusMeasurement::Unavailable);
    assert_eq!(snapshot.emitted, BusMeasurement::Unavailable);
    assert_eq!(snapshot.coalesced, BusMeasurement::Unavailable);
    assert_eq!(snapshot.dropped, BusMeasurement::Unavailable);
    assert_eq!(snapshot.freshness, AccountingFreshness::Unavailable);
}

#[test]
fn accounting_counter_overflow_saturates_instead_of_wrapping() {
    let bus = MessageBus::bounded(1);

    bus.record_outbound_coalesced(u64::MAX);
    bus.record_outbound_coalesced(1);

    assert_eq!(
        bus.accounting_snapshot().outbound.coalesced,
        BusMeasurement::Available(u64::MAX)
    );
}

#[test]
fn accounting_snapshot_and_reset_returns_previous_counts_then_restarts_available_counts() {
    let bus = MessageBus::bounded(2);

    bus.publish_inbound(inbound("first"));
    let first = bus.accounting_snapshot_and_reset();
    let second = bus.accounting_snapshot_and_reset();
    bus.publish_inbound(inbound("second"));

    assert_eq!(first.inbound.depth, BusMeasurement::Available(1));
    assert_eq!(first.inbound.accepted, BusMeasurement::Available(1));
    assert_eq!(second.inbound.depth, BusMeasurement::Available(1));
    assert_eq!(second.inbound.accepted, BusMeasurement::Available(0));
    assert_eq!(second.inbound.emitted, BusMeasurement::Available(0));
    assert_eq!(
        bus.accounting_snapshot().inbound.accepted,
        BusMeasurement::Available(1)
    );
}
