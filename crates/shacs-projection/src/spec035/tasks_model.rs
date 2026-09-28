use super::{
    Spec035TaskAction, Spec035TaskActionStatus, Spec035TaskDetail, Spec035TaskOwner,
    Spec035TaskOwnerKind, Spec035TaskState, Spec035TasksCoverage, Spec035TasksParseError,
    Spec035TasksValidationError, Spec035TasksValidationErrorKind, SPEC035_TASKS_ROWS_MAX,
    SPEC035_TASKS_SCHEMA_VERSION,
};
use crate::{Spec031Freshness, Spec031ObservedAtUnixMs};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035TaskRowInput {
    pub owner: Spec035TaskOwner,
    pub observed_at_unix_ms: Spec031ObservedAtUnixMs,
    pub freshness: Spec031Freshness,
    pub state: Spec035TaskState,
    pub detail: Spec035TaskDetail,
    pub action: Option<Spec035TaskAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TaskRow {
    owner: Spec035TaskOwner,
    observed_at_unix_ms: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
    state: Spec035TaskState,
    detail: Spec035TaskDetail,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action: Option<Spec035TaskAction>,
}

impl Spec035TaskRow {
    pub fn try_new(input: Spec035TaskRowInput) -> Result<Self, Spec035TasksValidationError> {
        if input.owner.kind() != input.detail.owner_kind() {
            return Err(Spec035TasksValidationError::new(
                Spec035TasksValidationErrorKind::DetailOwnerMismatch,
            ));
        }
        if matches!(
            input.state,
            Spec035TaskState::Done | Spec035TaskState::Recovered
        ) && input.freshness != Spec031Freshness::Current
        {
            return Err(Spec035TasksValidationError::new(
                Spec035TasksValidationErrorKind::MisleadingSuccess,
            ));
        }
        if matches!(
            input.freshness,
            Spec031Freshness::Unavailable | Spec031Freshness::Unknown
        ) {
            return Err(Spec035TasksValidationError::new(
                Spec035TasksValidationErrorKind::MisleadingSuccess,
            ));
        }
        if input.action.is_some_and(|action| {
            action.status == Spec035TaskActionStatus::Completed
                && !matches!(
                    input.state,
                    Spec035TaskState::Done | Spec035TaskState::Recovered
                )
                && !(input.owner.kind() == Spec035TaskOwnerKind::Child
                    && input.state == Spec035TaskState::Cancelled)
        }) {
            return Err(Spec035TasksValidationError::new(
                Spec035TasksValidationErrorKind::InvalidAction,
            ));
        }
        Ok(Self {
            owner: input.owner,
            observed_at_unix_ms: input.observed_at_unix_ms,
            freshness: input.freshness,
            state: input.state,
            detail: input.detail,
            action: input.action,
        })
    }

    pub const fn owner(&self) -> &Spec035TaskOwner {
        &self.owner
    }

    pub const fn observed_at_unix_ms(&self) -> Spec031ObservedAtUnixMs {
        self.observed_at_unix_ms
    }

    pub const fn freshness(&self) -> Spec031Freshness {
        self.freshness
    }

    pub const fn state(&self) -> Spec035TaskState {
        self.state
    }

    pub const fn detail(&self) -> &Spec035TaskDetail {
        &self.detail
    }

    pub const fn action(&self) -> Option<Spec035TaskAction> {
        self.action
    }

    pub fn to_input(&self) -> Spec035TaskRowInput {
        Spec035TaskRowInput {
            owner: self.owner.clone(),
            observed_at_unix_ms: self.observed_at_unix_ms,
            freshness: self.freshness,
            state: self.state,
            detail: self.detail.clone(),
            action: self.action,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec035TaskRowWire {
    owner: Spec035TaskOwner,
    observed_at_unix_ms: Spec031ObservedAtUnixMs,
    freshness: Spec031Freshness,
    state: Spec035TaskState,
    detail: Spec035TaskDetail,
    #[serde(default)]
    action: Option<Spec035TaskAction>,
}

impl<'de> Deserialize<'de> for Spec035TaskRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Spec035TaskRowWire::deserialize(deserializer)?;
        Self::try_new(Spec035TaskRowInput {
            owner: wire.owner,
            observed_at_unix_ms: wire.observed_at_unix_ms,
            freshness: wire.freshness,
            state: wire.state,
            detail: wire.detail,
            action: wire.action,
        })
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Spec035TasksProjectionKind {
    Tasks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TasksProjection {
    schema_version: u32,
    kind: Spec035TasksProjectionKind,
    rows: Vec<Spec035TaskRow>,
    coverage: Spec035TasksCoverage,
}

impl Spec035TasksProjection {
    pub fn try_new(
        mut rows: Vec<Spec035TaskRow>,
        coverage: Spec035TasksCoverage,
    ) -> Result<Self, Spec035TasksValidationError> {
        if rows.len() > SPEC035_TASKS_ROWS_MAX {
            return Err(Spec035TasksValidationError::new(
                Spec035TasksValidationErrorKind::TooManyRows,
            ));
        }
        rows.sort_by(|left, right| {
            (left.owner.kind(), left.owner.locator())
                .cmp(&(right.owner.kind(), right.owner.locator()))
        });
        if rows.windows(2).any(|pair| pair[0].owner == pair[1].owner) {
            return Err(Spec035TasksValidationError::new(
                Spec035TasksValidationErrorKind::DuplicateOwnerLocator,
            ));
        }
        for kind in Spec035TaskOwnerKind::ALL {
            let actual = rows.iter().filter(|row| row.owner.kind() == kind).count();
            let declared = coverage
                .for_kind(kind)
                .count()
                .and_then(|count| usize::try_from(count.as_u64()).ok());
            if declared != Some(actual) && !(declared.is_none() && actual == 0) {
                return Err(Spec035TasksValidationError::new(
                    Spec035TasksValidationErrorKind::CoverageMismatch,
                ));
            }
        }
        Ok(Self {
            schema_version: SPEC035_TASKS_SCHEMA_VERSION,
            kind: Spec035TasksProjectionKind::Tasks,
            rows,
            coverage,
        })
    }

    pub fn rows(&self) -> &[Spec035TaskRow] {
        &self.rows
    }

    pub const fn coverage(&self) -> &Spec035TasksCoverage {
        &self.coverage
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec035TasksParseError> {
        serde_json::from_str::<Spec035TasksProjectionWire>(input)
            .map_err(Spec035TasksParseError::from_serde)
            .and_then(Self::try_from_wire)
    }

    fn try_from_wire(wire: Spec035TasksProjectionWire) -> Result<Self, Spec035TasksParseError> {
        if wire.schema_version != SPEC035_TASKS_SCHEMA_VERSION
            || wire.kind != Spec035TasksProjectionKind::Tasks
        {
            return Err(Spec035TasksParseError::invalid_schema());
        }
        Self::try_new(wire.rows, wire.coverage).map_err(Spec035TasksParseError::from_validation)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec035TasksProjectionWire {
    schema_version: u32,
    kind: Spec035TasksProjectionKind,
    rows: Vec<Spec035TaskRow>,
    coverage: Spec035TasksCoverage,
}
