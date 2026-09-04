use super::*;

pub(crate) struct ExecutionLedger;

#[derive(Clone)]
pub(crate) struct ExecutionQueue;

impl ExecutionLedger {
    pub(crate) fn new_queue() -> Result<ExecutionQueue, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub(crate) fn ensure_capacity(
        _path_count: usize,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub(crate) fn arm(_paths: &[PathBuf]) -> Result<Self, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub(crate) fn arm_on(
        _paths: &[PathBuf],
        _queue: ExecutionQueue,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub(crate) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}
