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

#[test]
fn spec031_readiness_covers_prd003_failure_observations() -> Result<(), Box<dyn Error>> {
    for (name, observation, expected) in failure_cases()? {
        let mut observations = ready_required_components()?;
        observations.retain(|item| item.kind != observation.kind);
        observations.push(observation);
        let report = spec031_aggregate_readiness(&observations)?;

        assert_eq!(report.envelope().state(), expected, "{name}");
    }

    Ok(())
}

#[test]
fn spec031_readiness_preserves_queue_zero_as_present_and_missing_capacity(
) -> Result<(), Box<dyn Error>> {
    let queue = spec031_aggregate_readiness(&[Spec031ReadinessObservation {
        queue_depth: Some(Spec031Count::new(0)),
        queue_capacity: None,
        ..queue_blocked()?
    }])?;
    let queue_component = queue
        .components()
        .iter()
        .find(|component| component.kind == Spec031ReadinessComponentKind::Queue)
        .ok_or("missing queue component")?;
    assert_eq!(queue_component.queue_depth, Some(Spec031Count::new(0)));
    assert_eq!(queue_component.queue_capacity, None);

    Ok(())
}

#[test]
fn spec031_readiness_disabled_plugin_uses_plugin_app_component() -> Result<(), Box<dyn Error>> {
    let mut observations = ready_required_components()?;
    observations.retain(|item| item.kind != Spec031ReadinessComponentKind::PluginApp);
    observations.push(disabled_plugin()?);

    let report = spec031_aggregate_readiness(&observations)?;
    let plugin = report
        .components()
        .iter()
        .find(|component| component.kind == Spec031ReadinessComponentKind::PluginApp)
        .ok_or("missing plugin app component")?;

    assert_eq!(plugin.requirement, Spec031ReadinessRequirement::Required);
    assert_eq!(plugin.state, Spec031Availability::Unavailable);
    assert_eq!(plugin.reason_code, Spec031ReasonCode::Unsupported);
    assert_eq!(report.envelope().state(), Spec031Availability::Unknown);

    Ok(())
}

type FailureCase = (
    &'static str,
    Spec031ReadinessObservation,
    Spec031Availability,
);

fn failure_cases() -> Result<[FailureCase; 7], Spec031ConstructionError> {
    Ok([
        (
            "absent_credentials",
            required(
                Spec031ReadinessComponentKind::ProviderAuth,
                Spec031Availability::Blocked,
                Spec031Freshness::Current,
                Spec031ReasonCode::Missing,
                "provider credentials absent",
            )?,
            Spec031Availability::Blocked,
        ),
        (
            "blocked_migration",
            required(
                Spec031ReadinessComponentKind::Storage,
                Spec031Availability::Blocked,
                Spec031Freshness::Current,
                Spec031ReasonCode::Blocked,
                "migration blocks storage",
            )?,
            Spec031Availability::Blocked,
        ),
        (
            "unknown_containment",
            required(
                Spec031ReadinessComponentKind::Containment,
                Spec031Availability::Unknown,
                Spec031Freshness::Unknown,
                Spec031ReasonCode::Missing,
                "containment evidence unknown",
            )?,
            Spec031Availability::Unknown,
        ),
        (
            "failed_channel",
            required(
                Spec031ReadinessComponentKind::ChannelWorker,
                Spec031Availability::Blocked,
                Spec031Freshness::Current,
                Spec031ReasonCode::Blocked,
                "channel worker failed",
            )?,
            Spec031Availability::Blocked,
        ),
        (
            "disabled_plugin",
            disabled_plugin()?,
            Spec031Availability::Unknown,
        ),
        (
            "missing_app_owner",
            required(
                Spec031ReadinessComponentKind::PluginApp,
                Spec031Availability::Unavailable,
                Spec031Freshness::Unavailable,
                Spec031ReasonCode::MissingExternalOwnerEvidence,
                "app owner missing",
            )?,
            Spec031Availability::Unknown,
        ),
        (
            "queue_admission_block",
            Spec031ReadinessObservation {
                queue_depth: Some(Spec031Count::new(0)),
                queue_capacity: None,
                ..queue_blocked()?
            },
            Spec031Availability::Blocked,
        ),
    ])
}

fn disabled_plugin() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    required(
        Spec031ReadinessComponentKind::PluginApp,
        Spec031Availability::Unavailable,
        Spec031Freshness::Unavailable,
        Spec031ReasonCode::Unsupported,
        "plugin disabled",
    )
}

fn queue_blocked() -> Result<Spec031ReadinessObservation, Spec031ConstructionError> {
    required(
        Spec031ReadinessComponentKind::Queue,
        Spec031Availability::Blocked,
        Spec031Freshness::Current,
        Spec031ReasonCode::Blocked,
        "queue admission blocked",
    )
}
