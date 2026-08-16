use serde_json::Value;
use shacs_projection::{
    DataSurface, Spec031Freshness, Spec035MediaDisclosure, Spec035MediaProjection,
    Spec035MediaState, TraceStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProjectionView {
    lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaLineageView {
    artifact_ref: String,
    analyzer_ref: Option<String>,
    snapshot_ref: Option<String>,
    evidence_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaReasonView {
    code: String,
    summary: String,
}

impl MediaProjectionView {
    pub fn unavailable() -> Self {
        Self {
            lines: vec![
                "media: state=unavailable reason=unavailable freshness=unavailable".to_owned(),
                "media lineage: unavailable".to_owned(),
                "media disclosure: unavailable".to_owned(),
            ],
        }
    }

    pub fn from_session_payload(payload: Option<&Value>) -> Self {
        let Some(value) = payload
            .and_then(|value| value.get("metadata"))
            .and_then(|metadata| metadata.get("media_capability"))
        else {
            return Self::unavailable();
        };
        let Ok(projection) = Spec035MediaProjection::from_json_value(value.clone()) else {
            return Self::unavailable();
        };
        Self::from_projection(&projection)
    }

    pub fn from_projection(projection: &Spec035MediaProjection) -> Self {
        let Ok(canonical) = serde_json::to_value(projection) else {
            return Self::unavailable();
        };
        let Some(lineage) = MediaLineageView::from_canonical(&canonical) else {
            return Self::unavailable();
        };
        let Some(reason) = MediaReasonView::from_canonical(&canonical) else {
            return Self::unavailable();
        };
        Self {
            lines: canonical_lines(projection, &lineage, &reason),
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

impl MediaLineageView {
    fn from_canonical(canonical: &Value) -> Option<Self> {
        let lineage = canonical.get("lineage")?;
        Some(Self {
            artifact_ref: lineage.get("artifact_ref")?.as_str()?.to_owned(),
            analyzer_ref: optional_string(lineage, "analyzer_ref")?,
            snapshot_ref: optional_string(lineage, "snapshot_ref")?,
            evidence_digest: optional_string(lineage, "evidence_digest")?,
        })
    }
}

impl MediaReasonView {
    fn from_canonical(canonical: &Value) -> Option<Self> {
        let reason = canonical.get("reason")?;
        Some(Self {
            code: reason.get("code")?.as_str()?.to_owned(),
            summary: sanitized_reason(reason.get("safe_summary")?.as_str()?),
        })
    }
}

fn optional_string(value: &Value, key: &str) -> Option<Option<String>> {
    match value.get(key) {
        Some(value) => value.as_str().map(|value| Some(value.to_owned())),
        None => Some(None),
    }
}

fn canonical_lines(
    projection: &Spec035MediaProjection,
    lineage: &MediaLineageView,
    reason: &MediaReasonView,
) -> Vec<String> {
    vec![
        format!(
            "media: state={} reason={} freshness={}",
            state_label(projection.state()),
            reason.code,
            freshness_label(projection.freshness())
        ),
        format!("media reason: {}", reason.summary),
        format!("media lineage: artifact={}", lineage.artifact_ref),
        format!(
            "media lineage: analyzer={} snapshot={}",
            lineage.analyzer_ref.as_deref().unwrap_or("unavailable"),
            lineage.snapshot_ref.as_deref().unwrap_or("unavailable")
        ),
        format!(
            "media evidence: {}",
            lineage.evidence_digest.as_deref().unwrap_or("unavailable")
        ),
        disclosure_line(projection.disclosure()),
    ]
}

fn sanitized_reason(summary: &str) -> String {
    summary
        .split_whitespace()
        .map(|token| {
            if contains_forbidden_material(token) {
                "[redacted]"
            } else {
                token
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_forbidden_material(token: &str) -> bool {
    token.contains("://")
        || token.starts_with('/')
        || (token.len() >= 16
            && token.len() % 4 == 0
            && token.ends_with('=')
            && token.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=')
            }))
}

fn disclosure_line(disclosure: &Spec035MediaDisclosure) -> String {
    match disclosure {
        Spec035MediaDisclosure::Recorded(fact) => format!(
            "media disclosure: recorded raw_content_possible={} surfaces={} trace={}",
            fact.raw_content_possible,
            fact.surfaces
                .iter()
                .map(|surface| data_surface_label(*surface))
                .collect::<Vec<_>>()
                .join(","),
            trace_status_label(fact.trace_status)
        ),
        Spec035MediaDisclosure::Unavailable => "media disclosure: unavailable".to_owned(),
    }
}

const fn state_label(state: Spec035MediaState) -> &'static str {
    match state {
        Spec035MediaState::Included => "included",
        Spec035MediaState::Unsupported => "unsupported",
        Spec035MediaState::ExtractionFailed => "extraction_failed",
        Spec035MediaState::AnalyzerMissing => "analyzer_missing",
        Spec035MediaState::Truncated => "truncated",
        Spec035MediaState::Unavailable => "unavailable",
    }
}

const fn freshness_label(freshness: Spec031Freshness) -> &'static str {
    match freshness {
        Spec031Freshness::Current => "current",
        Spec031Freshness::Stale => "stale",
        Spec031Freshness::Unavailable => "unavailable",
        Spec031Freshness::Unknown => "unknown",
    }
}

const fn data_surface_label(surface: DataSurface) -> &'static str {
    match surface {
        DataSurface::Session => "session",
        DataSurface::Log => "log",
        DataSurface::Trace => "trace",
        DataSurface::ToolOutput => "tool_output",
        DataSurface::ExtensionData => "extension_data",
    }
}

const fn trace_status_label(status: TraceStatus) -> &'static str {
    match status {
        TraceStatus::Disabled => "disabled",
        TraceStatus::Preview => "preview",
        TraceStatus::Enabled => "enabled",
        TraceStatus::Unavailable => "unavailable",
    }
}
