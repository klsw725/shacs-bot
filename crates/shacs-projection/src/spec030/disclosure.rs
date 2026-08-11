use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataSurface {
    Session,
    Log,
    Trace,
    ToolOutput,
    ExtensionData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceStatus {
    Disabled,
    Preview,
    Enabled,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TraceDestination {
    LocalOnly,
    ConfiguredRemote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataDisclosureProjection {
    pub raw_content_possible: bool,
    pub surfaces: Vec<DataSurface>,
    pub trace: TraceDisclosureProjection,
}

impl DataDisclosureProjection {
    pub(super) fn unavailable() -> Self {
        Self {
            raw_content_possible: true,
            surfaces: vec![
                DataSurface::Session,
                DataSurface::Log,
                DataSurface::Trace,
                DataSurface::ToolOutput,
                DataSurface::ExtensionData,
            ],
            trace: TraceDisclosureProjection {
                status: TraceStatus::Unavailable,
                preview: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TraceDisclosureProjection {
    pub status: TraceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<TracePreviewProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TracePreviewProjection {
    pub record_count: u64,
    pub approximate_bytes: u64,
    pub destination: TraceDestination,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exporter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_summary: Option<String>,
}

pub(super) fn validate_trace(
    trace: &TraceDisclosureProjection,
) -> Result<(), super::Spec030ValidationError> {
    match trace.status {
        TraceStatus::Preview => super::validation::require(
            trace.preview.is_some(),
            super::Spec030ValidationViolation::MissingEvidence,
        ),
        TraceStatus::Enabled => {
            let Some(preview) = trace.preview.as_ref() else {
                return super::validation::require(
                    false,
                    super::Spec030ValidationViolation::MissingEvidence,
                );
            };
            match preview.destination {
                TraceDestination::LocalOnly => Ok(()),
                TraceDestination::ConfiguredRemote => super::validation::require(
                    preview
                        .exporter
                        .as_deref()
                        .is_some_and(|value| !value.trim().is_empty())
                        && preview
                            .endpoint_summary
                            .as_deref()
                            .is_some_and(|value| !value.trim().is_empty()),
                    super::Spec030ValidationViolation::MissingEvidence,
                ),
            }
        }
        TraceStatus::Disabled | TraceStatus::Unavailable => super::validation::require(
            trace.preview.is_none(),
            super::Spec030ValidationViolation::InconsistentStatus,
        ),
    }
}
