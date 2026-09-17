use super::{Spec035TasksValidationError, Spec035TasksValidationErrorKind};
use crate::{
    Spec031Count, Spec031SubjectRef, Spec033AutomationJobStatus, Spec033DeliveryStatus,
    Spec033GoalFact,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TaskOwnerKind {
    Goal,
    Child,
    Workflow,
    Automation,
    App,
    Recovery,
}

impl Spec035TaskOwnerKind {
    pub const ALL: [Self; 6] = [
        Self::Goal,
        Self::Child,
        Self::Workflow,
        Self::Automation,
        Self::App,
        Self::Recovery,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TaskOwner {
    kind: Spec035TaskOwnerKind,
    locator: Spec031SubjectRef,
}

impl Spec035TaskOwner {
    pub fn try_new(
        kind: Spec035TaskOwnerKind,
        locator: &str,
    ) -> Result<Self, Spec035TasksValidationError> {
        let locator = Spec031SubjectRef::try_new(locator).map_err(|_| {
            Spec035TasksValidationError::new(Spec035TasksValidationErrorKind::UnsafeLocator)
        })?;
        Ok(Self { kind, locator })
    }

    pub const fn kind(&self) -> Spec035TaskOwnerKind {
        self.kind
    }

    pub const fn locator(&self) -> &Spec031SubjectRef {
        &self.locator
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TaskState {
    Unavailable,
    Unknown,
    Pending,
    Requested,
    Running,
    Paused,
    Blocked,
    Done,
    Failed,
    TimedOut,
    Cancelled,
    Suppressed,
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TaskActionKind {
    Pause,
    Resume,
    Stop,
    Retry,
    Recover,
    Inspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TaskActionStatus {
    Available,
    Requested,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TaskAction {
    pub kind: Spec035TaskActionKind,
    pub status: Spec035TaskActionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035ChildTerminal {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035AppProcessState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
    RecoveryNeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035RecoveryStatus {
    Healthy,
    Recoverable,
    InspectOnly,
    Blocked,
    Recovered,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035WorkflowPhase {
    Planned,
    Admitted,
    Running,
    WaitingForChildren,
    Verifying,
    Synthesizing,
    WaitingForUser,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spec035TaskDetail {
    Goal {
        fact: Spec033GoalFact,
    },
    Child {
        attempt: Spec031Count,
        terminal: Option<Spec035ChildTerminal>,
    },
    Workflow {
        phase: Spec035WorkflowPhase,
        completed_steps: Spec031Count,
        active_children: Spec031Count,
    },
    Automation {
        job_status: Spec033AutomationJobStatus,
        delivery_status: Spec033DeliveryStatus,
    },
    App {
        process_state: Spec035AppProcessState,
    },
    Recovery {
        status: Spec035RecoveryStatus,
        issue_count: Spec031Count,
    },
}

impl Spec035TaskDetail {
    pub const fn owner_kind(&self) -> Spec035TaskOwnerKind {
        match self {
            Self::Goal { .. } => Spec035TaskOwnerKind::Goal,
            Self::Child { .. } => Spec035TaskOwnerKind::Child,
            Self::Workflow { .. } => Spec035TaskOwnerKind::Workflow,
            Self::Automation { .. } => Spec035TaskOwnerKind::Automation,
            Self::App { .. } => Spec035TaskOwnerKind::App,
            Self::Recovery { .. } => Spec035TaskOwnerKind::Recovery,
        }
    }
}
