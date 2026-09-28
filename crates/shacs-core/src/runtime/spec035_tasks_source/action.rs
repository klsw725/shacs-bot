use super::super::{Spec035TasksAssemblerError, SurfaceActionOutcome, SurfaceActionOutcomeKind};
use shacs_app::app::AppId;
use shacs_projection::{
    Spec035TaskActionKind, Spec035TaskActionStatus, Spec035TaskOwner, Spec035TaskOwnerKind,
    Spec035TasksProjection, Spec035TransportCapability,
};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec035TasksSemanticAction {
    GoalPause { locator: String },
    GoalResume { locator: String },
    AppStop { app_id: AppId },
    AppRecover { app_id: AppId },
    RuntimeRecover,
}

impl Spec035TasksSemanticAction {
    pub fn parse(
        owner: &str,
        action: &str,
        locator: &str,
    ) -> Result<Self, Spec035TasksSourceError> {
        let owner_kind = match owner {
            "goal" => Spec035TaskOwnerKind::Goal,
            "app" => Spec035TaskOwnerKind::App,
            "recovery" => Spec035TaskOwnerKind::Recovery,
            "child" | "workflow" | "automation" => {
                return Err(Spec035TasksSourceError::UnsupportedAction)
            }
            _ => return Err(Spec035TasksSourceError::InvalidActionRequest),
        };
        Spec035TaskOwner::try_new(owner_kind, locator)
            .map_err(|_| Spec035TasksSourceError::InvalidActionRequest)?;
        match (owner_kind, action) {
            (Spec035TaskOwnerKind::Goal, "pause") => Ok(Self::GoalPause {
                locator: locator.to_owned(),
            }),
            (Spec035TaskOwnerKind::Goal, "resume") => Ok(Self::GoalResume {
                locator: locator.to_owned(),
            }),
            (Spec035TaskOwnerKind::App, "stop") => Ok(Self::AppStop {
                app_id: AppId::parse(locator.to_owned())
                    .map_err(|_| Spec035TasksSourceError::InvalidActionRequest)?,
            }),
            (Spec035TaskOwnerKind::App, "recover") => Ok(Self::AppRecover {
                app_id: AppId::parse(locator.to_owned())
                    .map_err(|_| Spec035TasksSourceError::InvalidActionRequest)?,
            }),
            (Spec035TaskOwnerKind::Recovery, "recover") if locator == "runtime:recovery" => {
                Ok(Self::RuntimeRecover)
            }
            (Spec035TaskOwnerKind::Goal, _)
            | (Spec035TaskOwnerKind::App, _)
            | (Spec035TaskOwnerKind::Recovery, _)
            | (Spec035TaskOwnerKind::Child, _)
            | (Spec035TaskOwnerKind::Workflow, _)
            | (Spec035TaskOwnerKind::Automation, _) => {
                Err(Spec035TasksSourceError::UnsupportedAction)
            }
        }
    }

    pub const fn owner_kind(&self) -> Spec035TaskOwnerKind {
        match self {
            Self::GoalPause { .. } | Self::GoalResume { .. } => Spec035TaskOwnerKind::Goal,
            Self::AppStop { .. } | Self::AppRecover { .. } => Spec035TaskOwnerKind::App,
            Self::RuntimeRecover => Spec035TaskOwnerKind::Recovery,
        }
    }

    pub fn locator(&self) -> &str {
        match self {
            Self::GoalPause { locator } | Self::GoalResume { locator } => locator,
            Self::AppStop { app_id } | Self::AppRecover { app_id } => app_id.as_str(),
            Self::RuntimeRecover => "runtime:recovery",
        }
    }

    pub const fn kind(&self) -> Spec035TaskActionKind {
        match self {
            Self::GoalPause { .. } => Spec035TaskActionKind::Pause,
            Self::GoalResume { .. } => Spec035TaskActionKind::Resume,
            Self::AppStop { .. } => Spec035TaskActionKind::Stop,
            Self::AppRecover { .. } | Self::RuntimeRecover => Spec035TaskActionKind::Recover,
        }
    }

    pub const fn transport_capability(&self) -> Spec035TransportCapability {
        match self {
            Self::GoalPause { .. } => Spec035TransportCapability::TaskPause,
            Self::GoalResume { .. } => Spec035TransportCapability::TaskResume,
            Self::AppStop { .. } => Spec035TransportCapability::TaskStop,
            Self::AppRecover { .. } | Self::RuntimeRecover => {
                Spec035TransportCapability::TaskRecover
            }
        }
    }
}

pub fn validate_spec035_task_action(
    projection: &Spec035TasksProjection,
    action: &Spec035TasksSemanticAction,
) -> Result<(), Spec035TasksSourceError> {
    let row = projection
        .rows()
        .iter()
        .find(|row| {
            row.owner().kind() == action.owner_kind()
                && row.owner().locator().as_str() == action.locator()
        })
        .ok_or(Spec035TasksSourceError::OwnerLocatorNotFound)?;
    let advertised = row.action().is_some_and(|advertised| {
        advertised.kind == action.kind() && advertised.status == Spec035TaskActionStatus::Available
    });
    if !advertised {
        return Err(Spec035TasksSourceError::ActionNotAdvertised);
    }
    Ok(())
}

pub fn accept_spec035_surface_action_outcome(
    outcome: SurfaceActionOutcome,
) -> Result<(), Spec035TasksSourceError> {
    match outcome.kind {
        SurfaceActionOutcomeKind::Requested | SurfaceActionOutcomeKind::Completed => Ok(()),
        SurfaceActionOutcomeKind::Unavailable => {
            Err(Spec035TasksSourceError::OwnerActionUnavailable)
        }
        SurfaceActionOutcomeKind::StaleLineage => {
            Err(Spec035TasksSourceError::OwnerActionStaleLineage)
        }
    }
}

#[derive(Debug)]
pub enum Spec035TasksSourceError {
    InvalidActionRequest,
    UnsupportedAction,
    OwnerLocatorNotFound,
    ActionNotAdvertised,
    OwnerActionUnavailable,
    OwnerActionStaleLineage,
    Owner(String),
    Assembly(Spec035TasksAssemblerError),
}

impl Display for Spec035TasksSourceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidActionRequest => formatter.write_str("invalid tasks action request"),
            Self::UnsupportedAction => formatter.write_str("unsupported tasks owner/action pair"),
            Self::OwnerLocatorNotFound => formatter.write_str("tasks owner locator was not found"),
            Self::ActionNotAdvertised => formatter.write_str("tasks action is not advertised"),
            Self::OwnerActionUnavailable => {
                formatter.write_str("tasks owner action is unavailable")
            }
            Self::OwnerActionStaleLineage => {
                formatter.write_str("tasks owner action lineage is stale")
            }
            Self::Owner(error) => write!(formatter, "tasks owner source failed: {error}"),
            Self::Assembly(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for Spec035TasksSourceError {}

impl From<Spec035TasksAssemblerError> for Spec035TasksSourceError {
    fn from(error: Spec035TasksAssemblerError) -> Self {
        Self::Assembly(error)
    }
}
