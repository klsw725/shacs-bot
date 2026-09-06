use super::{
    project_video_analyzer_spec035, Spec035MediaProjectionStore, Spec035MediaProjectionStoreError,
    VideoAnalyzerSpec035Error, VideoAnalyzerSpec035Input,
};
use crate::runtime::{
    project_video_analyzer, VideoAnalysisPolicy, VideoAnalyzerCapability,
    VideoAnalyzerOutcomeInput, VideoAnalyzerOwnerFactsInput, VideoAnalyzerOwnerFactsProjection,
    VideoAnalyzerProjectionError, VideoAnalyzerProjectionInput, VideoContextAnalysis,
    VideoContextError,
};
use sha2::{Digest, Sha256};
use shacs_projection::{
    Spec031ExternalOwnerRef, Spec031Freshness, Spec035MediaProjection,
};
use std::fmt::{Display, Formatter};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub(crate) struct VideoAnalyzerSpec035Publisher {
    store: Spec035MediaProjectionStore,
    owner_facts: Option<VideoAnalyzerOwnerFactsProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoAnalyzerSpec035PublicationStatus {
    Published,
    Reconciled,
    CommitStatusUnknown,
}

#[derive(Debug)]
pub(crate) enum VideoAnalyzerSpec035PublicationError {
    Projection(VideoAnalyzerProjectionError),
    Mapping(VideoAnalyzerSpec035Error),
    Store(Spec035MediaProjectionStoreError),
    InvalidArtifactRef,
}

impl VideoAnalyzerSpec035Publisher {
    pub(crate) const fn new(
        store: Spec035MediaProjectionStore,
        owner_facts: Option<VideoAnalyzerOwnerFactsProjection>,
    ) -> Self {
        Self { store, owner_facts }
    }

    pub(crate) fn publish_result(
        &self,
        bytes: &[u8],
        duration_seconds: Option<u64>,
        policy: VideoAnalysisPolicy,
        result: &Result<VideoContextAnalysis, VideoContextError>,
    ) -> Result<VideoAnalyzerSpec035PublicationStatus, VideoAnalyzerSpec035PublicationError> {
        let outcome = match result {
            Ok(analysis) => VideoAnalyzerOutcomeInput::Included(analysis),
            Err(VideoContextError::Unsupported(reason)) => {
                VideoAnalyzerOutcomeInput::Unsupported(reason)
            }
            Err(VideoContextError::Failed(reason)) => VideoAnalyzerOutcomeInput::Failed(reason),
            Err(VideoContextError::Cancelled) => VideoAnalyzerOutcomeInput::Cancelled,
            Err(VideoContextError::TimedOut) => VideoAnalyzerOutcomeInput::TimedOut,
        };
        let mut analyzer = project_video_analyzer(VideoAnalyzerProjectionInput {
            capability: VideoAnalyzerCapability::Configured,
            duration_seconds,
            policy,
            outcome: Some(outcome),
            owner_facts: VideoAnalyzerOwnerFactsInput::unavailable(Spec031Freshness::Current),
        })?;
        if let Some(owner_facts) = self.owner_facts.as_ref() {
            analyzer.owner_facts = owner_facts.clone();
        }
        let artifact_ref = Spec031ExternalOwnerRef::try_new(&format!(
            "spec034://media/artifact/{:x}",
            Sha256::digest(bytes)
        ))
        .map_err(|_| VideoAnalyzerSpec035PublicationError::InvalidArtifactRef)?;
        let projection = project_video_analyzer_spec035(VideoAnalyzerSpec035Input {
            artifact_ref: &artifact_ref,
            analyzer: &analyzer,
        })?;
        self.publish_projection_with(&projection, Spec035MediaProjectionStore::publish)
    }

    fn publish_projection_with<P>(
        &self,
        projection: &Spec035MediaProjection,
        publish: P,
    ) -> Result<VideoAnalyzerSpec035PublicationStatus, VideoAnalyzerSpec035PublicationError>
    where
        P: FnOnce(
            &Spec035MediaProjectionStore,
            &Spec035MediaProjection,
        ) -> Result<(), Spec035MediaProjectionStoreError>,
    {
        match publish(&self.store, projection) {
            Ok(()) => Ok(VideoAnalyzerSpec035PublicationStatus::Published),
            Err(Spec035MediaProjectionStoreError::CommitStatusUnknown(_)) => {
                match self.store.read() {
                    Ok(Some(observed)) if observed == *projection => {
                        Ok(VideoAnalyzerSpec035PublicationStatus::Reconciled)
                    }
                    Ok(Some(_)) | Ok(None) | Err(_) => {
                        Ok(VideoAnalyzerSpec035PublicationStatus::CommitStatusUnknown)
                    }
                }
            }
            Err(error) => Err(VideoAnalyzerSpec035PublicationError::Store(error)),
        }
    }
}

impl From<VideoAnalyzerProjectionError> for VideoAnalyzerSpec035PublicationError {
    fn from(error: VideoAnalyzerProjectionError) -> Self {
        Self::Projection(error)
    }
}

impl From<VideoAnalyzerSpec035Error> for VideoAnalyzerSpec035PublicationError {
    fn from(error: VideoAnalyzerSpec035Error) -> Self {
        Self::Mapping(error)
    }
}

impl From<Spec035MediaProjectionStoreError> for VideoAnalyzerSpec035PublicationError {
    fn from(error: Spec035MediaProjectionStoreError) -> Self {
        Self::Store(error)
    }
}

impl Display for VideoAnalyzerSpec035PublicationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Projection(error) => write!(formatter, "analyzer projection failed: {error}"),
            Self::Mapping(error) => write!(formatter, "Spec035 mapping failed: {error}"),
            Self::Store(error) => write!(formatter, "projection publication failed: {error}"),
            Self::InvalidArtifactRef => formatter.write_str("artifact reference is invalid"),
        }
    }
}

impl std::error::Error for VideoAnalyzerSpec035PublicationError {}
