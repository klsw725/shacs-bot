use super::{reason_summary, severity, Projection};
use shacs_projection::{
    Spec031AppCapability, Spec031Capability, Spec031ConstructionError, Spec031ContextCapability,
    Spec031Count, Spec031DiagnosticsCapability, Spec031Envelope, Spec031EnvelopeInput,
    Spec031InclusionReason, Spec031Lineage, Spec031MediaCapability, Spec031PluginCapability,
    Spec031ProgressCapability, Spec031ProgressDelivery, Spec031ReadinessCapability, Spec031Reason,
    Spec031SafeSummary, Spec031SchemaVersion, Spec031SessionCapability, Spec031Source,
    Spec031SubagentCapability, Spec031SubjectRef, Spec031ToolCapability,
};

pub(super) fn envelope(
    projection: Projection,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: projection.kind(),
        state: projection.availability(),
        severity: severity(projection.availability()),
        reason: Spec031Reason {
            code: projection.reason_code(),
            safe_summary: Spec031SafeSummary::try_new(reason_summary(projection.reason_code()))?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(projection.subject_ref())?,
            parent_ref: None,
            action_ref: None,
            digest: None,
        },
        source: Spec031Source {
            owner: projection.owner(),
            observed_at_unix_ms: None,
            freshness: projection.freshness(),
        },
        capability: capability(projection),
        children: Vec::new(),
    })
}

fn capability(projection: Projection) -> Spec031Capability {
    match projection {
        Projection::Session {
            active_turn_count, ..
        } => Spec031Capability::Session(Spec031SessionCapability {
            active_turn_count: Some(count(active_turn_count)),
        }),
        Projection::Subagent { child_count } => {
            Spec031Capability::Subagent(Spec031SubagentCapability {
                child_count: Some(count(child_count)),
            })
        }
        Projection::Tool { attempt_count } => Spec031Capability::Tool(Spec031ToolCapability {
            attempt_count: Some(count(attempt_count)),
        }),
        Projection::Diagnostics {
            component_count, ..
        } => Spec031Capability::Diagnostics(Spec031DiagnosticsCapability {
            component_count: Some(count(component_count)),
        }),
        Projection::Readiness { .. } => Spec031Capability::Readiness(Spec031ReadinessCapability {
            availability: projection.availability(),
            component_count: None,
            queue_depth: None,
            queue_capacity: None,
            remediation: None,
        }),
        Projection::Context { included } => Spec031Capability::Context(Spec031ContextCapability {
            reason: inclusion_reason(included),
        }),
        Projection::Plugin { .. } => Spec031Capability::Plugin(Spec031PluginCapability {
            availability: projection.availability(),
        }),
        Projection::App { .. } => Spec031Capability::App(Spec031AppCapability {
            availability: projection.availability(),
        }),
        Projection::Media { artifact_count } => Spec031Capability::Media(Spec031MediaCapability {
            reason: inclusion_reason(artifact_count > 0),
        }),
        Projection::Progress { blocked } => {
            Spec031Capability::Progress(Spec031ProgressCapability::delivery(if blocked {
                Spec031ProgressDelivery::Dropped
            } else {
                Spec031ProgressDelivery::Live
            }))
        }
    }
}

fn inclusion_reason(included: bool) -> Spec031InclusionReason {
    if included {
        Spec031InclusionReason::Included
    } else {
        Spec031InclusionReason::Blocked
    }
}

fn count(value: usize) -> Spec031Count {
    Spec031Count::new(u64::try_from(value).unwrap_or(u64::MAX))
}
