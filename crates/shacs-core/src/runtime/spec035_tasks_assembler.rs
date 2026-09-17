mod mapping;

use self::mapping::execution::{automation_row, child_row, goal_row};
use self::mapping::{app_row, recovery_row, workflow_row};
use super::SurfaceActionOutcome;
use shacs_app::app_lifecycle::AppLifecycleReceipt;
use shacs_projection::{
    Spec031Freshness, Spec031ObservedAtUnixMs, Spec033AutomationFact, Spec033GoalFact,
    Spec035TaskCount, Spec035TaskRow, Spec035TasksCoverage, Spec035TasksProjection,
    Spec035TasksValidationError, SPEC035_TASKS_ROWS_MAX,
};
use shacs_session::durable_child::ReplayChildTask;
use shacs_session::durable_replay::DurableReplayAdmission;
use shacs_workflow::WorkflowProjection;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct Spec035LocatedOwnerFact<T> {
    pub locator: String,
    pub fact: T,
}

#[derive(Debug, Clone)]
pub enum Spec035TasksOwnerSource<T> {
    Available {
        observed_at_unix_ms: Spec031ObservedAtUnixMs,
        freshness: Spec031Freshness,
        facts: Vec<Spec035LocatedOwnerFact<T>>,
    },
    Unavailable {
        freshness: Spec031Freshness,
    },
}

#[derive(Debug, Clone)]
pub enum Spec035RecoveryOwnerFact {
    Admission(DurableReplayAdmission),
    Recovered {
        admission: DurableReplayAdmission,
        outcome: SurfaceActionOutcome,
    },
}

#[derive(Debug, Clone)]
pub struct Spec035TasksOwnerSnapshots {
    pub goal: Spec035TasksOwnerSource<Spec033GoalFact>,
    pub child: Spec035TasksOwnerSource<ReplayChildTask>,
    pub workflow: Spec035TasksOwnerSource<WorkflowProjection>,
    pub automation: Spec035TasksOwnerSource<Spec033AutomationFact>,
    pub app: Spec035TasksOwnerSource<AppLifecycleReceipt>,
    pub recovery: Spec035TasksOwnerSource<Spec035RecoveryOwnerFact>,
}

impl Spec035TasksOwnerSnapshots {
    pub fn all_unavailable(freshness: Spec031Freshness) -> Self {
        Self {
            goal: Spec035TasksOwnerSource::Unavailable { freshness },
            child: Spec035TasksOwnerSource::Unavailable { freshness },
            workflow: Spec035TasksOwnerSource::Unavailable { freshness },
            automation: Spec035TasksOwnerSource::Unavailable { freshness },
            app: Spec035TasksOwnerSource::Unavailable { freshness },
            recovery: Spec035TasksOwnerSource::Unavailable { freshness },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035TasksAssemblerErrorKind {
    InvalidSourceFreshness,
    InvalidOwnerFact,
    TooManyOwnerFacts,
    CountOverflow,
    Projection,
}

#[derive(Debug)]
pub struct Spec035TasksAssemblerError {
    kind: Spec035TasksAssemblerErrorKind,
}

impl Spec035TasksAssemblerError {
    pub const fn kind(&self) -> Spec035TasksAssemblerErrorKind {
        self.kind
    }

    pub(super) const fn new(kind: Spec035TasksAssemblerErrorKind) -> Self {
        Self { kind }
    }
}

impl Display for Spec035TasksAssemblerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Spec035 tasks assembly failed: {:?}", self.kind)
    }
}

impl std::error::Error for Spec035TasksAssemblerError {}

impl From<Spec035TasksValidationError> for Spec035TasksAssemblerError {
    fn from(_error: Spec035TasksValidationError) -> Self {
        Self::new(Spec035TasksAssemblerErrorKind::Projection)
    }
}

pub fn assemble_spec035_tasks(
    snapshots: Spec035TasksOwnerSnapshots,
) -> Result<Spec035TasksProjection, Spec035TasksAssemblerError> {
    let lengths = [
        source_len(&snapshots.goal)?,
        source_len(&snapshots.child)?,
        source_len(&snapshots.workflow)?,
        source_len(&snapshots.automation)?,
        source_len(&snapshots.app)?,
        source_len(&snapshots.recovery)?,
    ];
    let total = lengths.into_iter().try_fold(0_usize, |total, length| {
        total.checked_add(length).ok_or_else(|| {
            Spec035TasksAssemblerError::new(Spec035TasksAssemblerErrorKind::TooManyOwnerFacts)
        })
    })?;
    if total > SPEC035_TASKS_ROWS_MAX {
        return Err(Spec035TasksAssemblerError::new(
            Spec035TasksAssemblerErrorKind::TooManyOwnerFacts,
        ));
    }

    let coverage = Spec035TasksCoverage {
        goal: source_count(&snapshots.goal)?,
        child: source_count(&snapshots.child)?,
        workflow: source_count(&snapshots.workflow)?,
        automation: source_count(&snapshots.automation)?,
        app: source_count(&snapshots.app)?,
        recovery: source_count(&snapshots.recovery)?,
    };
    let mut rows = Vec::with_capacity(total);
    append_rows(&mut rows, snapshots.goal, goal_row)?;
    append_rows(&mut rows, snapshots.child, child_row)?;
    append_rows(&mut rows, snapshots.workflow, workflow_row)?;
    append_rows(&mut rows, snapshots.automation, automation_row)?;
    append_rows(&mut rows, snapshots.app, app_row)?;
    append_rows(&mut rows, snapshots.recovery, recovery_row)?;
    Spec035TasksProjection::try_new(rows, coverage).map_err(Into::into)
}

fn source_len<T>(source: &Spec035TasksOwnerSource<T>) -> Result<usize, Spec035TasksAssemblerError> {
    match source {
        Spec035TasksOwnerSource::Available {
            freshness, facts, ..
        } => {
            require_available_freshness(*freshness)?;
            Ok(facts.len())
        }
        Spec035TasksOwnerSource::Unavailable { freshness } => {
            require_unavailable_freshness(*freshness)?;
            Ok(0)
        }
    }
}

fn source_count<T>(
    source: &Spec035TasksOwnerSource<T>,
) -> Result<Spec035TaskCount, Spec035TasksAssemblerError> {
    match source {
        Spec035TasksOwnerSource::Available { facts, .. } => Ok(Spec035TaskCount::available(
            u64::try_from(facts.len()).map_err(|_| {
                Spec035TasksAssemblerError::new(Spec035TasksAssemblerErrorKind::CountOverflow)
            })?,
        )),
        Spec035TasksOwnerSource::Unavailable { .. } => Ok(Spec035TaskCount::Unavailable),
    }
}

fn append_rows<T>(
    rows: &mut Vec<Spec035TaskRow>,
    source: Spec035TasksOwnerSource<T>,
    map: fn(
        Spec035LocatedOwnerFact<T>,
        Spec031ObservedAtUnixMs,
        Spec031Freshness,
    ) -> Result<Spec035TaskRow, Spec035TasksAssemblerError>,
) -> Result<(), Spec035TasksAssemblerError> {
    if let Spec035TasksOwnerSource::Available {
        observed_at_unix_ms,
        freshness,
        facts,
    } = source
    {
        for fact in facts {
            rows.push(map(fact, observed_at_unix_ms, freshness)?);
        }
    }
    Ok(())
}

fn require_available_freshness(
    freshness: Spec031Freshness,
) -> Result<(), Spec035TasksAssemblerError> {
    match freshness {
        Spec031Freshness::Current | Spec031Freshness::Stale => Ok(()),
        Spec031Freshness::Unavailable | Spec031Freshness::Unknown => Err(
            Spec035TasksAssemblerError::new(Spec035TasksAssemblerErrorKind::InvalidSourceFreshness),
        ),
    }
}

fn require_unavailable_freshness(
    freshness: Spec031Freshness,
) -> Result<(), Spec035TasksAssemblerError> {
    match freshness {
        Spec031Freshness::Unavailable | Spec031Freshness::Unknown => Ok(()),
        Spec031Freshness::Current | Spec031Freshness::Stale => Err(
            Spec035TasksAssemblerError::new(Spec035TasksAssemblerErrorKind::InvalidSourceFreshness),
        ),
    }
}
