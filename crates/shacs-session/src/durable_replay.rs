use crate::durable_event::{
    DurableEventError, DurableEventPayload, DurableEventRecord, DurableEventStore,
    SESSION_TURN_ACCEPTED, SESSION_TURN_COMPLETED, SESSION_TURN_FAILED, WORKFLOW_COMPLETED,
    WORKFLOW_FAILED, WORKFLOW_PLANNED,
};
use crate::durable_work::{
    apply_persisted_durable_work_event, DurableWorkReducerError, DurableWorkReplayState,
};
use chrono::{SecondsFormat, Utc};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CURRENT_DURABLE_CHECKPOINT_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_DURABLE_REDUCER_SCHEMA_VERSION: u32 = 1;
pub const DURABLE_REPLAY_STATE_SCHEMA: &str = "shacs.durable_replay_state.v1";
const MAX_CHECKPOINT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableReplayState {
    pub reducer_schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_through: Option<u64>,
    #[serde(default)]
    pub sessions: BTreeMap<String, ReplaySessionState>,
    #[serde(default)]
    pub workflows: BTreeMap<String, ReplayWorkflowState>,
    #[serde(default)]
    pub work: DurableWorkReplayState,
    #[serde(default)]
    pub children: DurableChildReplayState,
}

impl DurableReplayState {
    pub fn event_zero() -> Self {
        Self {
            reducer_schema_version: CURRENT_DURABLE_REDUCER_SCHEMA_VERSION,
            applied_through: None,
            sessions: BTreeMap::new(),
            workflows: BTreeMap::new(),
            work: DurableWorkReplayState::default(),
            children: DurableChildReplayState::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaySessionState {
    #[serde(default)]
    pub turns: BTreeMap<String, ReplayTurnState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayTurnState {
    pub status: ReplayTurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_effect_count: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayTurnStatus {
    Open,
    Completed,
    Failed,
    ResponseCompletedWithoutAccepted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayWorkflowState {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub status: ReplayWorkflowStatus,
    pub planned_sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_sequence: Option<u64>,
    pub harness_plan_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_result_count: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayWorkflowStatus {
    Planned,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableReplayReducerError {
    Sequence { expected: u64, found: u64 },
    MissingTurnId { sequence: u64 },
    MissingPayloadField { sequence: u64, field: String },
    DuplicateAcceptedTurn { sequence: u64 },
    MissingAcceptedTurn { sequence: u64 },
    DuplicateTerminalTurn { sequence: u64 },
    MissingWorkflowPlan { sequence: u64 },
    DuplicateWorkflowLifecycle { sequence: u64 },
    Work(DurableWorkReducerError),
    Child(DurableChildReducerError),
}

impl fmt::Display for DurableReplayReducerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sequence { expected, found } => {
                write!(
                    formatter,
                    "expected replay sequence {expected}, found {found}"
                )
            }
            Self::MissingTurnId { sequence } => {
                write!(formatter, "event {sequence} is missing turn_id")
            }
            Self::MissingPayloadField { sequence, field } => {
                write!(
                    formatter,
                    "event {sequence} is missing payload field {field}"
                )
            }
            Self::DuplicateAcceptedTurn { sequence } => {
                write!(formatter, "event {sequence} duplicates an accepted turn")
            }
            Self::MissingAcceptedTurn { sequence } => {
                write!(formatter, "event {sequence} has no accepted turn")
            }
            Self::DuplicateTerminalTurn { sequence } => {
                write!(formatter, "event {sequence} duplicates a terminal turn")
            }
            Self::MissingWorkflowPlan { sequence } => {
                write!(formatter, "event {sequence} has no workflow plan")
            }
            Self::DuplicateWorkflowLifecycle { sequence } => {
                write!(
                    formatter,
                    "event {sequence} duplicates a workflow lifecycle"
                )
            }
            Self::Work(error) => error.fmt(formatter),
            Self::Child(error) => error.fmt(formatter),
        }
    }
}

impl Error for DurableReplayReducerError {}

impl From<DurableWorkReducerError> for DurableReplayReducerError {
    fn from(error: DurableWorkReducerError) -> Self {
        Self::Work(error)
    }
}

impl From<DurableChildReducerError> for DurableReplayReducerError {
    fn from(error: DurableChildReducerError) -> Self {
        Self::Child(error)
    }
}

pub fn apply_durable_event(
    state: &mut DurableReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableReplayReducerError> {
    let expected = state.applied_through.unwrap_or(0).saturating_add(1);
    if event.sequence != expected {
        return Err(DurableReplayReducerError::Sequence {
            expected,
            found: event.sequence,
        });
    }
    if apply_persisted_durable_work_event(&mut state.work, event)? {
        state.applied_through = Some(event.sequence);
        return Ok(());
    }
    if apply_durable_child_event(&mut state.children, event)? {
        state.applied_through = Some(event.sequence);
        return Ok(());
    }
    match event.kind.as_str() {
        SESSION_TURN_ACCEPTED => apply_turn_accepted(state, event)?,
        SESSION_TURN_COMPLETED => apply_turn_terminal(state, event, false)?,
        SESSION_TURN_FAILED => apply_turn_terminal(state, event, true)?,
        WORKFLOW_PLANNED => apply_workflow_planned(state, event)?,
        WORKFLOW_COMPLETED => apply_workflow_terminal(state, event, false)?,
        WORKFLOW_FAILED => apply_workflow_terminal(state, event, true)?,
        _ => {}
    }
    state.applied_through = Some(event.sequence);
    Ok(())
}

fn apply_turn_accepted(
    state: &mut DurableReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableReplayReducerError> {
    let turn_id = required_turn_id(event)?;
    let turns = &mut state
        .sessions
        .entry(event.session_id.clone())
        .or_default()
        .turns;
    if turns.contains_key(turn_id) {
        return Err(DurableReplayReducerError::DuplicateAcceptedTurn {
            sequence: event.sequence,
        });
    }
    let payload = inline_payload(event);
    turns.insert(
        turn_id.to_owned(),
        ReplayTurnState {
            status: ReplayTurnStatus::Open,
            accepted_sequence: Some(event.sequence),
            terminal_sequence: None,
            content_hash: payload
                .and_then(|value| value.get("content_hash"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            media_count: payload
                .and_then(|value| value.get("media_count"))
                .and_then(Value::as_u64),
            stop_reason: None,
            command: None,
            tool_count: None,
            outcome_count: None,
            pending_effect_count: None,
        },
    );
    Ok(())
}

fn apply_turn_terminal(
    state: &mut DurableReplayState,
    event: &DurableEventRecord,
    failed: bool,
) -> Result<(), DurableReplayReducerError> {
    let turn_id = required_turn_id(event)?;
    let payload = inline_payload(event);
    let command = payload
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if let Some(turn) = state
        .sessions
        .get_mut(&event.session_id)
        .and_then(|session| session.turns.get_mut(turn_id))
    {
        if !matches!(turn.status, ReplayTurnStatus::Open) {
            return Err(DurableReplayReducerError::DuplicateTerminalTurn {
                sequence: event.sequence,
            });
        }
        turn.status = if failed {
            ReplayTurnStatus::Failed
        } else {
            ReplayTurnStatus::Completed
        };
        turn.terminal_sequence = Some(event.sequence);
        turn.stop_reason = payload
            .and_then(|value| value.get("stop_reason"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        turn.command = command;
        turn.tool_count = payload
            .and_then(|value| value.get("tool_count"))
            .and_then(Value::as_u64);
        turn.outcome_count = payload
            .and_then(|value| value.get("outcome_count"))
            .and_then(Value::as_u64);
        turn.pending_effect_count = payload
            .and_then(|value| value.get("pending_effect_count"))
            .and_then(Value::as_u64);
        return Ok(());
    }
    if failed {
        return Err(DurableReplayReducerError::MissingAcceptedTurn {
            sequence: event.sequence,
        });
    }
    state
        .sessions
        .entry(event.session_id.clone())
        .or_default()
        .turns
        .insert(
            turn_id.to_owned(),
            ReplayTurnState {
                status: ReplayTurnStatus::ResponseCompletedWithoutAccepted,
                accepted_sequence: None,
                terminal_sequence: Some(event.sequence),
                content_hash: None,
                media_count: None,
                stop_reason: payload
                    .and_then(|value| value.get("stop_reason"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                command,
                tool_count: None,
                outcome_count: None,
                pending_effect_count: None,
            },
        );
    Ok(())
}

fn apply_workflow_planned(
    state: &mut DurableReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableReplayReducerError> {
    let payload = inline_payload(event);
    let workflow_id = required_payload_string(event, payload, "workflow_id")?;
    let harness_plan_digest = required_payload_string(event, payload, "harness_plan_digest")?;
    if state.workflows.contains_key(&workflow_id) {
        return Err(DurableReplayReducerError::DuplicateWorkflowLifecycle {
            sequence: event.sequence,
        });
    }
    state.workflows.insert(
        workflow_id,
        ReplayWorkflowState {
            session_id: event.session_id.clone(),
            turn_id: event.turn_id.clone(),
            status: ReplayWorkflowStatus::Planned,
            planned_sequence: event.sequence,
            terminal_sequence: None,
            harness_plan_digest,
            terminal_state: None,
            child_result_count: None,
        },
    );
    Ok(())
}

fn apply_workflow_terminal(
    state: &mut DurableReplayState,
    event: &DurableEventRecord,
    failed: bool,
) -> Result<(), DurableReplayReducerError> {
    let payload = inline_payload(event);
    let workflow_id = required_payload_string(event, payload, "workflow_id")?;
    let Some(workflow) = state.workflows.get_mut(&workflow_id) else {
        return Err(DurableReplayReducerError::MissingWorkflowPlan {
            sequence: event.sequence,
        });
    };
    if !matches!(workflow.status, ReplayWorkflowStatus::Planned) {
        return Err(DurableReplayReducerError::DuplicateWorkflowLifecycle {
            sequence: event.sequence,
        });
    }
    workflow.status = if failed {
        ReplayWorkflowStatus::Failed
    } else {
        ReplayWorkflowStatus::Completed
    };
    workflow.terminal_sequence = Some(event.sequence);
    workflow.terminal_state = payload
        .and_then(|value| value.get("state"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    workflow.child_result_count = payload
        .and_then(|value| value.get("child_result_count"))
        .and_then(Value::as_u64);
    Ok(())
}

fn required_turn_id(event: &DurableEventRecord) -> Result<&str, DurableReplayReducerError> {
    event
        .turn_id
        .as_deref()
        .ok_or(DurableReplayReducerError::MissingTurnId {
            sequence: event.sequence,
        })
}

fn inline_payload(event: &DurableEventRecord) -> Option<&Value> {
    match &event.payload {
        DurableEventPayload::Inline { data, .. } => Some(data),
        DurableEventPayload::Artifact { .. } => None,
    }
}

fn required_payload_string(
    event: &DurableEventRecord,
    payload: Option<&Value>,
    field: &str,
) -> Result<String, DurableReplayReducerError> {
    payload
        .and_then(|value| value.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| DurableReplayReducerError::MissingPayloadField {
            sequence: event.sequence,
            field: field.to_owned(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DurableCheckpointBody {
    frame_version: u32,
    schema_version: u32,
    checkpoint_id: String,
    included_sequence: u64,
    reducer_schema_version: u32,
    state_schema: String,
    state_digest: String,
    recorded_at: String,
    state: DurableReplayState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DurableCheckpointFrame {
    body: DurableCheckpointBody,
    checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableCheckpointRecord {
    pub checkpoint_id: String,
    pub included_sequence: u64,
    pub state_digest: String,
    pub recorded_at: String,
    pub state: DurableReplayState,
}

#[derive(Debug)]
pub enum DurableCheckpointError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    Validation(String),
    Corruption(String),
}

impl fmt::Display for DurableCheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "durable checkpoint I/O failed: {error}"),
            Self::Serialization(error) => {
                write!(
                    formatter,
                    "durable checkpoint serialization failed: {error}"
                )
            }
            Self::Validation(reason) => {
                write!(formatter, "durable checkpoint validation failed: {reason}")
            }
            Self::Corruption(reason) => write!(formatter, "durable checkpoint corrupt: {reason}"),
        }
    }
}

impl Error for DurableCheckpointError {}

impl From<std::io::Error> for DurableCheckpointError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DurableCheckpointError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[derive(Debug, Clone)]
pub struct DurableCheckpointStore {
    root: PathBuf,
    lock_path: PathBuf,
}

impl DurableCheckpointStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, DurableCheckpointError> {
        let root = root.as_ref().to_path_buf();
        reject_symlink(&root)?;
        fs::create_dir_all(&root)?;
        reject_symlink(&root)?;
        let root = fs::canonicalize(root)?;
        Ok(Self {
            lock_path: root.join("checkpoints.lock"),
            root,
        })
    }

    pub fn write(
        &self,
        state: &DurableReplayState,
    ) -> Result<DurableCheckpointRecord, DurableCheckpointError> {
        validate_state(state)?;
        let _lock = acquire_lock(&self.lock_path)?;
        let included_sequence = state.applied_through.unwrap_or(0);
        let state_digest = digest_value(state)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| DurableCheckpointError::Validation(error.to_string()))?
            .as_nanos();
        let checkpoint_id = format!("checkpoint-{included_sequence:020}-{nonce:039}");
        let body = DurableCheckpointBody {
            frame_version: 1,
            schema_version: CURRENT_DURABLE_CHECKPOINT_SCHEMA_VERSION,
            checkpoint_id: checkpoint_id.clone(),
            included_sequence,
            reducer_schema_version: CURRENT_DURABLE_REDUCER_SCHEMA_VERSION,
            state_schema: DURABLE_REPLAY_STATE_SCHEMA.to_owned(),
            state_digest: state_digest.clone(),
            recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            state: state.clone(),
        };
        let body_bytes = canonical_bytes(&body)?;
        let frame = DurableCheckpointFrame {
            checksum: checksum(&body_bytes),
            body,
        };
        let bytes = serde_json::to_vec(&frame)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(DurableCheckpointError::Validation(format!(
                "checkpoint exceeds {MAX_CHECKPOINT_BYTES} bytes"
            )));
        }
        let path = self.root.join(format!("{checkpoint_id}.json"));
        write_atomic(&path, &bytes)?;
        Ok(DurableCheckpointRecord {
            checkpoint_id,
            included_sequence,
            state_digest,
            recorded_at: frame.body.recorded_at,
            state: state.clone(),
        })
    }

    pub fn candidate_paths(&self) -> Result<Vec<PathBuf>, DurableCheckpointError> {
        let _lock = acquire_lock(&self.lock_path)?;
        self.candidate_paths_unlocked()
    }

    fn candidate_paths_unlocked(&self) -> Result<Vec<PathBuf>, DurableCheckpointError> {
        let mut paths = fs::read_dir(&self.root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("checkpoint-") && name.ends_with(".json"))
            })
            .collect::<Vec<_>>();
        paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        Ok(paths)
    }

    pub fn quarantine_unusable_candidates(
        &self,
        event_store: &DurableEventStore,
        last_event_sequence: u64,
    ) -> Result<usize, DurableCheckpointError> {
        let _lock = acquire_lock(&self.lock_path)?;
        let paths = self.candidate_paths_unlocked()?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| DurableCheckpointError::Validation(error.to_string()))?
            .as_nanos();
        let mut quarantined = 0;
        for (index, path) in paths.into_iter().enumerate() {
            let unusable = match self.read_candidate(&path) {
                Ok(record) if record.included_sequence <= last_event_sequence => {
                    !checkpoint_matches_event_prefix(event_store, &record).map_err(|error| {
                        DurableCheckpointError::Validation(format!(
                            "durable event validation failed: {error}"
                        ))
                    })?
                }
                Ok(_) | Err(_) => true,
            };
            if !unusable {
                continue;
            }
            let rejected = self.root.join(format!(
                ".rejected-{nonce:039}-{index:06}-{}",
                candidate_name(&path)
            ));
            fs::rename(&path, rejected)?;
            quarantined += 1;
        }
        if quarantined > 0 {
            OpenOptions::new().read(true).open(&self.root)?.sync_all()?;
        }
        Ok(quarantined)
    }

    pub fn read_candidate(
        &self,
        path: &Path,
    ) -> Result<DurableCheckpointRecord, DurableCheckpointError> {
        if path.parent() != Some(self.root.as_path()) {
            return Err(DurableCheckpointError::Validation(
                "checkpoint candidate is outside the checkpoint root".to_owned(),
            ));
        }
        reject_symlink(path)?;
        let file = open_read_file(path)?;
        let mut bytes = Vec::new();
        file.take((MAX_CHECKPOINT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(DurableCheckpointError::Corruption(
                "checkpoint exceeds maximum size".to_owned(),
            ));
        }
        let frame = serde_json::from_slice::<DurableCheckpointFrame>(&bytes)?;
        let body_bytes = canonical_bytes(&frame.body)?;
        if frame.checksum != checksum(&body_bytes) {
            return Err(DurableCheckpointError::Corruption(
                "checkpoint frame checksum mismatch".to_owned(),
            ));
        }
        if frame.body.frame_version != 1
            || frame.body.schema_version != CURRENT_DURABLE_CHECKPOINT_SCHEMA_VERSION
            || frame.body.reducer_schema_version != CURRENT_DURABLE_REDUCER_SCHEMA_VERSION
            || frame.body.state_schema != DURABLE_REPLAY_STATE_SCHEMA
        {
            return Err(DurableCheckpointError::Corruption(
                "checkpoint schema is incompatible".to_owned(),
            ));
        }
        validate_state(&frame.body.state)?;
        if frame.body.included_sequence != frame.body.state.applied_through.unwrap_or(0) {
            return Err(DurableCheckpointError::Corruption(
                "checkpoint included_sequence does not match state".to_owned(),
            ));
        }
        if frame.body.state_digest != digest_value(&frame.body.state)? {
            return Err(DurableCheckpointError::Corruption(
                "checkpoint state digest mismatch".to_owned(),
            ));
        }
        Ok(DurableCheckpointRecord {
            checkpoint_id: frame.body.checkpoint_id,
            included_sequence: frame.body.included_sequence,
            state_digest: frame.body.state_digest,
            recorded_at: frame.body.recorded_at,
            state: frame.body.state,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableRecoveryStatus {
    Healthy,
    Recoverable,
    InspectOnly,
    Blocked,
}

impl DurableRecoveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Recoverable => "recoverable",
            Self::InspectOnly => "inspect_only",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableRecoveryIssueKind {
    CheckpointCorrupt,
    CheckpointAheadOfEvents,
    EventCorrupt,
    EventIncompatible,
    IncompleteTail,
    ReducerViolation,
    OrphanCheckpoint,
}

impl DurableRecoveryIssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CheckpointCorrupt => "checkpoint_corrupt",
            Self::CheckpointAheadOfEvents => "checkpoint_ahead_of_events",
            Self::EventCorrupt => "event_corrupt",
            Self::EventIncompatible => "event_incompatible",
            Self::IncompleteTail => "incomplete_tail",
            Self::ReducerViolation => "reducer_violation",
            Self::OrphanCheckpoint => "orphan_checkpoint",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableRecoveryIssue {
    pub kind: DurableRecoveryIssueKind,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableRecoveryHint {
    RewriteCheckpoint,
    DiscardIncompleteTail,
    InspectEventStore,
}

impl DurableRecoveryHint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RewriteCheckpoint => "rewrite_checkpoint",
            Self::DiscardIncompleteTail => "discard_incomplete_tail",
            Self::InspectEventStore => "inspect_event_store",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableReplayAdmission {
    pub status: DurableRecoveryStatus,
    pub writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<DurableReplayState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_used: Option<String>,
    pub replayed_event_count: usize,
    #[serde(default)]
    pub issues: Vec<DurableRecoveryIssue>,
    #[serde(default)]
    pub recovery_hints: Vec<DurableRecoveryHint>,
}

impl DurableReplayAdmission {
    pub fn healthy_event_zero() -> Self {
        Self {
            status: DurableRecoveryStatus::Healthy,
            writable: true,
            state: Some(DurableReplayState::event_zero()),
            checkpoint_used: None,
            replayed_event_count: 0,
            issues: Vec::new(),
            recovery_hints: Vec::new(),
        }
    }
}

pub fn evaluate_durable_recovery(
    event_root: impl AsRef<Path>,
    checkpoint_root: impl AsRef<Path>,
) -> DurableReplayAdmission {
    let event_root = event_root.as_ref();
    let checkpoint_root = checkpoint_root.as_ref();
    let event_path = event_root.join("events.log");
    if !event_path.exists() {
        let orphaned = checkpoint_root.exists()
            && fs::read_dir(checkpoint_root)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .any(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"));
        if orphaned {
            return blocked_admission(
                DurableRecoveryIssueKind::OrphanCheckpoint,
                "checkpoint exists without a durable event store",
            );
        }
        return DurableReplayAdmission::healthy_event_zero();
    }
    let event_store = match DurableEventStore::open(event_root) {
        Ok(store) => store,
        Err(error) => return event_error_admission(error),
    };
    let event_scan = match event_store.scan(0) {
        Ok(scan) => scan,
        Err(error) => return event_error_admission(error),
    };
    if !event_scan.compatibility.is_current() {
        return DurableReplayAdmission {
            status: DurableRecoveryStatus::InspectOnly,
            writable: false,
            state: None,
            checkpoint_used: None,
            replayed_event_count: 0,
            issues: vec![DurableRecoveryIssue {
                kind: DurableRecoveryIssueKind::EventIncompatible,
                detail: format!("{:?}", event_scan.compatibility),
            }],
            recovery_hints: vec![DurableRecoveryHint::InspectEventStore],
        };
    }
    let checkpoint_store = match DurableCheckpointStore::open(checkpoint_root) {
        Ok(store) => store,
        Err(error) => {
            return blocked_admission(
                DurableRecoveryIssueKind::CheckpointCorrupt,
                &error.to_string(),
            )
        }
    };
    let paths = match checkpoint_store.candidate_paths() {
        Ok(paths) => paths,
        Err(error) => {
            return blocked_admission(
                DurableRecoveryIssueKind::CheckpointCorrupt,
                &error.to_string(),
            )
        }
    };
    let mut issues = Vec::new();
    let mut selected = None;
    for path in &paths {
        match checkpoint_store.read_candidate(path) {
            Ok(record) if record.included_sequence <= event_scan.last_sequence.unwrap_or(0) => {
                match checkpoint_matches_event_prefix(&event_store, &record) {
                    Ok(true) => {
                        selected = Some(record);
                        break;
                    }
                    Ok(false) => issues.push(DurableRecoveryIssue {
                        kind: DurableRecoveryIssueKind::CheckpointCorrupt,
                        detail: format!(
                            "{} does not match the durable event prefix",
                            candidate_name(path)
                        ),
                    }),
                    Err(error) => return event_error_admission(error),
                }
            }
            Ok(_) => issues.push(DurableRecoveryIssue {
                kind: DurableRecoveryIssueKind::CheckpointAheadOfEvents,
                detail: candidate_name(path),
            }),
            Err(error) => issues.push(DurableRecoveryIssue {
                kind: DurableRecoveryIssueKind::CheckpointCorrupt,
                detail: format!("{}: {error}", candidate_name(path)),
            }),
        }
    }
    let checkpoint_used = selected
        .as_ref()
        .map(|checkpoint| checkpoint.checkpoint_id.clone());
    let mut state = selected
        .map(|checkpoint| checkpoint.state)
        .unwrap_or_else(DurableReplayState::event_zero);
    let after_sequence = state.applied_through.unwrap_or(0);
    let mut reducer_error = None;
    let visit = event_store.visit_from_sequence(after_sequence, |event| {
        if reducer_error.is_none() {
            reducer_error = apply_durable_event(&mut state, event).err();
        }
    });
    let summary = match visit {
        Ok(summary) => summary,
        Err(error) => return event_error_admission(error),
    };
    if let Some(error) = reducer_error {
        return DurableReplayAdmission {
            status: DurableRecoveryStatus::Blocked,
            writable: false,
            state: Some(state),
            checkpoint_used,
            replayed_event_count: summary.visited,
            issues: vec![DurableRecoveryIssue {
                kind: DurableRecoveryIssueKind::ReducerViolation,
                detail: error.to_string(),
            }],
            recovery_hints: vec![DurableRecoveryHint::InspectEventStore],
        };
    }
    if summary.incomplete_tail {
        issues.push(DurableRecoveryIssue {
            kind: DurableRecoveryIssueKind::IncompleteTail,
            detail: "durable event store has an incomplete final frame".to_owned(),
        });
        return DurableReplayAdmission {
            status: DurableRecoveryStatus::Recoverable,
            writable: false,
            state: Some(state),
            checkpoint_used,
            replayed_event_count: summary.visited,
            issues,
            recovery_hints: vec![DurableRecoveryHint::DiscardIncompleteTail],
        };
    }
    let used_fallback = !issues.is_empty();
    DurableReplayAdmission {
        status: if used_fallback {
            DurableRecoveryStatus::Recoverable
        } else {
            DurableRecoveryStatus::Healthy
        },
        writable: !used_fallback,
        state: Some(state),
        checkpoint_used,
        replayed_event_count: summary.visited,
        issues,
        recovery_hints: if used_fallback {
            vec![DurableRecoveryHint::RewriteCheckpoint]
        } else {
            Vec::new()
        },
    }
}

fn checkpoint_matches_event_prefix(
    event_store: &DurableEventStore,
    checkpoint: &DurableCheckpointRecord,
) -> Result<bool, DurableEventError> {
    let mut replayed = DurableReplayState::event_zero();
    let mut reducer_failed = false;
    event_store.visit_from_sequence(0, |event| {
        if event.sequence <= checkpoint.included_sequence && !reducer_failed {
            reducer_failed = apply_durable_event(&mut replayed, event).is_err();
        }
    })?;
    Ok(!reducer_failed && replayed == checkpoint.state)
}

fn event_error_admission(error: DurableEventError) -> DurableReplayAdmission {
    blocked_admission(DurableRecoveryIssueKind::EventCorrupt, &error.to_string())
}

fn blocked_admission(kind: DurableRecoveryIssueKind, detail: &str) -> DurableReplayAdmission {
    DurableReplayAdmission {
        status: DurableRecoveryStatus::Blocked,
        writable: false,
        state: None,
        checkpoint_used: None,
        replayed_event_count: 0,
        issues: vec![DurableRecoveryIssue {
            kind,
            detail: detail.to_owned(),
        }],
        recovery_hints: vec![DurableRecoveryHint::InspectEventStore],
    }
}

fn validate_state(state: &DurableReplayState) -> Result<(), DurableCheckpointError> {
    if state.reducer_schema_version != CURRENT_DURABLE_REDUCER_SCHEMA_VERSION {
        return Err(DurableCheckpointError::Validation(
            "unsupported reducer schema version".to_owned(),
        ));
    }
    Ok(())
}

fn digest_value(value: &impl Serialize) -> Result<String, serde_json::Error> {
    Ok(checksum(&canonical_bytes(value)?))
}

fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&serde_json::to_value(value)?)
}

fn checksum(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn candidate_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("invalid-checkpoint")
        .to_owned()
}

fn acquire_lock(path: &Path) -> std::io::Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    let file = open_regular_file(path, options)?;
    FileExt::lock(&file)?;
    Ok(file)
}

fn open_read_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    open_regular_file(path, options)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "checkpoint has no parent")
    })?;
    let temp = parent.join(format!(".{}.tmp", candidate_name(path)));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = open_regular_file(&temp, options)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        OpenOptions::new().read(true).open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
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
            "checkpoint path is not a regular file",
        ));
    }
    Ok(file)
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "durable checkpoint path is a symlink",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
use crate::durable_child::{
    apply_durable_child_event, DurableChildReducerError, DurableChildReplayState,
};
