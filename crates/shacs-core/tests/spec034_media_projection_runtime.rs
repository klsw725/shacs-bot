#[path = "spec034_video_analyzer_owner_facts/support.rs"]
mod owner_support;

use shacs_core::runtime::{
    project_video_analyzer, AnalyzerInvocation, ContextBuildRequest, ContextBuilder,
    Spec035MediaProjectionStore, VideoAnalysisPolicy, VideoAnalyzerCapability,
    VideoAnalyzerOwnerFactsProjection, VideoAnalyzerProjectionInput, VideoContextAnalysis,
    VideoContextAnalyzer, VideoContextError, VideoContextRequest,
};
use shacs_projection::{Spec031Freshness, Spec035MediaState};
use std::error::Error;
use std::sync::Arc;

#[derive(Debug)]
struct IncludedAnalyzer;

impl VideoContextAnalyzer for IncludedAnalyzer {
    fn analyze(
        &self,
        _invocation: &AnalyzerInvocation,
        _request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        Ok(VideoContextAnalysis {
            metadata: None,
            subtitles: Some("runtime subtitle".to_owned()),
            scene_summary: Some("runtime scene".to_owned()),
            keyframe_summary: None,
            extracted_audio_path: None,
            extracted_audio_mime: None,
            extracted_audio_byte_length: None,
            extracted_audio_duration_seconds: None,
            component_failures: Vec::new(),
            truncated: false,
        })
    }
}

#[test]
fn real_analyzer_invocation_publishes_included_projection() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    let media = tempfile::tempdir()?;
    let attachments = media.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("runtime.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media_paths = vec![video.to_string_lossy().to_string()];
    let owner = runtime_owner_facts()?;
    let store = Spec035MediaProjectionStore::new(data.path());
    let builder = ContextBuilder::new(workspace.path())
        .with_media_roots([media.path().to_path_buf()])
        .with_video_analyzer(Arc::new(IncludedAnalyzer))
        .with_video_projection_publication(store.clone(), Some(owner));

    // When
    let messages = builder.build_messages(ContextBuildRequest {
        media: &media_paths,
        ..ContextBuildRequest::new("inspect")
    });
    let projection = store
        .read()?
        .ok_or_else(|| format!("projection was not published: {messages:?}"))?;

    // Then
    assert_eq!(projection.state(), Spec035MediaState::Included);
    let serialized = serde_json::to_string(&projection)?;
    assert!(serialized.contains("spec034://media/artifact/"));
    assert!(!serialized.contains(video.to_string_lossy().as_ref()));
    assert!(messages[1]["content"][1]["text"]
        .as_str()
        .ok_or("attachment note missing")?
        .contains("[attachment:included_text]"));
    Ok(())
}

#[test]
fn missing_store_remains_unavailable_after_runtime_analysis() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    let media = tempfile::tempdir()?;
    let attachments = media.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("runtime.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media_paths = vec![video.to_string_lossy().to_string()];
    let store = Spec035MediaProjectionStore::new(data.path());
    let builder = ContextBuilder::new(workspace.path())
        .with_media_roots([media.path().to_path_buf()])
        .with_video_analyzer(Arc::new(IncludedAnalyzer));

    // When
    let messages = builder.build_messages(ContextBuildRequest {
        media: &media_paths,
        ..ContextBuildRequest::new("inspect")
    });

    // Then
    assert!(store.read()?.is_none());
    assert!(messages[1]["content"][1]["text"]
        .as_str()
        .ok_or("attachment note missing")?
        .contains("[attachment:included_text]"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn invalid_publication_target_fails_closed_without_attachment_success() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::symlink;

    // Given
    let workspace = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    let media = tempfile::tempdir()?;
    let attachments = media.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("runtime.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media_paths = vec![video.to_string_lossy().to_string()];
    let store = Spec035MediaProjectionStore::new(data.path());
    std::fs::create_dir_all(store.path().parent().ok_or("store parent")?)?;
    let target = data.path().join("outside.json");
    std::fs::write(&target, b"{}")?;
    symlink(&target, store.path())?;
    let builder = ContextBuilder::new(workspace.path())
        .with_media_roots([media.path().to_path_buf()])
        .with_video_analyzer(Arc::new(IncludedAnalyzer))
        .with_video_projection_publication(store, Some(runtime_owner_facts()?));

    // When
    let messages = builder.build_messages(ContextBuildRequest {
        media: &media_paths,
        ..ContextBuildRequest::new("inspect")
    });

    // Then
    let rendered = serde_json::to_string(&messages)?;
    assert!(rendered.contains("[attachment:extraction_failed]"));
    assert!(!rendered.contains("[attachment:included_text]"));
    assert_eq!(std::fs::read(&target)?, b"{}");
    Ok(())
}

fn runtime_owner_facts() -> Result<VideoAnalyzerOwnerFactsProjection, Box<dyn Error>> {
    let owner = owner_support::OwnerFixture::new("snapshot:034:runtime", None)?;
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: owner.input(Spec031Freshness::Current),
    })?
    .owner_facts)
}

fn mp4_video_bytes(duration_seconds: u32) -> Vec<u8> {
    let mut mvhd_payload = vec![0_u8; 20];
    mvhd_payload[12..16].copy_from_slice(&1_u32.to_be_bytes());
    mvhd_payload[16..20].copy_from_slice(&duration_seconds.to_be_bytes());
    let mvhd = mp4_box(*b"mvhd", &mvhd_payload);
    let moov = mp4_box(*b"moov", &mvhd);
    let mut bytes = mp4_box(*b"ftyp", b"isom\0\0\0\0");
    bytes.extend(moov);
    bytes
}

fn mp4_box(box_type: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len() + 8).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(payload.len() + 8);
    bytes.extend(size.to_be_bytes());
    bytes.extend(box_type);
    bytes.extend(payload);
    bytes
}
