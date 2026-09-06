#[path = "spec034_video_analyzer_spec035/support.rs"]
mod support;

use shacs_core::runtime::{
    project_media_evidence_diagnostics, project_video_analyzer_spec035,
    MediaEvidenceDiagnosticsInput, VideoAnalyzerSpec035Error, VideoAnalyzerSpec035Input,
    VideoAnalyzerStatus,
};
use shacs_projection::{Spec031Freshness, Spec035MediaDisclosure, Spec035MediaState};
use std::error::Error;
use support::{analyzer_projection, artifact_ref, disclosure, stale_success_projection};

#[test]
fn mapper_exhaustively_preserves_terminal_status_meaning() -> Result<(), Box<dyn Error>> {
    // Given
    let cases = [
        (
            VideoAnalyzerStatus::AnalyzerMissing,
            Spec035MediaState::AnalyzerMissing,
        ),
        (
            VideoAnalyzerStatus::Unsupported,
            Spec035MediaState::Unsupported,
        ),
        (
            VideoAnalyzerStatus::DurationCap,
            Spec035MediaState::Unsupported,
        ),
        (
            VideoAnalyzerStatus::ExtractionFailed,
            Spec035MediaState::ExtractionFailed,
        ),
        (
            VideoAnalyzerStatus::Cancelled,
            Spec035MediaState::ExtractionFailed,
        ),
        (
            VideoAnalyzerStatus::Timeout,
            Spec035MediaState::ExtractionFailed,
        ),
        (VideoAnalyzerStatus::Included, Spec035MediaState::Included),
        (VideoAnalyzerStatus::Truncated, Spec035MediaState::Truncated),
    ];

    for (status, expected) in cases {
        // When
        let analyzer = analyzer_projection(status)?;
        let mapped = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
            artifact_ref: &artifact_ref()?,
            analyzer: &analyzer,
        })?;

        // Then
        assert_eq!(mapped.state(), expected, "source status {status:?}");
    }
    Ok(())
}

#[test]
fn mapper_rejects_configured_nonterminal_status() -> Result<(), Box<dyn Error>> {
    // Given
    let analyzer = analyzer_projection(VideoAnalyzerStatus::Configured)?;

    // When
    let result = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref()?,
        analyzer: &analyzer,
    });

    // Then
    assert_eq!(
        result,
        Err(VideoAnalyzerSpec035Error::NonTerminalConfigured)
    );
    Ok(())
}

#[test]
fn mapper_downgrades_stale_success_without_stale_lineage() -> Result<(), Box<dyn Error>> {
    // Given
    let analyzer = stale_success_projection()?;

    // When
    let mapped = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref()?,
        analyzer: &analyzer,
    })?;
    let value = serde_json::to_value(&mapped)?;

    // Then
    assert_eq!(mapped.state(), Spec035MediaState::Unavailable);
    assert_eq!(mapped.freshness(), Spec031Freshness::Stale);
    assert!(value["lineage"].get("analyzer_ref").is_none());
    assert!(value["lineage"].get("snapshot_ref").is_none());
    assert!(value["lineage"].get("evidence_digest").is_none());
    Ok(())
}

#[test]
fn mapper_rejects_incomplete_current_owner_facts() -> Result<(), Box<dyn Error>> {
    // Given
    let mut analyzer = analyzer_projection(VideoAnalyzerStatus::Included)?;
    analyzer.owner_facts.snapshot = None;

    // When
    let result = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref()?,
        analyzer: &analyzer,
    });

    // Then
    assert_eq!(
        result,
        Err(VideoAnalyzerSpec035Error::InconsistentOwnerFacts)
    );
    Ok(())
}

#[test]
fn mapper_copies_current_owner_facts_once_and_shares_diagnostics_digest(
) -> Result<(), Box<dyn Error>> {
    // Given
    let analyzer = analyzer_projection(VideoAnalyzerStatus::Included)?;
    let diagnostics = project_media_evidence_diagnostics(MediaEvidenceDiagnosticsInput {
        artifacts: &[],
        analyzer: &analyzer,
        disclosure: &disclosure(),
    })?;

    // When
    let mapped = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref()?,
        analyzer: &analyzer,
    })?;
    let value = serde_json::to_value(&mapped)?;

    // Then
    let source = analyzer
        .owner_facts
        .source
        .as_ref()
        .ok_or("source missing")?;
    let owner = mapped.owner_facts();
    let mapped_source = owner
        .analyzer_source
        .as_ref()
        .ok_or("mapped source missing")?;
    assert_eq!(mapped_source.analyzer_ref, source.analyzer_ref);
    assert_eq!(mapped_source.source, source.source);
    assert_eq!(mapped_source.activation, source.activation);
    assert_eq!(mapped_source.trust, source.trust);
    assert_eq!(
        mapped_source.trusted_code_disclosure,
        source.trusted_code_disclosure
    );
    assert_eq!(
        owner.sandbox.as_ref(),
        analyzer.owner_facts.sandbox.as_ref()
    );
    assert_eq!(
        owner.credential.as_ref(),
        analyzer.owner_facts.credential.as_ref()
    );
    let source_disclosure = analyzer
        .owner_facts
        .disclosure
        .as_ref()
        .ok_or("source disclosure missing")?;
    let Spec035MediaDisclosure::Recorded(mapped_disclosure) = mapped.disclosure() else {
        return Err("mapped disclosure missing".into());
    };
    assert_eq!(
        mapped_disclosure.raw_content_possible,
        source_disclosure.raw_content_possible
    );
    assert_eq!(mapped_disclosure.surfaces, source_disclosure.surfaces);
    assert_eq!(
        mapped_disclosure.trace_status,
        source_disclosure.trace.status
    );
    assert_eq!(
        owner
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.provenance_digest.as_str()),
        analyzer
            .owner_facts
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.provenance_digest.as_str())
    );
    assert_eq!(
        value["owner_facts"]
            .as_object()
            .ok_or("owner object")?
            .len(),
        5
    );
    assert_eq!(
        value["lineage"]["evidence_digest"].as_str(),
        diagnostics.analyzer.evidence_digest.as_deref()
    );
    assert_eq!(
        value["lineage"]["snapshot_ref"].as_str(),
        analyzer
            .owner_facts
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot_id.as_str())
    );
    Ok(())
}
