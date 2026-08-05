use super::owner_refs::{action_ref, digest, subject_ref, summary};
use super::owner_values::{
    canonical_capability, external_owner, freshness, missing_capability, reason, source_owner,
    state,
};
use super::redaction::{construction_error, Spec031ConstructionViolation};
use super::{
    Spec031ActionRef, Spec031Availability, Spec031Capability, Spec031ConstructionError,
    Spec031Digest, Spec031Envelope, Spec031EnvelopeInput, Spec031FixtureFamily, Spec031Freshness,
    Spec031Lineage, Spec031ObservedAtUnixMs, Spec031ParentRef, Spec031ProjectionKind,
    Spec031Reason, Spec031ReasonCode, Spec031SafeSummary, Spec031SchemaVersion, Spec031Severity,
    Spec031Source, Spec031SourceOwner, Spec031SubjectRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031OwnerEvidenceReason {
    MissingExternalOwnerEvidence,
}

impl Spec031OwnerEvidenceReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingExternalOwnerEvidence => "missing_external_owner_evidence",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031OwnerRecordProjectionInput {
    pub family: Spec031FixtureFamily,
    pub subject_ref: Spec031SubjectRef,
    pub parent_ref: Option<Spec031ParentRef>,
    pub action_ref: Option<Spec031ActionRef>,
    pub digest: Option<Spec031Digest>,
    pub owner: Spec031SourceOwner,
    pub observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    pub freshness: Spec031Freshness,
    pub state: Spec031Availability,
    pub severity: Spec031Severity,
    pub reason_code: Spec031ReasonCode,
    pub safe_summary: Spec031SafeSummary,
    pub capability: Spec031Capability,
}

pub fn spec031_project_owner_record(
    input: Spec031OwnerRecordProjectionInput,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    if !capability_matches_family(input.family, &input.capability) {
        return Err(construction_error(
            "capability.kind",
            Spec031ConstructionViolation::CapabilityFamilyMismatch,
        ));
    }
    Spec031Envelope::try_new(Spec031EnvelopeInput {
        schema_version: Spec031SchemaVersion::CURRENT,
        kind: projection_kind(input.family),
        state: input.state,
        severity: input.severity,
        reason: Spec031Reason {
            code: input.reason_code,
            safe_summary: input.safe_summary,
        },
        lineage: Spec031Lineage {
            subject_ref: input.subject_ref,
            parent_ref: input.parent_ref,
            action_ref: input.action_ref,
            digest: input.digest,
        },
        source: Spec031Source {
            owner: input.owner,
            observed_at_unix_ms: input.observed_at_unix_ms,
            freshness: input.freshness,
        },
        capability: input.capability,
        children: Vec::new(),
    })
}

pub fn spec031_missing_external_owner_evidence(
    family: Spec031FixtureFamily,
) -> Result<Spec031Envelope, Spec031ConstructionError> {
    spec031_project_owner_record(Spec031OwnerRecordProjectionInput {
        family,
        subject_ref: Spec031SubjectRef::try_new(subject_ref(family))?,
        parent_ref: Some(Spec031ParentRef::try_new(
            "parent:external-owner:canonical",
        )?),
        action_ref: Some(Spec031ActionRef::try_new(action_ref(family))?),
        digest: Some(Spec031Digest::try_new(digest(family))?),
        owner: external_owner(family),
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
        freshness: Spec031Freshness::Unavailable,
        state: Spec031Availability::Unavailable,
        severity: Spec031Severity::Error,
        reason_code: Spec031ReasonCode::MissingExternalOwnerEvidence,
        safe_summary: Spec031SafeSummary::try_new("external owner evidence is missing")?,
        capability: missing_capability(family),
    })
}

pub(super) fn canonical_owner_record_input(
    family: Spec031FixtureFamily,
) -> Result<Spec031OwnerRecordProjectionInput, Spec031ConstructionError> {
    Ok(Spec031OwnerRecordProjectionInput {
        family,
        subject_ref: Spec031SubjectRef::try_new(subject_ref(family))?,
        parent_ref: Some(Spec031ParentRef::try_new("parent:session:canonical")?),
        action_ref: Some(Spec031ActionRef::try_new(action_ref(family))?),
        digest: Some(Spec031Digest::try_new(digest(family))?),
        owner: source_owner(family),
        observed_at_unix_ms: Some(Spec031ObservedAtUnixMs::new(31)),
        freshness: freshness(family),
        state: state(family),
        severity: super::owner_values::severity(family),
        reason_code: reason(family),
        safe_summary: Spec031SafeSummary::try_new(summary(family))?,
        capability: canonical_capability(family),
    })
}

fn projection_kind(family: Spec031FixtureFamily) -> Spec031ProjectionKind {
    match family {
        Spec031FixtureFamily::Session => Spec031ProjectionKind::Session,
        Spec031FixtureFamily::Turn => Spec031ProjectionKind::Turn,
        Spec031FixtureFamily::Subagent => Spec031ProjectionKind::Subagent,
        Spec031FixtureFamily::Tool => Spec031ProjectionKind::Tool,
        Spec031FixtureFamily::Approval => Spec031ProjectionKind::Approval,
        Spec031FixtureFamily::Recovery => Spec031ProjectionKind::Diagnostics,
        Spec031FixtureFamily::Readiness => Spec031ProjectionKind::Readiness,
        Spec031FixtureFamily::Context => Spec031ProjectionKind::Context,
        Spec031FixtureFamily::Extension => Spec031ProjectionKind::Plugin,
        Spec031FixtureFamily::ExternalAppOwner => Spec031ProjectionKind::App,
        Spec031FixtureFamily::ExternalMediaOwner => Spec031ProjectionKind::Media,
        Spec031FixtureFamily::Delivery => Spec031ProjectionKind::Progress,
        Spec031FixtureFamily::ReleaseEvidence => Spec031ProjectionKind::ReleaseEvidence,
    }
}

fn capability_matches_family(family: Spec031FixtureFamily, capability: &Spec031Capability) -> bool {
    matches!(
        (family, capability),
        (Spec031FixtureFamily::Session, Spec031Capability::Session(_))
            | (Spec031FixtureFamily::Turn, Spec031Capability::Turn(_))
            | (
                Spec031FixtureFamily::Subagent,
                Spec031Capability::Subagent(_)
            )
            | (Spec031FixtureFamily::Tool, Spec031Capability::Tool(_))
            | (
                Spec031FixtureFamily::Approval,
                Spec031Capability::Approval(_)
            )
            | (
                Spec031FixtureFamily::Recovery,
                Spec031Capability::Diagnostics(_)
            )
            | (
                Spec031FixtureFamily::Readiness,
                Spec031Capability::Readiness(_)
            )
            | (Spec031FixtureFamily::Context, Spec031Capability::Context(_))
            | (
                Spec031FixtureFamily::Extension,
                Spec031Capability::Plugin(_)
            )
            | (
                Spec031FixtureFamily::ExternalAppOwner,
                Spec031Capability::App(_)
            )
            | (
                Spec031FixtureFamily::ExternalMediaOwner,
                Spec031Capability::Media(_)
            )
            | (
                Spec031FixtureFamily::Delivery,
                Spec031Capability::Progress(_)
            )
            | (
                Spec031FixtureFamily::ReleaseEvidence,
                Spec031Capability::ReleaseEvidence(_)
            )
    )
}
