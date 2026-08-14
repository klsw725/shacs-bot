pub use shacs_channels::{InboundMessage, OutboundMessage, OwnerAcceptedAutomationResult};

mod accounting;

use accounting::QueueAccountingCounters;
pub use accounting::{
    AccountingFreshness, BusMeasurement, MessageBusAccountingSnapshot, QueueAccountingSnapshot,
};

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[cfg(test)]
type PushWaitHook = Arc<(Mutex<bool>, Condvar)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageBusError {
    QueueFull { capacity: usize },
}

impl fmt::Display for MessageBusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull { capacity } => write!(formatter, "message queue is full: {capacity}"),
        }
    }
}

impl std::error::Error for MessageBusError {}

#[derive(Default, Clone)]
pub struct MessageBus {
    inbound: Arc<QueueState<InboundMessage>>,
    outbound: Arc<QueueState<OutboundMessage>>,
}

struct QueueState<T> {
    // Lock order is queue -> accounting. Queue mutations update their counters
    // while the queue guard is still held, so snapshots cannot observe half of
    // an accepted/emitted/dropped transition.
    queue: Mutex<VecDeque<T>>,
    accounting: Mutex<QueueAccountingCounters>,
    available: Condvar,
    capacity: Option<usize>,
    #[cfg(test)]
    push_wait_hook: Mutex<Option<PushWaitHook>>,
}

impl<T> Default for QueueState<T> {
    fn default() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            accounting: Mutex::new(QueueAccountingCounters::default()),
            available: Condvar::new(),
            capacity: None,
            #[cfg(test)]
            push_wait_hook: Mutex::new(None),
        }
    }
}

impl MessageBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bounded(capacity: usize) -> Self {
        Self {
            inbound: Arc::new(QueueState::bounded(capacity)),
            outbound: Arc::new(QueueState::bounded(capacity)),
        }
    }

    pub fn publish_inbound(&self, message: InboundMessage) {
        self.inbound.push(message);
    }

    pub fn try_publish_inbound(&self, message: InboundMessage) -> Result<(), MessageBusError> {
        self.inbound.try_push(message)
    }

    pub fn consume_inbound(&self) -> Option<InboundMessage> {
        self.try_consume_inbound()
    }

    pub fn try_consume_inbound(&self) -> Option<InboundMessage> {
        self.inbound.try_pop()
    }

    pub fn drain_inbound_matching<F>(&self, limit: usize, matcher: F) -> Vec<InboundMessage>
    where
        F: FnMut(&InboundMessage) -> bool,
    {
        self.inbound.drain_matching(limit, matcher)
    }

    pub fn consume_inbound_blocking(&self) -> InboundMessage {
        self.inbound.pop_blocking()
    }

    pub fn publish_outbound(&self, message: OutboundMessage) {
        self.outbound.push_lossy(message);
    }

    pub fn try_publish_outbound(&self, message: OutboundMessage) -> Result<(), MessageBusError> {
        self.outbound.try_push(message)
    }

    pub fn consume_outbound(&self) -> Option<OutboundMessage> {
        self.try_consume_outbound()
    }

    pub fn try_consume_outbound(&self) -> Option<OutboundMessage> {
        self.outbound.try_pop()
    }

    pub fn consume_outbound_blocking(&self) -> OutboundMessage {
        self.outbound.pop_blocking()
    }

    pub fn inbound_size(&self) -> usize {
        self.inbound.len()
    }

    pub fn outbound_size(&self) -> usize {
        self.outbound.len()
    }

    pub fn accounting_snapshot(&self) -> MessageBusAccountingSnapshot {
        MessageBusAccountingSnapshot {
            inbound: self.inbound.accounting_snapshot(),
            outbound: self.outbound.accounting_snapshot(),
        }
    }

    pub fn accounting_snapshot_and_reset(&self) -> MessageBusAccountingSnapshot {
        MessageBusAccountingSnapshot {
            inbound: self.inbound.accounting_snapshot_and_reset(),
            outbound: self.outbound.accounting_snapshot_and_reset(),
        }
    }

    pub fn record_outbound_coalesced(&self, count: u64) {
        self.outbound.record_coalesced(count);
    }
}

impl<T> QueueState<T> {
    fn bounded(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            accounting: Mutex::new(QueueAccountingCounters::default()),
            available: Condvar::new(),
            capacity: Some(capacity),
            #[cfg(test)]
            push_wait_hook: Mutex::new(None),
        }
    }

    fn push(&self, message: T) {
        let mut queue = self.lock_queue();
        while self
            .capacity
            .is_some_and(|capacity| queue.len() >= capacity)
        {
            #[cfg(test)]
            self.notify_push_waiting();
            queue = self
                .available
                .wait(queue)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        queue.push_back(message);
        self.lock_accounting().accept();
        self.available.notify_all();
    }

    fn try_push(&self, message: T) -> Result<(), MessageBusError> {
        let mut queue = self.lock_queue();
        if let Some(capacity) = self.capacity {
            if queue.len() >= capacity {
                return Err(MessageBusError::QueueFull { capacity });
            }
        }
        queue.push_back(message);
        self.lock_accounting().accept();
        self.available.notify_one();
        Ok(())
    }

    fn push_lossy(&self, message: T) {
        let mut queue = self.lock_queue();
        if self
            .capacity
            .is_some_and(|capacity| queue.len() >= capacity)
            && queue.pop_front().is_some()
        {
            self.lock_accounting().drop_one();
        }
        queue.push_back(message);
        self.lock_accounting().accept();
        self.available.notify_all();
    }

    fn try_pop(&self) -> Option<T> {
        let mut queue = self.lock_queue();
        let item = queue.pop_front();
        if item.is_some() {
            self.lock_accounting().emit();
            self.available.notify_all();
        }
        item
    }

    fn pop_blocking(&self) -> T {
        let mut queue = self.lock_queue();
        loop {
            if let Some(message) = queue.pop_front() {
                self.lock_accounting().emit();
                self.available.notify_all();
                return message;
            }
            queue = self
                .available
                .wait(queue)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    fn drain_matching<F>(&self, limit: usize, mut matcher: F) -> Vec<T>
    where
        F: FnMut(&T) -> bool,
    {
        if limit == 0 {
            return Vec::new();
        }
        let mut queue = self.lock_queue();
        let mut retained = VecDeque::with_capacity(queue.len());
        let mut drained = Vec::new();
        while let Some(item) = queue.pop_front() {
            if drained.len() < limit && matcher(&item) {
                drained.push(item);
            } else {
                retained.push_back(item);
            }
        }
        *queue = retained;
        if !drained.is_empty() {
            self.lock_accounting().emit_many(drained.len());
            self.available.notify_all();
        }
        drained
    }

    fn len(&self) -> usize {
        self.lock_queue().len()
    }

    fn accounting_snapshot(&self) -> QueueAccountingSnapshot {
        let queue = self.lock_queue();
        let accounting = *self.lock_accounting();
        QueueAccountingSnapshot::current(queue.len(), self.capacity, accounting)
    }

    fn accounting_snapshot_and_reset(&self) -> QueueAccountingSnapshot {
        let queue = self.lock_queue();
        let accounting = self.lock_accounting().reset();
        QueueAccountingSnapshot::current(queue.len(), self.capacity, accounting)
    }

    fn record_coalesced(&self, count: u64) {
        self.lock_accounting().coalesce(count);
    }

    fn lock_queue(&self) -> MutexGuard<'_, VecDeque<T>> {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_accounting(&self) -> MutexGuard<'_, QueueAccountingCounters> {
        self.accounting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(test)]
    fn set_push_wait_hook(&self, hook: PushWaitHook) {
        *self
            .push_wait_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(hook);
    }

    #[cfg(test)]
    fn notify_push_waiting(&self) {
        let hook = self
            .push_wait_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(hook) = hook {
            let (lock, condvar) = &*hook;
            let mut waiting = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            *waiting = true;
            condvar.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc, Condvar, Mutex};
    use std::thread;
    use std::time::Duration;

    fn wait_for_hook(pair: &(Mutex<bool>, Condvar)) {
        let (lock, condvar) = pair;
        let mut observed = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while !*observed {
            let (next, timeout) = condvar
                .wait_timeout(observed, Duration::from_secs(5))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(!timeout.timed_out(), "producer should reach wait boundary");
            observed = next;
        }
    }

    #[test]
    fn bus_preserves_fifo_and_sizes_for_inbound_and_outbound() {
        let bus = MessageBus::new();
        bus.publish_inbound(InboundMessage::new("telegram", "user", "chat", "one"));
        bus.publish_inbound(InboundMessage::new("telegram", "user", "chat", "two"));
        bus.publish_outbound(OutboundMessage::new("telegram", "chat", "reply"));

        assert_eq!(bus.inbound_size(), 2);
        assert_eq!(bus.outbound_size(), 1);
        assert_eq!(
            bus.consume_inbound().map(|message| message.content),
            Some("one".to_owned())
        );
        assert_eq!(
            bus.consume_inbound().map(|message| message.content),
            Some("two".to_owned())
        );
        assert_eq!(
            bus.consume_outbound().map(|message| message.content),
            Some("reply".to_owned())
        );
        assert!(bus.try_consume_inbound().is_none());
        assert!(bus.try_consume_outbound().is_none());
    }

    #[test]
    fn bounded_bus_reports_capacity_for_both_queues() {
        let bus = MessageBus::bounded(1);
        assert_eq!(
            bus.try_publish_inbound(InboundMessage::new("a", "b", "c", "1")),
            Ok(())
        );
        assert_eq!(
            bus.try_publish_inbound(InboundMessage::new("a", "b", "c", "2")),
            Err(MessageBusError::QueueFull { capacity: 1 })
        );
        assert_eq!(
            bus.try_publish_outbound(OutboundMessage::new("a", "c", "1")),
            Ok(())
        );
        assert_eq!(
            bus.try_publish_outbound(OutboundMessage::new("a", "c", "2")),
            Err(MessageBusError::QueueFull { capacity: 1 })
        );
    }

    #[test]
    fn bounded_bus_blocking_publish_waits_for_capacity() {
        let bus = MessageBus::bounded(1);
        bus.publish_inbound(InboundMessage::new("a", "b", "c", "first"));
        let producer = bus.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            started_tx.send(()).expect("publish start signal");
            producer.publish_inbound(InboundMessage::new("a", "b", "c", "second"));
            done_tx.send(()).expect("publish completion signal");
        });

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("bounded publisher should start");
        assert!(done_rx.recv_timeout(Duration::from_millis(20)).is_err());
        assert_eq!(
            bus.consume_inbound().map(|message| message.content),
            Some("first".to_owned())
        );
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("bounded publisher should wake after consume");
        assert_eq!(
            bus.consume_inbound().map(|message| message.content),
            Some("second".to_owned())
        );
        handle.join().expect("publisher join");
    }

    #[test]
    fn bounded_bus_blocking_consume_wakes_waiting_accounted_producer() {
        let bus = MessageBus::bounded(1);
        bus.inbound
            .set_push_wait_hook(Arc::new((Mutex::new(false), Condvar::new())));
        let hook = bus
            .inbound
            .push_wait_hook
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .expect("push wait hook")
            .clone();
        bus.publish_inbound(InboundMessage::new("a", "b", "c", "first"));

        let producer = bus.clone();
        let (producer_done_tx, producer_done_rx) = mpsc::channel();
        let producer_handle = thread::spawn(move || {
            producer.publish_inbound(InboundMessage::new("a", "b", "c", "second"));
            producer_done_tx
                .send(())
                .expect("producer completion signal");
        });

        wait_for_hook(&hook);

        let consumer = bus.clone();
        let (consumer_done_tx, consumer_done_rx) = mpsc::channel();
        let consumer_handle = thread::spawn(move || {
            let content = consumer.consume_inbound_blocking().content;
            consumer_done_tx
                .send(content)
                .expect("consumer completion signal");
        });

        assert_eq!(
            consumer_done_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("consumer should complete after consuming seeded item"),
            "first"
        );
        producer_done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("producer should wake after blocking consumer frees capacity");
        producer_handle.join().expect("producer join");
        consumer_handle.join().expect("consumer join");

        let snapshot = bus.accounting_snapshot().inbound;
        assert_eq!(snapshot.accepted, BusMeasurement::Available(2));
        assert_eq!(snapshot.emitted, BusMeasurement::Available(1));
        assert_eq!(snapshot.dropped, BusMeasurement::Available(0));
        assert_eq!(snapshot.depth, BusMeasurement::Available(1));
        assert_eq!(
            bus.consume_inbound().map(|message| message.content),
            Some("second".to_owned())
        );
    }

    #[test]
    fn bounded_bus_outbound_publish_keeps_newest_message_without_blocking() {
        let bus = MessageBus::bounded(1);
        bus.publish_outbound(OutboundMessage::new("a", "c", "progress"));
        bus.publish_outbound(OutboundMessage::new("a", "c", "final"));
        assert_eq!(
            bus.consume_outbound().map(|message| message.content),
            Some("final".to_owned())
        );
    }

    #[test]
    fn cloned_bus_handles_share_queues_and_blocking_consumers_wake() {
        let bus = MessageBus::new();
        let producer = bus.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            ready_tx.send(()).expect("ready signal");
            producer.consume_inbound_blocking().content
        });

        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("consumer ready");
        bus.publish_inbound(InboundMessage::new("telegram", "user", "chat", "wake"));

        assert_eq!(handle.join().expect("consumer join"), "wake");
    }

    #[test]
    fn drain_matching_limits_matches_and_preserves_retained_fifo() {
        let bus = MessageBus::new();
        for content in ["a1", "b1", "a2", "a3", "b2"] {
            bus.publish_inbound(InboundMessage::new("telegram", "user", "chat", content));
        }

        let drained = bus.drain_inbound_matching(2, |message| message.content.starts_with('a'));
        assert_eq!(
            drained
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            ["a1", "a2"]
        );
        let retained = std::iter::from_fn(|| bus.consume_inbound())
            .map(|message| message.content)
            .collect::<Vec<_>>();
        assert_eq!(retained, ["b1", "a3", "b2"]);
    }
}
