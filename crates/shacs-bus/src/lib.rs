pub use shacs_channels::{InboundMessage, OutboundMessage};

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

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
    queue: Mutex<VecDeque<T>>,
    available: Condvar,
    capacity: Option<usize>,
}

impl<T> Default for QueueState<T> {
    fn default() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            available: Condvar::new(),
            capacity: None,
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
        self.outbound.push(message);
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
}

impl<T> QueueState<T> {
    fn bounded(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            available: Condvar::new(),
            capacity: Some(capacity),
        }
    }

    fn push(&self, message: T) {
        let mut queue = self.lock_queue();
        queue.push_back(message);
        self.available.notify_one();
    }

    fn try_push(&self, message: T) -> Result<(), MessageBusError> {
        let mut queue = self.lock_queue();
        if let Some(capacity) = self.capacity {
            if queue.len() >= capacity {
                return Err(MessageBusError::QueueFull { capacity });
            }
        }
        queue.push_back(message);
        self.available.notify_one();
        Ok(())
    }

    fn try_pop(&self) -> Option<T> {
        self.lock_queue().pop_front()
    }

    fn pop_blocking(&self) -> T {
        let mut queue = self.lock_queue();
        loop {
            if let Some(message) = queue.pop_front() {
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
        drained
    }

    fn len(&self) -> usize {
        self.lock_queue().len()
    }

    fn lock_queue(&self) -> MutexGuard<'_, VecDeque<T>> {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

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
