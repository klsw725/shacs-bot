use super::super::file_context::{VideoAnalysisPolicy, VideoContextAnalysis, VideoMetadata};
use super::{VideoAnalyzerEvidenceProjection, VideoComponentFailureProjection};
use shacs_redaction::redact_string;

const MAX_COMPONENT_FAILURES: usize = 8;
const MAX_COMPONENT_NAME_CHARS: usize = 80;
const MAX_FAILURE_REASON_CHARS: usize = 240;

pub(super) fn bounded_evidence(
    analysis: &VideoContextAnalysis,
    policy: VideoAnalysisPolicy,
) -> VideoAnalyzerEvidenceProjection {
    let metadata = analysis
        .metadata
        .as_ref()
        .map(|value| bounded_metadata(value, policy.max_metadata_chars));
    let subtitles = analysis.subtitles.as_deref().map(|value| {
        bounded_safe_text(
            value,
            policy.max_subtitle_chars,
            "subtitle content unavailable",
        )
    });
    let scene_summary = analysis.scene_summary.as_deref().map(|value| {
        bounded_safe_text(value, policy.max_summary_chars, "scene summary unavailable")
    });
    let keyframe_summary = analysis.keyframe_summary.as_deref().map(|value| {
        bounded_safe_text(
            value,
            policy.max_summary_chars,
            "keyframe summary unavailable",
        )
    });
    let failures_truncated = analysis.component_failures.len() > MAX_COMPONENT_FAILURES;
    let component_failures = analysis
        .component_failures
        .iter()
        .take(MAX_COMPONENT_FAILURES)
        .map(|failure| VideoComponentFailureProjection {
            component: bounded_safe_text(&failure.component, MAX_COMPONENT_NAME_CHARS, "component")
                .0,
            reason: bounded_safe_text(
                &failure.reason,
                MAX_FAILURE_REASON_CHARS,
                "component failure details unavailable",
            )
            .0,
        })
        .collect();
    let truncated = analysis.truncated
        || failures_truncated
        || metadata.as_ref().is_some_and(|value| value.1)
        || subtitles.as_ref().is_some_and(|value| value.1)
        || scene_summary.as_ref().is_some_and(|value| value.1)
        || keyframe_summary.as_ref().is_some_and(|value| value.1);
    VideoAnalyzerEvidenceProjection {
        metadata: metadata.map(|value| value.0),
        subtitles: subtitles.map(|value| value.0),
        scene_summary: scene_summary.map(|value| value.0),
        keyframe_summary: keyframe_summary.map(|value| value.0),
        component_failures,
        truncated,
    }
}

fn bounded_metadata(metadata: &VideoMetadata, max_chars: usize) -> (String, bool) {
    let mut lines = Vec::new();
    if let Some(duration) = metadata.duration_seconds {
        lines.push(format!("duration_seconds={duration}"));
    }
    if let Some(container) = metadata.container.as_deref() {
        lines.push(format!(
            "container={}",
            bounded_safe_text(container, 80, "unavailable").0
        ));
    }
    if let Some(codec) = metadata.video_codec.as_deref() {
        lines.push(format!(
            "video_codec={}",
            bounded_safe_text(codec, 80, "unavailable").0
        ));
    }
    if let Some(codec) = metadata.audio_codec.as_deref() {
        lines.push(format!(
            "audio_codec={}",
            bounded_safe_text(codec, 80, "unavailable").0
        ));
    }
    if let (Some(width), Some(height)) = (metadata.width, metadata.height) {
        lines.push(format!("resolution={width}x{height}"));
    }
    lines.push(format!(
        "audio_track_available={}",
        metadata.audio_track_available
    ));
    let tracks = metadata
        .subtitle_tracks
        .iter()
        .take(16)
        .map(|track| bounded_safe_text(track, 80, "unavailable").0)
        .collect::<Vec<_>>()
        .join(", ");
    lines.push(format!(
        "subtitle_tracks={}",
        if tracks.is_empty() { "none" } else { &tracks }
    ));
    let tracks_truncated = metadata.subtitle_tracks.len() > 16;
    let (text, text_truncated) = truncate_chars(&lines.join("\n"), max_chars);
    (text, tracks_truncated || text_truncated)
}

pub(super) fn bounded_safe_text(value: &str, max_chars: usize, fallback: &str) -> (String, bool) {
    let original_chars = value.chars().count();
    let redacted = redact_string(value);
    let lowered = redacted.to_ascii_lowercase();
    let safe = if redacted.contains('/')
        || redacted.contains('\\')
        || redacted.contains("://")
        || lowered.starts_with("file:")
        || lowered.starts_with("data:")
    {
        fallback.to_owned()
    } else {
        redacted
    };
    let (text, truncated) = truncate_chars(&safe, max_chars);
    (text, truncated || original_chars > max_chars)
}

fn truncate_chars(value: &str, max_chars: usize) -> (String, bool) {
    let original_chars = value.chars().count();
    (
        value.chars().take(max_chars).collect(),
        original_chars > max_chars,
    )
}

pub(super) fn has_evidence(evidence: &VideoAnalyzerEvidenceProjection) -> bool {
    evidence.metadata.is_some()
        || evidence.subtitles.is_some()
        || evidence.scene_summary.is_some()
        || evidence.keyframe_summary.is_some()
        || !evidence.component_failures.is_empty()
}
