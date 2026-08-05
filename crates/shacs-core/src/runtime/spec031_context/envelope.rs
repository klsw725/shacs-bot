use super::reason::{availability, severity};
use super::types::Spec031ContextEvidenceRow;
use shacs_projection::{
    Spec031Capability, Spec031ConstructionError, Spec031ContextCapability, Spec031Envelope,
    Spec031EnvelopeInput, Spec031Freshness, Spec031Lineage, Spec031ParentRef,
    Spec031ProjectionKind, Spec031Reason, Spec031ReasonCode, Spec031SchemaVersion, Spec031Source,
    Spec031SourceOwner, Spec031SubjectRef,
};

pub(super) fn envelope_from_row(
    row: &Spec031ContextEvidenceRow,
    parent_ref: Option<&str>,
    freshness: Spec031Freshness,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: Spec031ProjectionKind::Context,
        state: availability(row.reason),
        severity: severity(row.reason),
        reason: Spec031Reason {
            code: Spec031ReasonCode::from(row.reason),
            safe_summary: row.result_summary.clone(),
        },
        lineage: Spec031Lineage {
            subject_ref: Spec031SubjectRef::try_new(row.opaque_ref.as_str())?,
            parent_ref: parent_ref.map(Spec031ParentRef::try_new).transpose()?,
            action_ref: None,
            digest: None,
        },
        source: Spec031Source {
            owner: Spec031SourceOwner::Spec031,
            observed_at_unix_ms: None,
            freshness,
        },
        capability: Spec031Capability::Context(Spec031ContextCapability { reason: row.reason }),
        children: Vec::new(),
    })
}
