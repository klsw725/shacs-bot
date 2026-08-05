use shacs_projection::spec031::{
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031ConstructionError,
    Spec031Envelope, Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage,
    Spec031ObservedAtUnixMs, Spec031ParentRef, Spec031ProgressCapability, Spec031ProgressDelivery,
    Spec031ProjectionKind, Spec031Reason, Spec031ReasonCode, Spec031SafeSummary,
    Spec031SchemaVersion, Spec031Severity, Spec031Source, Spec031SourceOwner, Spec031SubjectRef,
};

use super::ChannelDeliveryObservation;

pub(super) struct ChannelProjectionParts {
    pub(super) channel: String,
    pub(super) event_kind: &'static str,
    pub(super) delivery: Spec031ProgressDelivery,
    pub(super) state: Spec031Availability,
    pub(super) severity: Spec031Severity,
    pub(super) reason_code: Spec031ReasonCode,
    pub(super) safe_summary: String,
    pub(super) parent_ref: Option<String>,
    pub(super) action_ref: Option<String>,
    pub(super) freshness: Spec031Freshness,
    pub(super) delivery_observation: ChannelDeliveryObservation,
}

impl ChannelProjectionParts {
    pub(super) fn into_envelope(
        self,
        observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    ) -> Result<Spec031Envelope, Spec031ConstructionError> {
        Spec031Envelope::try_new(Spec031EnvelopeInput {
            schema_version: Spec031SchemaVersion::CURRENT,
            kind: Spec031ProjectionKind::Progress,
            state: self.state,
            severity: self.severity,
            reason: Spec031Reason {
                code: self.reason_code,
                safe_summary: Spec031SafeSummary::try_new(&self.safe_summary)?,
            },
            lineage: self.lineage()?,
            source: Spec031Source {
                owner: Spec031SourceOwner::Channel,
                observed_at_unix_ms,
                freshness: self.freshness,
            },
            capability: Spec031Capability::Progress(
                self.delivery_observation
                    .apply_to(Spec031ProgressCapability::delivery(self.delivery)),
            ),
            children: Vec::new(),
        })
    }

    pub(super) fn with_lineage(
        mut self,
        parent_ref: Option<String>,
        action_ref: Option<String>,
    ) -> Self {
        self.parent_ref = parent_ref;
        self.action_ref = action_ref;
        self
    }

    pub(super) const fn with_freshness(mut self, freshness: Spec031Freshness) -> Self {
        self.freshness = freshness;
        self
    }

    fn lineage(&self) -> Result<Spec031Lineage, Spec031ConstructionError> {
        Ok(Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(&format!(
                "subject:channel:{}:{}",
                self.channel, self.event_kind
            ))?,
            parent_ref: self
                .parent_ref
                .as_deref()
                .map(Spec031ParentRef::try_new)
                .transpose()?,
            action_ref: self
                .action_ref
                .as_deref()
                .map(Spec031ActionRef::try_new)
                .transpose()?,
            digest: None,
        })
    }
}
