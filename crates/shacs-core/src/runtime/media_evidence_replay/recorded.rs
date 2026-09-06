use super::model::{
    projection_digest, safe_digest, safe_snapshot, AnalyzerEvidenceSummary,
    MediaEvidenceAvailability, MediaEvidenceDiagnostics, MediaEvidenceProjectionError,
    MediaEvidenceReplayDependencies, MediaEvidenceReplayError, MediaEvidenceReplayReceipt,
    MediaEvidenceReplaySource, RecordedAnalyzerStatus, RecordedArtifactStatus,
    MEDIA_EVIDENCE_SCHEMA,
};
use shacs_projection::Spec031Freshness;

pub fn replay_recorded_media_evidence(
    recorded: &str,
    _dependencies: &dyn MediaEvidenceReplayDependencies,
) -> Result<MediaEvidenceReplayReceipt, MediaEvidenceReplayError> {
    let value: serde_json::Value =
        serde_json::from_str(recorded).map_err(|_| MediaEvidenceReplayError::Malformed)?;
    if value.get("schema").and_then(serde_json::Value::as_str) != Some(MEDIA_EVIDENCE_SCHEMA) {
        return Err(MediaEvidenceReplayError::UnknownSchema);
    }
    let projection: MediaEvidenceDiagnostics =
        serde_json::from_value(value).map_err(|_| MediaEvidenceReplayError::Malformed)?;
    let expected = projection_digest(&projection).map_err(map_projection_error)?;
    if projection.facts_digest != expected {
        return Err(MediaEvidenceReplayError::DigestMismatch);
    }
    match projection.freshness {
        Spec031Freshness::Current => {}
        Spec031Freshness::Stale => return Err(MediaEvidenceReplayError::StaleFacts),
        Spec031Freshness::Unavailable | Spec031Freshness::Unknown => {
            return Err(MediaEvidenceReplayError::UnavailableFacts);
        }
    }
    if projection.availability != MediaEvidenceAvailability::Available {
        return Err(MediaEvidenceReplayError::UnavailableFacts);
    }
    if !valid_analyzer_state(&projection.analyzer) {
        return Err(MediaEvidenceReplayError::InvalidAnalyzerState);
    }
    let snapshot = projection
        .snapshot
        .filter(safe_snapshot)
        .ok_or(MediaEvidenceReplayError::UnavailableFacts)?;
    let artifact_status = match projection.artifacts.len() {
        0 => RecordedArtifactStatus::Unavailable,
        _ => RecordedArtifactStatus::Recorded,
    };
    Ok(MediaEvidenceReplayReceipt {
        source: MediaEvidenceReplaySource::RecordedMetadata,
        artifact_status,
        artifact_count: projection.artifacts.len(),
        analyzer_status: projection.analyzer.status,
        disclosure: projection.disclosure,
        snapshot,
        facts_digest: projection.facts_digest,
    })
}

fn valid_analyzer_state(analyzer: &AnalyzerEvidenceSummary) -> bool {
    match analyzer.status {
        RecordedAnalyzerStatus::Included => {
            analyzer.evidence_available
                && analyzer.evidence_digest.as_deref().is_some_and(safe_digest)
                && !analyzer.truncated
        }
        RecordedAnalyzerStatus::Truncated => {
            analyzer.evidence_available
                && analyzer.evidence_digest.as_deref().is_some_and(safe_digest)
                && analyzer.truncated
        }
        RecordedAnalyzerStatus::Configured
        | RecordedAnalyzerStatus::AnalyzerMissing
        | RecordedAnalyzerStatus::Unsupported
        | RecordedAnalyzerStatus::ExtractionFailed
        | RecordedAnalyzerStatus::DurationCap
        | RecordedAnalyzerStatus::Cancelled
        | RecordedAnalyzerStatus::Timeout => {
            !analyzer.evidence_available
                && analyzer.evidence_digest.is_none()
                && analyzer.component_failure_count == 0
                && !analyzer.truncated
        }
    }
}

const fn map_projection_error(_error: MediaEvidenceProjectionError) -> MediaEvidenceReplayError {
    MediaEvidenceReplayError::Malformed
}
