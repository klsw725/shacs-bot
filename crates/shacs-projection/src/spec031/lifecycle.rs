use super::{
    lifecycle_envelope, Spec031ActionRef, Spec031ApprovalState, Spec031Envelope,
    Spec031ObservedAtUnixMs, Spec031ParentRef, Spec031ProgressDelivery, Spec031SubjectRef,
};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031LifecycleLineage {
    pub subject_ref: Spec031SubjectRef,
    pub parent_ref: Option<Spec031ParentRef>,
    pub action_ref: Option<Spec031ActionRef>,
    pub digest: Option<super::Spec031Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031LifecycleInput {
    pub lineage: Spec031LifecycleLineage,
    pub facts: Vec<Spec031LifecycleFact>,
    pub observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031RuntimeControlKind {
    Stop,
    Restart,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031RuntimeControlState {
    Requested,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031RecoveryState {
    Interrupted,
    Requested,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031TerminalOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031LifecycleFact {
    Approval {
        state: Spec031ApprovalState,
    },
    RuntimeControl {
        kind: Spec031RuntimeControlKind,
        state: Spec031RuntimeControlState,
    },
    Progress {
        delivery: Spec031ProgressDelivery,
    },
    Terminal {
        outcome: Spec031TerminalOutcome,
    },
    Recovery {
        state: Spec031RecoveryState,
    },
    PendingFollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031LifecycleError {
    Construction,
    DuplicateTerminal,
    StaleLineage,
}

impl fmt::Display for Spec031LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Construction => write!(formatter, "invalid lifecycle projection input"),
            Self::DuplicateTerminal => write!(formatter, "duplicate terminal lifecycle fact"),
            Self::StaleLineage => write!(formatter, "stale lifecycle lineage"),
        }
    }
}

impl Error for Spec031LifecycleError {}

pub fn spec031_project_lifecycle(
    input: Spec031LifecycleInput,
) -> Result<Vec<Spec031Envelope>, Spec031LifecycleError> {
    let mut seen_terminal = false;
    let mut runtime_request: Option<Spec031RuntimeControlKind> = None;
    let mut interrupted = false;
    let mut envelopes = Vec::with_capacity(input.facts.len());
    for fact in input.facts.iter().copied() {
        match fact {
            Spec031LifecycleFact::Approval { state } => {
                envelopes.push(lifecycle_envelope::approval(&input, state)?)
            }
            Spec031LifecycleFact::RuntimeControl { kind, state } => {
                if state == Spec031RuntimeControlState::Completed && runtime_request != Some(kind) {
                    return Err(Spec031LifecycleError::StaleLineage);
                }
                if state == Spec031RuntimeControlState::Requested {
                    runtime_request = Some(kind);
                }
                envelopes.push(lifecycle_envelope::runtime_control(&input, state)?);
            }
            Spec031LifecycleFact::Progress { delivery } => {
                envelopes.push(lifecycle_envelope::progress(&input, delivery)?)
            }
            Spec031LifecycleFact::Terminal { outcome } => {
                if seen_terminal {
                    return Err(Spec031LifecycleError::DuplicateTerminal);
                }
                seen_terminal = true;
                envelopes.push(lifecycle_envelope::terminal(&input, outcome)?);
            }
            Spec031LifecycleFact::Recovery { state } => {
                let repeated = state == Spec031RecoveryState::Interrupted && interrupted;
                interrupted |= state == Spec031RecoveryState::Interrupted;
                envelopes.push(lifecycle_envelope::recovery(&input, state, repeated)?);
            }
            Spec031LifecycleFact::PendingFollowUp => {
                envelopes.push(lifecycle_envelope::pending_follow_up(&input)?)
            }
        }
    }
    Ok(envelopes)
}
