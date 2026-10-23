#[path = "spec034_diagnostics_replay/support.rs"]
mod support;

use shacs_core::runtime::{
    project_media_evidence_diagnostics, replay_recorded_media_evidence, MediaEvidenceAvailability,
    MediaEvidenceDiagnosticsInput, MediaEvidenceReplayDependencies, MediaEvidenceReplayError,
    MediaEvidenceReplaySource, RecordedArtifactStatus,
};
use shacs_projection::Spec031Freshness;
use std::error::Error;
use support::{
    analyzer_projection, artifact_record, disclosure, ownerless_analyzer_projection,
    recorded_with_analyzer_mutation, DependencySpies,
};

#[test]
fn dependency_spies_observe_each_live_callable_invocation() {
    // Given
    let spies = DependencySpies::default();

    // When
    spies.request_network();
    spies.resolve_credential();
    spies.invoke_analyzer();
    spies.resolve_resource();

    // Then
    assert_eq!(spies.counts(), [1, 1, 1, 1]);
}

#[test]
fn diagnostics_serializes_only_bounded_safe_recorded_facts() -> Result<(), Box<dyn Error>> {
    // Given
    let artifact = artifact_record()?;
    let analyzer = analyzer_projection()?;
    let disclosure = disclosure();

    // When
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: std::slice::from_ref(&artifact),
        analyzer: &analyzer,
        disclosure: &disclosure,
    })?;
    let serialized = serde_json::to_string(&diagnostics)?;

    // Then
    assert!(serialized.contains("artifact-034"));
    assert!(serialized.contains("included"));
    assert!(serialized.contains("snapshot:034"));
    assert!(serialized.contains("raw_content_possible"));
    for forbidden in [
        "c2VjcmV0",
        "https://",
        "?token=",
        "Bearer",
        "provider raw body",
        "/Users/",
        "subtitle text",
        "scene text",
        "keyframe text",
    ] {
        assert!(!serialized.contains(forbidden), "leaked {forbidden}");
    }
    Ok(())
}

#[test]
fn replay_uses_callable_dependency_environment_without_invoking_live_dependencies(
) -> Result<(), Box<dyn Error>> {
    // Given
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: &[artifact_record()?],
        analyzer: &analyzer_projection()?,
        disclosure: &disclosure(),
    })?;
    let recorded = serde_json::to_string(&diagnostics)?;
    let spies = DependencySpies::default();

    // When
    let receipt = replay_recorded_media_evidence(&recorded, &spies)?;

    // Then
    assert_eq!(receipt.source, MediaEvidenceReplaySource::RecordedMetadata);
    assert_eq!(receipt.artifact_status, RecordedArtifactStatus::Recorded);
    assert_eq!(receipt.artifact_count, 1);
    assert_eq!(receipt.snapshot.snapshot_id, "snapshot:034");
    assert_eq!(spies.counts(), [0, 0, 0, 0]);
    Ok(())
}

#[test]
fn replay_fails_closed_for_malformed_unknown_stale_or_tampered_facts() -> Result<(), Box<dyn Error>>
{
    // Given
    let malformed = "{";
    let unknown = r#"{"schema":"shacs.spec034.media-evidence.v999"}"#;
    let mut stale = analyzer_projection()?;
    stale.owner_facts.freshness = Spec031Freshness::Stale;
    let stale_record = serde_json::to_string(&project_media_evidence_diagnostics(
        MediaEvidenceDiagnosticsInput {
            artifacts: &[artifact_record()?],
            analyzer: &stale,
            disclosure: &disclosure(),
        },
    )?)?;
    let valid = serde_json::to_string(&project_media_evidence_diagnostics(
        MediaEvidenceDiagnosticsInput {
            artifacts: &[artifact_record()?],
            analyzer: &analyzer_projection()?,
            disclosure: &disclosure(),
        },
    )?)?;
    let tampered = valid.replace("artifact-034", "artifact-035");
    let spies = DependencySpies::default();

    // When / Then
    assert_eq!(
        replay_recorded_media_evidence(malformed, &spies),
        Err(MediaEvidenceReplayError::Malformed)
    );
    assert_eq!(
        replay_recorded_media_evidence(unknown, &spies),
        Err(MediaEvidenceReplayError::UnknownSchema)
    );
    assert_eq!(
        replay_recorded_media_evidence(&stale_record, &spies),
        Err(MediaEvidenceReplayError::StaleFacts)
    );
    assert_eq!(
        replay_recorded_media_evidence(&tampered, &spies),
        Err(MediaEvidenceReplayError::DigestMismatch)
    );
    assert_eq!(spies.counts(), [0, 0, 0, 0]);
    Ok(())
}

#[test]
fn replay_rejects_digest_valid_included_status_without_recorded_evidence(
) -> Result<(), Box<dyn Error>> {
    // Given
    let recorded = recorded_with_analyzer_mutation(|analyzer| {
        analyzer["evidence_available"] = serde_json::json!(false);
        analyzer["evidence_digest"] = serde_json::Value::Null;
    })?;
    let spies = DependencySpies::default();

    // When
    let result = replay_recorded_media_evidence(&recorded, &spies);

    // Then
    assert_eq!(result, Err(MediaEvidenceReplayError::InvalidAnalyzerState));
    assert_eq!(spies.counts(), [0, 0, 0, 0]);
    Ok(())
}

#[test]
fn replay_rejects_digest_valid_truncated_status_without_truncation_evidence(
) -> Result<(), Box<dyn Error>> {
    // Given
    let recorded = recorded_with_analyzer_mutation(|analyzer| {
        analyzer["status"] = serde_json::json!("truncated");
        analyzer["truncated"] = serde_json::json!(false);
    })?;

    // When
    let result = replay_recorded_media_evidence(&recorded, &DependencySpies::default());

    // Then
    assert_eq!(result, Err(MediaEvidenceReplayError::InvalidAnalyzerState));
    Ok(())
}

#[test]
fn replay_rejects_digest_valid_failure_status_with_success_evidence() -> Result<(), Box<dyn Error>>
{
    // Given
    let recorded = recorded_with_analyzer_mutation(|analyzer| {
        analyzer["status"] = serde_json::json!("extraction_failed");
    })?;

    // When
    let result = replay_recorded_media_evidence(&recorded, &DependencySpies::default());

    // Then
    assert_eq!(result, Err(MediaEvidenceReplayError::InvalidAnalyzerState));
    Ok(())
}

#[test]
fn diagnostics_reports_ownerless_analyzer_facts_unavailable() -> Result<(), Box<dyn Error>> {
    // Given
    let analyzer = ownerless_analyzer_projection()?;

    // When
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: &[artifact_record()?],
        analyzer: &analyzer,
        disclosure: &disclosure(),
    })?;

    // Then
    assert_eq!(
        diagnostics.availability,
        MediaEvidenceAvailability::Unavailable
    );
    assert!(diagnostics.snapshot.is_none());
    let replay = replay_recorded_media_evidence(
        &serde_json::to_string(&diagnostics)?,
        &DependencySpies::default(),
    );
    assert_eq!(replay, Err(MediaEvidenceReplayError::UnavailableFacts));
    Ok(())
}
