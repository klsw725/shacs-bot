#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusMeasurement<T> {
    Available(T),
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountingFreshness {
    Current,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueAccountingSnapshot {
    pub depth: BusMeasurement<usize>,
    pub capacity: BusMeasurement<usize>,
    pub accepted: BusMeasurement<u64>,
    pub emitted: BusMeasurement<u64>,
    pub coalesced: BusMeasurement<u64>,
    pub dropped: BusMeasurement<u64>,
    pub freshness: AccountingFreshness,
}

impl QueueAccountingSnapshot {
    pub const fn unavailable() -> Self {
        Self {
            depth: BusMeasurement::Unavailable,
            capacity: BusMeasurement::Unavailable,
            accepted: BusMeasurement::Unavailable,
            emitted: BusMeasurement::Unavailable,
            coalesced: BusMeasurement::Unavailable,
            dropped: BusMeasurement::Unavailable,
            freshness: AccountingFreshness::Unavailable,
        }
    }

    pub(crate) const fn current(
        depth: usize,
        capacity: Option<usize>,
        counters: QueueAccountingCounters,
    ) -> Self {
        Self {
            depth: BusMeasurement::Available(depth),
            capacity: match capacity {
                Some(capacity) => BusMeasurement::Available(capacity),
                None => BusMeasurement::Unavailable,
            },
            accepted: BusMeasurement::Available(counters.accepted),
            emitted: BusMeasurement::Available(counters.emitted),
            coalesced: BusMeasurement::Available(counters.coalesced),
            dropped: BusMeasurement::Available(counters.dropped),
            freshness: AccountingFreshness::Current,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageBusAccountingSnapshot {
    pub inbound: QueueAccountingSnapshot,
    pub outbound: QueueAccountingSnapshot,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueueAccountingCounters {
    accepted: u64,
    emitted: u64,
    coalesced: u64,
    dropped: u64,
}

impl QueueAccountingCounters {
    pub(crate) fn accept(&mut self) {
        self.accepted = self.accepted.saturating_add(1);
    }

    pub(crate) fn emit(&mut self) {
        self.emitted = self.emitted.saturating_add(1);
    }

    pub(crate) fn emit_many(&mut self, count: usize) {
        let count = u64::try_from(count).unwrap_or(u64::MAX);
        self.emitted = self.emitted.saturating_add(count);
    }

    pub(crate) fn coalesce(&mut self, count: u64) {
        self.coalesced = self.coalesced.saturating_add(count);
    }

    pub(crate) fn drop_one(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    pub(crate) fn reset(&mut self) -> Self {
        let snapshot = *self;
        *self = Self::default();
        snapshot
    }
}
