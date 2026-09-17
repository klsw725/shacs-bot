use super::{invalid_owner_fact, row, MappedRow, Observation};
use crate::runtime::spec035_tasks_assembler::{
    Spec035LocatedOwnerFact, Spec035TasksAssemblerError,
};
use shacs_projection::{
    Spec031Count, Spec031Freshness, Spec031ObservedAtUnixMs, Spec033AutomationFact,
    Spec033AutomationJobStatus, Spec033GoalFact, Spec033GoalStatus, Spec035ChildTerminal,
    Spec035TaskAction, Spec035TaskActionKind, Spec035TaskActionStatus, Spec035TaskDetail,
    Spec035TaskRow, Spec035TaskState,
};
use shacs_session::durable_child::{ReplayChildTask, ReplayChildTaskState};

pub(in crate::runtime::spec035_tasks_assembler) fn goal_row(
    located: Spec035LocatedOwnerFact<Spec033GoalFact>,
    observed_at: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    let (state, action) = match located.fact.status {
        Spec033GoalStatus::Unavailable => return invalid_owner_fact(),
        Spec033GoalStatus::Active => (
            Spec035TaskState::Running,
            Some(available_action(Spec035TaskActionKind::Pause)),
        ),
        Spec033GoalStatus::Paused => (
            Spec035TaskState::Paused,
            Some(available_action(Spec035TaskActionKind::Resume)),
        ),
        Spec033GoalStatus::Blocked => (
            Spec035TaskState::Blocked,
            Some(available_action(Spec035TaskActionKind::Resume)),
        ),
        Spec033GoalStatus::Done => (Spec035TaskState::Done, None),
        Spec033GoalStatus::Cleared => (Spec035TaskState::Cancelled, None),
    };
    row(
        located.locator,
        Observation {
            observed_at,
            freshness,
        },
        MappedRow {
            state,
            detail: Spec035TaskDetail::Goal { fact: located.fact },
            action,
        },
    )
}

const fn available_action(kind: Spec035TaskActionKind) -> Spec035TaskAction {
    Spec035TaskAction {
        kind,
        status: Spec035TaskActionStatus::Available,
    }
}

pub(in crate::runtime::spec035_tasks_assembler) fn automation_row(
    located: Spec035LocatedOwnerFact<Spec033AutomationFact>,
    observed_at: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    let state = match located.fact.job_status {
        Spec033AutomationJobStatus::Pending => Spec035TaskState::Pending,
        Spec033AutomationJobStatus::Succeeded => Spec035TaskState::Done,
        Spec033AutomationJobStatus::Failed => Spec035TaskState::Failed,
        Spec033AutomationJobStatus::TimedOut => Spec035TaskState::TimedOut,
        Spec033AutomationJobStatus::Cancelled => Spec035TaskState::Cancelled,
        Spec033AutomationJobStatus::Suppressed => Spec035TaskState::Suppressed,
    };
    row(
        located.locator,
        Observation {
            observed_at,
            freshness,
        },
        MappedRow {
            state,
            detail: Spec035TaskDetail::Automation {
                job_status: located.fact.job_status,
                delivery_status: located.fact.delivery_status,
            },
            action: None,
        },
    )
}

pub(in crate::runtime::spec035_tasks_assembler) fn child_row(
    located: Spec035LocatedOwnerFact<ReplayChildTask>,
    observed_at: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
) -> Result<Spec035TaskRow, Spec035TasksAssemblerError> {
    let (state, terminal) = match located.fact.state {
        ReplayChildTaskState::Spawned => (Spec035TaskState::Pending, None),
        ReplayChildTaskState::Running => (Spec035TaskState::Running, None),
        ReplayChildTaskState::Completed => (
            Spec035TaskState::Done,
            Some(Spec035ChildTerminal::Completed),
        ),
        ReplayChildTaskState::Failed => {
            (Spec035TaskState::Failed, Some(Spec035ChildTerminal::Failed))
        }
        ReplayChildTaskState::TimedOut => (
            Spec035TaskState::TimedOut,
            Some(Spec035ChildTerminal::TimedOut),
        ),
        ReplayChildTaskState::Cancelled => (
            Spec035TaskState::Cancelled,
            Some(Spec035ChildTerminal::Cancelled),
        ),
    };
    let action = located.fact.cancellation_requested_sequence.map(|_| {
        let status = match located.fact.state {
            ReplayChildTaskState::Cancelled => Spec035TaskActionStatus::Completed,
            ReplayChildTaskState::Spawned
            | ReplayChildTaskState::Running
            | ReplayChildTaskState::Completed
            | ReplayChildTaskState::Failed
            | ReplayChildTaskState::TimedOut => Spec035TaskActionStatus::Requested,
        };
        Spec035TaskAction {
            kind: Spec035TaskActionKind::Stop,
            status,
        }
    });
    row(
        located.locator,
        Observation {
            observed_at,
            freshness,
        },
        MappedRow {
            state,
            detail: Spec035TaskDetail::Child {
                attempt: Spec031Count::new(u64::from(located.fact.attempt)),
                terminal,
            },
            action,
        },
    )
}
