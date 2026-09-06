use shacs_core::runtime::{
    project_video_analyzer, AnalyzerInvocation, ContextBuildRequest, ContextBuilder,
    VideoAnalysisPolicy, VideoAnalyzerCapability, VideoAnalyzerOutcomeInput,
    VideoAnalyzerOwnerFactsInput, VideoAnalyzerProjectionInput, VideoAnalyzerStatus,
    VideoContextAnalysis, VideoContextAnalyzer, VideoContextError, VideoContextRequest,
    VideoMetadata,
};
use shacs_projection::Spec031Freshness;
use std::error::Error;
use std::sync::Arc;

#[path = "spec034_video_analyzer/agent_loop.rs"]
mod agent_loop;
#[path = "spec034_video_analyzer/controlled_child.rs"]
mod controlled_child;
#[path = "spec034_video_analyzer/runtime.rs"]
mod runtime;
#[path = "spec034_video_analyzer/runtime_supervision.rs"]
mod runtime_supervision;
#[path = "spec034_video_analyzer/support.rs"]
mod support;

#[derive(Debug)]
struct FixtureAnalyzer;

impl VideoContextAnalyzer for FixtureAnalyzer {
    fn analyze(
        &self,
        _invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        Ok(included_analysis(request.duration_seconds))
    }
}

#[test]
fn baseline_routes_injected_and_missing_analyzers_without_behavior_drift(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("clip.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media = vec![video.to_string_lossy().to_string()];

    let included = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .with_video_analyzer(Arc::new(FixtureAnalyzer))
        .build_messages(ContextBuildRequest {
            media: &media,
            ..ContextBuildRequest::new("inspect")
        });
    let included_blocks = included[1]["content"]
        .as_array()
        .ok_or("missing included blocks")?;
    assert!(included_blocks[1]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("[attachment:included_text]"));
    assert!(included_blocks[2]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("[Video scene summary]\nfixture scene"));

    let missing = ContextBuilder::new(workspace.path())
        .with_media_roots([media_root.path().to_path_buf()])
        .build_messages(ContextBuildRequest {
            media: &media,
            ..ContextBuildRequest::new("inspect")
        });
    let missing_note = missing[1]["content"][1]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(missing_note.contains("[attachment:unsupported]"));
    assert!(missing_note.contains("video analyzer is not configured"));
    Ok(())
}

#[test]
fn analyzer_status_matrix() -> Result<(), Box<dyn Error>> {
    let policy = VideoAnalysisPolicy::default();
    let included = included_analysis(Some(6));
    let truncated = VideoContextAnalysis {
        subtitles: Some("s".repeat(policy.max_subtitle_chars + 1)),
        ..included.clone()
    };
    let cases = [
        (
            VideoAnalyzerCapability::Configured,
            None,
            None,
            VideoAnalyzerStatus::Configured,
        ),
        (
            VideoAnalyzerCapability::Missing,
            None,
            None,
            VideoAnalyzerStatus::AnalyzerMissing,
        ),
        (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Unsupported("codec unavailable")),
            VideoAnalyzerStatus::Unsupported,
        ),
        (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Failed("analyzer failed")),
            VideoAnalyzerStatus::ExtractionFailed,
        ),
        (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Included(&included)),
            VideoAnalyzerStatus::Included,
        ),
        (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Included(&truncated)),
            VideoAnalyzerStatus::Truncated,
        ),
        (
            VideoAnalyzerCapability::Configured,
            Some(policy.max_duration_seconds + 1),
            None,
            VideoAnalyzerStatus::DurationCap,
        ),
        (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::Cancelled),
            VideoAnalyzerStatus::Cancelled,
        ),
        (
            VideoAnalyzerCapability::Configured,
            None,
            Some(VideoAnalyzerOutcomeInput::TimedOut),
            VideoAnalyzerStatus::Timeout,
        ),
    ];

    for (capability, duration_seconds, outcome, expected) in cases {
        let projection = project_video_analyzer(VideoAnalyzerProjectionInput {
            capability,
            duration_seconds,
            policy,
            outcome,
            owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
        })?;
        assert_eq!(projection.status, expected);
    }
    Ok(())
}

#[test]
fn bounded_serialization_scrubs_oversized_and_malformed_evidence() -> Result<(), Box<dyn Error>> {
    let policy = VideoAnalysisPolicy::default();
    let secret = "https://signed.example/video?token=secret /Users/private/movie.mp4";
    let analysis = VideoContextAnalysis {
        metadata: Some(VideoMetadata {
            duration_seconds: Some(2),
            container: Some(secret.to_owned()),
            video_codec: Some(secret.to_owned()),
            audio_codec: None,
            width: Some(1920),
            height: Some(1080),
            audio_track_available: false,
            subtitle_tracks: vec![secret.to_owned(); 128],
        }),
        subtitles: Some(format!(
            "{secret}{}",
            "s".repeat(policy.max_subtitle_chars * 2)
        )),
        scene_summary: Some("v".repeat(policy.max_summary_chars * 2)),
        keyframe_summary: Some("k".repeat(policy.max_summary_chars * 2)),
        extracted_audio_path: Some("/Users/private/audio.m4a".into()),
        extracted_audio_mime: Some("audio/mp4".to_owned()),
        extracted_audio_byte_length: Some(u64::MAX),
        extracted_audio_duration_seconds: Some(u64::MAX),
        component_failures: Vec::new(),
        truncated: false,
    };
    let projection = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: Some(2),
        policy,
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Stale),
    })?;
    let serialized = serde_json::to_string(&projection)?;

    assert_eq!(projection.status, VideoAnalyzerStatus::Truncated);
    assert!(serialized.len() <= 24_000);
    assert!(!serialized.contains("signed.example"));
    assert!(!serialized.contains("token=secret"));
    assert!(!serialized.contains("/Users/"));
    assert!(!serialized.contains("audio.m4a"));
    assert!(serialized.contains("stale_owner_facts"));
    Ok(())
}

#[test]
fn misleading_success_after_missing_or_duration_cap_is_rejected() {
    let analysis = included_analysis(Some(901));
    for input in [
        VideoAnalyzerProjectionInput {
            capability: VideoAnalyzerCapability::Missing,
            duration_seconds: None,
            policy: VideoAnalysisPolicy::default(),
            outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
            owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
        },
        VideoAnalyzerProjectionInput {
            capability: VideoAnalyzerCapability::Configured,
            duration_seconds: Some(901),
            policy: VideoAnalysisPolicy::default(),
            outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
            owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Unavailable),
        },
    ] {
        assert!(project_video_analyzer(input).is_err());
    }
}

fn included_analysis(duration_seconds: Option<u64>) -> VideoContextAnalysis {
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
