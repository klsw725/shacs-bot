mod owner;
mod publication;
mod status;
mod store;
#[cfg(test)]
mod store_tests;

pub(crate) use publication::{
    VideoAnalyzerSpec035PublicationStatus, VideoAnalyzerSpec035Publisher,
};
pub use store::{
    Spec035MediaProjectionStore, Spec035MediaProjectionStoreError,
    Spec035MediaProjectionTransactionStage,
};

use self::owner::map_owner_facts;
use self::status::{map_terminal_state, reason_summary};
use super::media_evidence_replay::analyzer_evidence_digest;
use super::VideoAnalyzerProjection;
use shacs_projection::{
    Spec031ExternalOwnerRef, Spec031Freshness, Spec031SafeSummary, Spec035MediaDigest,
    Spec035MediaLineage, Spec035MediaProjection, Spec035MediaProjectionInput, Spec035MediaReason,
    Spec035MediaState, Spec035MediaValidationError, Spec035MediaValidationErrorKind,
};
use std::fmt::{Display, Formatter};

pub struct VideoAnalyzerSpec035Input<'a> {
    pub artifact_ref: &'a Spec031ExternalOwnerRef,
    pub analyzer: &'a VideoAnalyzerProjection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoAnalyzerSpec035Error {
    NonTerminalConfigured,
    InconsistentAnalyzerFacts,
    InconsistentOwnerFacts,
    InvalidTarget(Spec035MediaValidationErrorKind),
}

pub fn project_video_analyzer_spec035(
    input: VideoAnalyzerSpec035Input<'_>,
) -> Result<Spec035MediaProjection, VideoAnalyzerSpec035Error> {
    let mut state = map_terminal_state(input.analyzer)?;
    let owner = map_owner_facts(&input.analyzer.owner_facts)?;
    if matches!(
        state,
        Spec035MediaState::Included | Spec035MediaState::Truncated
    ) && owner.input.freshness != Spec031Freshness::Current
    {
        state = Spec035MediaState::Unavailable;
    }
    if state == Spec035MediaState::AnalyzerMissing
        && (owner.input.freshness != Spec031Freshness::Unavailable
            || !owner.input.unavailable_reasons.contains(
                &shacs_projection::Spec035MediaOwnerUnavailableReason::MissingAnalyzerOwnerRef,
            ))
    {
        return Err(VideoAnalyzerSpec035Error::InconsistentOwnerFacts);
    }
    let evidence_digest = match state {
        Spec035MediaState::Included | Spec035MediaState::Truncated => {
            Some(Spec035MediaDigest::try_new(&analyzer_evidence_digest(
                input
                    .analyzer
                    .evidence
                    .as_ref()
                    .ok_or(VideoAnalyzerSpec035Error::InconsistentAnalyzerFacts)?,
            )?)?)
        }
        Spec035MediaState::Unsupported
        | Spec035MediaState::ExtractionFailed
        | Spec035MediaState::AnalyzerMissing
        | Spec035MediaState::Unavailable => None,
    };
    Spec035MediaProjection::try_new(Spec035MediaProjectionInput {
        state,
        reason: Spec035MediaReason {
            code: state.into(),
            safe_summary: Spec031SafeSummary::try_new(reason_summary(state))
                .map_err(|_| VideoAnalyzerSpec035Error::InconsistentAnalyzerFacts)?,
        },
        lineage: Spec035MediaLineage {
            artifact_ref: input.artifact_ref.clone(),
            analyzer_ref: owner.analyzer_ref,
            snapshot_ref: owner.snapshot_ref,
            evidence_digest,
        },
        owner_facts: owner.input,
    })
    .map_err(Into::into)
}

impl From<Spec035MediaValidationError> for VideoAnalyzerSpec035Error {
    fn from(error: Spec035MediaValidationError) -> Self {
        Self::InvalidTarget(error.kind())
    }
}

impl From<super::MediaEvidenceProjectionError> for VideoAnalyzerSpec035Error {
    fn from(_error: super::MediaEvidenceProjectionError) -> Self {
        Self::InconsistentAnalyzerFacts
    }
}

impl Display for VideoAnalyzerSpec035Error {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "video analyzer Spec035 mapping failed: {self:?}")
    }
}

impl std::error::Error for VideoAnalyzerSpec035Error {}
