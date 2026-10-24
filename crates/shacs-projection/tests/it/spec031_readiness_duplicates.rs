use shacs_projection::*;
use std::error::Error;

const NOW: Spec031ObservedAtUnixMs = Spec031ObservedAtUnixMs::new(31);

fn observation(
    requirement: Spec031ReadinessRequirement,
    state: Spec031Availability,
    summary: &str,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    Ok(Spec031ReadinessObservation {
        kind: Spec031ReadinessComponentKind::Storage,
        requirement,
        state,
        freshness: Spec031Freshness::Current,
        reason_code: reason_for(state),
        safe_summary: Spec031SafeSummary::try_new(summary)?,
        observed_at_unix_ms: Some(NOW),
        queue_depth: None,
        queue_capacity: None,
    })
}

#[test]
fn spec031_readiness_rejects_duplicate_components_order_independently() -> Result<(), Box<dyn Error>>
{
    for duplicate_case in [
        [
            observation(
                Spec031ReadinessRequirement::Required,
                Spec031Availability::Ready,
                "required one",
            )?,
            observation(
                Spec031ReadinessRequirement::Required,
                Spec031Availability::Blocked,
                "required two",
            )?,
        ],
        [
            observation(
                Spec031ReadinessRequirement::Optional,
                Spec031Availability::Unavailable,
                "optional one",
            )?,
            observation(
                Spec031ReadinessRequirement::Optional,
                Spec031Availability::Unknown,
                "optional two",
            )?,
        ],
        [
            observation(
                Spec031ReadinessRequirement::Required,
                Spec031Availability::Ready,
                "required",
            )?,
            observation(
                Spec031ReadinessRequirement::Optional,
                Spec031Availability::Degraded,
                "optional",
            )?,
        ],
    ] {
        for observations in [
            duplicate_case.clone(),
            [duplicate_case[1].clone(), duplicate_case[0].clone()],
        ] {
            assert_eq!(
                spec031_aggregate_readiness(&observations),
                Err(Spec031ReadinessAggregationError::DuplicateComponent {
                    kind: Spec031ReadinessComponentKind::Storage,
                })
            );
        }
    }

    Ok(())
}

const fn reason_for(state: Spec031Availability) -> Spec031ReasonCode {
    match state {
        Spec031Availability::Ready => Spec031ReasonCode::Included,
        Spec031Availability::Degraded => Spec031ReasonCode::Degraded,
        Spec031Availability::Blocked => Spec031ReasonCode::Blocked,
        Spec031Availability::Unavailable | Spec031Availability::Unknown => {
            Spec031ReasonCode::Missing
        }
    }
}
