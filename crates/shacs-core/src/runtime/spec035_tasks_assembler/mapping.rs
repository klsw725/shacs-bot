pub(super) mod execution;

use super::{
    Spec035LocatedOwnerFact, Spec035RecoveryOwnerFact, Spec035TasksAssemblerError,
    Spec035TasksAssemblerErrorKind,
};
use crate::runtime::SurfaceActionOutcomeKind;
use shacs_app::app_lifecycle::{AppLifecycleAction, AppLifecycleReceipt, AppProcessState};
use shacs_projection::{
    Spec031Count, Spec031Freshness, Spec031ObservedAtUnixMs, Spec035AppProcessState,
    Spec035RecoveryStatus, Spec035TaskAction, Spec035TaskActionKind, Spec035TaskActionStatus,
    Spec035TaskDetail, Spec035TaskOwner, Spec035TaskRow, Spec035TaskRowInput, Spec035TaskState,
    Spec035WorkflowPhase,
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

pub(super) fn app_row(
    located: Spec035LocatedOwnerFact<AppLifecycleReceipt>,
    observed_at: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    if located.fact.current_state == AppProcessState::Stopped && !located.fact.completed {
        return invalid_owner_fact();
    }
    let (state, process_state) = app_state(&located.fact);
    let action = app_action(&located.fact, state);
    row(
        located.locator,
        Observation {
            observed_at,
            freshness,
        },
        MappedRow {
            state,
            detail: Spec035TaskDetail::App { process_state },
            action,
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

fn app_state(receipt: &AppLifecycleReceipt) -> (Spec035TaskState, Spec035AppProcessState) {
    let process = match receipt.current_state {
        AppProcessState::Installed | AppProcessState::Stopped => Spec035AppProcessState::Stopped,
        AppProcessState::Starting => Spec035AppProcessState::Starting,
        AppProcessState::Running => Spec035AppProcessState::Running,
        AppProcessState::Stopping => Spec035AppProcessState::Stopping,
        AppProcessState::Failed => Spec035AppProcessState::Failed,
        AppProcessState::RecoveryNeeded => Spec035AppProcessState::RecoveryNeeded,
    };
    let state = match receipt.current_state {
        AppProcessState::Installed => Spec035TaskState::Pending,
        AppProcessState::Starting | AppProcessState::Running => Spec035TaskState::Running,
        AppProcessState::Stopping => Spec035TaskState::Requested,
        AppProcessState::Stopped
            if receipt.action == AppLifecycleAction::Recover && receipt.completed =>
        {
            Spec035TaskState::Recovered
        }
        AppProcessState::Stopped => Spec035TaskState::Done,
        AppProcessState::Failed => Spec035TaskState::Failed,
        AppProcessState::RecoveryNeeded => Spec035TaskState::Blocked,
    };
    (state, process)
}

fn app_action(receipt: &AppLifecycleReceipt, state: Spec035TaskState) -> Option<Spec035TaskAction> {
    let (kind, status) = match (receipt.completed, receipt.action, receipt.current_state) {
        (false, AppLifecycleAction::Stop, _) => (
            Spec035TaskActionKind::Stop,
            Spec035TaskActionStatus::Requested,
        ),
        (false, AppLifecycleAction::Recover, _) => (
            Spec035TaskActionKind::Recover,
            Spec035TaskActionStatus::Requested,
        ),
        (_, _, AppProcessState::Running) => (
            Spec035TaskActionKind::Stop,
            Spec035TaskActionStatus::Available,
        ),
        (_, _, AppProcessState::RecoveryNeeded) => (
            Spec035TaskActionKind::Recover,
            Spec035TaskActionStatus::Available,
        ),
        (true, AppLifecycleAction::Stop, _) if state == Spec035TaskState::Done => (
            Spec035TaskActionKind::Stop,
            Spec035TaskActionStatus::Completed,
        ),
        (true, AppLifecycleAction::Recover, _) if state == Spec035TaskState::Recovered => (
            Spec035TaskActionKind::Recover,
            Spec035TaskActionStatus::Completed,
        ),
        _ => return None,
    };
    Some(Spec035TaskAction { kind, status })
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
