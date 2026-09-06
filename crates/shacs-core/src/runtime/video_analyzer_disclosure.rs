use serde::Serialize;
use shacs_projection::{DataDisclosureProjection, DataSurface, TraceDestination, TraceStatus};
use shacs_redaction::redact_string;

const MAX_DISCLOSURE_SUMMARY_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerDisclosureProjection {
    pub raw_content_possible: bool,
    pub surfaces: Vec<DataSurface>,
    pub trace: VideoAnalyzerTraceDisclosureProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerTraceDisclosureProjection {
    pub status: TraceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<VideoAnalyzerTracePreviewProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerTracePreviewProjection {
    pub record_count: u64,
    pub approximate_bytes: u64,
    pub destination: TraceDestination,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_summary: Option<String>,
}

pub(super) fn project_analyzer_disclosure(
    disclosure: &DataDisclosureProjection,
) -> VideoAnalyzerDisclosureProjection {
    let mut surfaces = disclosure.surfaces.clone();
    surfaces.sort_by_key(|surface| surface_rank(*surface));
    surfaces.dedup();
    VideoAnalyzerDisclosureProjection {
        raw_content_possible: disclosure.raw_content_possible,
        surfaces,
        trace: VideoAnalyzerTraceDisclosureProjection {
            status: disclosure.trace.status,
            preview: disclosure.trace.preview.as_ref().map(|preview| {
                VideoAnalyzerTracePreviewProjection {
                    record_count: preview.record_count,
                    approximate_bytes: preview.approximate_bytes,
                    destination: preview.destination,
                    exporter: preview.exporter.as_deref().map(bounded_redacted_summary),
                    endpoint_summary: preview
                        .endpoint_summary
                        .as_deref()
                        .map(bounded_redacted_summary),
                }
            }),
        },
    }
}

fn bounded_redacted_summary(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    if [
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "credential",
        "password",
        "secret",
        "token",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
    {
        return "[redacted]".to_owned();
    }
    if value.contains(['/', '\\', '?', '#', '@', '=']) {
        return "unavailable".to_owned();
    }
    redact_string(value)
        .chars()
        .take(MAX_DISCLOSURE_SUMMARY_CHARS)
        .collect()
}

const fn surface_rank(surface: DataSurface) -> u8 {
    match surface {
        DataSurface::Session => 0,
        DataSurface::Log => 1,
        DataSurface::Trace => 2,
        DataSurface::ToolOutput => 3,
        DataSurface::ExtensionData => 4,
    }
}
