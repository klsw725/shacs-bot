use std::path::Path;

use shacs_session::durable_event::{
    DurableEventPayload, DurableEventStore, WORK_CANCELLED, WORK_TERMINAL,
};
use shacs_session::durable_work::{
    apply_persisted_durable_work_event, DurableWorkReplayState, ReplayWorkState, WorkTerminal,
};

use super::{
    DurableDispatchError, EntrustedWorkReadRequest, ObservationBlock, ObservationError,
    ObservationNextWake, ObservationRecord, ObservationResult, ObservationWake,
    ENTRUSTED_OBSERVATION_OUTCOME,
};

pub(super) struct ObservationLedger {
    pub state: DurableWorkReplayState,
    pub history: Vec<ObservationRecord>,
    pub baseline: Option<ObservationRecord>,
    pub decisions: Vec<super::ObservationDecision>,
}

impl ObservationLedger {
    pub fn read(root: &Path, request: &EntrustedWorkReadRequest) -> Result<Self, ObservationError> {
        let mut ledger = Self {
            state: DurableWorkReplayState::default(),
            history: Vec::new(),
            baseline: None,
            decisions: Vec::new(),
        };
        if !root.exists() {
            return Ok(ledger);
        }
        let store = DurableEventStore::open(root).map_err(DurableDispatchError::from)?;
        let prefix = format!("observation:{}:", request.work_id);
        let mut failure = None;
        let summary = store
            .visit_from_sequence(0, |event| {
                if failure.is_some() || event.session_id != request.owner_session_id {
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
                if event.kind == WORK_CANCELLED {
                    match super::entrusted_comparison_cancelled::from_event(
                        root,
                        event,
                        &ledger.state,
                    ) {
                        Ok(decision) => ledger.push_decision(decision),
                        Err(error) => failure = Some(error),
                    }
                    return;
                }
                if event.kind != WORK_TERMINAL {
                    return;
                }
                let parsed =
                    serde_json::from_value::<WorkTerminal>(data.clone()).and_then(|terminal| {
                        if terminal.outcome_ref != ENTRUSTED_OBSERVATION_OUTCOME {
                            return Ok(None);
                        }
                        terminal
                            .facts
                            .map(serde_json::from_value::<ObservationRecord>)
                            .transpose()
                    });
                match parsed {
                    Ok(Some(record)) => {
                        if let Some(decision) = &record.decision {
                            if decision.verdict.establishes_baseline() {
                                ledger.baseline = Some(record.clone());
                            }
                            ledger.push_decision(decision.clone());
                        } else {
                            let mut legacy = record.clone();
                            if let ObservationResult::Observed { summary, .. } = &mut legacy.result
                            {
                                *summary = None;
                            }
                            let mut decision =
                                super::entrusted_comparison_evaluate::compare(&legacy, None, None);
                            decision.basis =
                                super::ObservationDecisionBasis::LegacyObservationUncompared;
                            ledger.push_decision(decision);
                        }
                        ledger.history.push(record);
                        if ledger.history.len() > 32 {
                            ledger.history.remove(0);
                        }
                    }
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

    fn push_decision(&mut self, decision: super::ObservationDecision) {
        self.decisions.push(decision);
        if self.decisions.len() > 32 {
            self.decisions.remove(0);
        }
    }

    pub fn next(&self, initial: &ObservationWake) -> ObservationNextWake {
        let prefix = initial.lineage_prefix();
        let latest = self
            .state
            .items
            .values()
            .filter(|item| item.work_id.starts_with(&prefix))
            .max_by_key(|item| item.enqueued_sequence);
        if let Some(item) = latest {
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
                    };
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
            let Some(record) = self
                .history
                .iter()
                .rev()
                .find(|record| record.wake.durable_work_id() == item.work_id)
            else {
                return ObservationNextWake::Stopped {
                    reason: ObservationBlock::RecoveryNeeded,
                };
            };
            return match record.next_wake_at_ms {
                Some(due_at_ms) => {
                    let mut wake = initial.clone();
                    wake.due_at_ms = due_at_ms;
                    ObservationNextWake::AwaitingEnqueue {
                        work_id: wake.durable_work_id(),
                        due_at_ms,
                    }
                }
                None => ObservationNextWake::Stopped {
                    reason: match record.result {
                        ObservationResult::Blocked { reason }
                        | ObservationResult::Unavailable { reason } => reason,
                        ObservationResult::Observed { .. } => ObservationBlock::RecoveryNeeded,
                    },
                },
            };
        }
        ObservationNextWake::AwaitingEnqueue {
            work_id: initial.durable_work_id(),
            due_at_ms: initial.due_at_ms,
        }
    }
}
