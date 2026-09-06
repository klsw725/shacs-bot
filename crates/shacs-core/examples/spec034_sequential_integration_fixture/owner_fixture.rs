#[path = "../../tests/spec034_video_analyzer_owner_facts/support.rs"]
mod support;

use shacs_core::runtime::{
    project_video_analyzer, project_video_analyzer_spec035, AnalyzerInvocation,
    ContextBuildRequest, ContextBuilder, Spec035MediaProjectionStore, VideoAnalysisPolicy,
    VideoAnalyzerCapability, VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput,
    VideoAnalyzerProjection, VideoAnalyzerProjectionInput, VideoAnalyzerSpec035Input,
    VideoContextAnalysis, VideoContextAnalyzer, VideoContextError, VideoContextRequest,
};
use shacs_projection::{
    DataDisclosureProjection, DataSurface, Spec031ExternalOwnerRef, Spec031Freshness,
    Spec035MediaProjection, Spec035MediaState, TraceDisclosureProjection, TraceStatus,
};
use std::error::Error;
use std::sync::Arc;
use support::OwnerFixture;

pub fn analyzer_projection() -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    let owner = OwnerFixture::new("snapshot:034", None)?;
    let analysis = VideoContextAnalysis {
        metadata: None,
        subtitles: Some("recorded subtitle".to_owned()),
        scene_summary: Some("recorded scene".to_owned()),
        keyframe_summary: None,
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    };
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
        owner_facts: owner.input(Spec031Freshness::Current),
    })?)
}

pub fn ownerless_projection() -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Current),
    })?)
}

pub fn disclosure() -> DataDisclosureProjection {
    DataDisclosureProjection {
        raw_content_possible: true,
        surfaces: vec![DataSurface::Session, DataSurface::Log],
        trace: TraceDisclosureProjection {
            status: TraceStatus::Unavailable,
            preview: None,
        },
    }
}

pub fn spec035_projection_for_state(
    state: Spec035MediaState,
) -> Result<Spec035MediaProjection, Box<dyn Error>> {
    match state {
        Spec035MediaState::Included => {
            return runtime_spec035_projection(Spec031Freshness::Current)
        }
        Spec035MediaState::Unavailable => {
            return runtime_spec035_projection(Spec031Freshness::Stale)
        }
        Spec035MediaState::Unsupported
        | Spec035MediaState::ExtractionFailed
        | Spec035MediaState::AnalyzerMissing
        | Spec035MediaState::Truncated => {}
    }
    let analyzer = analyzer_for_spec035_state(state)?;
    let artifact_ref =
        Spec031ExternalOwnerRef::try_new("spec034://media/artifact/sequential-surface")?;
    Ok(project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
        artifact_ref: &artifact_ref,
        analyzer: &analyzer,
    })?)
}

#[derive(Debug)]
struct RuntimeSurfaceAnalyzer;

impl VideoContextAnalyzer for RuntimeSurfaceAnalyzer {
    fn analyze(
        &self,
        _invocation: &AnalyzerInvocation,
        _request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        Ok(VideoContextAnalysis {
            metadata: None,
            subtitles: Some("runtime surface subtitle".to_owned()),
            scene_summary: Some("runtime surface scene".to_owned()),
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

fn runtime_spec035_projection(
    freshness: Spec031Freshness,
) -> Result<Spec035MediaProjection, Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    let media = tempfile::tempdir()?;
    let attachments = media.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("surface.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media_paths = vec![video.to_string_lossy().into_owned()];
    let owner = OwnerFixture::new("snapshot:034:surface-runtime", None)?;
    let owner_facts = project_video_analyzer(VideoAnalyzerProjectionInput {
        capability: VideoAnalyzerCapability::Configured,
        duration_seconds: None,
        policy: VideoAnalysisPolicy::default(),
        outcome: None,
        owner_facts: owner.input(freshness),
    })?
    .owner_facts;
    let store = Spec035MediaProjectionStore::new(data.path());
    let _messages = ContextBuilder::new(workspace.path())
        .with_media_roots([media.path().to_path_buf()])
        .with_video_analyzer(Arc::new(RuntimeSurfaceAnalyzer))
        .with_video_projection_publication(store.clone(), Some(owner_facts))
        .build_messages(ContextBuildRequest {
            media: &media_paths,
            ..ContextBuildRequest::new("inspect")
        });
    store
        .read()?
        .ok_or_else(|| "runtime media projection unavailable".into())
}

fn analyzer_for_spec035_state(
    state: Spec035MediaState,
) -> Result<VideoAnalyzerProjection, Box<dyn Error>> {
    let policy = VideoAnalysisPolicy::default();
    let owner = OwnerFixture::new("snapshot:034:surface", None)?;
    let analysis = VideoContextAnalysis {
        metadata: None,
        subtitles: Some(match state {
            Spec035MediaState::Truncated => "s".repeat(policy.max_subtitle_chars + 1),
            Spec035MediaState::Included
            | Spec035MediaState::Unsupported
            | Spec035MediaState::ExtractionFailed
            | Spec035MediaState::AnalyzerMissing
            | Spec035MediaState::Unavailable => "recorded subtitle".to_owned(),
        }),
        scene_summary: None,
        keyframe_summary: None,
        extracted_audio_path: None,
        extracted_audio_mime: None,
        extracted_audio_byte_length: None,
        extracted_audio_duration_seconds: None,
        component_failures: Vec::new(),
        truncated: false,
    };
    let (capability, outcome, owner_facts) = match state {
        Spec035MediaState::Included | Spec035MediaState::Truncated => (
            VideoAnalyzerCapability::Configured,
            Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
            owner.input(Spec031Freshness::Current),
        ),
        Spec035MediaState::Unsupported => (
            VideoAnalyzerCapability::Configured,
            Some(VideoAnalyzerOutcomeInput::Unsupported("codec unavailable")),
            owner.input(Spec031Freshness::Current),
        ),
        Spec035MediaState::ExtractionFailed => (
            VideoAnalyzerCapability::Configured,
            Some(VideoAnalyzerOutcomeInput::Failed("extraction failed")),
            owner.input(Spec031Freshness::Current),
        ),
        Spec035MediaState::AnalyzerMissing => (
            VideoAnalyzerCapability::Missing,
            None,
            VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Current),
        ),
        Spec035MediaState::Unavailable => (
            VideoAnalyzerCapability::Configured,
            Some(VideoAnalyzerOutcomeInput::Included(&analysis)),
            owner.input(Spec031Freshness::Stale),
        ),
    };
    Ok(project_video_analyzer(VideoAnalyzerProjectionInput {
        capability,
        duration_seconds: None,
        policy,
        outcome,
        owner_facts,
    })?)
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
