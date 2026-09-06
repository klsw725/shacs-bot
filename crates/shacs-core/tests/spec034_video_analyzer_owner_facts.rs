use shacs_core::runtime::{
    project_video_analyzer, VideoAnalysisPolicy, VideoAnalyzerCapability,
    VideoAnalyzerOwnerFactsInput, VideoAnalyzerOwnerUnavailableReason,
    VideoAnalyzerProjectionInput,
};
use shacs_projection::Spec031Freshness;
use std::error::Error;

#[path = "spec034_video_analyzer_owner_facts/support.rs"]
mod support;
use support::OwnerFixture;

#[test]
fn current_owner_facts_project_only_safe_canonical_summaries() -> Result<(), Box<dyn Error>> {
    let fixture = OwnerFixture::new("execution:spec034:fixture", None)?;
    let projection = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: fixture.input(Spec031Freshness::Current),
    })?;
    let serialized = serde_json::to_string(&projection.owner_facts)?;

    assert_eq!(projection.owner_facts.freshness, Spec031Freshness::Current);
    assert!(projection.owner_facts.unavailable_reasons.is_empty());
    assert!(serialized.contains("spec034://media/analyzer/fixture"));
    assert!(serialized.contains("explicitOrTrustedWorkspace"));
    assert!(serialized.contains("trusted_code_disclosure"));
    assert!(serialized.contains("credential"));
    assert!(serialized.contains("disclosure"));
    assert!(serialized.contains("execution:spec034:fixture"));
    assert!(!serialized.contains("/Users/private"));
    assert!(!serialized.contains("secret-token"));
    Ok(())
}

#[test]
fn missing_stale_and_malformed_owner_ids_are_unavailable() -> Result<(), Box<dyn Error>> {
    let fixture = OwnerFixture::new("execution:spec034:fixture", None)?;
    let missing = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: VideoAnalyzerOwnerFactsInput {
            analyzer_ref: None,
            ..fixture.input(Spec031Freshness::Current)
        },
    })?;
    assert!(missing
        .owner_facts
        .unavailable_reasons
        .contains(&VideoAnalyzerOwnerUnavailableReason::MissingAnalyzerOwnerRef));
    assert!(missing.owner_facts.source.is_none());

    let stale = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: fixture.input(Spec031Freshness::Stale),
    })?;
    assert_eq!(
        stale.owner_facts.unavailable_reasons,
        vec![VideoAnalyzerOwnerUnavailableReason::StaleOwnerFacts]
    );
    assert!(stale.owner_facts.snapshot.is_none());

    let malformed_fixture = OwnerFixture::new("/Users/private/snapshot", None)?;
    let malformed = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: malformed_fixture.input(Spec031Freshness::Current),
    })?;
    assert_eq!(
        malformed.owner_facts.unavailable_reasons,
        vec![VideoAnalyzerOwnerUnavailableReason::SnapshotRefMalformed]
    );
    assert!(!serde_json::to_string(&malformed)?.contains("/Users/private"));
    Ok(())
}
