//! Calls the classifier stopped in a row and in a session (Spec 048 PRD 020
//! scope 4): auto mode asks the user once three calls in a row or twenty in
//! the session did not run.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

pub(crate) const CONSECUTIVE_LIMIT: u32 = 3;
pub(crate) const TOTAL_LIMIT: u32 = 20;

/// Why auto mode asks about a call its classifier did not allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoModeEscalation {
    Consecutive,
    Total,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoModeBlockCounts {
    pub consecutive: u32,
    pub total: u32,
}

impl AutoModeBlockCounts {
    /// Counts one more stopped call. Reaching the total limit starts the
    /// total over and leaves the count in a row as it is.
    pub(crate) fn block(&mut self) -> Option<AutoModeEscalation> {
        self.consecutive = self.consecutive.saturating_add(1);
        self.total = self.total.saturating_add(1);
        if self.total >= TOTAL_LIMIT {
            self.total = 0;
            return Some(AutoModeEscalation::Total);
        }
        (self.consecutive >= CONSECUTIVE_LIMIT).then_some(AutoModeEscalation::Consecutive)
    }
}

/// The counts the calls of one run share, and the calls auto mode did not
/// run in the tool dispatch under way.
#[derive(Debug, Clone, Default)]
pub struct AutoModeBlocks(Arc<Mutex<BlockState>>);

#[derive(Debug, Default)]
struct BlockState {
    counts: AutoModeBlockCounts,
    not_run: BTreeSet<String>,
    /// Ids of calls the decision let run in this dispatch. An id that is
    /// also not run (the assistant reused it) gets no special handling.
    ran: BTreeSet<String>,
}

impl AutoModeBlocks {
    pub fn new(counts: AutoModeBlockCounts) -> Self {
        Self(Arc::new(Mutex::new(BlockState {
            counts,
            ..BlockState::default()
        })))
    }

    pub fn counts(&self) -> AutoModeBlockCounts {
        self.state().counts
    }

    /// The classifier allowed a call.
    pub(crate) fn allowed(&self) -> AutoModeBlockCounts {
        let mut state = self.state();
        state.counts.consecutive = 0;
        state.counts
    }

    pub(crate) fn blocked(&self) -> (AutoModeBlockCounts, Option<AutoModeEscalation>) {
        let mut state = self.state();
        let escalation = state.counts.block();
        (state.counts, escalation)
    }

    pub(crate) fn begin_dispatch(&self) {
        let mut state = self.state();
        state.not_run.clear();
        state.ran.clear();
    }

    pub(crate) fn mark_not_run(&self, call_id: &str) {
        self.state().not_run.insert(call_id.to_owned());
    }

    pub(crate) fn mark_ran(&self, call_id: &str) {
        self.state().ran.insert(call_id.to_owned());
    }

    /// Whether auto mode did not run this call in the dispatch under way,
    /// so its result text is auto mode's, not the tool's.
    pub(crate) fn did_not_run(&self, call_id: &str) -> bool {
        let state = self.state();
        state.not_run.contains(call_id) && !state.ran.contains(call_id)
    }

    fn state(&self) -> MutexGuard<'_, BlockState> {
        self.0.lock().expect("auto mode blocks lock poisoned")
    }
}

#[cfg(test)]
#[path = "block_counts_tests.rs"]
mod tests;
