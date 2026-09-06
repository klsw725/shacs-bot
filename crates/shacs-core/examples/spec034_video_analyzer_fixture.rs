use shacs_core::runtime::{
    project_video_analyzer, ContextBuildRequest, ContextBuilder, VideoAnalysisPolicy,
    VideoAnalyzerCapability, VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput,
    VideoAnalyzerProjectionInput, VideoContextAnalysis, VideoContextAnalyzer, VideoContextError,
    VideoContextRequest, VideoMetadata,
};
use shacs_projection::Spec031Freshness;
use std::error::Error;
use std::sync::Arc;

#[derive(Debug)]
struct FixtureAnalyzer;

impl VideoContextAnalyzer for FixtureAnalyzer {
    fn analyze(
        &self,
        _invocation: &shacs_core::runtime::AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        Ok(fixture_analysis(request.duration_seconds))
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("fixture.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media = vec![video.to_string_lossy().to_string()];
    let messages = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_video_analyzer(Arc::new(FixtureAnalyzer))
        .build_messages(ContextBuildRequest {
            media: &media,
            ..ContextBuildRequest::new("inspect")
        });
    let routing_included = messages[1]["content"][1]["text"]
        .as_str()
        .is_some_and(|note| note.contains("[attachment:included_text]"));

    let analysis = fixture_analysis(Some(6));
    let included = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: Some(6),
        policy: VideoAnalysisPolicy::default(),
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
    })?;
    let missing = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Missing,
        duration_seconds: Some(6),
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "fixture": "injected_video_analyzer",
            "routing_included": routing_included,
            "records": [included, missing],
        }))?
    );
    Ok(())
}

fn fixture_analysis(duration_seconds: Option<u64>) -> VideoContextAnalysis {
    VideoContextAnalysis {
        metadata: Some(VideoMetadata {
            duration_seconds,
            container: Some("mp4".to_owned()),
            video_codec: Some("h264".to_owned()),
            audio_codec: None,
            width: Some(640),
            height: Some(360),
            audio_track_available: false,
            subtitle_tracks: Vec::new(),
        }),
        subtitles: Some("fixture subtitles".to_owned()),
        scene_summary: Some("fixture scene".to_owned()),
        keyframe_summary: None,
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    }
}

fn mp4_video_bytes(duration_seconds: u64) -> Vec<u8> {
    let mut mvhd_payload = vec![0u8; 20];
    mvhd_payload[12..16].copy_from_slice(&1u32.to_be_bytes());
    mvhd_payload[16..20].copy_from_slice(
        &u32::try_from(duration_seconds)
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
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
