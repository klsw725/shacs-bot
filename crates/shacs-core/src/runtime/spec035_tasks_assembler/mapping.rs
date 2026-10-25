pub(super) mod execution;

use super::{
    Spec035LocatedOwnerFact, Spec035RecoveryOwnerFact, Spec035TasksAssemblerError,
    Spec035TasksAssemblerErrorKind,
};
use crate::runtime::SurfaceActionOutcomeKind;
use shacs_projection::{
    Spec031Count, Spec031Freshness, Spec031ObservedAtUnixMs, Spec035RecoveryStatus,
    Spec035TaskAction, Spec035TaskActionKind, Spec035TaskActionStatus, Spec035TaskDetail,
    Spec035TaskOwner, Spec035TaskRow, Spec035TaskRowInput, Spec035TaskState, Spec035WorkflowPhase,
};
use shacs_session::durable_replay::{DurableRecoveryStatus, DurableReplayAdmission};
use shacs_workflow::{WorkflowProjection, WorkflowRunState};

pub(super) fn workflow_row(
    located: Spec035LocatedOwnerFact<WorkflowProjection>,
    observed_at: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    let phase = workflow_phase(located.fact.state);
    let state = workflow_state(located.fact.state);
    let completed_steps = count(located.fact.progress_count)?;
    let active_children = count(located.fact.active_child_count)?;
    row(
        located.locator,
        Observation {
            observed_at,
            freshness,
        },
        MappedRow {
            state,
            detail: Spec035TaskDetail::Workflow {
                phase,
                completed_steps,
                active_children,
            },
            action: None,
        },
    )
}

pub(super) fn recovery_row(
    located: Spec035LocatedOwnerFact<Spec035RecoveryOwnerFact>,
    observed_at: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    let (admission, recovered) = match located.fact {
        Spec035RecoveryOwnerFact::Admission(admission) => (admission, false),
        Spec035RecoveryOwnerFact::Recovered { admission, outcome }
            if outcome.kind == SurfaceActionOutcomeKind::Completed
                && outcome.changed
                && admission.status == DurableRecoveryStatus::Healthy =>
        {
            (admission, true)
        }
        Spec035RecoveryOwnerFact::Recovered { .. } => return invalid_owner_fact(),
    };
    let (state, status, action) = recovery_state(&admission, recovered);
    row(
        located.locator,
        Observation {
            observed_at,
            freshness,
        },
        MappedRow {
            state,
            detail: Spec035TaskDetail::Recovery {
                status,
                issue_count: count(admission.issues.len())?,
            },
            action,
        },
    )
}

#[derive(Clone, Copy)]
pub(super) struct Observation {
    pub(super) observed_at: Spec031ObservedAtUnixMs,
    pub(super) freshness: Spec031Freshness,
}

pub(super) struct MappedRow {
    pub(super) state: Spec035TaskState,
    pub(super) detail: Spec035TaskDetail,
    pub(super) action: Option<Spec035TaskAction>,
}

pub(super) fn row(
    locator: String,
    observation: Observation,
    mapped: MappedRow,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    let owner = Spec035TaskOwner::try_new(mapped.detail.owner_kind(), &locator)?;
    Spec035TaskRow::try_new(Spec035TaskRowInput {
        owner,
        observed_at_unix_ms: observation.observed_at,
        freshness: observation.freshness,
        state: mapped.state,
        detail: mapped.detail,
        action: mapped.action,
    })
    .map_err(Into::into)
}

const fn workflow_phase(state: WorkflowRunState) -> Spec035WorkflowPhase {
    match state {
        WorkflowRunState::Planned => Spec035WorkflowPhase::Planned,
        WorkflowRunState::Admitted => Spec035WorkflowPhase::Admitted,
        WorkflowRunState::Running => Spec035WorkflowPhase::Running,
        WorkflowRunState::WaitingForChildren => Spec035WorkflowPhase::WaitingForChildren,
        WorkflowRunState::Verifying => Spec035WorkflowPhase::Verifying,
        WorkflowRunState::Synthesizing => Spec035WorkflowPhase::Synthesizing,
        WorkflowRunState::WaitingForUser => Spec035WorkflowPhase::WaitingForUser,
        WorkflowRunState::Blocked => Spec035WorkflowPhase::Blocked,
        WorkflowRunState::Completed => Spec035WorkflowPhase::Completed,
        WorkflowRunState::Failed => Spec035WorkflowPhase::Failed,
        WorkflowRunState::Cancelled => Spec035WorkflowPhase::Cancelled,
        WorkflowRunState::Stale => Spec035WorkflowPhase::Stale,
    }
}

const fn workflow_state(state: WorkflowRunState) -> Spec035TaskState {
    match state {
        WorkflowRunState::Planned | WorkflowRunState::Admitted => Spec035TaskState::Pending,
        WorkflowRunState::Running
        | WorkflowRunState::WaitingForChildren
        | WorkflowRunState::Verifying
        | WorkflowRunState::Synthesizing => Spec035TaskState::Running,
        WorkflowRunState::WaitingForUser => Spec035TaskState::Paused,
        WorkflowRunState::Blocked | WorkflowRunState::Stale => Spec035TaskState::Blocked,
        WorkflowRunState::Completed => Spec035TaskState::Done,
        WorkflowRunState::Failed => Spec035TaskState::Failed,
        WorkflowRunState::Cancelled => Spec035TaskState::Cancelled,
    }
}

fn recovery_state(
    admission: &DurableReplayAdmission,
    recovered: bool,
) -> (
    Spec035TaskState,
    Spec035RecoveryStatus,
    Option<Spec035TaskAction>,
) {
    if recovered {
        return (
            Spec035TaskState::Recovered,
            Spec035RecoveryStatus::Recovered,
            Some(Spec035TaskAction {
                kind: Spec035TaskActionKind::Recover,
                status: Spec035TaskActionStatus::Completed,
            }),
        );
    }
    match admission.status {
        DurableRecoveryStatus::Healthy => {
            (Spec035TaskState::Done, Spec035RecoveryStatus::Healthy, None)
        }
        DurableRecoveryStatus::Recoverable => (
            Spec035TaskState::Blocked,
            Spec035RecoveryStatus::Recoverable,
            None,
        ),
        DurableRecoveryStatus::InspectOnly => (
            Spec035TaskState::Blocked,
            Spec035RecoveryStatus::InspectOnly,
            Some(Spec035TaskAction {
                kind: Spec035TaskActionKind::Inspect,
                status: Spec035TaskActionStatus::Available,
            }),
        ),
        DurableRecoveryStatus::Blocked => (
            Spec035TaskState::Blocked,
            Spec035RecoveryStatus::Blocked,
            Some(Spec035TaskAction {
                kind: Spec035TaskActionKind::Inspect,
                status: Spec035TaskActionStatus::Available,
            }),
        ),
    }
}

fn count(value: usize) -> Result<Spec031Count, Spec035TasksAssemblerError> {
    u64::try_from(value)
        .map(Spec031Count::new)
        .map_err(|_| Spec035TasksAssemblerError::new(Spec035TasksAssemblerErrorKind::CountOverflow))
}

pub(super) fn invalid_owner_fact<T>() -> Result<T, Spec035TasksAssemblerError> {
    Err(Spec035TasksAssemblerError::new(
        Spec035TasksAssemblerErrorKind::InvalidOwnerFact,
    ))
}
