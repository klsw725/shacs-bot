use shacs_projection::{
    Spec031Availability, Spec031Freshness, Spec031ReadinessComponentKind,
    Spec031ReadinessObservation, Spec031ReadinessRequirement, Spec031ReasonCode,
    Spec031SafeSummary,
};

pub(super) fn observation(
    kind: Spec031ReadinessComponentKind,
    state: Spec031Availability,
    code: Spec031ReasonCode,
    summary: &str,
) -> Result<Spec031ReadinessObservation, String> {
    Ok(Spec031ReadinessObservation {
        kind,
        requirement: Spec031ReadinessRequirement::Required,
        state,
        freshness: freshness(state),
        reason_code: code,
        safe_summary: Spec031SafeSummary::try_new(summary).map_err(|error| error.to_string())?,
        observed_at_unix_ms: None,
        queue_depth: None,
        queue_capacity: None,
    })
}

fn freshness(state: Spec031Availability) -> Spec031Freshness {
    match state {
        Spec031Availability::Ready
        | Spec031Availability::Degraded
        | Spec031Availability::Blocked => Spec031Freshness::Current,
        Spec031Availability::Unavailable => Spec031Freshness::Unavailable,
        Spec031Availability::Unknown => Spec031Freshness::Unknown,
    }
}
