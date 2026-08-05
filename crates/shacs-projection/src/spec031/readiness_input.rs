use super::{
    Spec031Availability, Spec031ConstructionError, Spec031Freshness, Spec031ReadinessComponentKind,
    Spec031ReadinessObservation, Spec031ReadinessRequirement, Spec031ReasonCode,
    Spec031SafeSummary,
};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec031ReadinessAggregationError {
    DuplicateComponent { kind: Spec031ReadinessComponentKind },
    Construction(Spec031ConstructionError),
}

impl fmt::Display for Spec031ReadinessAggregationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateComponent { kind } => {
                write!(formatter, "duplicate readiness component: {kind:?}")
            }
            Self::Construction(error) => error.fmt(formatter),
        }
    }
}

impl Error for Spec031ReadinessAggregationError {}

impl From<Spec031ConstructionError> for Spec031ReadinessAggregationError {
    fn from(error: Spec031ConstructionError) -> Self {
        Self::Construction(error)
    }
}

pub(super) fn normalized_components(
    observations: &[Spec031ReadinessObservation],
) -> Result<Vec<Spec031ReadinessObservation>, Spec031ReadinessAggregationError> {
    reject_duplicate_input(observations)?;
    let mut components = observations
        .iter()
        .cloned()
        .map(normalized_observation)
        .collect::<Vec<_>>();
    for kind in Spec031ReadinessComponentKind::REQUIRED {
        if !components.iter().any(|component| {
            component.kind == kind && component.requirement == Spec031ReadinessRequirement::Required
        }) {
            components.push(missing_required_observation(kind)?);
        }
    }
    components.sort_by_key(|component| (component.kind, component.requirement));
    Ok(components)
}

fn reject_duplicate_input(
    observations: &[Spec031ReadinessObservation],
) -> Result<(), Spec031ReadinessAggregationError> {
    for (index, observation) in observations.iter().enumerate() {
        if observations
            .iter()
            .skip(index + 1)
            .any(|candidate| candidate.kind == observation.kind)
        {
            return Err(Spec031ReadinessAggregationError::DuplicateComponent {
                kind: observation.kind,
            });
        }
    }
    Ok(())
}

fn normalized_observation(
    mut observation: Spec031ReadinessObservation,
) -> Spec031ReadinessObservation {
    if observation.state == Spec031Availability::Ready
        && observation.freshness != Spec031Freshness::Current
    {
        observation.state = Spec031Availability::Degraded;
        observation.reason_code = Spec031ReasonCode::Degraded;
    }
    observation
}

fn missing_required_observation(
    kind: Spec031ReadinessComponentKind,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    Ok(Spec031ReadinessObservation {
        kind,
        requirement: Spec031ReadinessRequirement::Required,
        state: Spec031Availability::Unavailable,
        freshness: Spec031Freshness::Unavailable,
        reason_code: Spec031ReasonCode::Missing,
        safe_summary: Spec031SafeSummary::try_new("required readiness observation is missing")?,
        observed_at_unix_ms: None,
        queue_depth: None,
        queue_capacity: None,
    })
}
