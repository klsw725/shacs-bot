use super::{
    Spec035TransportEventKind, Spec035TransportEventMetadata, Spec035TransportGeneration,
    Spec035TransportSequence, Spec035TransportServerHello,
};
use crate::Spec031Count;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportPhase {
    AwaitingHello,
    AwaitingSnapshot,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportDecision {
    AcceptHello,
    ApplySnapshot,
    ApplyDelta,
    RejectPreSnapshot,
    RejectStaleGeneration,
    RejectDuplicate,
    RejectGap,
    RejectUnexpectedHello,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TransportOrderingCounters {
    applied_snapshots: Spec031Count,
    applied_deltas: Spec031Count,
    pre_snapshot_deltas: Spec031Count,
    stale_generations: Spec031Count,
    duplicate_sequences: Spec031Count,
    sequence_gaps: Spec031Count,
}

impl Spec035TransportOrderingCounters {
    pub const fn applied_snapshots(self) -> Spec031Count {
        self.applied_snapshots
    }

    pub const fn applied_deltas(self) -> Spec031Count {
        self.applied_deltas
    }

    pub const fn pre_snapshot_deltas(self) -> Spec031Count {
        self.pre_snapshot_deltas
    }

    pub const fn stale_generations(self) -> Spec031Count {
        self.stale_generations
    }

    pub const fn duplicate_sequences(self) -> Spec031Count {
        self.duplicate_sequences
    }

    pub const fn sequence_gaps(self) -> Spec031Count {
        self.sequence_gaps
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035TransportOrdering {
    phase: Spec035TransportPhase,
    generation: Option<Spec035TransportGeneration>,
    last_sequence: Option<Spec035TransportSequence>,
    counters: Spec035TransportOrderingCounters,
}

impl Spec035TransportOrdering {
    pub const fn new() -> Self {
        Self {
            phase: Spec035TransportPhase::AwaitingHello,
            generation: None,
            last_sequence: None,
            counters: Spec035TransportOrderingCounters {
                applied_snapshots: Spec031Count::new(0),
                applied_deltas: Spec031Count::new(0),
                pre_snapshot_deltas: Spec031Count::new(0),
                stale_generations: Spec031Count::new(0),
                duplicate_sequences: Spec031Count::new(0),
                sequence_gaps: Spec031Count::new(0),
            },
        }
    }

    pub const fn phase(&self) -> Spec035TransportPhase {
        self.phase
    }

    pub const fn counters(&self) -> Spec035TransportOrderingCounters {
        self.counters
    }

    pub const fn last_sequence(&self) -> Option<Spec035TransportSequence> {
        self.last_sequence
    }

    pub fn accept_hello(
        &mut self,
        hello: &Spec035TransportServerHello,
    ) -> Spec035TransportDecision {
        match self.phase {
            Spec035TransportPhase::AwaitingHello => {
                self.phase = Spec035TransportPhase::AwaitingSnapshot;
                self.generation = Some(hello.generation().clone());
                self.last_sequence = None;
                Spec035TransportDecision::AcceptHello
            }
            Spec035TransportPhase::AwaitingSnapshot | Spec035TransportPhase::Live => {
                Spec035TransportDecision::RejectUnexpectedHello
            }
        }
    }

    pub fn observe(
        &mut self,
        metadata: &Spec035TransportEventMetadata,
    ) -> Spec035TransportDecision {
        match self.phase {
            Spec035TransportPhase::AwaitingHello => self.reject_before_snapshot(metadata.kind()),
            Spec035TransportPhase::AwaitingSnapshot => self.observe_awaiting_snapshot(metadata),
            Spec035TransportPhase::Live => self.observe_live(metadata),
        }
    }

    fn observe_awaiting_snapshot(
        &mut self,
        metadata: &Spec035TransportEventMetadata,
    ) -> Spec035TransportDecision {
        match metadata.kind() {
            Spec035TransportEventKind::Snapshot => {
                if self.generation.as_ref() != Some(metadata.generation()) {
                    self.increment_stale();
                    return Spec035TransportDecision::RejectStaleGeneration;
                }
                self.phase = Spec035TransportPhase::Live;
                self.last_sequence = Some(metadata.sequence());
                self.counters.applied_snapshots = increment(self.counters.applied_snapshots);
                Spec035TransportDecision::ApplySnapshot
            }
            Spec035TransportEventKind::Delta => self.reject_before_snapshot(metadata.kind()),
        }
    }

    fn observe_live(
        &mut self,
        metadata: &Spec035TransportEventMetadata,
    ) -> Spec035TransportDecision {
        if self.generation.as_ref() != Some(metadata.generation()) {
            self.increment_stale();
            return Spec035TransportDecision::RejectStaleGeneration;
        }
        match metadata.kind() {
            Spec035TransportEventKind::Snapshot => self.reject_gap(),
            Spec035TransportEventKind::Delta => self.observe_live_delta(metadata.sequence()),
        }
    }

    fn observe_live_delta(
        &mut self,
        sequence: Spec035TransportSequence,
    ) -> Spec035TransportDecision {
        let Some(last_sequence) = self.last_sequence else {
            return self.reject_gap();
        };
        if sequence.as_u64() <= last_sequence.as_u64() {
            self.counters.duplicate_sequences = increment(self.counters.duplicate_sequences);
            return Spec035TransportDecision::RejectDuplicate;
        }
        if last_sequence.as_u64().checked_add(1) != Some(sequence.as_u64()) {
            return self.reject_gap();
        }
        self.last_sequence = Some(sequence);
        self.counters.applied_deltas = increment(self.counters.applied_deltas);
        Spec035TransportDecision::ApplyDelta
    }

    fn reject_before_snapshot(
        &mut self,
        kind: Spec035TransportEventKind,
    ) -> Spec035TransportDecision {
        match kind {
            Spec035TransportEventKind::Snapshot => Spec035TransportDecision::RejectPreSnapshot,
            Spec035TransportEventKind::Delta => {
                self.counters.pre_snapshot_deltas = increment(self.counters.pre_snapshot_deltas);
                Spec035TransportDecision::RejectPreSnapshot
            }
        }
    }

    fn increment_stale(&mut self) {
        self.counters.stale_generations = increment(self.counters.stale_generations);
    }

    fn reject_gap(&mut self) -> Spec035TransportDecision {
        self.counters.sequence_gaps = increment(self.counters.sequence_gaps);
        Spec035TransportDecision::RejectGap
    }
}

impl Default for Spec035TransportOrdering {
    fn default() -> Self {
        Self::new()
    }
}

const fn increment(count: Spec031Count) -> Spec031Count {
    Spec031Count::new(count.as_u64().saturating_add(1))
}
