use super::VideoAnalyzerSpec035Error;
use crate::runtime::{VideoAnalyzerOwnerFactsProjection, VideoAnalyzerOwnerUnavailableReason};
use shacs_projection::{
    Spec031ExternalOwnerRef, Spec031Freshness, Spec035MediaDigest, Spec035MediaDisclosureFact,
    Spec035MediaOpaqueRef, Spec035MediaOwnerFactInput, Spec035MediaOwnerFactsInput,
    Spec035MediaOwnerUnavailableReason,
};

pub(super) struct OwnerMapping {
    pub input: Spec035MediaOwnerFactsInput,
    pub analyzer_ref: Option<Spec031ExternalOwnerRef>,
    pub snapshot_ref: Option<Spec035MediaOpaqueRef>,
}

pub(super) fn map_owner_facts(
    owner: &VideoAnalyzerOwnerFactsProjection,
) -> Result<OwnerMapping, VideoAnalyzerSpec035Error> {
    match owner.freshness {
        Spec031Freshness::Current => map_current(owner),
        Spec031Freshness::Stale | Spec031Freshness::Unavailable | Spec031Freshness::Unknown => {
            map_unavailable(owner)
        }
    }
}

fn map_current(
    owner: &VideoAnalyzerOwnerFactsProjection,
) -> Result<OwnerMapping, VideoAnalyzerSpec035Error> {
    let (Some(source), Some(sandbox), Some(credential), Some(disclosure), Some(snapshot)) = (
        owner.source.as_ref(),
        owner.sandbox.as_ref(),
        owner.credential.as_ref(),
        owner.disclosure.as_ref(),
        owner.snapshot.as_ref(),
    ) else {
        return Err(VideoAnalyzerSpec035Error::InconsistentOwnerFacts);
    };
    if !owner.unavailable_reasons.is_empty() {
        return Err(VideoAnalyzerSpec035Error::InconsistentOwnerFacts);
    }
    let snapshot_ref = Spec035MediaOpaqueRef::try_new(&snapshot.snapshot_id)?;
    let analyzer_ref = source.analyzer_ref.clone();
    Ok(OwnerMapping {
        input: Spec035MediaOwnerFactsInput {
            freshness: Spec031Freshness::Current,
            unavailable_reasons: Vec::new(),
            facts: vec![
                Spec035MediaOwnerFactInput::AnalyzerSource {
                    analyzer_ref: analyzer_ref.clone(),
                    source: source.source,
                    activation: source.activation,
                    trust: source.trust,
                    trusted_code_disclosure: source.trusted_code_disclosure,
                },
                Spec035MediaOwnerFactInput::Sandbox(sandbox.clone()),
                Spec035MediaOwnerFactInput::Credential(credential.clone()),
                Spec035MediaOwnerFactInput::Disclosure(Spec035MediaDisclosureFact {
                    raw_content_possible: disclosure.raw_content_possible,
                    surfaces: disclosure.surfaces.clone(),
                    trace_status: disclosure.trace.status,
                }),
                Spec035MediaOwnerFactInput::Snapshot {
                    snapshot_ref: snapshot_ref.clone(),
                    provenance_digest: Spec035MediaDigest::try_new(&snapshot.provenance_digest)?,
                },
            ],
        },
        analyzer_ref: Some(analyzer_ref),
        snapshot_ref: Some(snapshot_ref),
    })
}

fn map_unavailable(
    owner: &VideoAnalyzerOwnerFactsProjection,
) -> Result<OwnerMapping, VideoAnalyzerSpec035Error> {
    if owner.unavailable_reasons.is_empty()
        || owner.source.is_some()
        || owner.sandbox.is_some()
        || owner.credential.is_some()
        || owner.disclosure.is_some()
        || owner.snapshot.is_some()
    {
        return Err(VideoAnalyzerSpec035Error::InconsistentOwnerFacts);
    }
    Ok(OwnerMapping {
        input: Spec035MediaOwnerFactsInput {
            freshness: owner.freshness,
            unavailable_reasons: owner
                .unavailable_reasons
                .iter()
                .copied()
                .map(map_unavailable_reason)
                .collect(),
            facts: Vec::new(),
        },
        analyzer_ref: None,
        snapshot_ref: None,
    })
}

const fn map_unavailable_reason(
    reason: VideoAnalyzerOwnerUnavailableReason,
) -> Spec035MediaOwnerUnavailableReason {
    match reason {
        VideoAnalyzerOwnerUnavailableReason::MissingAnalyzerOwnerRef => {
            Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef
        }
        VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts => {
            Spec035MediaOwnerUnavailableReason::MissingSpec030OwnerFacts
        }
        VideoAnalyzerOwnerUnavailableReason::MissingExecutionSnapshot => {
            Spec035MediaOwnerUnavailableReason::MissingExecutionSnapshot
        }
        VideoAnalyzerOwnerUnavailableReason::StaleOwnerFacts => {
            Spec035MediaOwnerUnavailableReason::StaleOwnerFacts
        }
        VideoAnalyzerOwnerUnavailableReason::OwnerFactsUnavailable => {
            Spec035MediaOwnerUnavailableReason::OwnerFactsUnavailable
        }
        VideoAnalyzerOwnerUnavailableReason::OwnerFreshnessUnknown => {
            Spec035MediaOwnerUnavailableReason::OwnerFreshnessUnknown
        }
        VideoAnalyzerOwnerUnavailableReason::AnalyzerResourceMismatch => {
            Spec035MediaOwnerUnavailableReason::AnalyzerResourceMismatch
        }
        VideoAnalyzerOwnerUnavailableReason::SnapshotResourceMissing => {
            Spec035MediaOwnerUnavailableReason::SnapshotResourceMissing
        }
        VideoAnalyzerOwnerUnavailableReason::SnapshotProvenanceInvalid => {
            Spec035MediaOwnerUnavailableReason::SnapshotProvenanceInvalid
        }
        VideoAnalyzerOwnerUnavailableReason::SnapshotRefMalformed => {
            Spec035MediaOwnerUnavailableReason::SnapshotRefMalformed
        }
    }
}
