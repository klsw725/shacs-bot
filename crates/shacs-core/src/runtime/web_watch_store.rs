use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_session::durable_event::{DurableEventPayload, DurableEventStore, WORK_TERMINAL};
use shacs_session::durable_work::{
    apply_persisted_durable_work_event, DurableWorkReplayState, ReplayWorkState, WorkTerminal,
};
use shacs_session::Session;

use super::{
    DurableDispatchError, EntrustedWorkMetadataError, ObservationBlock, ObservationError,
    ObservationNextWake, WebCheckResult, WebFailureInterval, WebIntervalCloseReason,
    WebIntervalClosure, WebObservationRecord, WebRuntimeGap, WebWatchEntry, WebWatchWake,
    WEB_WATCH_METADATA_KEY, WEB_WATCH_OUTCOME,
};

const HISTORY_LIMIT: usize = 32;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebWatchStore {
    watches: Vec<WebWatchEntry>,
}

pub(super) fn web_watches(
    session: &Session,
) -> Result<Vec<WebWatchEntry>, EntrustedWorkMetadataError> {
    session
        .metadata
        .get(WEB_WATCH_METADATA_KEY)
        .cloned()
        .map(serde_json::from_value::<WebWatchStore>)
        .transpose()
        .map(|store| store.unwrap_or_default().watches)
        .map_err(EntrustedWorkMetadataError::Deserialize)
}

pub(super) fn store_web_watches(
    session: &mut Session,
    watches: Vec<WebWatchEntry>,
) -> Result<(), EntrustedWorkMetadataError> {
    let value = serde_json::to_value(WebWatchStore { watches })
        .map_err(EntrustedWorkMetadataError::Serialize)?;
    session
        .metadata
        .insert(WEB_WATCH_METADATA_KEY.to_owned(), value);
    Ok(())
}

/// Facts replayed from the durable ledger for one web watch.
pub(super) struct WebWatchLedger {
    pub state: DurableWorkReplayState,
    pub history: Vec<WebObservationRecord>,
    pub comparable: Option<WebObservationRecord>,
    pub last_success: Option<WebObservationRecord>,
    pub intervals: Vec<WebFailureInterval>,
    pub gaps: Vec<WebRuntimeGap>,
}

impl WebWatchLedger {
    pub fn read(root: &Path, owner: &str, work_id: &str) -> Result<Self, ObservationError> {
        let mut ledger = Self {
            state: DurableWorkReplayState::default(),
            history: Vec::new(),
            comparable: None,
            last_success: None,
            intervals: Vec::new(),
            gaps: Vec::new(),
        };
        if !root.exists() {
            return Ok(ledger);
        }
        let store = DurableEventStore::open(root).map_err(DurableDispatchError::from)?;
        let prefix = format!("web-watch:{work_id}:");
        let mut failure = None;
        let summary = store
            .visit_from_sequence(0, |event| {
                if failure.is_some() || event.session_id != owner {
                    return;
                }
                let DurableEventPayload::Inline { data, .. } = &event.payload else {
                    return;
                };
                if !data
                    .get("work_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id.starts_with(&prefix))
                {
                    return;
                }
                if let Err(error) = apply_persisted_durable_work_event(&mut ledger.state, event) {
                    failure = Some(DurableDispatchError::InvalidWork(error.to_string()));
                    return;
                }
                if event.kind != WORK_TERMINAL {
                    return;
                }
                let parsed =
                    serde_json::from_value::<WorkTerminal>(data.clone()).and_then(|terminal| {
                        if terminal.outcome_ref != WEB_WATCH_OUTCOME {
                            return Ok(None);
                        }
                        terminal
                            .facts
                            .map(serde_json::from_value::<WebObservationRecord>)
                            .transpose()
                    });
                match parsed {
                    Ok(Some(record)) => ledger.apply(record),
                    Ok(None) => {}
                    Err(error) => failure = Some(DurableDispatchError::Serialization(error)),
                }
            })
            .map_err(DurableDispatchError::from)?;
        if let Some(error) = failure {
            return Err(error.into());
        }
        if summary.incomplete_tail || !summary.compatibility.is_current() {
            return Err(ObservationError::Blocked(ObservationBlock::RecoveryNeeded));
        }
        Ok(ledger)
    }

    fn apply(&mut self, record: WebObservationRecord) {
        let revision = record.wake.condition_revision;
        let transition = record.wake.goal_transition_id.clone();
        if let Some(open) = self
            .intervals
            .last_mut()
            .filter(|interval| interval.closed.is_none())
        {
            if open.condition_revision != revision || open.goal_transition_id != transition {
                open.closed = Some(WebIntervalClosure {
                    reason: WebIntervalCloseReason::LineageChanged,
                    at_ms: None,
                });
            }
        }
        let open = self
            .intervals
            .last_mut()
            .filter(|interval| interval.closed.is_none());
        match &record.result {
            WebCheckResult::Observed { .. } => {
                if let Some(open) = open {
                    open.closed = Some(WebIntervalClosure {
                        reason: WebIntervalCloseReason::Recovered,
                        at_ms: Some(record.observed_at_ms),
                    });
                }
                self.last_success = Some(record.clone());
            }
            WebCheckResult::Failed { failure, .. } => match open {
                Some(open) => {
                    open.failure_count = open.failure_count.saturating_add(1);
                    open.last_failure_at_ms = record.observed_at_ms;
                    open.last_failure = *failure;
                }
                None => self.intervals.push(WebFailureInterval {
                    interval_id: format!(
                        "web-failure:{:x}",
                        Sha256::digest(record.wake.durable_work_id().as_bytes())
                    ),
                    condition_revision: revision,
                    goal_transition_id: transition,
                    started_at_ms: record.observed_at_ms,
                    last_failure_at_ms: record.observed_at_ms,
                    failure_count: 1,
                    first_failure: *failure,
                    last_failure: *failure,
                    closed: None,
                }),
            },
            WebCheckResult::Blocked { .. } => {}
        }
        if record.decision.verdict.establishes_baseline() {
            self.comparable = Some(record.clone());
        }
        if let Some(gap) = record.runtime_gap {
            self.gaps.push(gap);
        }
        self.history.push(record);
        trim(&mut self.history);
        trim(&mut self.gaps);
        trim(&mut self.intervals);
    }

    /// Next wake of the lineage `initial` belongs to, following the latest durable item.
    pub fn next(&self, initial: &WebWatchWake) -> ObservationNextWake {
        let prefix = initial.lineage_prefix();
        let Some(item) = self
            .state
            .items
            .values()
            .filter(|item| item.work_id.starts_with(&prefix))
            .max_by_key(|item| item.enqueued_sequence)
        else {
            return ObservationNextWake::AwaitingEnqueue {
                work_id: initial.durable_work_id(),
                due_at_ms: initial.due_at_ms,
            };
        };
        if item.cancellation_requested_sequence.is_some() {
            return ObservationNextWake::Stopped {
                reason: ObservationBlock::Cancelled,
            };
        }
        match item.state {
            ReplayWorkState::Pending | ReplayWorkState::WaitingRetry => {
                return match item.next_wake_at_ms {
                    Some(due_at_ms) => ObservationNextWake::Queued {
                        work_id: item.work_id.clone(),
                        due_at_ms,
                    },
                    None => ObservationNextWake::Stopped {
                        reason: ObservationBlock::RecoveryNeeded,
                    },
                }
            }
            ReplayWorkState::Leased => {
                return ObservationNextWake::Leased {
                    work_id: item.work_id.clone(),
                }
            }
            ReplayWorkState::Cancelled => {
                return ObservationNextWake::Stopped {
                    reason: ObservationBlock::Cancelled,
                }
            }
            ReplayWorkState::Terminal => {}
        }
        match self
            .history
            .iter()
            .rev()
            .find(|record| record.wake.durable_work_id() == item.work_id)
            .and_then(|record| record.next_wake_at_ms)
        {
            Some(due_at_ms) => {
                let mut wake = initial.clone();
                wake.due_at_ms = due_at_ms;
                ObservationNextWake::AwaitingEnqueue {
                    work_id: wake.durable_work_id(),
                    due_at_ms,
                }
            }
            None => ObservationNextWake::Stopped {
                reason: ObservationBlock::RecoveryNeeded,
            },
        }
    }
}

fn trim<T>(items: &mut Vec<T>) {
    if items.len() > HISTORY_LIMIT {
        items.drain(..items.len() - HISTORY_LIMIT);
    }
}
