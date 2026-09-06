use super::{
    project_video_analyzer_spec035, Spec035MediaProjectionStore,
    Spec035MediaProjectionStoreError, Spec035MediaProjectionTransactionStage,
    VideoAnalyzerSpec035Input,
};
use crate::runtime::{
    project_video_analyzer, VideoAnalysisPolicy, VideoAnalyzerCapability,
    VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput, VideoAnalyzerProjectionInput,
};
use shacs_projection::{Spec031ExternalOwnerRef, Spec031Freshness};
use std::error::Error;

#[test]
fn normal_publish_still_roundtrips_the_exact_projection() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let projection = canonical_projection()?;

    // When
    store.publish(&projection)?;

    // Then
    assert_eq!(store.read()?.as_ref(), Some(&projection));
    Ok(())
}

#[test]
fn interrupted_temp_record_still_cannot_replace_the_target() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let projection = canonical_projection()?;
    store.publish(&projection)?;
    std::fs::write(
        store.path().parent().ok_or("store parent")?.join("current.json.tmp-interrupted"),
        b"{partial",
    )?;

    // When
    let observed = store.read()?;

    // Then
    assert_eq!(observed.as_ref(), Some(&projection));
    Ok(())
}

#[test]
fn parent_sync_failure_after_rename_reports_unknown_and_keeps_target_readable(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let projection = canonical_projection()?;

    // When
    let result = store.publish_with_parent_sync(&projection, |_| {
        Err(std::io::Error::other("injected parent sync failure"))
    });

    // Then
    assert!(matches!(
        result,
        Err(Spec035MediaProjectionStoreError::CommitStatusUnknown(
            Spec035MediaProjectionTransactionStage::Renamed
        ))
    ));
    assert_eq!(store.read()?.as_ref(), Some(&projection));
    Ok(())
}

pub(super) fn canonical_projection(
) -> Result<shacs_projection::Spec035MediaProjection, Box<dyn Error>> {
    let analyzer = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: Some(VideoAnalyzerOutcomeInput::Unsupported("unsupported")),
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
    })?;
    let artifact_ref = Spec031ExternalOwnerRef::try_new("spec034://media/artifact/unknown-test")?;
    Ok(project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref,
        analyzer: &analyzer,
    })?)
}
