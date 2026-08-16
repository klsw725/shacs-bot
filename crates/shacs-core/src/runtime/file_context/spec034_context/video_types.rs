pub use crate::runtime::AnalyzerInvocation;
use std::path::PathBuf;

const MAX_STORED_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const MAX_VIDEO_SUBTITLE_CHARS: usize = 8_000;
pub(crate) const MAX_VIDEO_SUMMARY_CHARS: usize = 4_000;
const MAX_VIDEO_METADATA_CHARS: usize = 2_000;
pub(crate) const MAX_VIDEO_DURATION_SECONDS: u64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoAnalysisPolicy {
    pub max_byte_length: u64,
    pub max_duration_seconds: u64,
    pub max_subtitle_chars: usize,
    pub max_summary_chars: usize,
    pub max_metadata_chars: usize,
    pub allow_unknown_duration: bool,
}

impl Default for VideoAnalysisPolicy {
    fn default() -> Self {
        Self {
            max_byte_length: MAX_STORED_ATTACHMENT_BYTES,
            max_duration_seconds: MAX_VIDEO_DURATION_SECONDS,
            max_subtitle_chars: MAX_VIDEO_SUBTITLE_CHARS,
            max_summary_chars: MAX_VIDEO_SUMMARY_CHARS,
            max_metadata_chars: MAX_VIDEO_METADATA_CHARS,
            allow_unknown_duration: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoMetadata {
    pub duration_seconds: Option<u64>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio_track_available: bool,
    pub subtitle_tracks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoContextRequest {
    pub file_path: PathBuf,
    pub detected_mime: String,
    pub byte_length: u64,
    pub duration_seconds: Option<u64>,
    pub policy: VideoAnalysisPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoComponentFailure {
    pub component: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoContextAnalysis {
    pub metadata: Option<VideoMetadata>,
    pub subtitles: Option<String>,
    pub scene_summary: Option<String>,
    pub keyframe_summary: Option<String>,
    pub extracted_audio_path: Option<PathBuf>,
    pub extracted_audio_mime: Option<String>,
    pub extracted_audio_byte_length: Option<u64>,
    pub extracted_audio_duration_seconds: Option<u64>,
    pub component_failures: Vec<VideoComponentFailure>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoContextError {
    Unsupported(String),
    Failed(String),
    Cancelled,
    TimedOut,
}

pub trait VideoContextAnalyzer: std::fmt::Debug + Send + Sync {
    fn analyze(
        &self,
        invocation: &AnalyzerInvocation,
        request: VideoContextRequest,
    ) -> Result<VideoContextAnalysis, VideoContextError>;
}
