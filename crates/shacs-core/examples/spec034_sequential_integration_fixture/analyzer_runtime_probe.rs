use serde::Serialize;
use shacs_core::runtime::{
    AnalyzerInvocation, AnalyzerMediaProvenance, ContextBuildRequest, ContextBuilder,
    VideoContextAnalysis, VideoContextAnalyzer, VideoContextError, VideoContextRequest,
    VideoMetadata,
};
use std::error::Error;
use std::sync::{Arc, Mutex};

#[derive(Debug, Serialize)]
pub struct AnalyzerRuntimeProbe {
    pub injected: bool,
    pub stored_provenance: bool,
    pub generated_provenance: bool,
    pub minimum_fields: bool,
}

#[derive(Debug)]
struct CapturingAnalyzer {
    observed: Arc<Mutex<Vec<AnalyzerMediaProvenance>>>,
}

impl VideoContextAnalyzer for CapturingAnalyzer {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError> {
        self.observed
            .lock()
            .map_err(|error| VideoContextError::Failed(error.to_string()))?
            .push(invocation.provenance());
        Ok(VideoContextAnalysis {
            metadata: Some(VideoMetadata {
                duration_seconds: request.duration_seconds,
                container: Some("mp4".to_owned()),
                video_codec: Some("h264".to_owned()),
                audio_codec: None,
                width: Some(640),
                height: Some(360),
                audio_track_available: false,
                subtitle_tracks: Vec::new(),
            }),
            subtitles: Some("runtime injected subtitle".to_owned()),
            scene_summary: Some("runtime injected scene".to_owned()),
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

pub fn run() -> Result<AnalyzerRuntimeProbe, Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let media_root = tempfile::tempdir()?;
    let attachments = media_root.path().join("attachments/cli");
    std::fs::create_dir_all(&attachments)?;
    let video = attachments.join("clip.mp4");
    std::fs::write(&video, mp4_video_bytes(6))?;
    let media = vec![video.to_string_lossy().to_string()];
    let observed = Arc::new(Mutex::new(Vec::new()));
    let inbound = build(
        workspace.path(),
        media_root.path(),
        &media,
        Arc::clone(&observed),
        AnalyzerMediaProvenance::Inbound,
    );
    let generated = build(
        workspace.path(),
        media_root.path(),
        &media,
        Arc::clone(&observed),
        AnalyzerMediaProvenance::Generated,
    );
    let observed = observed.lock().map_err(|error| error.to_string())?.clone();
    let rendered = serde_json::to_string(&(inbound, generated))?;
    Ok(AnalyzerRuntimeProbe {
        injected: rendered.contains("runtime injected scene"),
        stored_provenance: observed.first() == Some(&AnalyzerMediaProvenance::Inbound),
        generated_provenance: observed.get(1) == Some(&AnalyzerMediaProvenance::Generated),
        minimum_fields: rendered.contains("duration_seconds")
            && rendered.contains("runtime injected subtitle")
            && rendered.contains("runtime injected scene"),
    })
}

fn build(
    workspace: &std::path::Path,
    media_root: &std::path::Path,
    media: &[String],
    observed: Arc<Mutex<Vec<AnalyzerMediaProvenance>>>,
    provenance: AnalyzerMediaProvenance,
) -> Vec<serde_json::Value> {
    ContextBuilder::new(workspace)
        .with_media_roots([media_root.to_path_buf()])
        .with_video_analyzer(Arc::new(CapturingAnalyzer { observed }))
        .with_video_media_provenance(provenance)
        .build_messages(ContextBuildRequest {
            media,
            ..ContextBuildRequest::new("inspect")
        })
}

fn mp4_video_bytes(duration_seconds: u64) -> Vec<u8> {
    let mut mvhd_payload = vec![0_u8; 20];
    mvhd_payload[12..16].copy_from_slice(&1_u32.to_be_bytes());
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
