use crate::durable_event::{
    DurableEventAppendError, DurableEventError, DurableEventInput, DurableEventPayload,
    DurableEventRecord, DurableEventStore, RUNTIME_RESTART_REQUESTED, RUNTIME_STOP_REQUESTED,
    WORK_CANCELLED, WORK_CANCEL_REQUESTED, WORK_ENQUEUED, WORK_LEASED, WORK_REQUEUED,
    WORK_RETRY_SCHEDULED, WORK_TERMINAL,
};
use fs4::FileExt as Fs4FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_redaction::{redact_string, redact_value};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const CURRENT_DURABLE_WORK_SCHEMA_VERSION: u32 = 1;
pub const MAX_INLINE_WORK_PAYLOAD_BYTES: usize = 16 * 1024;
pub const MAX_WORK_PAYLOAD_BYTES: usize = 1024 * 1024;
pub const MAX_RETAINED_TERMINAL_WORK_ITEMS: usize = 512;
pub const MAX_RETAINED_RUNTIME_REQUESTS: usize = 32;
pub const MAX_PROJECTED_WORK_IDS: usize = 256;
pub const MAX_DURABLE_WORK_OPEN_ITEMS: usize = 1024;
pub const MAX_DURABLE_WORK_PAYLOAD_STORE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_DURABLE_WORK_EVENT_LOG_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_DURABLE_WORK_ATTEMPTS: u32 = 5;
const PAYLOAD_WRITE_LOCK: &str = ".payload-write.lock";
const WORK_ENQUEUE_LOCK: &str = "work-enqueue.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum WorkPayloadRef {
    Inline {
        payload_type: String,
        data: Value,
        sha256: String,
    },
    Artifact {
        payload_type: String,
        artifact_ref: String,
        sha256: String,
        byte_len: u64,
    },
}

impl WorkPayloadRef {
    pub fn inline(payload_type: impl Into<String>, data: Value) -> Result<Self, DurableWorkError> {
        let payload_type = payload_type.into();
        validate_identifier("payload_type", &payload_type)?;
        let data = redact_value(&data);
        let bytes = serde_json::to_vec(&data)?;
        if bytes.len() > MAX_INLINE_WORK_PAYLOAD_BYTES {
            return Err(DurableWorkError::Validation(format!(
                "inline work payload exceeds {MAX_INLINE_WORK_PAYLOAD_BYTES} bytes"
            )));
        }
        Ok(Self::Inline {
            payload_type,
            sha256: checksum(&bytes),
            data,
        })
    }

    pub fn payload_type(&self) -> &str {
        match self {
            Self::Inline { payload_type, .. } | Self::Artifact { payload_type, .. } => payload_type,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayWorkState {
    Pending,
    Leased,
    WaitingRetry,
    Cancelled,
    Terminal,
}

impl ReplayWorkState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Terminal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkTerminalKind {
    Succeeded,
    Failed,
    Exhausted,
    Blocked,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayWorkItem {
    pub work_id: String,
    pub work_kind: String,
    pub session_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<String>,
    pub payload_ref: WorkPayloadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_hint: Option<String>,
    pub state: ReplayWorkState,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wake_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_owner_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_requested_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_kind: Option<WorkTerminalKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_facts: Option<Value>,
    pub enqueued_sequence: u64,
    pub updated_sequence: u64,
    pub enqueued_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeControlRequestKind {
    Stop,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeControlRequest {
    pub kind: RuntimeControlRequestKind,
    pub sequence: u64,
    pub requested_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_owner_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableWorkReplayState {
    #[serde(default)]
    pub items: BTreeMap<String, ReplayWorkItem>,
    #[serde(default)]
    pub terminal_evicted_count: u64,
    #[serde(default)]
    pub runtime_requests: Vec<RuntimeControlRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkEnqueued {
    pub work_id: String,
    pub work_kind: String,
    pub payload_ref: WorkPayloadRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wake_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLeased {
    pub work_id: String,
    pub lease_id: String,
    pub lease_owner_ref: String,
    pub attempt: u32,
    pub leased_at_ms: u64,
    pub lease_expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRetryScheduled {
    pub work_id: String,
    pub attempt: u32,
    pub next_wake_at_ms: u64,
    pub backoff_ms: u64,
    pub reason_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRequeued {
    pub work_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCancellation {
    pub work_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTerminal {
    pub work_id: String,
    pub terminal_kind: WorkTerminalKind,
    pub outcome_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeControlRequested {
    pub requested_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_owner_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableWorkReducerError {
    MissingInlinePayload { sequence: u64 },
    InvalidPayload { sequence: u64, reason: String },
    DuplicateWork { sequence: u64, work_id: String },
    MissingWork { sequence: u64, work_id: String },
    InvalidTransition { sequence: u64, work_id: String },
}

impl fmt::Display for DurableWorkReducerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingInlinePayload { sequence } => {
                write!(formatter, "work event {sequence} has no inline payload")
            }
            Self::InvalidPayload { sequence, reason } => {
                write!(
                    formatter,
                    "work event {sequence} payload is invalid: {reason}"
                )
            }
            Self::DuplicateWork { sequence, work_id } => {
                write!(formatter, "work event {sequence} duplicates work {work_id}")
            }
            Self::MissingWork { sequence, work_id } => {
                write!(
                    formatter,
                    "work event {sequence} references missing work {work_id}"
                )
            }
            Self::InvalidTransition { sequence, work_id } => {
                write!(
                    formatter,
                    "work event {sequence} is invalid for work {work_id}"
                )
            }
        }
    }
}

impl Error for DurableWorkReducerError {}

pub fn apply_durable_work_event(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<bool, DurableWorkReducerError> {
    match event.kind.as_str() {
        WORK_ENQUEUED => apply_enqueued(state, event)?,
        WORK_LEASED => apply_leased(state, event)?,
        WORK_RETRY_SCHEDULED => apply_retry_scheduled(state, event)?,
        WORK_REQUEUED => apply_requeued(state, event)?,
        WORK_CANCEL_REQUESTED => apply_cancel_requested(state, event)?,
        WORK_CANCELLED => apply_cancelled(state, event)?,
        WORK_TERMINAL => apply_terminal(state, event)?,
        RUNTIME_STOP_REQUESTED => {
            apply_runtime_request(state, event, RuntimeControlRequestKind::Stop)?
        }
        RUNTIME_RESTART_REQUESTED => {
            apply_runtime_request(state, event, RuntimeControlRequestKind::Restart)?
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(crate) fn normalize_durable_work_append(
    state: &DurableWorkReplayState,
    event: &mut DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    if event.kind != WORK_TERMINAL {
        return Ok(());
    }
    let terminal = parse_payload::<WorkTerminal>(event)?;
    let item =
        state
            .items
            .get(&terminal.work_id)
            .ok_or_else(|| DurableWorkReducerError::MissingWork {
                sequence: event.sequence,
                work_id: terminal.work_id.clone(),
            })?;
    if terminal.terminal_kind != WorkTerminalKind::Succeeded
        || item.cancellation_requested_sequence.is_none()
        || item.state.is_terminal()
    {
        return Ok(());
    }
    event.kind = WORK_CANCELLED.to_owned();
    event.payload = DurableEventPayload::inline(
        "durable_work",
        serde_json::to_value(WorkCancellation {
            work_id: terminal.work_id,
            reason: "success_after_cancellation_request".to_owned(),
        })
        .map_err(|error| invalid_payload(event.sequence, error.to_string()))?,
    );
    Ok(())
}

pub(crate) fn durable_work_append_requires_normalization(
    event: &DurableEventRecord,
) -> Result<bool, DurableWorkReducerError> {
    if event.kind != WORK_TERMINAL {
        return Ok(false);
    }
    Ok(parse_payload::<WorkTerminal>(event)?.terminal_kind == WorkTerminalKind::Succeeded)
}

fn apply_enqueued(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkEnqueued>(event)?;
    validate_identifier_at(event.sequence, "work_id", &payload.work_id)?;
    validate_identifier_at(event.sequence, "work_kind", &payload.work_kind)?;
    validate_payload_ref(&payload.payload_ref)
        .map_err(|error| invalid_payload(event.sequence, error.to_string()))?;
    if state.items.contains_key(&payload.work_id) {
        return Err(DurableWorkReducerError::DuplicateWork {
            sequence: event.sequence,
            work_id: payload.work_id,
        });
    }
    let superseded_by = payload.dedupe_hint.as_ref().and_then(|hint| {
        state.items.values().find(|item| {
            item.session_key == event.session_id && item.dedupe_hint.as_ref() == Some(hint)
        })
    });
    let duplicate = superseded_by.is_some();
    let terminal_facts = superseded_by.map(|item| {
        serde_json::json!({
            "reason": "duplicate_dedupe_lineage",
            "supersedes_work_id": item.work_id,
            "supersedes_terminal_kind": item.terminal_kind,
            "supersedes_terminal_facts": item.terminal_facts,
        })
    });
    let (work_state, terminal_kind, terminal_sequence) = if duplicate {
        (
            ReplayWorkState::Terminal,
            Some(WorkTerminalKind::Superseded),
            Some(event.sequence),
        )
    } else {
        (ReplayWorkState::Pending, None, None)
    };
    state.items.insert(
        payload.work_id.clone(),
        ReplayWorkItem {
            work_id: payload.work_id,
            work_kind: payload.work_kind,
            session_key: event.session_id.clone(),
            turn_id: event.turn_id.clone(),
            effect_id: payload.effect_id,
            payload_ref: payload.payload_ref,
            dedupe_hint: payload.dedupe_hint,
            state: work_state,
            attempt: 0,
            next_wake_at_ms: payload.next_wake_at_ms,
            lease_id: None,
            lease_owner_ref: None,
            lease_expires_at_ms: None,
            cancellation_requested_sequence: None,
            terminal_kind,
            terminal_facts,
            enqueued_sequence: event.sequence,
            updated_sequence: event.sequence,
            enqueued_at: event.recorded_at.clone(),
            updated_at: event.recorded_at.clone(),
            terminal_sequence,
            terminal_at: duplicate.then(|| event.recorded_at.clone()),
        },
    );
    prune_terminal_items(state);
    Ok(())
}

fn apply_leased(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkLeased>(event)?;
    let item = open_item_mut(state, event.sequence, &payload.work_id)?;
    if !matches!(
        item.state,
        ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
    ) || item.cancellation_requested_sequence.is_some()
        || payload.attempt != item.attempt.saturating_add(1)
        || payload.lease_expires_at_ms <= payload.leased_at_ms
    {
        return Err(invalid_transition(event.sequence, payload.work_id));
    }
    item.state = ReplayWorkState::Leased;
    item.attempt = payload.attempt;
    item.next_wake_at_ms = None;
    item.lease_id = Some(payload.lease_id);
    item.lease_owner_ref = Some(payload.lease_owner_ref);
    item.lease_expires_at_ms = Some(payload.lease_expires_at_ms);
    item.updated_sequence = event.sequence;
    item.updated_at = event.recorded_at.clone();
    Ok(())
}

fn apply_retry_scheduled(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkRetryScheduled>(event)?;
    let item = open_item_mut(state, event.sequence, &payload.work_id)?;
    if item.state != ReplayWorkState::Leased
        || item.cancellation_requested_sequence.is_some()
        || payload.attempt != item.attempt
        || payload.next_wake_at_ms == 0
        || payload.backoff_ms == 0
    {
        return Err(invalid_transition(event.sequence, payload.work_id));
    }
    validate_identifier_at(event.sequence, "reason_ref", &payload.reason_ref)?;
    item.state = ReplayWorkState::WaitingRetry;
    item.next_wake_at_ms = Some(payload.next_wake_at_ms);
    clear_lease(item);
    item.updated_sequence = event.sequence;
    item.updated_at = event.recorded_at.clone();
    Ok(())
}

fn apply_requeued(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkRequeued>(event)?;
    validate_identifier_at(event.sequence, "reason", &payload.reason)?;
    let item = open_item_mut(state, event.sequence, &payload.work_id)?;
    if item.state != ReplayWorkState::Leased {
        return Err(invalid_transition(event.sequence, payload.work_id));
    }
    item.state = ReplayWorkState::Pending;
    item.next_wake_at_ms = None;
    clear_lease(item);
    item.updated_sequence = event.sequence;
    item.updated_at = event.recorded_at.clone();
    Ok(())
}

fn apply_cancel_requested(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkCancellation>(event)?;
    validate_identifier_at(event.sequence, "reason", &payload.reason)?;
    let item = item_mut(state, event.sequence, &payload.work_id)?;
    if item.cancellation_requested_sequence.is_some() {
        return Err(invalid_transition(event.sequence, payload.work_id));
    }
    item.cancellation_requested_sequence = Some(event.sequence);
    item.updated_sequence = event.sequence;
    item.updated_at = event.recorded_at.clone();
    Ok(())
}

fn apply_cancelled(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkCancellation>(event)?;
    validate_identifier_at(event.sequence, "reason", &payload.reason)?;
    let item = open_item_mut(state, event.sequence, &payload.work_id)?;
    if item.cancellation_requested_sequence.is_none() {
        return Err(invalid_transition(event.sequence, payload.work_id));
    }
    item.state = ReplayWorkState::Cancelled;
    item.terminal_sequence = Some(event.sequence);
    item.updated_sequence = event.sequence;
    item.updated_at = event.recorded_at.clone();
    item.terminal_at = Some(event.recorded_at.clone());
    item.next_wake_at_ms = None;
    clear_lease(item);
    prune_terminal_items(state);
    Ok(())
}

fn apply_terminal(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<WorkTerminal>(event)?;
    validate_identifier_at(event.sequence, "outcome_ref", &payload.outcome_ref)?;
    let item = open_item_mut(state, event.sequence, &payload.work_id)?;
    if payload.terminal_kind == WorkTerminalKind::Succeeded
        && item.cancellation_requested_sequence.is_some()
    {
        item.state = ReplayWorkState::Cancelled;
        item.terminal_sequence = Some(event.sequence);
        item.updated_sequence = event.sequence;
        item.updated_at = event.recorded_at.clone();
        item.terminal_at = Some(event.recorded_at.clone());
        item.next_wake_at_ms = None;
        clear_lease(item);
        prune_terminal_items(state);
        return Ok(());
    }
    if !matches!(
        payload.terminal_kind,
        WorkTerminalKind::Blocked | WorkTerminalKind::Superseded
    ) && item.state != ReplayWorkState::Leased
    {
        return Err(invalid_transition(event.sequence, payload.work_id));
    }
    item.state = ReplayWorkState::Terminal;
    item.terminal_kind = Some(payload.terminal_kind);
    item.terminal_facts = payload.facts;
    item.terminal_sequence = Some(event.sequence);
    item.updated_sequence = event.sequence;
    item.updated_at = event.recorded_at.clone();
    item.terminal_at = Some(event.recorded_at.clone());
    item.next_wake_at_ms = None;
    clear_lease(item);
    prune_terminal_items(state);
    Ok(())
}

fn apply_runtime_request(
    state: &mut DurableWorkReplayState,
    event: &DurableEventRecord,
    kind: RuntimeControlRequestKind,
) -> Result<(), DurableWorkReducerError> {
    let payload = parse_payload::<RuntimeControlRequested>(event)?;
    state.runtime_requests.push(RuntimeControlRequest {
        kind,
        sequence: event.sequence,
        requested_at_ms: payload.requested_at_ms,
        request_id: payload.request_id,
        target_owner_id: payload.target_owner_id,
    });
    if state.runtime_requests.len() > MAX_RETAINED_RUNTIME_REQUESTS {
        let remove = state.runtime_requests.len() - MAX_RETAINED_RUNTIME_REQUESTS;
        state.runtime_requests.drain(..remove);
    }
    Ok(())
}

fn parse_payload<T: for<'de> Deserialize<'de>>(
    event: &DurableEventRecord,
) -> Result<T, DurableWorkReducerError> {
    let DurableEventPayload::Inline { data, .. } = &event.payload else {
        return Err(DurableWorkReducerError::MissingInlinePayload {
            sequence: event.sequence,
        });
    };
    serde_json::from_value(data.clone()).map_err(|error| DurableWorkReducerError::InvalidPayload {
        sequence: event.sequence,
        reason: error.to_string(),
    })
}

fn item_mut<'a>(
    state: &'a mut DurableWorkReplayState,
    sequence: u64,
    work_id: &str,
) -> Result<&'a mut ReplayWorkItem, DurableWorkReducerError> {
    state
        .items
        .get_mut(work_id)
        .ok_or_else(|| DurableWorkReducerError::MissingWork {
            sequence,
            work_id: work_id.to_owned(),
        })
}

fn open_item_mut<'a>(
    state: &'a mut DurableWorkReplayState,
    sequence: u64,
    work_id: &str,
) -> Result<&'a mut ReplayWorkItem, DurableWorkReducerError> {
    let item = item_mut(state, sequence, work_id)?;
    if item.state.is_terminal() {
        return Err(invalid_transition(sequence, work_id.to_owned()));
    }
    Ok(item)
}

fn clear_lease(item: &mut ReplayWorkItem) {
    item.lease_id = None;
    item.lease_owner_ref = None;
    item.lease_expires_at_ms = None;
}

fn prune_terminal_items(state: &mut DurableWorkReplayState) {
    let mut terminal = state
        .items
        .values()
        .filter_map(|item| {
            item.terminal_sequence
                .map(|sequence| (sequence, item.work_id.clone()))
        })
        .collect::<Vec<_>>();
    if terminal.len() <= MAX_RETAINED_TERMINAL_WORK_ITEMS {
        return;
    }
    terminal.sort();
    let remove = terminal.len() - MAX_RETAINED_TERMINAL_WORK_ITEMS;
    for (_, work_id) in terminal.into_iter().take(remove) {
        state.items.remove(&work_id);
        state.terminal_evicted_count = state.terminal_evicted_count.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableWorkRecoveryStatus {
    Healthy,
    Recoverable,
    Blocked,
}

impl DurableWorkRecoveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Recoverable => "recoverable",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableWorkRecoveryIssueKind {
    ReplayUnavailable,
    MissingPayload,
    CorruptPayload,
    StaleLease,
}

impl DurableWorkRecoveryIssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReplayUnavailable => "replay_unavailable",
            Self::MissingPayload => "missing_payload",
            Self::CorruptPayload => "corrupt_payload",
            Self::StaleLease => "stale_lease",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableWorkRecoveryIssue {
    pub kind: DurableWorkRecoveryIssueKind,
    pub work_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableWorkAdmission {
    pub status: DurableWorkRecoveryStatus,
    pub writable: bool,
    pub pending_count: usize,
    pub leased_count: usize,
    pub waiting_retry_count: usize,
    pub cancellation_requested_count: usize,
    pub terminal_count: usize,
    pub terminal_evicted_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_wake_at_ms: Option<u64>,
    #[serde(default)]
    pub due_work_ids: Vec<String>,
    #[serde(default)]
    pub stale_lease_work_ids: Vec<String>,
    #[serde(default)]
    pub issues: Vec<DurableWorkRecoveryIssue>,
}

impl DurableWorkAdmission {
    pub fn blocked_by_replay(detail: impl Into<String>) -> Self {
        Self {
            status: DurableWorkRecoveryStatus::Blocked,
            writable: false,
            pending_count: 0,
            leased_count: 0,
            waiting_retry_count: 0,
            cancellation_requested_count: 0,
            terminal_count: 0,
            terminal_evicted_count: 0,
            next_wake_at_ms: None,
            due_work_ids: Vec::new(),
            stale_lease_work_ids: Vec::new(),
            issues: vec![DurableWorkRecoveryIssue {
                kind: DurableWorkRecoveryIssueKind::ReplayUnavailable,
                work_id: "runtime".to_owned(),
                detail: detail.into(),
            }],
        }
    }
}

pub fn evaluate_durable_work_recovery(
    state: &DurableWorkReplayState,
    payload_root: impl AsRef<Path>,
    now_ms: u64,
) -> DurableWorkAdmission {
    evaluate_durable_work_recovery_for_owner(state, payload_root, now_ms, None)
}

pub fn evaluate_durable_work_recovery_for_owner(
    state: &DurableWorkReplayState,
    payload_root: impl AsRef<Path>,
    now_ms: u64,
    active_lease_owner_ref: Option<&str>,
) -> DurableWorkAdmission {
    let payload_store = DurableWorkPayloadStore::new(payload_root);
    let mut pending_count = 0;
    let mut leased_count = 0;
    let mut waiting_retry_count = 0;
    let mut cancellation_requested_count = 0;
    let mut terminal_count = 0;
    let mut next_wake_at_ms = None;
    let mut due_work_ids = Vec::new();
    let mut stale_lease_work_ids = Vec::new();
    let mut issues = Vec::new();
    for item in state.items.values() {
        match item.state {
            ReplayWorkState::Pending => pending_count += 1,
            ReplayWorkState::Leased => leased_count += 1,
            ReplayWorkState::WaitingRetry => waiting_retry_count += 1,
            ReplayWorkState::Cancelled | ReplayWorkState::Terminal => terminal_count += 1,
        }
        if item.cancellation_requested_sequence.is_some() && !item.state.is_terminal() {
            cancellation_requested_count += 1;
        }
        if !item.state.is_terminal() {
            if let Err(error) = payload_store.verify(&item.payload_ref) {
                issues.push(DurableWorkRecoveryIssue {
                    kind: match error {
                        DurableWorkError::Io(ref io)
                            if io.kind() == std::io::ErrorKind::NotFound =>
                        {
                            DurableWorkRecoveryIssueKind::MissingPayload
                        }
                        _ => DurableWorkRecoveryIssueKind::CorruptPayload,
                    },
                    work_id: item.work_id.clone(),
                    detail: error.to_string(),
                });
            }
        }
        match item.state {
            ReplayWorkState::Pending if item.cancellation_requested_sequence.is_none() => {
                if item.next_wake_at_ms.map_or(true, |wake| wake <= now_ms)
                    && due_work_ids.len() < MAX_PROJECTED_WORK_IDS
                {
                    due_work_ids.push(item.work_id.clone());
                }
                next_wake_at_ms = min_option(next_wake_at_ms, item.next_wake_at_ms);
            }
            ReplayWorkState::WaitingRetry if item.cancellation_requested_sequence.is_none() => {
                if item.next_wake_at_ms.is_some_and(|wake| wake <= now_ms)
                    && due_work_ids.len() < MAX_PROJECTED_WORK_IDS
                {
                    due_work_ids.push(item.work_id.clone());
                }
                next_wake_at_ms = min_option(next_wake_at_ms, item.next_wake_at_ms);
            }
            ReplayWorkState::Leased => {
                if item
                    .lease_expires_at_ms
                    .is_some_and(|expiry| expiry <= now_ms)
                    && !active_lease_owner_ref
                        .is_some_and(|owner| item.lease_owner_ref.as_deref() == Some(owner))
                {
                    if stale_lease_work_ids.len() < MAX_PROJECTED_WORK_IDS {
                        stale_lease_work_ids.push(item.work_id.clone());
                    }
                    issues.push(DurableWorkRecoveryIssue {
                        kind: DurableWorkRecoveryIssueKind::StaleLease,
                        work_id: item.work_id.clone(),
                        detail: "work lease expired without a terminal outcome".to_owned(),
                    });
                } else {
                    next_wake_at_ms = min_option(next_wake_at_ms, item.lease_expires_at_ms);
                }
            }
            _ => {}
        }
    }
    let blocked = issues.iter().any(|issue| {
        matches!(
            issue.kind,
            DurableWorkRecoveryIssueKind::MissingPayload
                | DurableWorkRecoveryIssueKind::CorruptPayload
        )
    });
    let recoverable = !blocked && !stale_lease_work_ids.is_empty();
    DurableWorkAdmission {
        status: if blocked {
            DurableWorkRecoveryStatus::Blocked
        } else if recoverable {
            DurableWorkRecoveryStatus::Recoverable
        } else {
            DurableWorkRecoveryStatus::Healthy
        },
        writable: !blocked && !recoverable,
        pending_count,
        leased_count,
        waiting_retry_count,
        cancellation_requested_count,
        terminal_count,
        terminal_evicted_count: state.terminal_evicted_count,
        next_wake_at_ms,
        due_work_ids,
        stale_lease_work_ids,
        issues,
    }
}

#[derive(Debug)]
pub enum DurableWorkError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    Validation(String),
}

impl fmt::Display for DurableWorkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "durable work payload I/O failed: {error}"),
            Self::Serialization(error) => {
                write!(
                    formatter,
                    "durable work payload serialization failed: {error}"
                )
            }
            Self::Validation(reason) => {
                write!(
                    formatter,
                    "durable work payload validation failed: {reason}"
                )
            }
        }
    }
}

impl Error for DurableWorkError {}

impl From<std::io::Error> for DurableWorkError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DurableWorkError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[derive(Debug, Clone)]
pub struct DurableWorkEnqueueInput {
    pub work_id: String,
    pub work_kind: String,
    pub session_key: String,
    pub turn_id: Option<String>,
    pub effect_id: Option<String>,
    pub payload_ref: WorkPayloadRef,
    pub dedupe_hint: Option<String>,
    pub next_wake_at_ms: Option<u64>,
}

pub struct DurableWorkEnqueuer {
    events: DurableEventStore,
    enqueue_lock_path: PathBuf,
}

pub struct DurableWorkMutationGuard {
    _file: File,
}

impl DurableWorkEnqueuer {
    pub fn open(event_root: impl AsRef<Path>) -> Result<Self, DurableEventError> {
        let event_root = event_root.as_ref().to_path_buf();
        Ok(Self {
            enqueue_lock_path: event_root.join(WORK_ENQUEUE_LOCK),
            events: DurableEventStore::open(event_root)?,
        })
    }

    pub fn enqueue(
        &mut self,
        input: DurableWorkEnqueueInput,
    ) -> Result<DurableEventRecord, DurableWorkEnqueueError> {
        let _lock = acquire_work_enqueue_lock(&self.enqueue_lock_path)?;
        ensure_event_log_quota(&self.events)?;
        let state = replay_work_state(&self.events)?;
        if let Some(existing) = state.items.get(&input.work_id) {
            if existing.work_kind != input.work_kind
                || existing.session_key != input.session_key
                || existing.turn_id != input.turn_id
                || existing.effect_id != input.effect_id
                || existing.dedupe_hint != input.dedupe_hint
                || existing.payload_ref != input.payload_ref
            {
                return Err(DurableWorkEnqueueError::IdentityConflict(input.work_id));
            }
            return enqueued_record(&self.events, &input.work_id)?
                .ok_or(DurableWorkEnqueueError::MissingWork(input.work_id));
        }
        ensure_enqueue_quotas(&self.events, &state)?;
        let payload = WorkEnqueued {
            work_id: input.work_id,
            work_kind: input.work_kind,
            payload_ref: input.payload_ref,
            dedupe_hint: input.dedupe_hint,
            next_wake_at_ms: input.next_wake_at_ms,
            effect_id: input.effect_id,
        };
        let data = serde_json::to_value(&payload)?;
        let mut event = DurableEventInput::new(
            input.session_key,
            WORK_ENQUEUED,
            DurableEventPayload::inline("durable_work", data),
        );
        event.turn_id = input.turn_id;
        event.causation_id = payload.effect_id.clone();
        Ok(self.events.append(event)?)
    }

    pub fn acquire_mutation_lock(&self) -> Result<DurableWorkMutationGuard, DurableWorkError> {
        Ok(DurableWorkMutationGuard {
            _file: acquire_work_enqueue_lock(&self.enqueue_lock_path)?,
        })
    }

    pub fn enqueue_json(
        &mut self,
        payloads: &DurableWorkPayloadStore,
        payload_type: &str,
        data: &Value,
        input: DurableWorkEnqueueJsonInput,
    ) -> Result<DurableEventRecord, DurableWorkEnqueueError> {
        self.enqueue_json_with_writers(
            payloads,
            payload_type,
            data,
            input,
            write_content_addressed,
            |events, event| events.append_classified(event),
        )
    }

    fn enqueue_json_with_writers(
        &mut self,
        payloads: &DurableWorkPayloadStore,
        payload_type: &str,
        data: &Value,
        input: DurableWorkEnqueueJsonInput,
        write_payload: impl FnOnce(&Path, &[u8]) -> Result<(), DurableWorkError>,
        append: impl FnOnce(
            &mut DurableEventStore,
            DurableEventInput,
        ) -> Result<DurableEventRecord, DurableEventAppendError>,
    ) -> Result<DurableEventRecord, DurableWorkEnqueueError> {
        let _lock = acquire_work_enqueue_lock(&self.enqueue_lock_path)?;
        ensure_event_log_quota(&self.events)?;
        let state = replay_work_state(&self.events)?;
        let payload_ref = payloads.payload_ref(payload_type, data)?;
        let payload = WorkEnqueued {
            work_id: input.work_id,
            work_kind: input.work_kind,
            payload_ref,
            dedupe_hint: input.dedupe_hint,
            next_wake_at_ms: input.next_wake_at_ms,
            effect_id: input.effect_id,
        };
        if let Some(existing) = state.items.get(&payload.work_id) {
            if existing.work_kind != payload.work_kind
                || existing.session_key != input.session_key
                || existing.turn_id != input.turn_id
                || existing.effect_id != payload.effect_id
                || existing.dedupe_hint != payload.dedupe_hint
                || existing.payload_ref != payload.payload_ref
            {
                return Err(DurableWorkEnqueueError::IdentityConflict(payload.work_id));
            }
            return enqueued_record(&self.events, &payload.work_id)?
                .ok_or(DurableWorkEnqueueError::MissingWork(payload.work_id));
        }
        ensure_enqueue_quotas(&self.events, &state)?;
        let payload_write = payloads.write_json_guarded(payload_type, data, write_payload)?;
        let data = serde_json::to_value(&payload)?;
        let mut event = DurableEventInput::new(
            input.session_key,
            WORK_ENQUEUED,
            DurableEventPayload::inline("durable_work", data),
        );
        event.turn_id = input.turn_id;
        event.causation_id = payload.effect_id.clone();
        match append(&mut self.events, event) {
            Ok(record) => Ok(record),
            Err(DurableEventAppendError::DefinitelyNotCommitted(error)) => {
                payload_write.rollback()?;
                Err(error.into())
            }
            Err(DurableEventAppendError::CommitUnknown(error)) => {
                Err(DurableWorkEnqueueError::CommitUnknown(error))
            }
        }
    }
}

pub struct DurableWorkEnqueueJsonInput {
    pub work_id: String,
    pub work_kind: String,
    pub session_key: String,
    pub turn_id: Option<String>,
    pub effect_id: Option<String>,
    pub dedupe_hint: Option<String>,
    pub next_wake_at_ms: Option<u64>,
}

#[derive(Debug)]
pub enum DurableWorkEnqueueError {
    Event(DurableEventError),
    CommitUnknown(DurableEventError),
    Work(DurableWorkError),
    IdentityConflict(String),
    MissingWork(String),
}

impl fmt::Display for DurableWorkEnqueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event(error) => error.fmt(formatter),
            Self::CommitUnknown(error) => {
                write!(formatter, "durable work enqueue commit is unknown: {error}")
            }
            Self::Work(error) => error.fmt(formatter),
            Self::IdentityConflict(work_id) => {
                write!(
                    formatter,
                    "durable work {work_id} already exists with different identity"
                )
            }
            Self::MissingWork(work_id) => write!(formatter, "durable work {work_id} is missing"),
        }
    }
}

impl Error for DurableWorkEnqueueError {}

impl From<DurableEventError> for DurableWorkEnqueueError {
    fn from(error: DurableEventError) -> Self {
        Self::Event(error)
    }
}

impl From<DurableWorkError> for DurableWorkEnqueueError {
    fn from(error: DurableWorkError) -> Self {
        Self::Work(error)
    }
}

impl From<serde_json::Error> for DurableWorkEnqueueError {
    fn from(error: serde_json::Error) -> Self {
        Self::Work(DurableWorkError::Serialization(error))
    }
}

#[derive(Debug, Clone)]
pub struct DurableWorkPayloadStore {
    root: PathBuf,
}

impl DurableWorkPayloadStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, DurableWorkError> {
        let root = root.as_ref();
        reject_symlink(root)?;
        fs::create_dir_all(root)?;
        reject_symlink(root)?;
        Ok(Self {
            root: fs::canonicalize(root)?,
        })
    }

    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn write_json(
        &self,
        payload_type: impl Into<String>,
        data: &Value,
    ) -> Result<WorkPayloadRef, DurableWorkError> {
        let payload_type = payload_type.into();
        validate_identifier("payload_type", &payload_type)?;
        reject_symlink(&self.root)?;
        fs::create_dir_all(&self.root)?;
        reject_symlink(&self.root)?;
        let data = redact_value(data);
        let bytes = serde_json::to_vec(&data)?;
        if bytes.len() > MAX_WORK_PAYLOAD_BYTES {
            return Err(DurableWorkError::Validation(format!(
                "work payload exceeds {MAX_WORK_PAYLOAD_BYTES} bytes"
            )));
        }
        let sha256 = checksum(&bytes);
        let file_name = format!("{}.json", sha256.trim_start_matches("sha256:"));
        let path = self.root.join(&file_name);
        let _lock = acquire_payload_write_lock(&self.root)?;
        if !path.exists()
            && self
                .stored_artifact_bytes()?
                .saturating_add(bytes.len() as u64)
                > MAX_DURABLE_WORK_PAYLOAD_STORE_BYTES
        {
            return Err(DurableWorkError::Validation(format!(
                "durable work payload store exceeds {MAX_DURABLE_WORK_PAYLOAD_STORE_BYTES} bytes"
            )));
        }
        write_content_addressed(&path, &bytes)?;
        Ok(WorkPayloadRef::Artifact {
            payload_type,
            artifact_ref: file_name,
            sha256,
            byte_len: bytes.len() as u64,
        })
    }

    fn write_json_guarded(
        &self,
        payload_type: &str,
        data: &Value,
        write_payload: impl FnOnce(&Path, &[u8]) -> Result<(), DurableWorkError>,
    ) -> Result<PayloadWriteGuard, DurableWorkError> {
        reject_symlink(&self.root)?;
        let payload_ref = self.payload_ref(payload_type, data)?;
        let WorkPayloadRef::Artifact {
            artifact_ref,
            byte_len,
            ..
        } = &payload_ref
        else {
            return Err(DurableWorkError::Validation(
                "guarded work payload must be an artifact".to_owned(),
            ));
        };
        let bytes = serde_json::to_vec(&redact_value(data))?;
        let path = self.root.join(artifact_ref);
        let lock = acquire_payload_write_lock(&self.root)?;
        let created = !path.exists();
        if created
            && self.stored_artifact_bytes()?.saturating_add(*byte_len)
                > MAX_DURABLE_WORK_PAYLOAD_STORE_BYTES
        {
            return Err(DurableWorkError::Validation(format!(
                "durable work payload store exceeds {MAX_DURABLE_WORK_PAYLOAD_STORE_BYTES} bytes"
            )));
        }
        write_payload(&path, &bytes)?;
        Ok(PayloadWriteGuard {
            created_path: created.then_some(path),
            _lock: lock,
        })
    }

    fn payload_ref(
        &self,
        payload_type: &str,
        data: &Value,
    ) -> Result<WorkPayloadRef, DurableWorkError> {
        validate_identifier("payload_type", payload_type)?;
        let bytes = serde_json::to_vec(&redact_value(data))?;
        if bytes.len() > MAX_WORK_PAYLOAD_BYTES {
            return Err(DurableWorkError::Validation(format!(
                "work payload exceeds {MAX_WORK_PAYLOAD_BYTES} bytes"
            )));
        }
        let sha256 = checksum(&bytes);
        Ok(WorkPayloadRef::Artifact {
            payload_type: payload_type.to_owned(),
            artifact_ref: format!("{}.json", sha256.trim_start_matches("sha256:")),
            sha256,
            byte_len: bytes.len() as u64,
        })
    }

    pub fn read_json(&self, payload_ref: &WorkPayloadRef) -> Result<Value, DurableWorkError> {
        self.verify(payload_ref)?;
        match payload_ref {
            WorkPayloadRef::Inline { data, .. } => Ok(data.clone()),
            WorkPayloadRef::Artifact { artifact_ref, .. } => {
                let bytes = read_bounded(&self.root.join(artifact_ref))?;
                Ok(serde_json::from_slice(&bytes)?)
            }
        }
    }

    pub fn verify(&self, payload_ref: &WorkPayloadRef) -> Result<(), DurableWorkError> {
        reject_symlink(&self.root)?;
        validate_payload_ref(payload_ref)?;
        let (bytes, expected) = match payload_ref {
            WorkPayloadRef::Inline { data, sha256, .. } => {
                (serde_json::to_vec(data)?, sha256.as_str())
            }
            WorkPayloadRef::Artifact {
                artifact_ref,
                sha256,
                byte_len,
                ..
            } => {
                let bytes = read_bounded(&self.root.join(artifact_ref))?;
                if bytes.len() as u64 != *byte_len {
                    return Err(DurableWorkError::Validation(
                        "work payload byte length mismatch".to_owned(),
                    ));
                }
                (bytes, sha256.as_str())
            }
        };
        if checksum(&bytes) != expected {
            return Err(DurableWorkError::Validation(
                "work payload checksum mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn stored_artifact_bytes(&self) -> Result<u64, DurableWorkError> {
        reject_symlink(&self.root)?;
        let mut total = 0u64;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_file() && entry.file_name() != PAYLOAD_WRITE_LOCK {
                total = total.saturating_add(metadata.len());
            }
        }
        Ok(total)
    }
}

struct PayloadWriteGuard {
    created_path: Option<PathBuf>,
    _lock: File,
}

impl PayloadWriteGuard {
    fn rollback(self) -> Result<(), DurableWorkError> {
        if let Some(path) = self.created_path {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

fn validate_payload_ref(payload_ref: &WorkPayloadRef) -> Result<(), DurableWorkError> {
    validate_identifier("payload_type", payload_ref.payload_type())?;
    match payload_ref {
        WorkPayloadRef::Inline { data, sha256, .. } => {
            validate_digest(sha256)?;
            if serde_json::to_vec(data)?.len() > MAX_INLINE_WORK_PAYLOAD_BYTES {
                return Err(DurableWorkError::Validation(
                    "inline work payload exceeds maximum size".to_owned(),
                ));
            }
        }
        WorkPayloadRef::Artifact {
            artifact_ref,
            sha256,
            byte_len,
            ..
        } => {
            validate_digest(sha256)?;
            if *byte_len as usize > MAX_WORK_PAYLOAD_BYTES || *byte_len == 0 {
                return Err(DurableWorkError::Validation(
                    "work payload byte length is invalid".to_owned(),
                ));
            }
            let path = Path::new(artifact_ref);
            let expected_file = format!(
                "{}.json",
                sha256.strip_prefix("sha256:").unwrap_or_default()
            );
            if path.is_absolute()
                || path.components().count() != 1
                || artifact_ref.contains('/')
                || artifact_ref.contains('\\')
                || !artifact_ref.ends_with(".json")
                || *artifact_ref != expected_file
                || redact_string(artifact_ref) != *artifact_ref
            {
                return Err(DurableWorkError::Validation(
                    "work payload artifact reference is invalid".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_digest(value: &str) -> Result<(), DurableWorkError> {
    let hex = value.strip_prefix("sha256:").ok_or_else(|| {
        DurableWorkError::Validation("work payload digest must use sha256".to_owned())
    })?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DurableWorkError::Validation(
            "work payload digest is malformed".to_owned(),
        ));
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<(), DurableWorkError> {
    if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(DurableWorkError::Validation(format!(
            "{name} is empty, oversized, or contains control characters"
        )));
    }
    Ok(())
}

fn validate_identifier_at(
    sequence: u64,
    name: &str,
    value: &str,
) -> Result<(), DurableWorkReducerError> {
    validate_identifier(name, value).map_err(|error| invalid_payload(sequence, error.to_string()))
}

fn invalid_payload(sequence: u64, reason: String) -> DurableWorkReducerError {
    DurableWorkReducerError::InvalidPayload { sequence, reason }
}

fn invalid_transition(sequence: u64, work_id: String) -> DurableWorkReducerError {
    DurableWorkReducerError::InvalidTransition { sequence, work_id }
}

fn min_option(current: Option<u64>, candidate: Option<u64>) -> Option<u64> {
    match (current, candidate) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (None, value) | (value, None) => value,
    }
}

fn checksum(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, DurableWorkError> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    let mut file = open_regular_file(path, options)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_WORK_PAYLOAD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_WORK_PAYLOAD_BYTES {
        return Err(DurableWorkError::Validation(
            "work payload exceeds maximum size".to_owned(),
        ));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadSyncTarget {
    File,
    ParentDirectory,
}

fn write_content_addressed(path: &Path, bytes: &[u8]) -> Result<(), DurableWorkError> {
    write_content_addressed_with_sync(path, bytes, |_target, file| file.sync_all())
}

fn write_content_addressed_with_sync(
    path: &Path,
    bytes: &[u8],
    mut sync: impl FnMut(PayloadSyncTarget, &File) -> std::io::Result<()>,
) -> Result<(), DurableWorkError> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let file = match open_regular_file(path, options) {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.flush()?;
            file
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = read_bounded(path)?;
            if existing != bytes {
                return Err(DurableWorkError::Validation(
                    "content-addressed work payload collision".to_owned(),
                ));
            }
            let mut options = OpenOptions::new();
            options.read(true);
            open_regular_file(path, options)?
        }
        Err(error) => return Err(error.into()),
    };
    sync(PayloadSyncTarget::File, &file)?;
    if let Some(parent) = path.parent() {
        let parent = OpenOptions::new().read(true).open(parent)?;
        sync(PayloadSyncTarget::ParentDirectory, &parent)?;
    }
    Ok(())
}

fn replay_work_state(
    events: &DurableEventStore,
) -> Result<DurableWorkReplayState, DurableWorkEnqueueError> {
    let mut state = DurableWorkReplayState::default();
    let mut reducer_error = None;
    events.visit_from_sequence(0, |event| {
        if reducer_error.is_none() {
            reducer_error = apply_durable_work_event(&mut state, event).err();
        }
    })?;
    if let Some(error) = reducer_error {
        return Err(DurableWorkEnqueueError::Work(DurableWorkError::Validation(
            error.to_string(),
        )));
    }
    Ok(state)
}

fn ensure_enqueue_quotas(
    events: &DurableEventStore,
    state: &DurableWorkReplayState,
) -> Result<(), DurableWorkError> {
    ensure_event_log_quota(events)?;
    let open_work_count = state
        .items
        .values()
        .filter(|item| !item.state.is_terminal())
        .count();
    if open_work_count >= MAX_DURABLE_WORK_OPEN_ITEMS {
        return Err(DurableWorkError::Validation(format!(
            "open durable work limit {MAX_DURABLE_WORK_OPEN_ITEMS} reached"
        )));
    }
    Ok(())
}

fn ensure_event_log_quota(events: &DurableEventStore) -> Result<(), DurableWorkError> {
    if events.path().metadata()?.len() >= MAX_DURABLE_WORK_EVENT_LOG_BYTES {
        return Err(DurableWorkError::Validation(format!(
            "durable event log exceeds {MAX_DURABLE_WORK_EVENT_LOG_BYTES} bytes"
        )));
    }
    Ok(())
}

fn enqueued_record(
    events: &DurableEventStore,
    work_id: &str,
) -> Result<Option<DurableEventRecord>, DurableEventError> {
    let scan = events.scan(usize::MAX)?;
    Ok(scan.records.into_iter().find(|record| {
        record.kind == WORK_ENQUEUED
            && match &record.payload {
                DurableEventPayload::Inline { data, .. } => {
                    data.get("work_id").and_then(Value::as_str) == Some(work_id)
                }
                DurableEventPayload::Artifact { .. } => false,
            }
    }))
}

fn acquire_work_enqueue_lock(path: &Path) -> Result<File, DurableWorkError> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    let file = open_regular_file(path, options)?;
    Fs4FileExt::lock(&file)?;
    Ok(file)
}

fn acquire_payload_write_lock(root: &Path) -> Result<File, DurableWorkError> {
    let path = root.join(PAYLOAD_WRITE_LOCK);
    reject_symlink(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    let file = open_regular_file(&path, options)?;
    Fs4FileExt::lock(&file)?;
    Ok(file)
}

fn open_regular_file(path: &Path, mut options: OpenOptions) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "work payload path is not a regular file",
        ));
    }
    Ok(file)
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "durable work payload path is a symlink",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durable_event::{DurableEventFrameDurable, WORK_ENQUEUED};
    use serde_json::json;
    use std::cell::{Cell, RefCell};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum DurabilityStep {
        PayloadFile,
        PayloadParentDirectory,
        Event,
    }

    fn enqueue_input(work_id: &str) -> DurableWorkEnqueueJsonInput {
        DurableWorkEnqueueJsonInput {
            work_id: work_id.to_owned(),
            work_kind: "test.work".to_owned(),
            session_key: "session-1".to_owned(),
            turn_id: None,
            effect_id: None,
            dedupe_hint: None,
            next_wake_at_ms: None,
        }
    }

    #[test]
    fn enqueue_json_rolls_back_new_payload_when_frame_is_unwritten() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let payloads = DurableWorkPayloadStore::open(root.path().join("payloads"))?;
        let mut enqueuer = DurableWorkEnqueuer::open(&event_root)?;
        let payload = json!({"message": "unwritten"});
        let payload_ref = payloads.payload_ref("test.payload", &payload)?;

        let error = enqueuer
            .enqueue_json_with_writers(
                &payloads,
                "test.payload",
                &payload,
                enqueue_input("work-unwritten"),
                write_content_addressed,
                |events, event| {
                    events.append_with_writer(event, |_path, _bytes| {
                        Err(std::io::Error::other("injected before write"))
                    })
                },
            )
            .err()
            .ok_or("expected unwritten append failure")?;

        assert!(matches!(error, DurableWorkEnqueueError::Event(_)));
        assert!(payloads.verify(&payload_ref).is_err());
        let events = DurableEventStore::open(&event_root)?;
        assert!(events.scan(1)?.records.is_empty());
        assert!(events.is_writable());
        Ok(())
    }

    #[test]
    fn enqueue_json_rolls_back_new_payload_when_frame_is_partial() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let payloads = DurableWorkPayloadStore::open(root.path().join("payloads"))?;
        let mut enqueuer = DurableWorkEnqueuer::open(&event_root)?;
        let payload = json!({"message": "partial"});
        let payload_ref = payloads.payload_ref("test.payload", &payload)?;

        let error = enqueuer
            .enqueue_json_with_writers(
                &payloads,
                "test.payload",
                &payload,
                enqueue_input("work-partial"),
                write_content_addressed,
                |events, event| {
                    events.append_with_writer(event, |path, bytes| {
                        let mut file = OpenOptions::new().append(true).open(path)?;
                        file.write_all(&bytes[..bytes.len() / 2])?;
                        file.sync_all()?;
                        Err(std::io::Error::other("injected partial write"))
                    })
                },
            )
            .err()
            .ok_or("expected partial append failure")?;

        assert!(matches!(error, DurableWorkEnqueueError::Event(_)));
        assert!(payloads.verify(&payload_ref).is_err());
        let events = DurableEventStore::open(&event_root)?;
        let scan = events.scan(1)?;
        assert!(scan.records.is_empty());
        assert!(scan.incomplete_tail);
        assert!(!events.is_writable());
        Ok(())
    }

    #[test]
    fn enqueue_json_keeps_payload_when_event_frame_is_durable() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let payloads = DurableWorkPayloadStore::open(root.path().join("payloads"))?;
        let mut enqueuer = DurableWorkEnqueuer::open(&event_root)?;
        let payload = json!({"message": "committed"});

        let record = enqueuer.enqueue_json_with_writers(
            &payloads,
            "test.payload",
            &payload,
            enqueue_input("work-1"),
            write_content_addressed,
            |events, event| {
                events.append_with_writer(event, |path, bytes| {
                    let mut file = OpenOptions::new().append(true).open(path)?;
                    file.write_all(bytes)?;
                    file.flush()?;
                    file.sync_all()?;
                    Ok(DurableEventFrameDurable)
                })
            },
        )?;

        assert_eq!(record.kind, WORK_ENQUEUED);
        let enqueued: WorkEnqueued = match record.payload {
            DurableEventPayload::Inline { data, .. } => serde_json::from_value(data)?,
            DurableEventPayload::Artifact { .. } => return Err("expected inline event".into()),
        };
        assert_eq!(payloads.read_json(&enqueued.payload_ref)?, payload);
        Ok(())
    }

    #[test]
    fn enqueue_json_preserves_payload_when_event_sync_is_unknown() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let payloads = DurableWorkPayloadStore::open(root.path().join("payloads"))?;
        let mut enqueuer = DurableWorkEnqueuer::open(&event_root)?;
        let payload = json!({"message": "sync-unknown"});
        let payload_ref = payloads.payload_ref("test.payload", &payload)?;

        let error = enqueuer
            .enqueue_json_with_writers(
                &payloads,
                "test.payload",
                &payload,
                enqueue_input("work-sync-unknown"),
                write_content_addressed,
                |events, event| {
                    events.append_with_writer(event, |path, bytes| {
                        let mut file = OpenOptions::new().append(true).open(path)?;
                        file.write_all(bytes)?;
                        file.flush()?;
                        Err(std::io::Error::other("injected event sync failure"))
                    })
                },
            )
            .err()
            .ok_or("expected event commit to remain unknown")?;

        assert!(matches!(error, DurableWorkEnqueueError::CommitUnknown(_)));
        assert_eq!(payloads.read_json(&payload_ref)?, payload);
        let mut reopened = DurableWorkEnqueuer::open(&event_root)?;
        let recovered = reopened.enqueue_json(
            &payloads,
            "test.payload",
            &payload,
            enqueue_input("work-sync-unknown"),
        )?;
        assert_eq!(recovered.sequence, 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn enqueue_json_preserves_payload_when_commit_readback_fails() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let payloads = DurableWorkPayloadStore::open(root.path().join("payloads"))?;
        let mut enqueuer = DurableWorkEnqueuer::open(&event_root)?;
        let event_path = event_root.join("events.log");
        let payload = json!({"message": "commit-unknown"});
        let payload_ref = payloads.payload_ref("test.payload", &payload)?;

        let result = enqueuer.enqueue_json_with_writers(
            &payloads,
            "test.payload",
            &payload,
            enqueue_input("work-unknown"),
            write_content_addressed,
            |events, event| {
                events.append_with_writer(event, |path, bytes| {
                    let mut file = OpenOptions::new().append(true).open(path)?;
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    fs::set_permissions(path, fs::Permissions::from_mode(0o200))?;
                    Err(std::io::Error::other("injected after durable write"))
                })
            },
        );
        fs::set_permissions(&event_path, fs::Permissions::from_mode(0o600))?;
        let error = result.err().ok_or("expected commit-unknown failure")?;

        assert!(matches!(error, DurableWorkEnqueueError::CommitUnknown(_)));
        assert_eq!(payloads.read_json(&payload_ref)?, payload);
        let mut reopened = DurableWorkEnqueuer::open(&event_root)?;
        let recovered = reopened.enqueue_json(
            &payloads,
            "test.payload",
            &payload,
            enqueue_input("work-unknown"),
        )?;
        assert_eq!(recovered.sequence, 1);
        Ok(())
    }

    #[test]
    fn enqueue_json_resyncs_existing_payload_after_file_sync_failure() -> Result<(), Box<dyn Error>>
    {
        assert_retry_resyncs_existing_payload_before_event(PayloadSyncTarget::File)
    }

    #[test]
    fn enqueue_json_resyncs_existing_payload_after_parent_sync_failure(
    ) -> Result<(), Box<dyn Error>> {
        assert_retry_resyncs_existing_payload_before_event(PayloadSyncTarget::ParentDirectory)
    }

    fn assert_retry_resyncs_existing_payload_before_event(
        failed_target: PayloadSyncTarget,
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let event_root = root.path().join("events");
        let payloads = DurableWorkPayloadStore::open(root.path().join("payloads"))?;
        let mut enqueuer = DurableWorkEnqueuer::open(&event_root)?;
        let payload = json!({"message": "retry-durability"});
        let append_called = Cell::new(false);

        let first_error = enqueuer
            .enqueue_json_with_writers(
                &payloads,
                "test.payload",
                &payload,
                enqueue_input("work-retry"),
                |path, bytes| {
                    write_content_addressed_with_sync(path, bytes, |target, file| {
                        if target == failed_target {
                            Err(std::io::Error::other("injected payload sync failure"))
                        } else {
                            file.sync_all()
                        }
                    })
                },
                |_events, _event| {
                    append_called.set(true);
                    Err(DurableEventError::Validation(
                        "event append must not run after payload sync failure".to_owned(),
                    )
                    .into())
                },
            )
            .err()
            .ok_or("expected injected payload sync failure")?;
        assert!(matches!(
            first_error,
            DurableWorkEnqueueError::Work(DurableWorkError::Io(_))
        ));
        assert!(!append_called.get());

        let order = RefCell::new(Vec::new());
        let record = enqueuer.enqueue_json_with_writers(
            &payloads,
            "test.payload",
            &payload,
            enqueue_input("work-retry"),
            |path, bytes| {
                write_content_addressed_with_sync(path, bytes, |target, file| {
                    file.sync_all()?;
                    order.borrow_mut().push(match target {
                        PayloadSyncTarget::File => DurabilityStep::PayloadFile,
                        PayloadSyncTarget::ParentDirectory => {
                            DurabilityStep::PayloadParentDirectory
                        }
                    });
                    Ok(())
                })
            },
            |events, event| {
                let record = events.append_classified(event)?;
                order.borrow_mut().push(DurabilityStep::Event);
                Ok(record)
            },
        )?;

        assert_eq!(record.kind, WORK_ENQUEUED);
        assert_eq!(
            order.into_inner(),
            vec![
                DurabilityStep::PayloadFile,
                DurabilityStep::PayloadParentDirectory,
                DurabilityStep::Event,
            ]
        );
        Ok(())
    }
}
