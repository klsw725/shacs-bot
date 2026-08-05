use super::{classify_runtime_ownership, marker, RuntimeOwnershipMutationLock};
use super::{SurfaceActionError, SurfaceActionOutcome, SurfaceActionOutcomeKind};
use serde::{Deserialize, Serialize};
use shacs_bus::MessageBus;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::{
    DurableWorkPayloadStore, ReplayWorkState, WorkPayloadRef, MAX_PROJECTED_WORK_IDS,
};
use std::path::{Path, PathBuf};

/// Spec031 surface IPC transport for approval button decisions.
///
/// The durable work terminal records whether the current runtime owner applied or
/// rejected this transport request. Permission allow/deny truth remains in the
/// `AgentLoop` session-owner facts for the referenced approval lineage.
pub const SURFACE_APPROVAL_WORK_KIND: &str = "runtime.surface_approval";
pub const SURFACE_APPROVAL_PAYLOAD_TYPE: &str = "shacs.surface_approval_request.v1";

const SURFACE_APPROVAL_SCHEMA_VERSION: u32 = 1;
const SURFACE_APPROVAL_LEASE_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceApprovalDecision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceApprovalRequest {
    pub schema_version: u32,
    pub request_id: String,
    pub requested_at_ms: u64,
    pub session_key: String,
    pub lineage: String,
    pub decision: SurfaceApprovalDecision,
    /// Internal owner-generation fence for the transport worker.
    ///
    /// This is not a user-facing owner identity and must not be projected in UI,
    /// API, or evidence output as the permission approval owner.
    pub target_owner_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceApprovalAvailability {
    Actionable { target_owner_id: String },
    Unavailable { reason: String },
}

impl SurfaceApprovalRequest {
    pub fn parse(value: serde_json::Value) -> Result<Self, SurfaceActionError> {
        let request: Self = serde_json::from_value(value)?;
        if request.schema_version != SURFACE_APPROVAL_SCHEMA_VERSION
            || request.request_id.trim().is_empty()
            || request.session_key.trim().is_empty()
            || request.lineage.trim().is_empty()
            || request.target_owner_id.trim().is_empty()
        {
            return Err(SurfaceActionError::Durable(
                "surface approval request is malformed".to_owned(),
            ));
        }
        Ok(request)
    }

    pub const fn approve(&self) -> bool {
        match self.decision {
            SurfaceApprovalDecision::Approve => true,
            SurfaceApprovalDecision::Deny => false,
        }
    }
}

pub fn request_surface_approval(
    data_dir: &Path,
    session_key: &str,
    lineage: &str,
    approve: bool,
    now_ms: u64,
) -> Result<SurfaceActionOutcome, SurfaceActionError> {
    let ownership_path = marker::runtime_ownership_marker_path(data_dir);
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(&ownership_path)?;
    let Some(owner) = marker::read_runtime_ownership_marker(&ownership_path)? else {
        return Ok(unavailable("no active runtime owner found"));
    };
    if classify_runtime_ownership(&owner, now_ms) != super::RuntimeOwnershipState::Active {
        return Ok(unavailable(
            "stale ownership marker exists; run `shacs-bot runtime recover`",
        ));
    }
    let decision = if approve {
        SurfaceApprovalDecision::Approve
    } else {
        SurfaceApprovalDecision::Deny
    };
    if let Some(outcome) = existing_request_outcome(data_dir, session_key, lineage, decision)? {
        return Ok(outcome);
    }
    let request_id = format!(
        "surface-approval-{}-{now_ms}",
        owner.owner_id.replace(':', "-")
    );
    let request = SurfaceApprovalRequest {
        schema_version: SURFACE_APPROVAL_SCHEMA_VERSION,
        request_id: request_id.clone(),
        requested_at_ms: now_ms,
        session_key: session_key.to_owned(),
        lineage: lineage.to_owned(),
        decision,
        target_owner_id: owner.owner_id.clone(),
    };
    let payload_value = serde_json::to_value(&request)?;
    let payload_ref = DurableWorkPayloadStore::open(runtime_durable_work_payload_root(data_dir))?
        .write_json(SURFACE_APPROVAL_PAYLOAD_TYPE, &payload_value)?;
    let mut dispatcher = super::super::DurableWorkDispatcher::open(
        runtime_durable_event_root(data_dir),
        runtime_durable_work_payload_root(data_dir),
        MessageBus::new(),
        owner.owner_id.clone(),
        SURFACE_APPROVAL_LEASE_MS,
    )
    .map_err(|error| SurfaceActionError::Durable(error.to_string()))?;
    dispatcher
        .enqueue_work(super::super::DurableWorkEnqueueInput {
            work_id: request_id,
            work_kind: SURFACE_APPROVAL_WORK_KIND.to_owned(),
            session_key: session_key.to_owned(),
            turn_id: None,
            effect_id: Some(lineage.to_owned()),
            payload_ref,
            dedupe_hint: Some(dedupe_hint(session_key, lineage)),
            next_wake_at_ms: None,
        })
        .map_err(|error| SurfaceActionError::Durable(error.to_string()))?;
    Ok(SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Requested,
        changed: true,
        detail: "permission approval request queued for runtime owner".to_owned(),
    })
}

pub fn surface_approval_availability(
    data_dir: &Path,
    now_ms: u64,
) -> Result<SurfaceApprovalAvailability, SurfaceActionError> {
    let ownership_path = marker::runtime_ownership_marker_path(data_dir);
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(&ownership_path)?;
    let Some(owner) = marker::read_runtime_ownership_marker(&ownership_path)? else {
        return Ok(SurfaceApprovalAvailability::Unavailable {
            reason: "no active runtime owner found".to_owned(),
        });
    };
    if classify_runtime_ownership(&owner, now_ms) != super::RuntimeOwnershipState::Active {
        return Ok(SurfaceApprovalAvailability::Unavailable {
            reason: "stale ownership marker exists; run `shacs-bot runtime recover`".to_owned(),
        });
    }
    Ok(SurfaceApprovalAvailability::Actionable {
        target_owner_id: owner.owner_id,
    })
}

fn existing_request_outcome(
    data_dir: &Path,
    session_key: &str,
    lineage: &str,
    decision: SurfaceApprovalDecision,
) -> Result<Option<SurfaceActionOutcome>, SurfaceActionError> {
    let replay = evaluate_durable_recovery(
        runtime_durable_event_root(data_dir),
        runtime_durable_checkpoint_root(data_dir),
    );
    let Some(state) = replay.state else {
        return Ok(None);
    };
    let payloads = DurableWorkPayloadStore::new(runtime_durable_work_payload_root(data_dir));
    let hint = dedupe_hint(session_key, lineage);
    for item in state.work.items.values().take(MAX_PROJECTED_WORK_IDS) {
        if item.work_kind != SURFACE_APPROVAL_WORK_KIND
            || item.state == ReplayWorkState::Cancelled
            || item.state == ReplayWorkState::Terminal
            || item.dedupe_hint.as_deref() != Some(hint.as_str())
        {
            continue;
        }
        let request = parse_payload(&payloads, &item.payload_ref)?;
        if request.decision == decision {
            return Ok(Some(SurfaceActionOutcome {
                kind: SurfaceActionOutcomeKind::Requested,
                changed: false,
                detail: "matching permission approval request is already queued".to_owned(),
            }));
        }
        return Ok(Some(SurfaceActionOutcome {
            kind: SurfaceActionOutcomeKind::StaleLineage,
            changed: false,
            detail: "conflicting permission approval request is already queued".to_owned(),
        }));
    }
    Ok(None)
}

fn parse_payload(
    payloads: &DurableWorkPayloadStore,
    payload_ref: &WorkPayloadRef,
) -> Result<SurfaceApprovalRequest, SurfaceActionError> {
    SurfaceApprovalRequest::parse(payloads.read_json(payload_ref)?)
}

fn unavailable(detail: &str) -> SurfaceActionOutcome {
    SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Unavailable,
        changed: false,
        detail: detail.to_owned(),
    }
}

fn dedupe_hint(session_key: &str, lineage: &str) -> String {
    format!("surface_approval:{session_key}:{lineage}")
}

fn runtime_durable_event_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("durable-events")
}

fn runtime_durable_checkpoint_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("durable-checkpoints")
}

fn runtime_durable_work_payload_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("work-payloads")
}
