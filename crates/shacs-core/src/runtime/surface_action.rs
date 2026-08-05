mod approval;
mod lock;
mod marker;
mod recover;

use lock::RuntimeOwnershipMutationLock;
use marker::{
    read_runtime_ownership_marker, runtime_ownership_marker_path,
    runtime_stop_request_marker_value, write_runtime_marker_atomically,
};
use serde_json::json;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, RUNTIME_RESTART_REQUESTED,
    RUNTIME_STOP_REQUESTED,
};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

pub use approval::{
    request_surface_approval, surface_approval_availability, SurfaceApprovalAvailability,
    SurfaceApprovalDecision, SurfaceApprovalRequest, SURFACE_APPROVAL_PAYLOAD_TYPE,
    SURFACE_APPROVAL_WORK_KIND,
};
pub use recover::recover_runtime_surface;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceAction {
    Stop,
    Restart,
    Recover,
    Approve {
        session_key: String,
        lineage: String,
    },
    Deny {
        session_key: String,
        lineage: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceActionRequestKind {
    Stop,
    Restart,
}

impl SurfaceActionRequestKind {
    const fn request(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }

    const fn event_kind(self) -> &'static str {
        match self {
            Self::Stop => RUNTIME_STOP_REQUESTED,
            Self::Restart => RUNTIME_RESTART_REQUESTED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceActionOutcome {
    pub kind: SurfaceActionOutcomeKind,
    pub changed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceActionOutcomeKind {
    Requested,
    Completed,
    Unavailable,
    StaleLineage,
}

#[derive(Debug)]
pub enum SurfaceActionError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidMarker(String),
    Durable(String),
}

impl fmt::Display for SurfaceActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "runtime surface action I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "runtime surface action JSON failed: {error}"),
            Self::InvalidMarker(reason) => write!(formatter, "runtime marker invalid: {reason}"),
            Self::Durable(reason) => write!(formatter, "runtime durable evidence failed: {reason}"),
        }
    }
}

impl std::error::Error for SurfaceActionError {}

impl From<io::Error> for SurfaceActionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for SurfaceActionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<shacs_session::durable_work::DurableWorkError> for SurfaceActionError {
    fn from(error: shacs_session::durable_work::DurableWorkError) -> Self {
        Self::Durable(error.to_string())
    }
}

pub fn request_runtime_control(
    data_dir: &Path,
    kind: SurfaceActionRequestKind,
    now_ms: u64,
) -> Result<SurfaceActionOutcome, SurfaceActionError> {
    let ownership_path = runtime_ownership_marker_path(data_dir);
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(&ownership_path)?;
    let Some(owner) = read_runtime_ownership_marker(&ownership_path)? else {
        return Ok(SurfaceActionOutcome {
            kind: SurfaceActionOutcomeKind::Unavailable,
            changed: false,
            detail: "no active runtime owner found".to_owned(),
        });
    };
    let ownership = classify_runtime_ownership(&owner, now_ms);
    if ownership != RuntimeOwnershipState::Active {
        return Ok(SurfaceActionOutcome {
            kind: SurfaceActionOutcomeKind::Unavailable,
            changed: false,
            detail: "stale ownership marker exists; run `shacs-bot runtime recover`".to_owned(),
        });
    }
    let request = kind.request();
    let request_id = format!("{request}-{}-{now_ms}", owner.owner_id);
    let event_sequence =
        append_runtime_control_request(data_dir, kind, now_ms, &request_id, &owner.owner_id)?;
    let current = read_runtime_ownership_marker(&ownership_path)?;
    if current.as_ref().map(|marker| marker.owner_id.as_str()) != Some(owner.owner_id.as_str()) {
        return Ok(SurfaceActionOutcome {
            kind: SurfaceActionOutcomeKind::StaleLineage,
            changed: false,
            detail: "runtime control request blocked by owner generation change".to_owned(),
        });
    }
    write_runtime_marker_atomically(
        &runtime_stop_request_marker_path(data_dir),
        &runtime_stop_request_marker_value(
            request,
            &request_id,
            Some(owner.pid),
            Some(&owner.owner_id),
            event_sequence,
            now_ms,
        ),
    )?;
    Ok(SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Requested,
        changed: true,
        detail: format!(
            "wrote {request} request for active runtime owner {}",
            owner.owner_id
        ),
    })
}

pub fn runtime_stop_request_marker_path(data_dir: &Path) -> PathBuf {
    marker::runtime_stop_request_marker_path(data_dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeOwnershipState {
    Active,
    Stale,
}

fn classify_runtime_ownership(
    marker: &marker::RuntimeOwnershipMarker,
    now_ms: u64,
) -> RuntimeOwnershipState {
    if !marker.process_evidence.pid_alive
        || marker.process_evidence.process_started_after_marker
        || now_ms > marker.expires_at_ms
    {
        return RuntimeOwnershipState::Stale;
    }
    RuntimeOwnershipState::Active
}

fn append_runtime_control_request(
    data_dir: &Path,
    kind: SurfaceActionRequestKind,
    requested_at_ms: u64,
    request_id: &str,
    target_owner_id: &str,
) -> Result<u64, SurfaceActionError> {
    let mut events = DurableEventStore::open(runtime_durable_event_root(data_dir))
        .map_err(|error| SurfaceActionError::Durable(error.to_string()))?;
    let mut payload = serde_json::to_value(super::runtime_control_payload(requested_at_ms))?;
    payload["request_id"] = json!(request_id);
    payload["target_owner_id"] = json!(target_owner_id);
    let record = events
        .append(DurableEventInput::new(
            "runtime",
            kind.event_kind(),
            DurableEventPayload::inline("durable_work", payload),
        ))
        .map_err(|error| SurfaceActionError::Durable(error.to_string()))?;
    Ok(record.sequence)
}

fn runtime_durable_event_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("durable-events")
}
