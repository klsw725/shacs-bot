use crate::app::{AppError, AppId, AppRegistryEntry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalKind {
    Install,
    Update,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactedIntent {
    pub digest: String,
    pub redacted: bool,
}

impl RedactedIntent {
    pub(crate) fn new(intent: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"shacs-authoring-intent-v1\n");
        hasher.update(intent.as_bytes());
        Self {
            digest: format!("sha256:{:x}", hasher.finalize()),
            redacted: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringProposal {
    pub proposal_id: String,
    pub app_id: AppId,
    pub kind: ProposalKind,
    pub user_intent: RedactedIntent,
    pub revision_digest: String,
    pub candidate_digest: String,
    pub installed_digest: Option<String>,
    pub installed_tree_digest: Option<String>,
    pub validation_summary: String,
    pub risk_summary: String,
    pub diff: Vec<String>,
    #[serde(skip_serializing, default)]
    pub(crate) candidate_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoringCheckpoint {
    pub checkpoint_id: String,
    pub proposal: AuthoringProposal,
    #[serde(skip_serializing, default)]
    pub snapshot_path: Option<PathBuf>,
    #[serde(default)]
    pub original_registry_entry: Option<AppRegistryEntry>,
}

#[derive(Debug)]
pub struct ApplyPending {
    pub(crate) checkpoint: AuthoringCheckpoint,
    pub(crate) target_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Passed,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEvidence {
    pub checkpoint_id: String,
    pub recovery_required: bool,
    pub phase: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallHandoff {
    pub checkpoint_id: String,
    pub app_id: AppId,
    pub version: String,
    pub digest: String,
    #[serde(skip_serializing)]
    pub registry_entry: AppRegistryEntry,
    pub runtime_authorization_created: bool,
    pub executable_activation_created: bool,
    pub process_started: bool,
}

#[derive(Debug)]
pub enum ApplyError {
    App(AppError),
    Io(io::Error),
    Json(serde_json::Error),
    AlreadyInstalled(AppId),
    NotInstalled(AppId),
    StaleRevision {
        expected: String,
        actual: String,
    },
    InstalledDigestChanged {
        expected: Option<String>,
        actual: Option<String>,
    },
    VerificationFailed {
        reason: String,
    },
    RecoveryNotRequired(String),
    UnsafeCandidate(PathBuf),
}

impl fmt::Display for ApplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::App(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "authoring flow I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "authoring flow JSON failed: {error}"),
            Self::AlreadyInstalled(app_id) => {
                write!(formatter, "app `{app_id}` is already installed")
            }
            Self::NotInstalled(app_id) => write!(formatter, "app `{app_id}` is not installed"),
            Self::StaleRevision { expected, actual } => write!(
                formatter,
                "stale revision: expected {expected}, found {actual}"
            ),
            Self::InstalledDigestChanged { expected, actual } => write!(
                formatter,
                "installed digest changed: expected {expected:?}, found {actual:?}"
            ),
            Self::VerificationFailed { reason } => {
                write!(formatter, "verification failed: {reason}")
            }
            Self::RecoveryNotRequired(checkpoint_id) => {
                write!(
                    formatter,
                    "checkpoint `{checkpoint_id}` does not require recovery"
                )
            }
            Self::UnsafeCandidate(path) => {
                write!(formatter, "unsafe candidate path `{}`", path.display())
            }
        }
    }
}

impl std::error::Error for ApplyError {}
impl From<AppError> for ApplyError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}
impl From<io::Error> for ApplyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for ApplyError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
