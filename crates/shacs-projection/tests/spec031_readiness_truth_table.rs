use shacs_projection::*;
use std::error::Error;

const NOW: Spec031ObservedAtUnixMs = Spec031ObservedAtUnixMs::new(31);

fn observation(
    kind: Spec031ReadinessComponentKind,
    requirement: Spec031ReadinessRequirement,
    state: Spec031Availability,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    Ok(Spec031ReadinessObservation {
        kind,
        requirement,
        state,
        freshness: Spec031Freshness::Current,
        reason_code: reason_for(state),
        safe_summary: Spec031SafeSummary::try_new("readiness truth table state")?,
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
            observation(
                kind,
                Spec031ReadinessRequirement::Required,
                Spec031Availability::Ready,
            )
        })
        .collect()
}

#[test]
fn spec031_readiness_truth_table_is_explicit_for_required_and_optional(
) -> Result<(), Box<dyn Error>> {
    for (name, required_state, optional_state, expected) in [
        (
            "all_ready",
            Spec031Availability::Ready,
            None,
            Spec031Availability::Ready,
        ),
        (
            "required_degraded",
            Spec031Availability::Degraded,
            None,
            Spec031Availability::Degraded,
        ),
        (
            "required_blocked",
            Spec031Availability::Blocked,
            Some(Spec031Availability::Degraded),
            Spec031Availability::Blocked,
        ),
        (
            "required_unknown_outranks_degraded",
            Spec031Availability::Unknown,
            Some(Spec031Availability::Degraded),
            Spec031Availability::Unknown,
        ),
        (
            "required_unavailable_outranks_degraded",
            Spec031Availability::Unavailable,
            Some(Spec031Availability::Degraded),
            Spec031Availability::Unknown,
        ),
        (
            "optional_degraded_lowers_to_degraded",
            Spec031Availability::Ready,
            Some(Spec031Availability::Degraded),
            Spec031Availability::Degraded,
        ),
        (
            "optional_blocked_lowers_to_degraded",
            Spec031Availability::Ready,
            Some(Spec031Availability::Blocked),
            Spec031Availability::Degraded,
        ),
        (
            "optional_unknown_stays_ready",
            Spec031Availability::Ready,
            Some(Spec031Availability::Unknown),
            Spec031Availability::Ready,
        ),
        (
            "optional_unavailable_stays_ready",
            Spec031Availability::Ready,
            Some(Spec031Availability::Unavailable),
            Spec031Availability::Ready,
        ),
    ] {
        let mut observations = ready_required_components()?;
        observations.retain(|item| item.kind != Spec031ReadinessComponentKind::Storage);
        observations.push(observation(
            Spec031ReadinessComponentKind::Storage,
            Spec031ReadinessRequirement::Required,
            required_state,
        )?);
        if let Some(state) = optional_state {
            observations.push(observation(
                Spec031ReadinessComponentKind::ExternalIntegration,
                Spec031ReadinessRequirement::Optional,
                state,
            )?);
        }

        assert_eq!(
            spec031_aggregate_readiness(&observations)?
                .envelope()
                .state(),
            expected,
            "{name}"
        );
    }

    Ok(())
}

#[test]
fn spec031_readiness_normalized_output_contains_each_required_kind_once(
) -> Result<(), Box<dyn Error>> {
    let mut observations = ready_required_components()?;
    observations.retain(|item| item.kind != Spec031ReadinessComponentKind::ProviderAuth);
    observations.push(observation(
        Spec031ReadinessComponentKind::ExternalIntegration,
        Spec031ReadinessRequirement::Optional,
        Spec031Availability::Unavailable,
    )?);
    let report = spec031_aggregate_readiness(&observations)?;

    for kind in Spec031ReadinessComponentKind::REQUIRED {
        assert_eq!(
            report
                .components()
                .iter()
                .filter(|component| component.kind == kind
                    && component.requirement == Spec031ReadinessRequirement::Required)
                .count(),
            1,
            "{kind:?}"
        );
    }
    let synthesized = report
        .components()
        .iter()
        .find(|component| component.kind == Spec031ReadinessComponentKind::ProviderAuth)
        .ok_or("missing synthesized provider auth")?;
    assert_eq!(synthesized.state, Spec031Availability::Unavailable);
    assert_eq!(synthesized.queue_depth, None);
    assert_eq!(synthesized.queue_capacity, None);

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
