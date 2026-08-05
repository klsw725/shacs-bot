use super::lifecycle::{Spec031RecoveryState, Spec031RuntimeControlState, Spec031TerminalOutcome};
use super::{
    Spec031ApprovalState, Spec031Availability, Spec031Freshness, Spec031ProgressDelivery,
    Spec031ReasonCode, Spec031Severity,
};

pub(super) const fn availability_for_approval(state: Spec031ApprovalState) -> Spec031Availability {
    match state {
        Spec031ApprovalState::Allowed => Spec031Availability::Ready,
        Spec031ApprovalState::Pending
        | Spec031ApprovalState::Denied
        | Spec031ApprovalState::Expired
        | Spec031ApprovalState::Skipped
        | Spec031ApprovalState::RetryConsumed => Spec031Availability::Blocked,
    }
}

pub(super) const fn severity_for_approval(state: Spec031ApprovalState) -> Spec031Severity {
    match state {
        Spec031ApprovalState::Allowed => Spec031Severity::Info,
        Spec031ApprovalState::Pending => Spec031Severity::Warning,
        Spec031ApprovalState::Denied
        | Spec031ApprovalState::Expired
        | Spec031ApprovalState::Skipped
        | Spec031ApprovalState::RetryConsumed => Spec031Severity::Error,
    }
}

pub(super) const fn reason_for_approval(state: Spec031ApprovalState) -> Spec031ReasonCode {
    match state {
        Spec031ApprovalState::Pending => Spec031ReasonCode::Requested,
        Spec031ApprovalState::Allowed => Spec031ReasonCode::Completed,
        Spec031ApprovalState::Denied => Spec031ReasonCode::Blocked,
        Spec031ApprovalState::Expired => Spec031ReasonCode::Missing,
        Spec031ApprovalState::Skipped => Spec031ReasonCode::Skipped,
        Spec031ApprovalState::RetryConsumed => Spec031ReasonCode::RetryConsumed,
    }
}

pub(super) const fn reason_for_runtime_control(
    state: Spec031RuntimeControlState,
) -> Spec031ReasonCode {
    match state {
        Spec031RuntimeControlState::Requested => Spec031ReasonCode::Requested,
        Spec031RuntimeControlState::Completed => Spec031ReasonCode::Completed,
    }
}

pub(super) const fn reason_for_recovery(state: Spec031RecoveryState) -> Spec031ReasonCode {
    match state {
        Spec031RecoveryState::Interrupted => Spec031ReasonCode::Interrupted,
        Spec031RecoveryState::Requested => Spec031ReasonCode::RecoveryRequested,
        Spec031RecoveryState::Completed => Spec031ReasonCode::RecoveryCompleted,
    }
}

pub(super) const fn terminal_availability(outcome: Spec031TerminalOutcome) -> Spec031Availability {
    match outcome {
        Spec031TerminalOutcome::Succeeded => Spec031Availability::Ready,
        Spec031TerminalOutcome::Failed | Spec031TerminalOutcome::Cancelled => {
            Spec031Availability::Blocked
        }
    }
}

pub(super) const fn terminal_severity(outcome: Spec031TerminalOutcome) -> Spec031Severity {
    match outcome {
        Spec031TerminalOutcome::Succeeded => Spec031Severity::Info,
        Spec031TerminalOutcome::Failed | Spec031TerminalOutcome::Cancelled => {
            Spec031Severity::Error
        }
    }
}

pub(super) const fn terminal_delivery(outcome: Spec031TerminalOutcome) -> Spec031ProgressDelivery {
    match outcome {
        Spec031TerminalOutcome::Succeeded => Spec031ProgressDelivery::FinalDelivered,
        Spec031TerminalOutcome::Failed | Spec031TerminalOutcome::Cancelled => {
            Spec031ProgressDelivery::FinalFailed
        }
    }
}

pub(super) const fn freshness(reason: Spec031ReasonCode) -> Spec031Freshness {
    match reason {
        Spec031ReasonCode::RepeatedInterruption => Spec031Freshness::Stale,
        _ => Spec031Freshness::Current,
    }
}

pub(super) const fn summary(reason: Spec031ReasonCode) -> &'static str {
    match reason {
        Spec031ReasonCode::Requested => "requested",
        Spec031ReasonCode::Completed => "completed",
        Spec031ReasonCode::Progress => "progress",
        Spec031ReasonCode::Final => "final",
        Spec031ReasonCode::Interrupted => "interrupted",
        Spec031ReasonCode::RecoveryRequested => "recovery requested",
        Spec031ReasonCode::RecoveryCompleted => "recovery completed",
        Spec031ReasonCode::RepeatedInterruption => "repeated interruption",
        Spec031ReasonCode::PendingFollowUp => "pending follow up",
        Spec031ReasonCode::RetryConsumed => "retry consumed",
        Spec031ReasonCode::Included => "included",
        Spec031ReasonCode::Skipped => "skipped",
        Spec031ReasonCode::Blocked => "blocked",
        Spec031ReasonCode::Degraded => "degraded",
        Spec031ReasonCode::Missing => "missing",
        Spec031ReasonCode::Unsupported => "unsupported",
        Spec031ReasonCode::ExtractionFailed => "extraction failed",
        Spec031ReasonCode::MissingExternalOwnerEvidence => "missing external owner evidence",
    }
}
