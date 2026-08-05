use super::lifecycle::{
    Spec031LifecycleError, Spec031LifecycleInput, Spec031RecoveryState, Spec031RuntimeControlState,
    Spec031TerminalOutcome,
};
use super::lifecycle_reason;
use super::{
    Spec031ApprovalCapability, Spec031ApprovalState, Spec031Availability, Spec031Capability,
    Spec031ConstructionError, Spec031DiagnosticsCapability, Spec031Envelope, Spec031EnvelopeInput,
    Spec031Lineage, Spec031ProgressCapability, Spec031ProgressDelivery, Spec031ProjectionKind,
    Spec031Reason, Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Severity,
    Spec031Source, Spec031SourceOwner,
};

pub(super) fn approval(
    input: &Spec031LifecycleInput,
    state: Spec031ApprovalState,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    envelope(
        input,
        Spec031ProjectionKind::Approval,
        lifecycle_reason::availability_for_approval(state),
        lifecycle_reason::severity_for_approval(state),
        lifecycle_reason::reason_for_approval(state),
        Spec031SourceOwner::Spec030,
        Spec031Capability::Approval(Spec031ApprovalCapability { state }),
    )
}

pub(super) fn runtime_control(
    input: &Spec031LifecycleInput,
    state: Spec031RuntimeControlState,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    diagnostics(input, lifecycle_reason::reason_for_runtime_control(state))
}

pub(super) fn progress(
    input: &Spec031LifecycleInput,
    delivery: Spec031ProgressDelivery,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    envelope(
        input,
        Spec031ProjectionKind::Progress,
        Spec031Availability::Blocked,
        Spec031Severity::Info,
        Spec031ReasonCode::Progress,
        Spec031SourceOwner::Channel,
        Spec031Capability::Progress(Spec031ProgressCapability::delivery(delivery)),
    )
}

pub(super) fn terminal(
    input: &Spec031LifecycleInput,
    outcome: Spec031TerminalOutcome,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    envelope(
        input,
        Spec031ProjectionKind::Progress,
        lifecycle_reason::terminal_availability(outcome),
        lifecycle_reason::terminal_severity(outcome),
        Spec031ReasonCode::Final,
        Spec031SourceOwner::Channel,
        Spec031Capability::Progress(Spec031ProgressCapability::delivery(
            lifecycle_reason::terminal_delivery(outcome),
        )),
    )
}

pub(super) fn recovery(
    input: &Spec031LifecycleInput,
    state: Spec031RecoveryState,
    repeated: bool,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    let reason = if repeated {
        Spec031ReasonCode::RepeatedInterruption
    } else {
        lifecycle_reason::reason_for_recovery(state)
    };
    diagnostics(input, reason)
}

pub(super) fn pending_follow_up(
    input: &Spec031LifecycleInput,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    envelope(
        input,
        Spec031ProjectionKind::Session,
        Spec031Availability::Blocked,
        Spec031Severity::Warning,
        Spec031ReasonCode::PendingFollowUp,
        Spec031SourceOwner::Session,
        Spec031Capability::Session(super::Spec031SessionCapability {
            active_turn_count: None,
        }),
    )
}

fn diagnostics(
    input: &Spec031LifecycleInput,
    reason: Spec031ReasonCode,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    envelope(
        input,
        Spec031ProjectionKind::Diagnostics,
        Spec031Availability::Blocked,
        Spec031Severity::Warning,
        reason,
        Spec031SourceOwner::Spec029,
        Spec031Capability::Diagnostics(Spec031DiagnosticsCapability {
            component_count: None,
        }),
    )
}

fn envelope(
    input: &Spec031LifecycleInput,
    kind: Spec031ProjectionKind,
    state: Spec031Availability,
    severity: Spec031Severity,
    reason: Spec031ReasonCode,
    owner: Spec031SourceOwner,
    capability: Spec031Capability,
) -> Result<Spec031Envelope, Spec031LifecycleError> {
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind,
        state,
        severity,
        reason: Spec031Reason {
            code: reason,
            safe_summary: Spec031SafeSummary::try_new(lifecycle_reason::summary(reason))
                .map_err(|_error: Spec031ConstructionError| Spec031LifecycleError::Construction)?,
        },
        lineage: Spec031Lineage {
            subject_ref: input.lineage.subject_ref.clone(),
            parent_ref: input.lineage.parent_ref.clone(),
            action_ref: input.lineage.action_ref.clone(),
            digest: input.lineage.digest.clone(),
        },
        source: Spec031Source {
            owner,
            observed_at_unix_ms: input.observed_at_unix_ms,
            freshness: lifecycle_reason::freshness(reason),
        },
        capability,
        children: Vec::new(),
    })
    .map_err(|_error| Spec031LifecycleError::Construction)
}
