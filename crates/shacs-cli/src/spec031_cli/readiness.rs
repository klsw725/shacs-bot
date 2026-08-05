use crate::{RuntimeCompatibility, RuntimeInspectReport, RuntimeOwnershipState};
use serde_json::Value;
use shacs_projection::{
    spec031_aggregate_readiness, Spec031Availability, Spec031ReadinessComponentKind,
    Spec031ReadinessObservation, Spec031ReadinessReport, Spec031ReasonCode,
};

pub(crate) fn report(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessReport, String> {
    spec031_aggregate_readiness(&[
        provider_auth(inspect)?,
        storage(inspect)?,
        containment(inspect)?,
        channel_worker(inspect)?,
        plugin_app(inspect)?,
        super::readiness_queue::queue(inspect)?,
    ])
    .map_err(|error| error.to_string())
}

pub(crate) fn value(inspect: &RuntimeInspectReport) -> Result<Value, String> {
    serde_json::to_value(report(inspect)?).map_err(|error| error.to_string())
}

pub(crate) fn lines(inspect: &RuntimeInspectReport) -> Result<Vec<String>, String> {
    let report = report(inspect)?;
    let mut lines = vec![super::render::envelope_line("readiness", report.envelope())];
    lines.extend(
        report
            .components()
            .iter()
            .zip(report.envelope().children())
            .map(|(component, envelope)| {
                super::readiness_render::component_line(component, envelope)
            }),
    );
    Ok(lines)
}

fn provider_auth(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessObservation, String> {
    let selected = inspect
        .providers
        .iter()
        .find(|provider| provider.name == inspect.provider)
        .or_else(|| inspect.providers.first());
    let (state, code, summary) = match selected {
        Some(provider) if provider.has_api_key => (
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "provider auth evidence is configured",
        ),
        Some(_) => (
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "provider auth evidence is missing",
        ),
        None => (
            Spec031Availability::Unavailable,
            Spec031ReasonCode::Missing,
            "provider auth owner observation is unavailable",
        ),
    };
    super::readiness_observation::observation(
        Spec031ReadinessComponentKind::ProviderAuth,
        state,
        code,
        summary,
    )
}

fn storage(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessObservation, String> {
    let blocked = !inspect.workspace_exists
        || inspect.lifecycle.migration_plan.blocked
        || inspect.lifecycle.migration_ledger.manual_recovery_required
        || !inspect.lifecycle.durable_recovery.writable
        || !inspect.lifecycle.durable_work.writable
        || !matches!(
            inspect.lifecycle.compatibility,
            RuntimeCompatibility::FullyCompatible
        );
    let (state, code, summary) = if blocked {
        (
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "storage or migration evidence blocks writable runtime readiness",
        )
    } else {
        (
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "storage and migration evidence allow runtime readiness",
        )
    };
    super::readiness_observation::observation(
        Spec031ReadinessComponentKind::Storage,
        state,
        code,
        summary,
    )
}

fn containment(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessObservation, String> {
    let (state, code, summary) = match inspect.containment.contained {
        Some(true) => (
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "containment evidence is present",
        ),
        Some(false) => (
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "containment evidence reports an unsafe runtime boundary",
        ),
        None => (
            Spec031Availability::Unknown,
            Spec031ReasonCode::Missing,
            "containment evidence is unknown for this runtime",
        ),
    };
    super::readiness_observation::observation(
        Spec031ReadinessComponentKind::Containment,
        state,
        code,
        summary,
    )
}

fn channel_worker(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessObservation, String> {
    let failed_supervision = inspect.supervision.components.iter().any(|component| {
        let state = component.state.as_str();
        state == "failed" || state == "error" || state == "stopped"
    });
    let failed_delivery = inspect.channel_restart.iter().any(|state| {
        state.delivery_statuses.iter().any(|delivery| {
            matches!(
                delivery.status,
                crate::ChannelDeliveryProjectionStatus::FailedHint
            )
        })
    });
    let (state, code, summary) = if failed_supervision {
        (
            Spec031Availability::Blocked,
            Spec031ReasonCode::Blocked,
            "channel worker supervision reports a failed component",
        )
    } else if failed_delivery {
        (
            Spec031Availability::Degraded,
            Spec031ReasonCode::Degraded,
            "channel restart evidence contains failed delivery hints",
        )
    } else if inspect.supervision.components.is_empty() && inspect.channel_restart.is_empty() {
        (
            Spec031Availability::Unavailable,
            Spec031ReasonCode::Missing,
            "channel worker owner observation is unavailable",
        )
    } else {
        (
            Spec031Availability::Ready,
            Spec031ReasonCode::Included,
            "channel worker evidence is available",
        )
    };
    super::readiness_observation::observation(
        Spec031ReadinessComponentKind::ChannelWorker,
        state,
        code,
        summary,
    )
}

fn plugin_app(inspect: &RuntimeInspectReport) -> Result<Spec031ReadinessObservation, String> {
    if inspect.lifecycle.ownership.state == RuntimeOwnershipState::Active {
        return super::readiness_observation::observation(
            Spec031ReadinessComponentKind::PluginApp,
            Spec031Availability::Degraded,
            Spec031ReasonCode::MissingExternalOwnerEvidence,
            "runtime owner exists but plugin and app lifecycle evidence is partial",
        );
    }
    super::readiness_observation::observation(
        Spec031ReadinessComponentKind::PluginApp,
        Spec031Availability::Unavailable,
        Spec031ReasonCode::MissingExternalOwnerEvidence,
        "plugin and app owner observation is unavailable",
    )
}
