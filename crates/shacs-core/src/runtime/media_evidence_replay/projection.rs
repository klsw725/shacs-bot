use super::model::{
    projection_digest, safe_snapshot, sha256, AnalyzerEvidenceSummary, ArtifactEvidenceSummary,
    MediaDisclosureSummary, MediaEvidenceAvailability, MediaEvidenceDiagnostics,
    MediaEvidenceDiagnosticsInput, MediaEvidenceProjectionError, RecordedAnalyzerStatus,
    MEDIA_EVIDENCE_SCHEMA,
};
use crate::generated_media::GeneratedArtifactRecord;
use crate::runtime::{
    VideoAnalyzerEvidenceProjection, VideoAnalyzerOwnerFactsProjection, VideoAnalyzerProjection,
    VideoAnalyzerStatus,
};
use shacs_projection::Spec031Freshness;

pub fn project_media_evidence_diagnostics(
    input: MediaEvidenceDiagnosticsInput<'_>,
) -> Result<MediaEvidenceDiagnostics, MediaEvidenceProjectionError> {
    let artifacts = input
        .artifacts
        .iter()
        .map(project_artifact)
        .collect::<Result<Vec<_>, _>>()?;
    let analyzer = project_analyzer(input.analyzer)?;
    let snapshot = validated_owner_snapshot(&input.analyzer.owner_facts, input.disclosure);
    let availability = match snapshot {
        Some(_) => MediaEvidenceAvailability::Available,
        None => MediaEvidenceAvailability::Unavailable,
    };
    let mut projection = MediaEvidenceDiagnostics {
        schema: MEDIA_EVIDENCE_SCHEMA.to_owned(),
        availability,
        artifacts,
        analyzer,
        disclosure: MediaDisclosureSummary {
            raw_content_possible: input.disclosure.raw_content_possible,
            surfaces: input.disclosure.surfaces.clone(),
        },
        snapshot,
        freshness: input.analyzer.owner_facts.freshness,
        facts_digest: String::new(),
    };
    projection.facts_digest = projection_digest(&projection)?;
    Ok(projection)
}

fn project_artifact(
    record: &GeneratedArtifactRecord,
) -> Result<ArtifactEvidenceSummary, MediaEvidenceProjectionError> {
    if record.schema != "shacs.generated-artifact.v1" {
        return Err(MediaEvidenceProjectionError::InvalidRecord);
    }
    Ok(ArtifactEvidenceSummary {
        artifact_id: record.artifact_id.as_str().to_owned(),
        kind: record.kind,
        operation: record.provenance.operation,
        byte_len: record.byte_len,
        sha256: record.sha256.as_str().to_owned(),
        retention: record.retention.clone(),
    })
}

fn project_analyzer(
    projection: &VideoAnalyzerProjection,
) -> Result<AnalyzerEvidenceSummary, MediaEvidenceProjectionError> {
    let (evidence_available, evidence_digest, component_failure_count, truncated) =
        match projection.evidence.as_ref() {
            Some(evidence) => (
                true,
                Some(analyzer_evidence_digest(evidence)?),
                evidence.component_failures.len(),
                evidence.truncated,
            ),
            None => (false, None, 0, false),
        };
    Ok(AnalyzerEvidenceSummary {
        status: match projection.status {
            VideoAnalyzerStatus::Configured => RecordedAnalyzerStatus::Configured,
            VideoAnalyzerStatus::AnalyzerMissing => RecordedAnalyzerStatus::AnalyzerMissing,
            VideoAnalyzerStatus::Unsupported => RecordedAnalyzerStatus::Unsupported,
            VideoAnalyzerStatus::ExtractionFailed => RecordedAnalyzerStatus::ExtractionFailed,
            VideoAnalyzerStatus::Included => RecordedAnalyzerStatus::Included,
            VideoAnalyzerStatus::Truncated => RecordedAnalyzerStatus::Truncated,
            VideoAnalyzerStatus::DurationCap => RecordedAnalyzerStatus::DurationCap,
            VideoAnalyzerStatus::Cancelled => RecordedAnalyzerStatus::Cancelled,
            VideoAnalyzerStatus::Timeout => RecordedAnalyzerStatus::Timeout,
        },
        evidence_available,
        evidence_digest,
        component_failure_count,
        truncated,
    })
}

pub(crate) fn analyzer_evidence_digest(
    evidence: &VideoAnalyzerEvidenceProjection,
) -> Result<String, MediaEvidenceProjectionError> {
    let bytes =
        serde_json::to_vec(evidence).map_err(|_| MediaEvidenceProjectionError::InvalidRecord)?;
    Ok(sha256(&bytes))
}

fn validated_owner_snapshot(
    owner: &VideoAnalyzerOwnerFactsProjection,
    disclosure: &shacs_projection::DataDisclosureProjection,
) -> Option<crate::runtime::VideoAnalyzerSnapshotProjection> {
    if owner.freshness != Spec031Freshness::Current
        || !owner.unavailable_reasons.is_empty()
        || owner.source.is_none()
        || owner.sandbox.is_none()
        || owner.credential.is_none()
    {
        return None;
    }
    let owner_disclosure = owner.disclosure.as_ref()?;
    if owner_disclosure.raw_content_possible != disclosure.raw_content_possible
        || owner_disclosure.surfaces != disclosure.surfaces
    {
        return None;
    }
    owner.snapshot.clone().filter(safe_snapshot)
}
