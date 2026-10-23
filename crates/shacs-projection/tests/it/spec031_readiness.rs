use shacs_projection::*;
use std::error::Error;

const NOW: Spec031ObservedAtUnixMs = Spec031ObservedAtUnixMs::new(31);

fn required(
    kind: Spec031ReadinessComponentKind,
    state: Spec031Availability,
    freshness: Spec031Freshness,
    reason_code: Spec031ReasonCode,
    summary: &str,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    Ok(Spec031ReadinessObservation {
        kind,
        requirement: Spec031ReadinessRequirement::Required,
        state,
        freshness,
        reason_code,
        safe_summary: Spec031SafeSummary::try_new(summary)?,
        observed_at_unix_ms: Some(NOW),
        queue_depth: None,
        queue_capacity: None,
    })
}

fn optional(
    kind: Spec031ReadinessComponentKind,
    state: Spec031Availability,
    freshness: Spec031Freshness,
    reason_code: Spec031ReasonCode,
    summary: &str,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    Ok(Spec031ReadinessObservation {
        kind,
        requirement: Spec031ReadinessRequirement::Optional,
        state,
        freshness,
        reason_code,
        safe_summary: Spec031SafeSummary::try_new(summary)?,
        observed_at_unix_ms: Some(NOW),
        queue_depth: None,
        queue_capacity: None,
    })
}

fn ready_required_components() -> Result<Vec<Spec031ReadinessObservation>, Spec031ConstructionError>
{
    Spec031ReadinessComponentKind::REQUIRED
        .into_iter()
        .map(|kind| {
            required(
                kind,
                Spec031Availability::Ready,
                Spec031Freshness::Current,
                Spec031ReasonCode::Included,
                "component ready",
            )
        })
        .collect()
}

fn aggregate_state(
    observations: &[Spec031ReadinessObservation],
) -> Result<Spec031Availability, Spec031ReadinessAggregationError> {
    Ok(spec031_aggregate_readiness(observations)?
        .envelope()
        .state())
}

#[test]
fn spec031_readiness_aggregates_required_state_combinations() -> Result<(), Box<dyn Error>> {
    for (name, override_observation, expected) in [
        (
            "ready",
            required(
                Spec031ReadinessComponentKind::Storage,
                Spec031Availability::Ready,
                Spec031Freshness::Current,
                Spec031ReasonCode::Included,
                "storage ready",
            )?,
            Spec031Availability::Ready,
        ),
        (
            "degraded",
            required(
                Spec031ReadinessComponentKind::Storage,
                Spec031Availability::Degraded,
                Spec031Freshness::Current,
                Spec031ReasonCode::Degraded,
                "storage degraded",
            )?,
            Spec031Availability::Degraded,
        ),
        (
            "blocked",
            required(
                Spec031ReadinessComponentKind::Storage,
                Spec031Availability::Blocked,
                Spec031Freshness::Current,
                Spec031ReasonCode::Blocked,
                "storage blocked",
            )?,
            Spec031Availability::Blocked,
        ),
    ] {
        let mut observations = ready_required_components()?;
        observations.retain(|observation| observation.kind != override_observation.kind);
        observations.push(override_observation);

        assert_eq!(aggregate_state(&observations)?, expected, "{name}");
    }

    Ok(())
}

#[test]
fn spec031_readiness_handles_missing_stale_and_optional_unavailable() -> Result<(), Box<dyn Error>>
{
    let mut missing_required = ready_required_components()?;
    missing_required
        .retain(|observation| observation.kind != Spec031ReadinessComponentKind::ProviderAuth);
    assert_eq!(
        aggregate_state(&missing_required)?,
        Spec031Availability::Unknown
    );

    let mut stale_ready = ready_required_components()?;
    stale_ready
        .retain(|observation| observation.kind != Spec031ReadinessComponentKind::ProviderAuth);
    stale_ready.push(required(
        Spec031ReadinessComponentKind::ProviderAuth,
        Spec031Availability::Ready,
        Spec031Freshness::Stale,
        Spec031ReasonCode::Included,
        "provider stale ready",
    )?);
    let stale_report = spec031_aggregate_readiness(&stale_ready)?;
    assert_eq!(
        stale_report.envelope().state(),
        Spec031Availability::Degraded
    );
    assert!(stale_report.components().iter().any(|component| {
        component.kind == Spec031ReadinessComponentKind::ProviderAuth
            && component.state == Spec031Availability::Degraded
    }));

    let mut optional_unavailable = ready_required_components()?;
    optional_unavailable.push(optional(
        Spec031ReadinessComponentKind::ExternalIntegration,
        Spec031Availability::Unavailable,
        Spec031Freshness::Unavailable,
        Spec031ReasonCode::Unsupported,
        "plugin disabled",
    )?);
    let optional_report = spec031_aggregate_readiness(&optional_unavailable)?;
    assert_eq!(
        optional_report.envelope().state(),
        Spec031Availability::Ready
    );
    assert!(optional_report.components().iter().any(|component| {
        component.requirement == Spec031ReadinessRequirement::Optional
            && component.state == Spec031Availability::Unavailable
    }));

    Ok(())
}
