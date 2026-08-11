use crate::state::{
    ApprovalActionState, ApprovalLineage, ApprovalStatus, PendingApproval, RuntimeSession,
    RuntimeSnapshot, SessionKey,
};
use serde_json::Value;
use shacs_config::{config_context, default_config_path};
use shacs_core::runtime::{surface_approval_availability, SurfaceApprovalAvailability};
use shacs_projection::Spec030RuntimeProjection;
use shacs_session::SessionManager;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait RuntimeProjectionSource {
    fn load(&self) -> Result<RuntimeSnapshot, TuiSourceError>;
}

#[derive(Debug, Clone)]
pub struct SessionRuntimeSource {
    config_path: Option<PathBuf>,
    workspace: PathBuf,
}

impl SessionRuntimeSource {
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        Self::with_config(None, workspace)
    }

    pub fn with_config(config_path: Option<PathBuf>, workspace: impl AsRef<Path>) -> Self {
        Self {
            config_path,
            workspace: workspace.as_ref().to_path_buf(),
        }
    }

    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    pub fn trusted_runtime_projection(&self) -> Spec030RuntimeProjection {
        shacs_api::observe_trusted_runtime(self.config_path.clone(), Some(self.workspace.clone()))
            .projection
    }
}

impl RuntimeProjectionSource for SessionRuntimeSource {
    fn load(&self) -> Result<RuntimeSnapshot, TuiSourceError> {
        let data_dir = config_context(
            Some(self.config_path.clone().unwrap_or_else(default_config_path)),
            Some(self.workspace.clone()),
        )
        .data_dir;
        let now_ms = now_ms();
        let Some(manager) =
            SessionManager::open_existing(&self.workspace).map_err(TuiSourceError::Store)?
        else {
            return Ok(RuntimeSnapshot {
                sessions: Vec::new(),
            });
        };
        let summaries = manager.list_session_ux().map_err(TuiSourceError::Store)?;
        let sessions = summaries
            .into_iter()
            .filter_map(|summary| {
                let detail = manager.session_ux_detail(&summary.key)?;
                let raw = manager.read_session_payload(&summary.key);
                Some(RuntimeSession {
                    key: SessionKey::new(detail.key).ok()?,
                    updated_at: detail.updated_at,
                    message_count: detail.message_count,
                    recovery_markers: detail.recovery_markers,
                    checkpoint_phase: detail.checkpoint_phase,
                    diagnostics_ref_count: detail.diagnostics_refs.len(),
                    workflow: detail.runtime_workflow,
                    execution: detail.runtime_execution,
                    pending_approval: raw
                        .as_ref()
                        .and_then(|payload| pending_approval(payload, &data_dir, now_ms)),
                })
            })
            .collect();
        Ok(RuntimeSnapshot { sessions })
    }
}

#[derive(Debug)]
pub enum TuiSourceError {
    Store(std::io::Error),
    StoreMissing,
}

impl std::fmt::Display for TuiSourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "session store could not be read: {error}"),
            Self::StoreMissing => write!(formatter, "session store was not found"),
        }
    }
}

impl std::error::Error for TuiSourceError {}

fn pending_approval(payload: &Value, data_dir: &Path, now_ms: u64) -> Option<PendingApproval> {
    let metadata = payload.get("metadata")?;
    metadata
        .get("pending_permission_approval")
        .and_then(|value| formal_approval(value, data_dir, now_ms))
        .or_else(|| {
            metadata
                .get("pending_recent_retry_approval")
                .and_then(retry_approval)
        })
}

fn formal_approval(value: &Value, data_dir: &Path, now_ms: u64) -> Option<PendingApproval> {
    let request = value.get("approval_request")?;
    let lineage = request
        .get("approval_request_id")
        .and_then(Value::as_str)
        .or_else(|| value.get("approval_request_id").and_then(Value::as_str))?;
    let tool_name = value
        .get("tool_call")
        .and_then(|tool| tool.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_owned();
    approval(
        lineage,
        tool_name,
        status(value),
        request.get("expires_at_unix_ms"),
        formal_action_state(data_dir, now_ms, status(value)),
    )
}

fn retry_approval(value: &Value) -> Option<PendingApproval> {
    let lineage = value.get("approval_request_id").and_then(Value::as_str)?;
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("recent_retry")
        .to_owned();
    approval(
        lineage,
        tool_name,
        status(value),
        value.get("expires_at_unix_ms"),
        ApprovalActionState::unavailable(
            "recent retry approval is process-local; reply in the original session channel",
        ),
    )
}

fn approval(
    lineage: &str,
    tool_name: String,
    status: ApprovalStatus,
    expires: Option<&Value>,
    action: ApprovalActionState,
) -> Option<PendingApproval> {
    Some(PendingApproval {
        lineage: ApprovalLineage::new(lineage).ok()?,
        tool_name,
        status,
        expires_at_unix_ms: expires.and_then(Value::as_u64),
        action,
    })
}

fn formal_action_state(
    data_dir: &Path,
    now_ms: u64,
    status: ApprovalStatus,
) -> ApprovalActionState {
    if status == ApprovalStatus::Executing {
        return ApprovalActionState::unavailable("permission approval is already executing");
    }
    match surface_approval_availability(data_dir, now_ms) {
        Ok(SurfaceApprovalAvailability::Actionable { target_owner_id }) => {
            ApprovalActionState::Actionable { target_owner_id }
        }
        Ok(SurfaceApprovalAvailability::Unavailable { reason }) => {
            ApprovalActionState::Unavailable { reason }
        }
        Err(error) => ApprovalActionState::Unavailable {
            reason: error.to_string(),
        },
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn status(value: &Value) -> ApprovalStatus {
    match value.get("status").and_then(Value::as_str) {
        Some("pending") => ApprovalStatus::Pending,
        Some("executing") => ApprovalStatus::Executing,
        Some(_) | None => ApprovalStatus::Unknown,
    }
}
