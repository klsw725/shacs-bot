#[path = "video_analyzer_bounds.rs"]
mod bounds;

use self::bounds::{bounded_evidence, bounded_safe_text, has_evidence};
use super::file_context::{VideoAnalysisPolicy, VideoContextAnalysis};
use super::video_analyzer_owner_facts::{project_owner_facts, VideoAnalyzerOwnerFactsInput};
use super::VideoAnalyzerOwnerFactsProjection;
use serde::Serialize;
use std::fmt;

const MAX_FAILURE_REASON_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoAnalyzerCapability {
    Configured,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoAnalyzerStatus {
    Configured,
    AnalyzerMissing,
    Unsupported,
    ExtractionFailed,
    Included,
    Truncated,
    DurationCap,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerProjection {
    pub capability: VideoAnalyzerCapability,
    pub status: VideoAnalyzerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<VideoAnalyzerEvidenceProjection>,
    pub owner_facts: VideoAnalyzerOwnerFactsProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerEvidenceProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitles: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyframe_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_failures: Vec<VideoComponentFailureProjection>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoComponentFailureProjection {
    pub component: String,
    pub reason: String,
}

pub enum VideoAnalyzerOutcomeInput<'a> {
    Included(&'a VideoContextAnalysis),
    Unsupported(&'a str),
    Failed(&'a str),
    Cancelled,
    TimedOut,
}

pub struct VideoAnalyzerProjectionInput<'a> {
    pub capability: VideoAnalyzerCapability,
    pub duration_seconds: Option<u64>,
    pub policy: VideoAnalysisPolicy,
    pub outcome: Option<VideoAnalyzerOutcomeInput<'a>>,
    pub owner_facts: VideoAnalyzerOwnerFactsInput<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoAnalyzerProjectionError {
    OutcomeWithoutAnalyzer,
    OutcomeAfterDurationCap,
    EmptyIncludedEvidence,
}

impl fmt::Display for VideoAnalyzerProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutcomeWithoutAnalyzer => {
                formatter.write_str("video analyzer outcome exists without a configured analyzer")
            }
            Self::OutcomeAfterDurationCap => {
                formatter.write_str("video analyzer outcome exists after duration cap rejection")
            }
            Self::EmptyIncludedEvidence => {
                formatter.write_str("video analyzer included outcome has no bounded evidence")
            }
        }
    }
}

impl std::error::Error for VideoAnalyzerProjectionError {}

pub fn project_video_analyzer(
    input: VideoAnalyzerProjectionInput<'_>,
) -> Result<VideoAnalyzerProjection, VideoAnalyzerProjectionError> {
    let owner_facts = project_owner_facts(input.owner_facts);
    if input
        .duration_seconds
        .is_some_and(|duration| duration > input.policy.max_duration_seconds)
    {
        if input.outcome.is_some() {
            return Err(VideoAnalyzerProjectionError::OutcomeAfterDurationCap);
        }
        return Ok(VideoAnalyzerProjection {
            capability: input.capability,
            status: VideoAnalyzerStatus::DurationCap,
            reason: Some("video duration exceeds configured limit".to_owned()),
            evidence: None,
            owner_facts,
        });
    }
    match (input.capability, input.outcome) {
        (VideoAnalyzerCapability::Missing, Some(_)) => {
            Err(VideoAnalyzerProjectionError::OutcomeWithoutAnalyzer)
        }
        (VideoAnalyzerCapability::Missing, None) => Ok(VideoAnalyzerProjection {
            capability: VideoAnalyzerCapability::Missing,
            status: VideoAnalyzerStatus::AnalyzerMissing,
            reason: Some("video analyzer is not configured".to_owned()),
            evidence: None,
            owner_facts,
        }),
        (VideoAnalyzerCapability::Configured, None) => Ok(VideoAnalyzerProjection {
            capability: VideoAnalyzerCapability::Configured,
            status: VideoAnalyzerStatus::Configured,
            reason: None,
            evidence: None,
            owner_facts,
        }),
        (
            VideoAnalyzerCapability::Configured,
            Some(VideoAnalyzerOutcomeInput::Unsupported(reason)),
        ) => Ok(failed_projection(
            VideoAnalyzerStatus::Unsupported,
            reason,
            owner_facts,
        )),
        (VideoAnalyzerCapability::Configured, Some(VideoAnalyzerOutcomeInput::Failed(reason))) => {
            Ok(failed_projection(
                VideoAnalyzerStatus::ExtractionFailed,
                reason,
                owner_facts,
            ))
        }
        (VideoAnalyzerCapability::Configured, Some(VideoAnalyzerOutcomeInput::Cancelled)) => {
            Ok(failed_projection(
                VideoAnalyzerStatus::Cancelled,
                "video analyzer cancelled",
                owner_facts,
            ))
        }
        (VideoAnalyzerCapability::Configured, Some(VideoAnalyzerOutcomeInput::TimedOut)) => {
            Ok(failed_projection(
                VideoAnalyzerStatus::Timeout,
                "video analyzer timed out",
                owner_facts,
            ))
        }
        (
            VideoAnalyzerCapability::Configured,
            Some(VideoAnalyzerOutcomeInput::Included(analysis)),
        ) => {
            let evidence = bounded_evidence(analysis, input.policy);
            if !has_evidence(&evidence) {
                return Err(VideoAnalyzerProjectionError::EmptyIncludedEvidence);
            }
            let status = if evidence.truncated {
                VideoAnalyzerStatus::Truncated
            } else {
                VideoAnalyzerStatus::Included
            };
            Ok(VideoAnalyzerProjection {
                capability: VideoAnalyzerCapability::Configured,
                status,
                reason: None,
                evidence: Some(evidence),
                owner_facts,
            })
        }
    }
}

fn failed_projection(
    status: VideoAnalyzerStatus,
    reason: &str,
    owner_facts: VideoAnalyzerOwnerFactsProjection,
) -> VideoAnalyzerProjection {
    VideoAnalyzerProjection {
        capability: VideoAnalyzerCapability::Configured,
        status,
        reason: Some(
            bounded_safe_text(
                reason,
                MAX_FAILURE_REASON_CHARS,
                "analyzer failure details unavailable",
            )
            .0,
        ),
        evidence: None,
        owner_facts,
    }
}
