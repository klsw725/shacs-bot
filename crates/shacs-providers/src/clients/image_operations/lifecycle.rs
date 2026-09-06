use crate::{
    ImageOperationResult, ProviderMediaCandidateId, ProviderMediaLifecycleObservation,
    ProviderMediaLifecycleStatus,
};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOperationLifecycleState {
    AwaitingStart,
    Started,
    Partial,
    Final,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageOperationLifecycle {
    state: ImageOperationLifecycleState,
    candidate_id: Option<ProviderMediaCandidateId>,
    last_sequence: Option<u32>,
}

impl Default for ImageOperationLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageOperationLifecycle {
    pub const fn new() -> Self {
        Self {
            state: ImageOperationLifecycleState::AwaitingStart,
            candidate_id: None,
            last_sequence: None,
        }
    }

    pub const fn state(&self) -> ImageOperationLifecycleState {
        self.state
    }

    pub fn apply(
        &mut self,
        observation: &ProviderMediaLifecycleObservation,
    ) -> Result<(), ImageLifecycleError> {
        let status = observation.status();
        if self.state == ImageOperationLifecycleState::AwaitingStart {
            self.state = match status {
                ProviderMediaLifecycleStatus::Started => ImageOperationLifecycleState::Started,
                ProviderMediaLifecycleStatus::Cancelled => ImageOperationLifecycleState::Cancelled,
                ProviderMediaLifecycleStatus::Partial
                | ProviderMediaLifecycleStatus::Final
                | ProviderMediaLifecycleStatus::Failed => return Err(self.invalid(status)),
            };
            self.candidate_id = Some(observation.candidate_id().clone());
            return Ok(());
        }
        if self.candidate_id.as_ref() != Some(observation.candidate_id()) {
            return Err(ImageLifecycleError::CandidateMismatch);
        }
        if matches!(
            self.state,
            ImageOperationLifecycleState::Final
                | ImageOperationLifecycleState::Failed
                | ImageOperationLifecycleState::Cancelled
        ) {
            return Err(self.invalid(status));
        }
        self.validate_sequence(observation.sequence())?;
        self.state = match status {
            ProviderMediaLifecycleStatus::Started => return Err(self.invalid(status)),
            ProviderMediaLifecycleStatus::Partial => ImageOperationLifecycleState::Partial,
            ProviderMediaLifecycleStatus::Final => ImageOperationLifecycleState::Final,
            ProviderMediaLifecycleStatus::Failed => ImageOperationLifecycleState::Failed,
            ProviderMediaLifecycleStatus::Cancelled => ImageOperationLifecycleState::Cancelled,
        };
        self.last_sequence = observation.sequence().or(self.last_sequence);
        Ok(())
    }

    pub fn finalize(
        &self,
        result: ImageOperationResult,
    ) -> Result<ImageOperationResult, ImageLifecycleError> {
        if self.state != ImageOperationLifecycleState::Final {
            return Err(ImageLifecycleError::FinalEventRequired);
        }
        Ok(result)
    }

    fn validate_sequence(&self, sequence: Option<u32>) -> Result<(), ImageLifecycleError> {
        if let (Some(last), Some(next)) = (self.last_sequence, sequence) {
            if next < last {
                return Err(ImageLifecycleError::SequenceRegression { last, next });
            }
        }
        Ok(())
    }

    const fn invalid(&self, event: ProviderMediaLifecycleStatus) -> ImageLifecycleError {
        ImageLifecycleError::InvalidTransition {
            from: self.state,
            event,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageLifecycleError {
    InvalidTransition {
        from: ImageOperationLifecycleState,
        event: ProviderMediaLifecycleStatus,
    },
    CandidateMismatch,
    SequenceRegression {
        last: u32,
        next: u32,
    },
    FinalEventRequired,
}

impl fmt::Display for ImageLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, event } => {
                write!(
                    formatter,
                    "invalid image lifecycle transition: {from:?} -> {event:?}"
                )
            }
            Self::CandidateMismatch => formatter.write_str("image lifecycle candidate changed"),
            Self::SequenceRegression { last, next } => {
                write!(
                    formatter,
                    "image lifecycle sequence regressed: {next} < {last}"
                )
            }
            Self::FinalEventRequired => {
                formatter.write_str("image operation requires an explicit final event")
            }
        }
    }
}

impl std::error::Error for ImageLifecycleError {}
