use super::{
    Spec031Availability, Spec031Capability, Spec031ConstructionError, Spec031Count,
    Spec031Envelope, Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage,
    Spec031ObservedAtUnixMs, Spec031ProjectionKind, Spec031ReadinessCapability,
    Spec031ReadinessObservation, Spec031Reason, Spec031ReasonCode, Spec031SafeSummary,
    Spec031SchemaVersion, Spec031Severity, Spec031Source, Spec031SourceOwner, Spec031SubjectRef,
};

pub(super) fn aggregate_envelope(
    state: Spec031Availability,
    freshness: Spec031Freshness,
    observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    component_count: Spec031Count,
    children: Vec<Spec031Envelope>,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Readiness,
        state,
        severity: severity(state),
        reason: Spec031Reason {
            code: reason_code(state),
            safe_summary: summary(state)?,
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new("subject:readiness:aggregate")?,
            parent_ref: None,
            action_ref: None,
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Projection,
            observed_at_unix_ms,
            freshness,
        },
        capability: Spec031Capability::Readiness(Spec031ReadinessCapability {
            availability: state,
            component_count: Some(component_count),
            queue_depth: None,
            queue_capacity: None,
            remediation: remediation(state)?,
        }),
        children,
    })
}

pub(super) fn component_envelopes(
    components: &[Spec031ReadinessObservation],
) -> Result<Vec<Spec031Envelope>, Spec031ConstructionError> {
    components.iter().map(component_envelope).collect()
}

fn component_envelope(
    component: &Spec031ReadinessObservation,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Readiness,
        state: component.state,
        severity: severity(component.state),
        reason: Spec031Reason {
            code: component.reason_code,
            safe_summary: component.safe_summary.clone(),
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(&format!(
                "subject:readiness:{}",
                component.kind.subject_slug()
            ))?,
            parent_ref: None,
            action_ref: None,
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Projection,
            observed_at_unix_ms: component.observed_at_unix_ms,
            freshness: component.freshness,
        },
        capability: Spec031Capability::Readiness(Spec031ReadinessCapability {
            availability: component.state,
            component_count: None,
            queue_depth: component.queue_depth,
            queue_capacity: component.queue_capacity,
            remediation: component_remediation(component)?,
        }),
        children: Vec::new(),
    })
}

const fn severity(state: Spec031Availability) -> Spec031Severity {
    match state {
        Spec031Availability::Ready => Spec031Severity::Info,
        Spec031Availability::Degraded | Spec031Availability::Unknown => Spec031Severity::Warning,
        Spec031Availability::Blocked | Spec031Availability::Unavailable => Spec031Severity::Error,
    }
}

const fn reason_code(state: Spec031Availability) -> Spec031ReasonCode {
    match state {
        Spec031Availability::Ready => Spec031ReasonCode::Included,
        Spec031Availability::Degraded => Spec031ReasonCode::Degraded,
        Spec031Availability::Blocked => Spec031ReasonCode::Blocked,
        Spec031Availability::Unavailable | Spec031Availability::Unknown => {
            Spec031ReasonCode::Missing
        }
    }
}

fn summary(state: Spec031Availability) -> Result<Spec031SafeSummary, Spec031ConstructionError> {
    let text = match state {
        Spec031Availability::Ready => "all required readiness observations are ready",
        Spec031Availability::Degraded => "runtime is usable with degraded readiness",
        Spec031Availability::Blocked => "required readiness observation is blocked",
        Spec031Availability::Unavailable | Spec031Availability::Unknown => {
            "required readiness observation is unavailable or unknown"
        }
    };
    Spec031SafeSummary::try_new(text)
}

fn remediation(
    state: Spec031Availability,
) -> Result<Option<Spec031SafeSummary>, Spec031ConstructionError> {
    let text = match state {
        Spec031Availability::Ready => return Ok(None),
        Spec031Availability::Degraded => {
            "inspect degraded component readiness before relying on limited features"
        }
        Spec031Availability::Blocked => {
            "resolve blocked required component evidence and rerun diagnostics"
        }
        Spec031Availability::Unavailable | Spec031Availability::Unknown => {
            "collect missing required component observation and rerun diagnostics"
        }
    };
    Ok(Some(Spec031SafeSummary::try_new(text)?))
}

fn component_remediation(
    component: &Spec031ReadinessObservation,
) -> Result<Option<Spec031SafeSummary>, Spec031ConstructionError> {
    let text = match component.state {
        Spec031Availability::Ready => return Ok(None),
        Spec031Availability::Degraded => {
            "review this component limitation and retry the affected feature later"
        }
        Spec031Availability::Blocked => match component.kind {
            super::Spec031ReadinessComponentKind::ProviderAuth => {
                "configure provider auth evidence and rerun diagnostics"
            }
            super::Spec031ReadinessComponentKind::Storage => {
                "run the documented storage or migration recovery command later"
            }
            super::Spec031ReadinessComponentKind::Containment => {
                "start from supported containment or accept degraded native-host evidence later"
            }
            super::Spec031ReadinessComponentKind::ChannelWorker => {
                "fix channel worker configuration or restart the local runtime later"
            }
            super::Spec031ReadinessComponentKind::PluginApp => {
                "inspect plugin or app owner diagnostics before enabling dependent features"
            }
            super::Spec031ReadinessComponentKind::Queue => {
                "clear or recover blocked durable work before admitting more runtime work"
            }
            super::Spec031ReadinessComponentKind::ExternalIntegration => {
                "configure the optional integration before using that feature"
            }
        },
        Spec031Availability::Unavailable | Spec031Availability::Unknown => match component.kind {
            super::Spec031ReadinessComponentKind::ProviderAuth => {
                "provide provider auth evidence and rerun diagnostics"
            }
            super::Spec031ReadinessComponentKind::Storage => {
                "inspect storage and migration state before starting writable runtime"
            }
            super::Spec031ReadinessComponentKind::Containment => {
                "collect containment evidence or treat native host as unknown"
            }
            super::Spec031ReadinessComponentKind::ChannelWorker => {
                "start runtime or inspect channel worker evidence later"
            }
            super::Spec031ReadinessComponentKind::PluginApp => {
                "collect plugin or app owner evidence later"
            }
            super::Spec031ReadinessComponentKind::Queue => {
                "collect queue depth and admission evidence later"
            }
            super::Spec031ReadinessComponentKind::ExternalIntegration => {
                "configure the optional integration before using that feature"
            }
        },
    };
    Ok(Some(Spec031SafeSummary::try_new(text)?))
}
