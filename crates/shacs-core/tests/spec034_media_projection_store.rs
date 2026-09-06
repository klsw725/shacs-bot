#[path = "spec034_video_analyzer_owner_facts/support.rs"]
mod owner_support;

use shacs_core::runtime::{
    project_video_analyzer, project_video_analyzer_spec035, Spec035MediaProjectionStore,
    VideoAnalysisPolicy, VideoAnalyzerCapability, VideoAnalyzerOutcomeInput,
    VideoAnalyzerProjectionInput, VideoAnalyzerSpec035Input, VideoContextAnalysis,
};
use shacs_projection::{Spec031ExternalOwnerRef, Spec031Freshness, Spec035MediaProjection};
use std::error::Error;

#[test]
fn missing_store_is_unavailable_without_creating_a_record() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());

    // When
    let projection = store.read()?;

    // Then
    assert!(projection.is_none());
    assert!(!store.path().exists());
    Ok(())
}

#[test]
fn publish_atomically_roundtrips_the_canonical_projection() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let projection = canonical_projection()?;

    // When
    store.publish(&projection)?;

    // Then
    assert_eq!(store.read()?.as_ref(), Some(&projection));
    assert_eq!(
        store.path().strip_prefix(root.path())?,
        std::path::Path::new("media/projections/current.json")
    );
    Ok(())
}

#[test]
fn malformed_current_record_fails_closed() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    std::fs::create_dir_all(store.path().parent().ok_or("store parent")?)?;
    std::fs::write(store.path(), b"{not-json")?;

    // When / Then
    assert!(store.read().is_err());
    Ok(())
}

#[test]
fn interrupted_temp_write_does_not_replace_the_published_record() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    let projection = canonical_projection()?;
    store.publish(&projection)?;
    std::fs::write(
        store
            .path()
            .parent()
            .ok_or("store parent")?
            .join("current.json.tmp-interrupted"),
        b"{partial",
    )?;

    // When
    let observed = store.read()?;

    // Then
    assert_eq!(observed.as_ref(), Some(&projection));
    Ok(())
}

fn canonical_projection() -> Result<Spec035MediaProjection, Box<dyn Error>> {
    let owner = owner_support::OwnerFixture::new("snapshot:034:store", None)?;
    let analysis = VideoContextAnalysis {
        metadata: None,
        subtitles: Some("stored evidence".to_owned()),
        scene_summary: None,
        keyframe_summary: None,
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    };
    let analyzer = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: owner.input(Spec031Freshness::Current),
    })?;
    let artifact_ref = Spec031ExternalOwnerRef::try_new("spec034://media/artifact/store-test")?;
    Ok(project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref,
        analyzer: &analyzer,
    })?)
}

#[cfg(unix)]
#[test]
fn symlinked_current_record_fails_closed() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    // Given
    let root = tempfile::tempdir()?;
    let store = Spec035MediaProjectionStore::new(root.path());
    std::fs::create_dir_all(store.path().parent().ok_or("store parent")?)?;
    let target = root.path().join("target.json");
    std::fs::write(&target, b"{}")?;
    symlink(target, store.path())?;

    // When / Then
    assert!(store.read().is_err());
    Ok(())
}
