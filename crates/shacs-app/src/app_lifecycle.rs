use crate::app::AppId;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static RECEIPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppProcessState {
    Installed,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    RecoveryNeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppLifecycleAction {
    Start,
    Stop,
    Restart,
    Recover,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppLifecycleBlocker {
    AppNotEnabled,
    ManifestDigestMismatch,
    WorkspaceUntrusted,
    TrustedRuntimeUnavailable,
    CredentialMissing { name: String },
    ActivationMissing { resource_ref: String },
    ActivationStale { activation_ref: String },
    ActivationDisabled { activation_ref: String },
    ActivationRevoked { activation_ref: String },
    ActivationRemoved { activation_ref: String },
    RuntimePrerequisiteMissing,
    ProcessPermissionDenied,
    OwnerAlreadyRunning,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLifecycleReceipt {
    pub receipt_id: String,
    pub request_id: String,
    pub app_id: AppId,
    pub action: AppLifecycleAction,
    pub previous_state: AppProcessState,
    pub current_state: AppProcessState,
    pub generation: u64,
    pub completed: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manifest_digest: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trusted_runtime_ref: String,
    #[serde(default)]
    pub credential_source_statuses: Vec<String>,
    #[serde(default)]
    pub blockers: Vec<AppLifecycleBlocker>,
    #[serde(default)]
    pub activation_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_snapshot_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_outcome: Option<AppProcessOutcomeEvidence>,
    pub occurred_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppProcessOutcomeEvidence {
    pub outcome: String,
    pub duration_ms: u64,
    pub cleanup_attempted: bool,
    pub descendant_cleanup_supported: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSupervisorSnapshot {
    pub app_id: AppId,
    pub state: AppProcessState,
    pub generation: u64,
    pub last_receipt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLifecycleReplay {
    pub receipts: Vec<AppLifecycleReceipt>,
    pub dispatch_count: usize,
}

pub struct AppSupervisorJournal {
    root: PathBuf,
}

impl AppSupervisorJournal {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn request(
        &self,
        app_id: &AppId,
        action: AppLifecycleAction,
    ) -> Result<AppLifecycleReceipt, AppLifecycleError> {
        let _lock = self.lock(app_id)?;
        let current = self
            .inspect_optional_unlocked(app_id)?
            .unwrap_or(AppSupervisorSnapshot {
                app_id: app_id.clone(),
                state: AppProcessState::Installed,
                generation: 0,
                last_receipt_id: String::new(),
            });
        let next = requested_state(current.state, action)?;
        let generation = current.generation + u64::from(action == AppLifecycleAction::Start);
        let request_id = unique_id("request");
        let receipt = AppLifecycleReceipt {
            receipt_id: unique_id("requested"),
            request_id,
            app_id: app_id.clone(),
            action,
            previous_state: current.state,
            current_state: next,
            generation,
            completed: false,
            manifest_digest: String::new(),
            trusted_runtime_ref: String::new(),
            credential_source_statuses: Vec::new(),
            blockers: Vec::new(),
            activation_refs: Vec::new(),
            execution_snapshot_ref: None,
            process_outcome: None,
            occurred_at_unix_ms: unix_ms_now(),
        };
        self.persist(&receipt)?;
        Ok(receipt)
    }

    pub fn complete(
        &self,
        requested: &AppLifecycleReceipt,
        state: AppProcessState,
        manifest_digest: impl Into<String>,
        trusted_runtime_ref: impl Into<String>,
        credential_source_statuses: Vec<String>,
    ) -> Result<AppLifecycleReceipt, AppLifecycleError> {
        let _lock = self.lock(&requested.app_id)?;
        if let Some(existing) = self.completed_receipt_unlocked(requested)? {
            return Ok(existing);
        }
        self.require_current_request_unlocked(requested)?;
        if requested.completed || !completion_allowed(requested.current_state, state) {
            return Err(AppLifecycleError::InvalidTransition {
                from: requested.current_state,
                to: state,
            });
        }
        let receipt = AppLifecycleReceipt {
            receipt_id: unique_id("completed"),
            request_id: requested.request_id.clone(),
            app_id: requested.app_id.clone(),
            action: requested.action,
            previous_state: requested.current_state,
            current_state: state,
            generation: requested.generation,
            completed: true,
            manifest_digest: manifest_digest.into(),
            trusted_runtime_ref: trusted_runtime_ref.into(),
            credential_source_statuses,
            blockers: Vec::new(),
            activation_refs: requested.activation_refs.clone(),
            execution_snapshot_ref: requested.execution_snapshot_ref.clone(),
            process_outcome: requested.process_outcome.clone(),
            occurred_at_unix_ms: unix_ms_now(),
        };
        self.persist(&receipt)?;
        Ok(receipt)
    }

    pub fn block(
        &self,
        requested: &AppLifecycleReceipt,
        blockers: Vec<AppLifecycleBlocker>,
    ) -> Result<AppLifecycleReceipt, AppLifecycleError> {
        let _lock = self.lock(&requested.app_id)?;
        if blockers.is_empty() {
            return Err(AppLifecycleError::MissingBlocker);
        }
        if requested.completed
            || !completion_allowed(requested.current_state, AppProcessState::Failed)
        {
            return Err(AppLifecycleError::InvalidTransition {
                from: requested.current_state,
                to: AppProcessState::Failed,
            });
        }
        if let Some(existing) = self.completed_receipt_unlocked(requested)? {
            return Ok(existing);
        }
        self.require_current_request_unlocked(requested)?;
        let receipt = AppLifecycleReceipt {
            receipt_id: unique_id("blocked"),
            request_id: requested.request_id.clone(),
            app_id: requested.app_id.clone(),
            action: requested.action,
            previous_state: requested.current_state,
            current_state: AppProcessState::Failed,
            generation: requested.generation,
            completed: true,
            manifest_digest: String::new(),
            trusted_runtime_ref: String::new(),
            credential_source_statuses: Vec::new(),
            blockers,
            activation_refs: requested.activation_refs.clone(),
            execution_snapshot_ref: requested.execution_snapshot_ref.clone(),
            process_outcome: requested.process_outcome.clone(),
            occurred_at_unix_ms: unix_ms_now(),
        };
        self.persist(&receipt)?;
        Ok(receipt)
    }

    pub fn attach_start_evidence(
        &self,
        requested: &mut AppLifecycleReceipt,
        activation_refs: Vec<String>,
        execution_snapshot_ref: Option<String>,
    ) {
        requested.activation_refs = activation_refs;
        requested.execution_snapshot_ref = execution_snapshot_ref;
    }

    pub fn inspect(&self, app_id: &AppId) -> Result<AppSupervisorSnapshot, AppLifecycleError> {
        let _lock = self.lock(app_id)?;
        self.inspect_optional_unlocked(app_id)?
            .ok_or_else(|| AppLifecycleError::UnknownApp(app_id.clone()))
    }

    pub fn with_locked_snapshot<T, E>(
        &self,
        app_id: &AppId,
        action: impl FnOnce(Option<&AppSupervisorSnapshot>) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<AppLifecycleError>,
    {
        let _lock = self.lock(app_id).map_err(E::from)?;
        let snapshot = self.inspect_optional_unlocked(app_id).map_err(E::from)?;
        action(snapshot.as_ref())
    }

    pub fn replay(&self, app_id: &AppId) -> Result<AppLifecycleReplay, AppLifecycleError> {
        let dir = self.receipt_dir(app_id);
        if !dir.exists() {
            return Ok(AppLifecycleReplay {
                receipts: Vec::new(),
                dispatch_count: 0,
            });
        }
        let mut receipts = fs::read_dir(dir)?
            .map(|entry| -> Result<AppLifecycleReceipt, AppLifecycleError> {
                let bytes = fs::read(entry?.path())?;
                Ok(serde_json::from_slice(&bytes)?)
            })
            .collect::<Result<Vec<_>, _>>()?;
        receipts.sort_by(|left, right| {
            left.occurred_at_unix_ms
                .cmp(&right.occurred_at_unix_ms)
                .then(left.receipt_id.cmp(&right.receipt_id))
        });
        Ok(AppLifecycleReplay {
            receipts,
            dispatch_count: 0,
        })
    }

    pub fn pending_request(
        &self,
        app_id: &AppId,
        action: AppLifecycleAction,
    ) -> Result<Option<AppLifecycleReceipt>, AppLifecycleError> {
        let replay = self.replay(app_id)?;
        let completed = replay
            .receipts
            .iter()
            .filter(|receipt| receipt.completed)
            .map(|receipt| receipt.request_id.clone())
            .collect::<BTreeSet<_>>();
        Ok(replay.receipts.into_iter().rev().find(|candidate| {
            candidate.action == action
                && !candidate.completed
                && !completed.contains(candidate.request_id.as_str())
        }))
    }

    fn inspect_optional_unlocked(
        &self,
        app_id: &AppId,
    ) -> Result<Option<AppSupervisorSnapshot>, AppLifecycleError> {
        let replay = self.replay(app_id)?;
        Ok(replay.receipts.last().map(|receipt| AppSupervisorSnapshot {
            app_id: receipt.app_id.clone(),
            state: receipt.current_state,
            generation: receipt.generation,
            last_receipt_id: receipt.receipt_id.clone(),
        }))
    }

    fn completed_receipt_unlocked(
        &self,
        requested: &AppLifecycleReceipt,
    ) -> Result<Option<AppLifecycleReceipt>, AppLifecycleError> {
        Ok(self
            .replay(&requested.app_id)?
            .receipts
            .into_iter()
            .find(|receipt| receipt.request_id == requested.request_id && receipt.completed))
    }

    fn require_current_request_unlocked(
        &self,
        requested: &AppLifecycleReceipt,
    ) -> Result<(), AppLifecycleError> {
        let current = self
            .inspect_optional_unlocked(&requested.app_id)?
            .ok_or_else(|| AppLifecycleError::UnknownApp(requested.app_id.clone()))?;
        if current.last_receipt_id != requested.receipt_id
            || current.state != requested.current_state
            || current.generation != requested.generation
        {
            return Err(AppLifecycleError::StaleRequest(
                requested.request_id.clone(),
            ));
        }
        Ok(())
    }

    fn lock(&self, app_id: &AppId) -> Result<File, AppLifecycleError> {
        let app_dir = self.app_dir(app_id);
        fs::create_dir_all(&app_dir)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(app_dir.join("lifecycle.lock"))?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn persist(&self, receipt: &AppLifecycleReceipt) -> Result<(), AppLifecycleError> {
        self.persist_receipt(receipt)?;
        write_json_atomic(
            &self.state_path(&receipt.app_id),
            &AppSupervisorSnapshot {
                app_id: receipt.app_id.clone(),
                state: receipt.current_state,
                generation: receipt.generation,
                last_receipt_id: receipt.receipt_id.clone(),
            },
        )
    }

    fn persist_receipt(&self, receipt: &AppLifecycleReceipt) -> Result<(), AppLifecycleError> {
        write_json_atomic(
            &self
                .receipt_dir(&receipt.app_id)
                .join(format!("{}.json", receipt.receipt_id)),
            receipt,
        )
    }

    fn app_dir(&self, app_id: &AppId) -> PathBuf {
        self.root.join("app-supervisor").join(app_id.as_str())
    }
    fn state_path(&self, app_id: &AppId) -> PathBuf {
        self.app_dir(app_id).join("state.json")
    }
    fn receipt_dir(&self, app_id: &AppId) -> PathBuf {
        self.app_dir(app_id).join("receipts")
    }
}

#[derive(Debug)]
pub enum AppLifecycleError {
    Io(io::Error),
    Json(serde_json::Error),
    UnknownApp(AppId),
    MissingBlocker,
    StaleRequest(String),
    InvalidTransition {
        from: AppProcessState,
        to: AppProcessState,
    },
}

impl fmt::Display for AppLifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "app lifecycle I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "app lifecycle JSON failed: {error}"),
            Self::UnknownApp(app_id) => write!(formatter, "no app lifecycle state for `{app_id}`"),
            Self::MissingBlocker => {
                formatter.write_str("blocked app lifecycle receipt requires a blocker")
            }
            Self::StaleRequest(request_id) => {
                write!(formatter, "stale app lifecycle request `{request_id}`")
            }
            Self::InvalidTransition { from, to } => write!(
                formatter,
                "invalid app lifecycle transition from {from:?} to {to:?}"
            ),
        }
    }
}

impl std::error::Error for AppLifecycleError {}
impl From<io::Error> for AppLifecycleError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<serde_json::Error> for AppLifecycleError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

fn requested_state(
    state: AppProcessState,
    action: AppLifecycleAction,
) -> Result<AppProcessState, AppLifecycleError> {
    match (state, action) {
        (
            AppProcessState::Installed | AppProcessState::Stopped | AppProcessState::Failed,
            AppLifecycleAction::Start,
        ) => Ok(AppProcessState::Starting),
        (AppProcessState::Running, AppLifecycleAction::Stop | AppLifecycleAction::Restart) => {
            Ok(AppProcessState::Stopping)
        }
        (
            AppProcessState::Installed
            | AppProcessState::Starting
            | AppProcessState::Running
            | AppProcessState::Stopping
            | AppProcessState::Stopped
            | AppProcessState::Failed
            | AppProcessState::RecoveryNeeded,
            AppLifecycleAction::Recover,
        ) => Ok(AppProcessState::RecoveryNeeded),
        _ => Err(AppLifecycleError::InvalidTransition {
            from: state,
            to: state,
        }),
    }
}

fn completion_allowed(from: AppProcessState, to: AppProcessState) -> bool {
    matches!(
        (from, to),
        (
            AppProcessState::Starting,
            AppProcessState::Running | AppProcessState::Failed | AppProcessState::RecoveryNeeded
        ) | (
            AppProcessState::Stopping,
            AppProcessState::Stopped | AppProcessState::Failed | AppProcessState::RecoveryNeeded
        ) | (
            AppProcessState::RecoveryNeeded,
            AppProcessState::Stopped | AppProcessState::RecoveryNeeded | AppProcessState::Failed
        )
    )
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), AppLifecycleError> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn unique_id(prefix: &str) -> String {
    let sequence = RECEIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!(
        "{prefix}-{}-{}-{sequence}",
        std::process::id(),
        unix_ms_now()
    )
}
fn unix_ms_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}
