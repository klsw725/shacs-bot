use shacs_projection::*;

const NOW: Spec031ObservedAtUnixMs = Spec031ObservedAtUnixMs::new(31);

pub struct ObservationSpec {
    kind: Spec031ReadinessComponentKind,
    requirement: Spec031ReadinessRequirement,
    state: Spec031Availability,
    freshness: Spec031Freshness,
    reason_code: Spec031ReasonCode,
    summary: &'static str,
    queue_depth: Option<Spec031Count>,
    queue_capacity: Option<Spec031Count>,
}

pub fn ready_required() -> Result<Vec<Spec031ReadinessObservation>, Spec031ConstructionError> {
    Spec031ReadinessComponentKind::REQUIRED
        .into_iter()
        .map(|kind| observation(ready_spec(kind, "component ready")))
        .collect()
}

pub fn observation(
    spec: ObservationSpec,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    Ok(Spec031ReadinessObservation {
        kind: spec.kind,
        requirement: spec.requirement,
        state: spec.state,
        freshness: spec.freshness,
        reason_code: spec.reason_code,
        safe_summary: Spec031SafeSummary::try_new(spec.summary)?,
        observed_at_unix_ms: Some(NOW),
        queue_depth: spec.queue_depth,
        queue_capacity: spec.queue_capacity,
    })
}

pub fn provider_auth_missing() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(spec(
        Spec031ReadinessComponentKind::ProviderAuth,
        Spec031Availability::Blocked,
        Spec031ReasonCode::Missing,
        "provider credentials absent",
    ))
}

pub fn storage_ready(
    summary: &'static str,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(ready_spec(Spec031ReadinessComponentKind::Storage, summary))
}

pub fn storage_blocked() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(spec(
        Spec031ReadinessComponentKind::Storage,
        Spec031Availability::Blocked,
        Spec031ReasonCode::Blocked,
        "migration blocks storage",
    ))
}

pub fn containment_unknown() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(spec(
        Spec031ReadinessComponentKind::Containment,
        Spec031Availability::Unknown,
        Spec031ReasonCode::Missing,
        "containment evidence unknown",
    ))
}

pub fn channel_blocked() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(spec(
        Spec031ReadinessComponentKind::ChannelWorker,
        Spec031Availability::Blocked,
        Spec031ReasonCode::Blocked,
        "channel worker failed",
    ))
}

pub fn plugin_disabled() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(spec(
        Spec031ReadinessComponentKind::PluginApp,
        Spec031Availability::Unavailable,
        Spec031ReasonCode::Unsupported,
        "plugin disabled",
    ))
}

pub fn app_owner_missing() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    observation(spec(
        Spec031ReadinessComponentKind::PluginApp,
        Spec031Availability::Unavailable,
        Spec031ReasonCode::MissingExternalOwnerEvidence,
        "app owner missing",
    ))
}

pub fn queue_blocked_zero_missing() -> Result<Spec031ReadinessObservation, Spec031ConstructionError>
{
    let mut queue = spec(
        Spec031ReadinessComponentKind::Queue,
        Spec031Availability::Blocked,
        Spec031ReasonCode::Blocked,
        "queue admission blocked",
    );
    queue.queue_depth = Some(Spec031Count::new(0));
    observation(queue)
}

pub fn provider_stale_ready() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    let mut provider = ready_spec(
        Spec031ReadinessComponentKind::ProviderAuth,
        "provider stale ready",
    );
    provider.freshness = Spec031Freshness::Stale;
    observation(provider)
}

pub fn optional_state(
    state: Spec031Availability,
) -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    let mut optional = spec(
        Spec031ReadinessComponentKind::ExternalIntegration,
        state,
        reason_for(state),
        "optional integration state",
    );
    optional.requirement = Spec031ReadinessRequirement::Optional;
    observation(optional)
}

pub fn optional_storage_degraded() -> Result<Spec031ReadinessObservation, Spec031ConstructionError>
{
    let mut optional = spec(
        Spec031ReadinessComponentKind::Storage,
        Spec031Availability::Degraded,
        Spec031ReasonCode::Degraded,
        "storage optional duplicate",
    );
    optional.requirement = Spec031ReadinessRequirement::Optional;
    observation(optional)
}

fn ready_spec(kind: Spec031ReadinessComponentKind, summary: &'static str) -> ObservationSpec {
    spec(
        kind,
        Spec031Availability::Ready,
        Spec031ReasonCode::Included,
        summary,
    )
}

fn spec(
    kind: Spec031ReadinessComponentKind,
    state: Spec031Availability,
    reason_code: Spec031ReasonCode,
    summary: &'static str,
) -> ObservationSpec {
    ObservationSpec {
        kind,
        requirement: Spec031ReadinessRequirement::Required,
        state,
        freshness: freshness_for(state),
        reason_code,
        summary,
        queue_depth: None,
        queue_capacity: None,
    }
}

const fn freshness_for(state: Spec031Availability) -> Spec031Freshness {
    match state {
        Spec031Availability::Ready
        | Spec031Availability::Degraded
        | Spec031Availability::Blocked => Spec031Freshness::Current,
        Spec031Availability::Unavailable => Spec031Freshness::Unavailable,
        Spec031Availability::Unknown => Spec031Freshness::Unknown,
    }
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
