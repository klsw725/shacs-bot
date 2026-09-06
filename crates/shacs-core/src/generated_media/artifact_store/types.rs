use super::GeneratedMediaContractError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactTransactionStage {
    PayloadSynced,
    RecordSynced,
    StagingDirectorySynced,
    Renamed,
    ParentDirectorySynced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactReadStage {
    BeforeArtifactDirectoryOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionDecision {
    Continue,
    Interrupt,
}

#[derive(Debug)]
pub enum ArtifactStoreError {
    InvalidContract(GeneratedMediaContractError),
    InvalidStore,
    InvalidRecord,
    ReferenceMismatch,
    NonRegularFile,
    AlreadyExists,
    SymlinkRejected,
    RemotePayloadRequiresPolicy,
    DigestMismatch,
    Interrupted(ArtifactTransactionStage),
    CommitStatusUnknown(ArtifactTransactionStage),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ArtifactStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContract(error) => write!(formatter, "{error}"),
            Self::InvalidStore => formatter.write_str("artifact store is invalid"),
            Self::InvalidRecord => formatter.write_str("artifact record is invalid"),
            Self::ReferenceMismatch => {
                formatter.write_str("artifact reference does not match the committed record")
            }
            Self::NonRegularFile => formatter.write_str("artifact source is not a regular file"),
            Self::AlreadyExists => formatter.write_str("artifact id already exists"),
            Self::SymlinkRejected => formatter.write_str("artifact store symlink rejected"),
            Self::RemotePayloadRequiresPolicy => {
                formatter.write_str("remote provider payload requires guarded output policy")
            }
            Self::DigestMismatch => formatter.write_str("artifact digest does not match record"),
            Self::Interrupted(stage) => {
                write!(formatter, "artifact transaction interrupted at {stage:?}")
            }
            Self::CommitStatusUnknown(stage) => {
                write!(formatter, "artifact commit status unknown after {stage:?}")
            }
            Self::Io(error) => write!(formatter, "artifact store I/O failure: {error}"),
            Self::Json(error) => write!(formatter, "artifact record JSON failure: {error}"),
        }
    }
}

impl std::error::Error for ArtifactStoreError {}

impl From<GeneratedMediaContractError> for ArtifactStoreError {
    fn from(error: GeneratedMediaContractError) -> Self {
        Self::InvalidContract(error)
    }
}
