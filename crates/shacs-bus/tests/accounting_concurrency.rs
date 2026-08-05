use shacs_bus::{BusMeasurement, InboundMessage, MessageBus, OutboundMessage};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::thread;
use std::time::Duration;

fn inbound(content: &str) -> InboundMessage {
    InboundMessage::new("telegram", "sender", "chat", content)
}

fn outbound(content: &str) -> OutboundMessage {
    OutboundMessage::new("telegram", "chat", content)
}

fn available_u64(value: BusMeasurement<u64>) -> u64 {
    match value {
        BusMeasurement::Available(value) => value,
        BusMeasurement::Unavailable => panic!("counter should be available"),
    }
}

fn available_usize(value: BusMeasurement<usize>) -> usize {
    match value {
        BusMeasurement::Available(value) => value,
        BusMeasurement::Unavailable => panic!("measurement should be available"),
    }
}

fn recv_watchdog<T>(receiver: &Receiver<T>) -> T {
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("test thread should acknowledge before watchdog expires")
}

fn open_gate(pair: &(Mutex<bool>, Condvar)) {
    let (lock, condvar) = pair;
    let mut open = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *open = true;
    condvar.notify_all();
}

fn wait_gate(pair: &(Mutex<bool>, Condvar)) {
    let (lock, condvar) = pair;
    let mut open = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while !*open {
        open = condvar
            .wait(open)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

#[test]
fn accounting_concurrent_producer_consumer_conserves_counts_for_interval() {
    let bus = Arc::new(MessageBus::bounded(64));
    let start = Arc::new(Barrier::new(3));
    let (producer_done_tx, producer_done_rx) = mpsc::channel();
    let (consumer_done_tx, consumer_done_rx) = mpsc::channel();

    let producer_bus = Arc::clone(&bus);
    let producer_start = Arc::clone(&start);
    let producer = thread::spawn(move || {
        producer_start.wait();
        for index in 0..32 {
            producer_bus.publish_inbound(inbound(&format!("inbound-{index}")));
        }
        producer_done_tx.send(()).expect("producer ack");
    });

    let consumer_bus = Arc::clone(&bus);
    let consumer_start = Arc::clone(&start);
    let consumer = thread::spawn(move || {
        consumer_start.wait();
        for _ in 0..32 {
            let _message = consumer_bus.consume_inbound_blocking();
        }
        consumer_done_tx.send(()).expect("consumer ack");
    });

    start.wait();
    recv_watchdog(&producer_done_rx);
    recv_watchdog(&consumer_done_rx);
    producer.join().expect("producer join");
    consumer.join().expect("consumer join");

    let snapshot = bus.accounting_snapshot().inbound;
    let accepted = available_u64(snapshot.accepted);
    let emitted = available_u64(snapshot.emitted);
    let dropped = available_u64(snapshot.dropped);
    let depth = available_usize(snapshot.depth) as u64;
    assert_eq!(accepted, emitted + dropped + depth);
    assert_eq!((accepted, emitted, dropped, depth), (32, 32, 0, 0));
}

#[test]
fn accounting_lossy_capacity_one_thread_keeps_newest_and_counts_one_drop() {
    let bus = Arc::new(MessageBus::bounded(1));
    bus.publish_outbound(outbound("progress"));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();

    let publisher_bus = Arc::clone(&bus);
    let publisher_gate = Arc::clone(&gate);
    let publisher = thread::spawn(move || {
        ready_tx.send(()).expect("ready ack");
        wait_gate(&publisher_gate);
        publisher_bus.publish_outbound(outbound("final"));
        done_tx.send(()).expect("done ack");
    });

    recv_watchdog(&ready_rx);
    open_gate(&gate);
    recv_watchdog(&done_rx);
    publisher.join().expect("publisher join");

    let snapshot = bus.accounting_snapshot().outbound;
    assert_eq!(snapshot.accepted, BusMeasurement::Available(2));
    assert_eq!(snapshot.dropped, BusMeasurement::Available(1));
    assert_eq!(
        bus.consume_outbound().map(|message| message.content),
        Some("final".to_owned())
    );
}

#[test]
fn accounting_snapshot_is_coherent_while_publish_is_phase_blocked() {
    let bus = Arc::new(MessageBus::bounded(2));
    bus.publish_inbound(inbound("first"));
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();

    let producer_bus = Arc::clone(&bus);
    let producer_gate = Arc::clone(&gate);
    let producer = thread::spawn(move || {
        ready_tx.send(()).expect("ready ack");
        wait_gate(&producer_gate);
        producer_bus.publish_inbound(inbound("second"));
        done_tx.send(()).expect("done ack");
    });

    recv_watchdog(&ready_rx);
    let before = bus.accounting_snapshot().inbound;
    assert_eq!(before.accepted, BusMeasurement::Available(1));
    assert_eq!(before.depth, BusMeasurement::Available(1));

    open_gate(&gate);
    recv_watchdog(&done_rx);
    producer.join().expect("producer join");
    let after = bus.accounting_snapshot().inbound;
    assert_eq!(after.accepted, BusMeasurement::Available(2));
    assert_eq!(after.depth, BusMeasurement::Available(2));
}

#[test]
fn accounting_snapshot_and_reset_boundary_excludes_phase_blocked_publish() {
    let bus = Arc::new(MessageBus::bounded(4));
    bus.publish_inbound(inbound("before-reset"));
    let producer_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let reset_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let (producer_ready_tx, producer_ready_rx) = mpsc::channel();
    let (producer_done_tx, producer_done_rx) = mpsc::channel();
    let (reset_done_tx, reset_done_rx) = mpsc::channel();

    let producer_bus = Arc::clone(&bus);
    let producer_gate_clone = Arc::clone(&producer_gate);
    let producer = thread::spawn(move || {
        producer_ready_tx.send(()).expect("producer ready ack");
        wait_gate(&producer_gate_clone);
        producer_bus.publish_inbound(inbound("after-reset"));
        producer_done_tx.send(()).expect("producer done ack");
    });

    let reset_bus = Arc::clone(&bus);
    let reset_gate_clone = Arc::clone(&reset_gate);
    let reset = thread::spawn(move || {
        wait_gate(&reset_gate_clone);
        reset_done_tx
            .send(reset_bus.accounting_snapshot_and_reset())
            .expect("reset done ack");
    });

    recv_watchdog(&producer_ready_rx);
    open_gate(&reset_gate);
    let reset_snapshot = recv_watchdog(&reset_done_rx).inbound;
    assert_eq!(reset_snapshot.accepted, BusMeasurement::Available(1));
    assert_eq!(reset_snapshot.depth, BusMeasurement::Available(1));

    open_gate(&producer_gate);
    recv_watchdog(&producer_done_rx);
    producer.join().expect("producer join");
    reset.join().expect("reset join");

    let after = bus.accounting_snapshot().inbound;
    assert_eq!(after.accepted, BusMeasurement::Available(1));
    assert_eq!(after.depth, BusMeasurement::Available(2));
}

#[test]
fn accounting_repeated_reset_reports_each_lossy_interval_once() {
    let bus = Arc::new(MessageBus::bounded(1));
    bus.publish_outbound(outbound("one"));
    bus.publish_outbound(outbound("two"));

    let first = bus.accounting_snapshot_and_reset().outbound;
    let second = bus.accounting_snapshot_and_reset().outbound;
    bus.publish_outbound(outbound("three"));
    bus.publish_outbound(outbound("four"));
    let third = bus.accounting_snapshot_and_reset().outbound;

    assert_eq!(first.accepted, BusMeasurement::Available(2));
    assert_eq!(first.dropped, BusMeasurement::Available(1));
    assert_eq!(second.accepted, BusMeasurement::Available(0));
    assert_eq!(second.dropped, BusMeasurement::Available(0));
    assert_eq!(third.accepted, BusMeasurement::Available(2));
    assert_eq!(third.dropped, BusMeasurement::Available(2));
    assert_eq!(third.depth, BusMeasurement::Available(1));
}
