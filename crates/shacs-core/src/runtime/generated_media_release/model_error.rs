use super::{PublicationStage, Spec034ReleaseManifest};

#[derive(Debug)]
pub enum Spec034ReleaseArtifactError {
    InvalidConfig,
    CommandFailed,
    CommitStatusUnknown(PublicationStage),
    InvalidEvidence,
    DigestMismatch,
    CleanupIdentityMismatch,
    CleanupResidual { leak_count: u8 },
    Io(std::io::Error),
    Json(serde_json::Error),
    Command(shacs_projection::Spec031ReleaseArtifactError),
    CleanupFailed(Box<Spec034ReleaseArtifactError>),
    CombinedFailure {
        primary: Box<Spec034ReleaseArtifactError>,
        cleanup: Box<Spec034ReleaseArtifactError>,
    },
}

impl Spec034ReleaseArtifactError {
    pub(crate) fn combine<T>(
        primary: Result<T, Self>,
        cleanup: Result<(), Self>,
    ) -> Result<T, Self> {
        match (primary, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(primary), Ok(())) => Err(primary),
            (Ok(_), Err(cleanup)) => Err(Self::CleanupFailed(Box::new(cleanup))),
            (Err(primary), Err(cleanup)) => Err(Self::CombinedFailure {
                primary: Box::new(primary),
                cleanup: Box::new(cleanup),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec034StructuralAudit {
    pub manifest: Spec034ReleaseManifest,
    pub content_digest: String,
    pub execution_attested: bool,
    pub structural_only: bool,
}

impl std::fmt::Display for Spec034ReleaseArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec034ReleaseArtifactError {}
