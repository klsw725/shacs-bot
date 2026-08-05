use super::{build, Projection};
use shacs_projection::{
    Spec031Availability, Spec031Capability, Spec031Freshness, Spec031ProgressDelivery,
    Spec031ProjectionKind, Spec031ReasonCode, Spec031Severity, Spec031SourceOwner,
};

pub(super) fn line(projection: Projection) -> String {
    match build::envelope(projection) {
        Ok(envelope) => envelope_line(projection.label(), &envelope),
        Err(error) => format!(
            "Spec031 {}: state=unavailable severity=error reason=unsupported lineage=subject:cli:projection detail={error}",
            projection.label()
        ),
    }
}

pub(crate) fn envelope_line(label: &str, envelope: &shacs_projection::Spec031Envelope) -> String {
    let parent = envelope
        .lineage()
        .parent_ref
        .as_ref()
        .map_or("none", |value| value.as_str());
    let action = envelope
        .lineage()
        .action_ref
        .as_ref()
        .map_or("none", |value| value.as_str());
    format!(
        "Spec031 {label}: kind={} state={} severity={} reason={} lineage={} parent={} action={} capability={} delivery={} source={} freshness={}",
        kind_label(envelope.kind()),
        availability_label(envelope.state()),
        severity_label(envelope.severity()),
        reason_label(envelope.reason().code),
        envelope.lineage().subject_ref.as_str(),
        parent,
        action,
        capability_label(envelope.capability()),
        delivery_label(envelope.capability()),
        owner_label(envelope.source().owner),
        freshness_label(envelope.source().freshness)
    )
}

fn kind_label(value: Spec031ProjectionKind) -> &'static str {
    match value {
        Spec031ProjectionKind::Session => "session",
        Spec031ProjectionKind::Turn => "turn",
        Spec031ProjectionKind::Subagent => "subagent",
        Spec031ProjectionKind::Approval => "approval",
        Spec031ProjectionKind::Tool => "tool",
        Spec031ProjectionKind::Context => "context",
        Spec031ProjectionKind::Plugin => "plugin",
        Spec031ProjectionKind::App => "app",
        Spec031ProjectionKind::Media => "media",
        Spec031ProjectionKind::Diagnostics => "diagnostics",
        Spec031ProjectionKind::ReleaseEvidence => "release_evidence",
        Spec031ProjectionKind::Readiness => "readiness",
        Spec031ProjectionKind::Progress => "progress",
    }
}

fn availability_label(value: Spec031Availability) -> &'static str {
    match value {
        Spec031Availability::Ready => "ready",
        Spec031Availability::Degraded => "degraded",
        Spec031Availability::Blocked => "blocked",
        Spec031Availability::Unavailable => "unavailable",
        Spec031Availability::Unknown => "unknown",
    }
}

fn severity_label(value: Spec031Severity) -> &'static str {
    match value {
        Spec031Severity::Info => "info",
        Spec031Severity::Warning => "warning",
        Spec031Severity::Error => "error",
        Spec031Severity::Critical => "critical",
    }
}

fn reason_label(value: Spec031ReasonCode) -> &'static str {
    match value {
        Spec031ReasonCode::Included => "included",
        Spec031ReasonCode::Skipped => "skipped",
        Spec031ReasonCode::Blocked => "blocked",
        Spec031ReasonCode::Degraded => "degraded",
        Spec031ReasonCode::Missing => "missing",
        Spec031ReasonCode::Unsupported => "unsupported",
        Spec031ReasonCode::ExtractionFailed => "extraction_failed",
        Spec031ReasonCode::MissingExternalOwnerEvidence => "missing_external_owner_evidence",
        Spec031ReasonCode::Requested => "requested",
        Spec031ReasonCode::Completed => "completed",
        Spec031ReasonCode::Progress => "progress",
        Spec031ReasonCode::Final => "final",
        Spec031ReasonCode::Interrupted => "interrupted",
        Spec031ReasonCode::RecoveryRequested => "recovery_requested",
        Spec031ReasonCode::RecoveryCompleted => "recovery_completed",
        Spec031ReasonCode::RepeatedInterruption => "repeated_interruption",
        Spec031ReasonCode::PendingFollowUp => "pending_follow_up",
        Spec031ReasonCode::RetryConsumed => "retry_consumed",
    }
}

fn capability_label(value: &Spec031Capability) -> &'static str {
    match value {
        Spec031Capability::Session(_) => "session",
        Spec031Capability::Turn(_) => "turn",
        Spec031Capability::Subagent(_) => "subagent",
        Spec031Capability::Approval(_) => "approval",
        Spec031Capability::Tool(_) => "tool",
        Spec031Capability::Context(_) => "context",
        Spec031Capability::Plugin(_) => "plugin",
        Spec031Capability::App(_) => "app",
        Spec031Capability::Media(_) => "media",
        Spec031Capability::Diagnostics(_) => "diagnostics",
        Spec031Capability::ReleaseEvidence(_) => "release_evidence",
        Spec031Capability::Readiness(_) => "readiness",
        Spec031Capability::Progress(_) => "progress",
    }
}

fn delivery_label(value: &Spec031Capability) -> &'static str {
    match value {
        Spec031Capability::Progress(progress) => progress_delivery_label(progress.delivery),
        Spec031Capability::Session(_)
        | Spec031Capability::Turn(_)
        | Spec031Capability::Subagent(_)
        | Spec031Capability::Approval(_)
        | Spec031Capability::Tool(_)
        | Spec031Capability::Context(_)
        | Spec031Capability::Plugin(_)
        | Spec031Capability::App(_)
        | Spec031Capability::Media(_)
        | Spec031Capability::Diagnostics(_)
        | Spec031Capability::ReleaseEvidence(_)
        | Spec031Capability::Readiness(_) => "none",
    }
}

fn progress_delivery_label(value: Spec031ProgressDelivery) -> &'static str {
    match value {
        Spec031ProgressDelivery::Live => "live",
        Spec031ProgressDelivery::Coalesced => "coalesced",
        Spec031ProgressDelivery::Dropped => "dropped",
        Spec031ProgressDelivery::Reconnected => "reconnected",
        Spec031ProgressDelivery::FinalDelivered => "final_delivered",
        Spec031ProgressDelivery::FinalPending => "final_pending",
        Spec031ProgressDelivery::FinalFailed => "final_failed",
        Spec031ProgressDelivery::FinalUnknown => "final_unknown",
    }
}

fn freshness_label(value: Spec031Freshness) -> &'static str {
    match value {
        Spec031Freshness::Current => "current",
        Spec031Freshness::Stale => "stale",
        Spec031Freshness::Unavailable => "unavailable",
        Spec031Freshness::Unknown => "unknown",
    }
}

fn owner_label(value: Spec031SourceOwner) -> &'static str {
    match value {
        Spec031SourceOwner::Spec029 => "spec029",
        Spec031SourceOwner::Spec030 => "spec030",
        Spec031SourceOwner::Spec031 => "spec031",
        Spec031SourceOwner::Spec032 => "spec032",
        Spec031SourceOwner::Spec033 => "spec033",
        Spec031SourceOwner::Spec034 => "spec034",
        Spec031SourceOwner::Spec035 => "spec035",
        Spec031SourceOwner::Session => "session",
        Spec031SourceOwner::Workflow => "workflow",
        Spec031SourceOwner::Channel => "channel",
        Spec031SourceOwner::Projection => "projection",
    }
}
