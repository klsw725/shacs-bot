use super::VideoAnalyzerSpec035Error;
use crate::runtime::{VideoAnalyzerProjection, VideoAnalyzerStatus};
use shacs_projection::Spec035MediaState;

pub(super) fn map_terminal_state(
    analyzer: &VideoAnalyzerProjection,
) -> Result<Spec035MediaState, VideoAnalyzerSpec035Error> {
    match analyzer.status {
        VideoAnalyzerStatus::Configured => Err(VideoAnalyzerSpec035Error::NonTerminalConfigured),
        VideoAnalyzerStatus::AnalyzerMissing => {
            require_absent_evidence(analyzer, Spec035MediaState::AnalyzerMissing)
        }
        VideoAnalyzerStatus::Unsupported | VideoAnalyzerStatus::DurationCap => {
            require_absent_evidence(analyzer, Spec035MediaState::Unsupported)
        }
        VideoAnalyzerStatus::ExtractionFailed
        | VideoAnalyzerStatus::Cancelled
        | VideoAnalyzerStatus::Timeout => {
            require_absent_evidence(analyzer, Spec035MediaState::ExtractionFailed)
        }
        VideoAnalyzerStatus::Included => match analyzer.evidence.as_ref() {
            Some(evidence) if !evidence.truncated => Ok(Spec035MediaState::Included),
            Some(_) | None => Err(VideoAnalyzerSpec035Error::InconsistentAnalyzerFacts),
        },
        VideoAnalyzerStatus::Truncated => match analyzer.evidence.as_ref() {
            Some(evidence) if evidence.truncated => Ok(Spec035MediaState::Truncated),
            Some(_) | None => Err(VideoAnalyzerSpec035Error::InconsistentAnalyzerFacts),
        },
    }
}

pub(super) const fn reason_summary(state: Spec035MediaState) -> &'static str {
    match state {
        Spec035MediaState::Included => "media evidence included",
        Spec035MediaState::Unsupported => "media capability unsupported",
        Spec035MediaState::ExtractionFailed => "media extraction failed",
        Spec035MediaState::AnalyzerMissing => "media analyzer missing",
        Spec035MediaState::Truncated => "media evidence truncated",
        Spec035MediaState::Unavailable => "media evidence unavailable",
    }
}

fn require_absent_evidence(
    analyzer: &VideoAnalyzerProjection,
    state: Spec035MediaState,
) -> Result<Spec035MediaState, VideoAnalyzerSpec035Error> {
    match analyzer.evidence {
        None => Ok(state),
        Some(_) => Err(VideoAnalyzerSpec035Error::InconsistentAnalyzerFacts),
    }
}
