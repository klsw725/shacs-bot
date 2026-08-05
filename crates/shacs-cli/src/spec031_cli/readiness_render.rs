use serde_json::{json, Value};
use shacs_projection::{
    Spec031Availability, Spec031Capability, Spec031Envelope, Spec031Freshness,
    Spec031ReadinessComponentKind, Spec031ReadinessObservation, Spec031ReasonCode,
};

pub(super) fn component_line(
    component: &Spec031ReadinessObservation,
    envelope: &Spec031Envelope,
) -> String {
    format!(
        "Spec031 readiness.{}: state={} severity={} reason={} freshness={} remediation={} depth={} capacity={}",
        component_label(component.kind),
        availability_label(component.state),
        severity_label(component.state),
        reason_label(component.reason_code),
        freshness_label(component.freshness),
        remediation_label(envelope),
        component
            .queue_depth
            .map_or_else(|| "unknown".to_owned(), |count| count.as_u64().to_string()),
        component
            .queue_capacity
            .map_or_else(|| "unknown".to_owned(), |count| count.as_u64().to_string())
    )
}

fn component_label(kind: Spec031ReadinessComponentKind) -> &'static str {
    match kind {
        Spec031ReadinessComponentKind::ProviderAuth => "provider_auth",
        Spec031ReadinessComponentKind::Storage => "storage",
        Spec031ReadinessComponentKind::Containment => "containment",
        Spec031ReadinessComponentKind::ChannelWorker => "channel_worker",
        Spec031ReadinessComponentKind::PluginApp => "plugin_app",
        Spec031ReadinessComponentKind::Queue => "queue",
        Spec031ReadinessComponentKind::ExternalIntegration => "external_integration",
    }
}

fn availability_label(state: Spec031Availability) -> &'static str {
    match state {
        Spec031Availability::Ready => "ready",
        Spec031Availability::Degraded => "degraded",
        Spec031Availability::Blocked => "blocked",
        Spec031Availability::Unavailable => "unavailable",
        Spec031Availability::Unknown => "unknown",
    }
}

fn severity_label(state: Spec031Availability) -> &'static str {
    match state {
        Spec031Availability::Ready => "info",
        Spec031Availability::Degraded | Spec031Availability::Unknown => "warning",
        Spec031Availability::Blocked | Spec031Availability::Unavailable => "error",
    }
}

fn reason_label(code: Spec031ReasonCode) -> &'static str {
    match code {
        Spec031ReasonCode::Included => "included",
        Spec031ReasonCode::Blocked => "blocked",
        Spec031ReasonCode::Degraded => "degraded",
        Spec031ReasonCode::Missing => "missing",
        Spec031ReasonCode::MissingExternalOwnerEvidence => "missing_external_owner_evidence",
        Spec031ReasonCode::Skipped
        | Spec031ReasonCode::Unsupported
        | Spec031ReasonCode::ExtractionFailed
        | Spec031ReasonCode::Requested
        | Spec031ReasonCode::Completed
        | Spec031ReasonCode::Progress
        | Spec031ReasonCode::Final
        | Spec031ReasonCode::Interrupted
        | Spec031ReasonCode::RecoveryRequested
        | Spec031ReasonCode::RecoveryCompleted
        | Spec031ReasonCode::RepeatedInterruption
        | Spec031ReasonCode::PendingFollowUp
        | Spec031ReasonCode::RetryConsumed => "other",
    }
}

fn freshness_label(freshness: Spec031Freshness) -> &'static str {
    match freshness {
        Spec031Freshness::Current => "current",
        Spec031Freshness::Stale => "stale",
        Spec031Freshness::Unavailable => "unavailable",
        Spec031Freshness::Unknown => "unknown",
    }
}

fn remediation_label(envelope: &Spec031Envelope) -> Value {
    match envelope.capability() {
        Spec031Capability::Readiness(capability) => capability
            .remediation
            .as_ref()
            .map_or(Value::Null, |summary| json!(summary.as_str())),
        _ => Value::Null,
    }
}
