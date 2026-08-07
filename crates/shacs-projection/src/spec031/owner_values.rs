use super::{
    Spec031AppCapability, Spec031ApprovalCapability, Spec031ApprovalState, Spec031Availability,
    Spec031Capability, Spec031ContextCapability, Spec031Count, Spec031DiagnosticsCapability,
    Spec031FixtureFamily, Spec031Freshness, Spec031InclusionReason, Spec031MediaCapability,
    Spec031PluginCapability, Spec031ProgressCapability, Spec031ProgressDelivery,
    Spec031ReadinessCapability, Spec031ReasonCode, Spec031ReleaseEvidenceCapability,
    Spec031SessionCapability, Spec031Severity, Spec031SourceOwner, Spec031SubagentCapability,
    Spec031ToolCapability, Spec031TurnCapability,
};

pub(super) fn source_owner(family: Spec031FixtureFamily) -> Spec031SourceOwner {
    match family {
        Spec031FixtureFamily::Session | Spec031FixtureFamily::Turn => Spec031SourceOwner::Session,
        Spec031FixtureFamily::Subagent
        | Spec031FixtureFamily::Tool
        | Spec031FixtureFamily::Approval => Spec031SourceOwner::Spec030,
        Spec031FixtureFamily::Recovery => Spec031SourceOwner::Spec029,
        Spec031FixtureFamily::Delivery => Spec031SourceOwner::Channel,
        Spec031FixtureFamily::Readiness => Spec031SourceOwner::Spec031,
        Spec031FixtureFamily::ExternalAppOwner => Spec031SourceOwner::Spec032,
        Spec031FixtureFamily::ExternalMediaOwner => Spec031SourceOwner::Spec034,
        Spec031FixtureFamily::Context
        | Spec031FixtureFamily::Extension
        | Spec031FixtureFamily::ReleaseEvidence => Spec031SourceOwner::Spec031,
    }
}

pub(super) fn external_owner(family: Spec031FixtureFamily) -> Spec031SourceOwner {
    match family {
        Spec031FixtureFamily::Readiness => Spec031SourceOwner::Spec031,
        Spec031FixtureFamily::ExternalAppOwner => Spec031SourceOwner::Spec032,
        Spec031FixtureFamily::ExternalMediaOwner => Spec031SourceOwner::Spec034,
        Spec031FixtureFamily::Session
        | Spec031FixtureFamily::Turn
        | Spec031FixtureFamily::Subagent
        | Spec031FixtureFamily::Tool
        | Spec031FixtureFamily::Approval
        | Spec031FixtureFamily::Recovery
        | Spec031FixtureFamily::Context
        | Spec031FixtureFamily::Extension
        | Spec031FixtureFamily::Delivery
        | Spec031FixtureFamily::ReleaseEvidence => Spec031SourceOwner::Spec031,
    }
}

pub(super) fn canonical_capability(family: Spec031FixtureFamily) -> Spec031Capability {
    match family {
        Spec031FixtureFamily::Session => Spec031Capability::Session(Spec031SessionCapability {
            active_turn_count: Some(Spec031Count::new(0)),
        }),
        Spec031FixtureFamily::Turn => {
            Spec031Capability::Turn(Spec031TurnCapability { turn_index: None })
        }
        Spec031FixtureFamily::Subagent => Spec031Capability::Subagent(Spec031SubagentCapability {
            child_count: Some(Spec031Count::new(1)),
        }),
        Spec031FixtureFamily::Tool => Spec031Capability::Tool(Spec031ToolCapability {
            attempt_count: Some(Spec031Count::new(0)),
        }),
        Spec031FixtureFamily::Approval => Spec031Capability::Approval(Spec031ApprovalCapability {
            state: Spec031ApprovalState::Pending,
        }),
        Spec031FixtureFamily::Recovery => {
            Spec031Capability::Diagnostics(Spec031DiagnosticsCapability {
                component_count: Some(Spec031Count::new(1)),
            })
        }
        Spec031FixtureFamily::Context => Spec031Capability::Context(Spec031ContextCapability {
            reason: Spec031InclusionReason::Included,
        }),
        Spec031FixtureFamily::Extension => Spec031Capability::Plugin(Spec031PluginCapability {
            availability: Spec031Availability::Degraded,
        }),
        Spec031FixtureFamily::Delivery => Spec031Capability::Progress(
            Spec031ProgressCapability::delivery(Spec031ProgressDelivery::Dropped),
        ),
        Spec031FixtureFamily::ReleaseEvidence => {
            Spec031Capability::ReleaseEvidence(Spec031ReleaseEvidenceCapability {
                blocker_count: Some(Spec031Count::new(1)),
            })
        }
        Spec031FixtureFamily::Readiness
        | Spec031FixtureFamily::ExternalAppOwner
        | Spec031FixtureFamily::ExternalMediaOwner => missing_capability(family),
    }
}

pub(super) fn missing_capability(family: Spec031FixtureFamily) -> Spec031Capability {
    match family {
        Spec031FixtureFamily::Readiness => {
            Spec031Capability::Readiness(Spec031ReadinessCapability {
                availability: Spec031Availability::Unavailable,
                component_count: None,
                queue_depth: None,
                queue_capacity: None,
                remediation: None,
            })
        }
        Spec031FixtureFamily::ExternalAppOwner => Spec031Capability::App(Spec031AppCapability {
            availability: Spec031Availability::Unavailable,
        }),
        Spec031FixtureFamily::ExternalMediaOwner => {
            Spec031Capability::Media(Spec031MediaCapability {
                reason: Spec031InclusionReason::Blocked,
            })
        }
        Spec031FixtureFamily::Session
        | Spec031FixtureFamily::Turn
        | Spec031FixtureFamily::Subagent
        | Spec031FixtureFamily::Tool
        | Spec031FixtureFamily::Approval
        | Spec031FixtureFamily::Recovery
        | Spec031FixtureFamily::Context
        | Spec031FixtureFamily::Extension
        | Spec031FixtureFamily::Delivery
        | Spec031FixtureFamily::ReleaseEvidence => canonical_capability(family),
    }
}

pub(super) fn state(family: Spec031FixtureFamily) -> Spec031Availability {
    match family {
        Spec031FixtureFamily::Session
        | Spec031FixtureFamily::Context
        | Spec031FixtureFamily::ReleaseEvidence => Spec031Availability::Ready,
        Spec031FixtureFamily::Turn => Spec031Availability::Unknown,
        Spec031FixtureFamily::Subagent
        | Spec031FixtureFamily::Tool
        | Spec031FixtureFamily::Approval
        | Spec031FixtureFamily::Recovery
        | Spec031FixtureFamily::Delivery => Spec031Availability::Blocked,
        Spec031FixtureFamily::Extension => Spec031Availability::Degraded,
        Spec031FixtureFamily::Readiness
        | Spec031FixtureFamily::ExternalAppOwner
        | Spec031FixtureFamily::ExternalMediaOwner => Spec031Availability::Unavailable,
    }
}

pub(super) fn severity(family: Spec031FixtureFamily) -> Spec031Severity {
    match state(family) {
        Spec031Availability::Ready => Spec031Severity::Info,
        Spec031Availability::Degraded | Spec031Availability::Unknown => Spec031Severity::Warning,
        Spec031Availability::Blocked | Spec031Availability::Unavailable => Spec031Severity::Error,
    }
}

pub(super) fn freshness(family: Spec031FixtureFamily) -> Spec031Freshness {
    match family {
        Spec031FixtureFamily::Delivery => Spec031Freshness::Stale,
        Spec031FixtureFamily::Turn
        | Spec031FixtureFamily::Readiness
        | Spec031FixtureFamily::ExternalAppOwner
        | Spec031FixtureFamily::ExternalMediaOwner => Spec031Freshness::Unavailable,
        Spec031FixtureFamily::Session
        | Spec031FixtureFamily::Subagent
        | Spec031FixtureFamily::Tool
        | Spec031FixtureFamily::Approval
        | Spec031FixtureFamily::Recovery
        | Spec031FixtureFamily::Context
        | Spec031FixtureFamily::Extension
        | Spec031FixtureFamily::ReleaseEvidence => Spec031Freshness::Current,
    }
}

pub(super) fn reason(family: Spec031FixtureFamily) -> Spec031ReasonCode {
    match state(family) {
        Spec031Availability::Ready => Spec031ReasonCode::Included,
        Spec031Availability::Degraded => Spec031ReasonCode::Degraded,
        Spec031Availability::Blocked => Spec031ReasonCode::Blocked,
        Spec031Availability::Unavailable | Spec031Availability::Unknown => {
            Spec031ReasonCode::Missing
        }
    }
}
