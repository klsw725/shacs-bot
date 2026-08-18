use crate::durable_event::{
    DurableEventError, DurableEventInput, DurableEventPayload, DurableEventRecord,
    DurableEventStore, CHILD_CANCEL_REQUESTED, CHILD_RESULT_RECORDED, CHILD_RUNNING, CHILD_SPAWNED,
    WORK_CANCELLED, WORK_CANCEL_REQUESTED, WORK_ENQUEUED, WORK_LEASED, WORK_TERMINAL,
};
use crate::durable_trace::{
    opaque_trace_ref, DurableTraceCorrelation, DurableTraceInput, DurableTraceSeverity,
    DurableTraceStore,
};
use crate::durable_work::{
    apply_persisted_durable_work_event, DurableWorkEnqueueInput, DurableWorkEnqueueJsonInput,
    DurableWorkEnqueuer, DurableWorkPayloadStore, DurableWorkReplayState, WorkCancellation,
    WorkLeased, WorkPayloadRef, WorkTerminal, WorkTerminalKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_RETAINED_TERMINAL_CHILDREN: usize = 512;
pub const MAX_RETAINED_CHILD_DECISIONS: usize = 512;
pub const CHILD_RUN_PAYLOAD_TYPE: &str = "shacs.child_run.v1";
pub const CHILD_RESULT_REENTRY_PAYLOAD_TYPE: &str = "shacs.child_result_reentry.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildSpawned {
    pub child_task_id: String,
    pub parent_turn_id: String,
    pub spawn_effect_id: String,
    pub correlation_id: String,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ref: Option<String>,
    pub attempt: u32,
    pub spawned_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildRunning {
    pub child_task_id: String,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildCancelRequested {
    pub child_task_id: String,
    pub requested_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildResultDecisionKind {
    Accepted,
    Duplicate,
    Late,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayChildTaskState {
    Spawned,
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
}

impl ReplayChildTaskState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::TimedOut | Self::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildResultRecorded {
    pub child_task_id: String,
    pub parent_turn_id: String,
    pub spawn_effect_id: String,
    pub correlation_id: String,
    pub idempotency_key: String,
    pub decision: ChildResultDecisionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<ReplayChildTaskState>,
    pub result_ref: String,
    pub finished_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayChildTask {
    pub child_task_id: String,
    pub session_id: String,
    pub parent_turn_id: String,
    pub spawn_effect_id: String,
    pub correlation_id: String,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ref: Option<String>,
    pub state: ReplayChildTaskState,
    pub attempt: u32,
    pub spawned_sequence: u64,
    pub spawned_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_requested_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_requested_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayChildDecision {
    pub sequence: u64,
    pub child_task_id: String,
    pub session_id: String,
    pub parent_turn_id: String,
    pub spawn_effect_id: String,
    pub decision: ChildResultDecisionKind,
    pub result_ref: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableChildReplayState {
    #[serde(default)]
    pub items: BTreeMap<String, ReplayChildTask>,
    #[serde(default)]
    pub decisions: Vec<ReplayChildDecision>,
    #[serde(default)]
    pub terminal_evicted_count: u64,
    #[serde(default)]
    pub decision_evicted_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DurableChildRepairSummary {
    pub terminal_work_repaired: usize,
    pub cancellations_completed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableChildReducerError {
    InvalidPayload {
        sequence: u64,
        reason: String,
    },
    DuplicateSpawn {
        sequence: u64,
        child_task_id: String,
    },
    MissingChild {
        sequence: u64,
        child_task_id: String,
    },
    InvalidTransition {
        sequence: u64,
        child_task_id: String,
    },
    CorrelationMismatch {
        sequence: u64,
        child_task_id: String,
    },
}

impl fmt::Display for DurableChildReducerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPayload { sequence, reason } => {
                write!(
                    formatter,
                    "child event {sequence} has invalid payload: {reason}"
                )
            }
            Self::DuplicateSpawn {
                sequence,
                child_task_id,
            } => write!(
                formatter,
                "child event {sequence} duplicates spawn for {child_task_id}"
            ),
            Self::MissingChild {
                sequence,
                child_task_id,
            } => write!(
                formatter,
                "child event {sequence} references missing child {child_task_id}"
            ),
            Self::InvalidTransition {
                sequence,
                child_task_id,
            } => write!(
                formatter,
                "child event {sequence} has invalid transition for {child_task_id}"
            ),
            Self::CorrelationMismatch {
                sequence,
                child_task_id,
            } => write!(
                formatter,
                "child event {sequence} has correlation mismatch for {child_task_id}"
            ),
        }
    }
}

impl Error for DurableChildReducerError {}

#[derive(Debug, Clone)]
pub struct DurableChildRecorder {
    event_root: PathBuf,
    payload_root: PathBuf,
    trace_root: PathBuf,
}

struct ChildEventAppend<'a> {
    session_id: &'a str,
    turn_id: Option<&'a str>,
    causation_id: Option<&'a str>,
    correlation_id: Option<&'a str>,
    kind: &'a str,
    payload_type: &'a str,
}

impl DurableChildRecorder {
    pub fn open(event_root: impl AsRef<Path>) -> Result<Self, DurableEventError> {
        Self::open_with_payload_root(
            event_root.as_ref(),
            default_payload_root(event_root.as_ref()),
        )
    }

    pub fn open_with_payload_root(
        event_root: impl AsRef<Path>,
        payload_root: impl AsRef<Path>,
    ) -> Result<Self, DurableEventError> {
        let event_root = event_root.as_ref().to_path_buf();
        DurableEventStore::open(&event_root)?;
        Ok(Self {
            trace_root: default_trace_root(&event_root),
            event_root,
            payload_root: payload_root.as_ref().to_path_buf(),
        })
    }

    pub fn record_spawned(
        &self,
        session_id: &str,
        event: &ChildSpawned,
    ) -> Result<DurableEventRecord, DurableEventError> {
        self.append(
            ChildEventAppend {
                session_id,
                turn_id: Some(&event.parent_turn_id),
                causation_id: Some(&event.spawn_effect_id),
                correlation_id: Some(&event.correlation_id),
                kind: CHILD_SPAWNED,
                payload_type: "child_spawned",
            },
            serde_json::to_value(event)?,
        )
    }

    pub fn replay_state(&self) -> Result<DurableChildReplayState, String> {
        let store = DurableEventStore::open(&self.event_root).map_err(|error| error.to_string())?;
        let mut state = DurableChildReplayState::default();
        let mut reducer_error = None;
        store
            .visit_from_sequence(0, |event| {
                if reducer_error.is_none() {
                    if let Err(error) = apply_durable_child_event(&mut state, event) {
                        reducer_error = Some(error.to_string());
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        if let Some(error) = reducer_error {
            return Err(error);
        }
        Ok(state)
    }

    pub fn write_child_run_artifact(&self, data: &Value) -> Result<String, String> {
        let payload_ref = self.write_artifact(CHILD_RUN_PAYLOAD_TYPE, data)?;
        child_ref_from_payload("child-run", &payload_ref)
    }

    pub fn child_run_payload_ref(&self, data: &Value) -> Result<WorkPayloadRef, String> {
        self.write_artifact(CHILD_RUN_PAYLOAD_TYPE, data)
    }

    pub fn write_result_artifact(&self, data: &Value) -> Result<String, String> {
        let payload_ref = self.write_artifact(CHILD_RESULT_REENTRY_PAYLOAD_TYPE, data)?;
        child_ref_from_payload("child-result", &payload_ref)
    }

    pub fn read_run_artifact(&self, run_ref: &str) -> Result<Value, String> {
        self.read_child_artifact("child-run", CHILD_RUN_PAYLOAD_TYPE, run_ref)
    }

    pub fn read_result_artifact(&self, result_ref: &str) -> Result<Value, String> {
        self.read_child_artifact(
            "child-result",
            CHILD_RESULT_REENTRY_PAYLOAD_TYPE,
            result_ref,
        )
    }

    pub fn ensure_child_run_work(
        &self,
        session_id: &str,
        parent_turn_id: &str,
        child_task_id: &str,
        spawn_effect_id: &str,
        payload_ref: WorkPayloadRef,
    ) -> Result<(), String> {
        self.ensure_work(WorkEnsureInput {
            session_id,
            turn_id: Some(parent_turn_id),
            work_id: child_run_work_id(child_task_id),
            work_kind: "subagent.child_run".to_owned(),
            payload_ref,
            dedupe_hint: Some(format!("subagent.child_run:{session_id}:{child_task_id}")),
            effect_id: Some(spawn_effect_id.to_owned()),
            next_wake_at_ms: None,
        })
    }

    pub fn enqueue_missing_child_runs(&self) -> Result<usize, String> {
        let child_state = self.replay_state()?;
        let work_state = self.replay_work_state()?;
        let mut enqueued = 0usize;
        for child in child_state
            .items
            .values()
            .filter(|child| child.state == ReplayChildTaskState::Spawned)
        {
            if work_state
                .items
                .contains_key(&child_run_work_id(&child.child_task_id))
            {
                continue;
            }
            let run_ref = child.run_ref.as_deref().ok_or_else(|| {
                format!("spawned child {} has no run artifact", child.child_task_id)
            })?;
            let run_artifact = self.read_run_artifact(run_ref)?;
            let payload_ref = self.child_run_payload_ref(&run_artifact)?;
            self.ensure_child_run_work(
                &child.session_id,
                &child.parent_turn_id,
                &child.child_task_id,
                &child.spawn_effect_id,
                payload_ref,
            )?;
            enqueued = enqueued.saturating_add(1);
        }
        Ok(enqueued)
    }

    pub fn lease_child_run_work(
        &self,
        child_task_id: &str,
        lease_owner_ref: &str,
        leased_at_ms: u64,
        lease_duration_ms: u64,
    ) -> Result<(), String> {
        let state = self.replay_work_state()?;
        let work_id = child_run_work_id(child_task_id);
        let item = state
            .items
            .get(&work_id)
            .ok_or_else(|| format!("durable child run work is missing: {work_id}"))?;
        if !matches!(
            item.state,
            crate::durable_work::ReplayWorkState::Pending
                | crate::durable_work::ReplayWorkState::WaitingRetry
        ) {
            return Err(format!(
                "durable child run work is not leaseable from {:?}: {work_id}",
                item.state
            ));
        }
        let attempt = item.attempt.saturating_add(1);
        self.append(
            ChildEventAppend {
                session_id: &item.session_key,
                turn_id: item.turn_id.as_deref(),
                causation_id: item.effect_id.as_deref(),
                correlation_id: None,
                kind: WORK_LEASED,
                payload_type: "durable_work",
            },
            serde_json::to_value(WorkLeased {
                work_id: work_id.clone(),
                lease_id: format!("lease-{work_id}-{attempt}-{leased_at_ms}"),
                lease_owner_ref: lease_owner_ref.to_owned(),
                attempt,
                leased_at_ms,
                lease_expires_at_ms: leased_at_ms.saturating_add(lease_duration_ms.max(1)),
            })
            .map_err(|error| error.to_string())?,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    pub fn finish_child_run_work(
        &self,
        child_task_id: &str,
        terminal_kind: WorkTerminalKind,
        outcome_ref: &str,
    ) -> Result<(), String> {
        let state = self.replay_work_state()?;
        let work_id = child_run_work_id(child_task_id);
        let item = state
            .items
            .get(&work_id)
            .ok_or_else(|| format!("durable child run work is missing: {work_id}"))?;
        if item.state.is_terminal() {
            return Ok(());
        }
        self.append(
            ChildEventAppend {
                session_id: &item.session_key,
                turn_id: item.turn_id.as_deref(),
                causation_id: item.effect_id.as_deref(),
                correlation_id: None,
                kind: WORK_TERMINAL,
                payload_type: "durable_work",
            },
            serde_json::to_value(WorkTerminal {
                work_id,
                terminal_kind,
                outcome_ref: outcome_ref.to_owned(),
                facts: None,
            })
            .map_err(|error| error.to_string())?,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    pub fn cancel_child_run_work(&self, child_task_id: &str, reason: &str) -> Result<(), String> {
        let state = self.replay_work_state()?;
        let work_id = child_run_work_id(child_task_id);
        let Some(item) = state.items.get(&work_id) else {
            return Ok(());
        };
        if item.state.is_terminal() {
            return Ok(());
        }
        if item.cancellation_requested_sequence.is_none() {
            self.append(
                ChildEventAppend {
                    session_id: &item.session_key,
                    turn_id: item.turn_id.as_deref(),
                    causation_id: item.effect_id.as_deref(),
                    correlation_id: None,
                    kind: WORK_CANCEL_REQUESTED,
                    payload_type: "durable_work",
                },
                serde_json::to_value(WorkCancellation {
                    work_id: work_id.clone(),
                    reason: reason.to_owned(),
                })
                .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
        }
        let state = self.replay_work_state()?;
        let item = state
            .items
            .get(&work_id)
            .ok_or_else(|| format!("durable child run work is missing: {work_id}"))?;
        self.append(
            ChildEventAppend {
                session_id: &item.session_key,
                turn_id: item.turn_id.as_deref(),
                causation_id: item.effect_id.as_deref(),
                correlation_id: None,
                kind: WORK_CANCELLED,
                payload_type: "durable_work",
            },
            serde_json::to_value(WorkCancellation {
                work_id,
                reason: reason.to_owned(),
            })
            .map_err(|error| error.to_string())?,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    pub fn ensure_parent_reentry_work(
        &self,
        session_id: &str,
        parent_turn_id: &str,
        child_task_id: &str,
        spawn_effect_id: &str,
        message: &Value,
    ) -> Result<(), String> {
        let mut enqueuer =
            DurableWorkEnqueuer::open(&self.event_root).map_err(|error| error.to_string())?;
        let payloads =
            DurableWorkPayloadStore::open(&self.payload_root).map_err(|error| error.to_string())?;
        enqueuer
            .enqueue_json(
                &payloads,
                "shacs.inbound_message.v1",
                message,
                DurableWorkEnqueueJsonInput {
                    work_id: child_reentry_work_id(child_task_id),
                    work_kind: "agent.inbound_turn".to_owned(),
                    session_key: session_id.to_owned(),
                    turn_id: Some(parent_turn_id.to_owned()),
                    effect_id: Some(spawn_effect_id.to_owned()),
                    dedupe_hint: Some(format!("subagent.reentry:{session_id}:{child_task_id}")),
                    next_wake_at_ms: None,
                },
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn enqueue_missing_accepted_reentries(&self) -> Result<usize, String> {
        let child_state = self.replay_state()?;
        let work_state = self.replay_work_state()?;
        let mut enqueued = 0usize;
        for child in child_state.items.values() {
            if !child.state.is_terminal() {
                continue;
            }
            let Some(result_ref) = child.result_ref.as_deref() else {
                continue;
            };
            if work_state
                .items
                .contains_key(&child_reentry_work_id(&child.child_task_id))
            {
                continue;
            }
            let artifact = self.read_result_artifact(result_ref)?;
            let Some(message) = artifact.get("reentry_message") else {
                continue;
            };
            if message.is_null() {
                continue;
            }
            self.ensure_parent_reentry_work(
                &child.session_id,
                &child.parent_turn_id,
                &child.child_task_id,
                &child.spawn_effect_id,
                message,
            )?;
            enqueued = enqueued.saturating_add(1);
        }
        Ok(enqueued)
    }

    pub fn recovery_work_gap_counts(&self) -> Result<(usize, usize), String> {
        let child_state = self.replay_state()?;
        let work_state = self.replay_work_state()?;
        let missing_child_runs = child_state
            .items
            .values()
            .filter(|child| {
                child.state == ReplayChildTaskState::Spawned
                    && !work_state
                        .items
                        .contains_key(&child_run_work_id(&child.child_task_id))
            })
            .count();
        let mut missing_parent_reentries = 0usize;
        for child in child_state
            .items
            .values()
            .filter(|child| child.state.is_terminal())
        {
            let Some(result_ref) = child.result_ref.as_deref() else {
                continue;
            };
            if work_state
                .items
                .contains_key(&child_reentry_work_id(&child.child_task_id))
            {
                continue;
            }
            let artifact = self.read_result_artifact(result_ref)?;
            if artifact
                .get("reentry_message")
                .is_some_and(|message| !message.is_null())
            {
                missing_parent_reentries = missing_parent_reentries.saturating_add(1);
            }
        }
        Ok((missing_child_runs, missing_parent_reentries))
    }

    pub fn repair_incomplete_lifecycle(
        &self,
        repaired_at_ms: u64,
    ) -> Result<DurableChildRepairSummary, String> {
        let child_state = self.replay_state()?;
        let mut summary = DurableChildRepairSummary::default();
        for child in child_state.items.values() {
            if !child.state.is_terminal() && child.cancellation_requested_sequence.is_some() {
                let result_ref = self.write_result_artifact(&json!({
                    "payload_type": CHILD_RESULT_REENTRY_PAYLOAD_TYPE,
                    "result": {
                        "child_task_id": child.child_task_id,
                        "session_id": child.session_id,
                        "parent_turn_id": child.parent_turn_id,
                        "spawn_effect_id": child.spawn_effect_id,
                        "status": "cancelled",
                        "summary": "Durable recovery completed a requested child cancellation.",
                    },
                    "decision": "recovery_cancelled",
                    "reentry_message": null,
                }))?;
                self.record_result(
                    &child.session_id,
                    &ChildResultRecorded {
                        child_task_id: child.child_task_id.clone(),
                        parent_turn_id: child.parent_turn_id.clone(),
                        spawn_effect_id: child.spawn_effect_id.clone(),
                        correlation_id: child.correlation_id.clone(),
                        idempotency_key: child.idempotency_key.clone(),
                        decision: ChildResultDecisionKind::Accepted,
                        terminal_state: Some(ReplayChildTaskState::Cancelled),
                        result_ref,
                        finished_at_ms: repaired_at_ms,
                    },
                )
                .map_err(|error| error.to_string())?;
                self.cancel_child_run_work(&child.child_task_id, "child_cancellation_recovered")?;
                summary.cancellations_completed = summary.cancellations_completed.saturating_add(1);
                continue;
            }
            if !child.state.is_terminal() {
                continue;
            }
            let work_state = self.replay_work_state()?;
            let Some(work) = work_state
                .items
                .get(&child_run_work_id(&child.child_task_id))
            else {
                continue;
            };
            if work.state.is_terminal() {
                continue;
            }
            match child.state {
                ReplayChildTaskState::Completed => self.finish_child_run_work(
                    &child.child_task_id,
                    WorkTerminalKind::Succeeded,
                    child.result_ref.as_deref().ok_or_else(|| {
                        format!("terminal child {} has no result ref", child.child_task_id)
                    })?,
                )?,
                ReplayChildTaskState::Failed | ReplayChildTaskState::TimedOut => self
                    .finish_child_run_work(
                        &child.child_task_id,
                        WorkTerminalKind::Failed,
                        child.result_ref.as_deref().ok_or_else(|| {
                            format!("terminal child {} has no result ref", child.child_task_id)
                        })?,
                    )?,
                ReplayChildTaskState::Cancelled => {
                    self.cancel_child_run_work(&child.child_task_id, "terminal_child_recovered")?
                }
                ReplayChildTaskState::Spawned | ReplayChildTaskState::Running => {}
            }
            summary.terminal_work_repaired = summary.terminal_work_repaired.saturating_add(1);
        }
        Ok(summary)
    }

    pub fn record_running(
        &self,
        session_id: &str,
        parent_turn_id: &str,
        spawn_effect_id: &str,
        correlation_id: &str,
        event: &ChildRunning,
    ) -> Result<DurableEventRecord, DurableEventError> {
        self.append(
            ChildEventAppend {
                session_id,
                turn_id: Some(parent_turn_id),
                causation_id: Some(spawn_effect_id),
                correlation_id: Some(correlation_id),
                kind: CHILD_RUNNING,
                payload_type: "child_running",
            },
            serde_json::to_value(event)?,
        )
    }

    pub fn record_cancel_requested(
        &self,
        session_id: &str,
        parent_turn_id: &str,
        spawn_effect_id: &str,
        correlation_id: &str,
        event: &ChildCancelRequested,
    ) -> Result<DurableEventRecord, DurableEventError> {
        self.append(
            ChildEventAppend {
                session_id,
                turn_id: Some(parent_turn_id),
                causation_id: Some(spawn_effect_id),
                correlation_id: Some(correlation_id),
                kind: CHILD_CANCEL_REQUESTED,
                payload_type: "child_cancel_requested",
            },
            serde_json::to_value(event)?,
        )
    }

    pub fn record_result(
        &self,
        session_id: &str,
        event: &ChildResultRecorded,
    ) -> Result<DurableEventRecord, DurableEventError> {
        if !valid_result_ref(&event.result_ref) {
            return Err(DurableEventError::Validation(
                "child result_ref must be an opaque child-result digest".to_owned(),
            ));
        }
        self.append(
            ChildEventAppend {
                session_id,
                turn_id: Some(&event.parent_turn_id),
                causation_id: Some(&event.spawn_effect_id),
                correlation_id: Some(&event.correlation_id),
                kind: CHILD_RESULT_RECORDED,
                payload_type: "child_result_recorded",
            },
            serde_json::to_value(event)?,
        )
    }

    fn write_artifact(&self, payload_type: &str, data: &Value) -> Result<WorkPayloadRef, String> {
        DurableWorkPayloadStore::open(&self.payload_root)
            .and_then(|store| store.write_json(payload_type, data))
            .map_err(|error| error.to_string())
    }

    fn read_child_artifact(
        &self,
        prefix: &str,
        payload_type: &str,
        value: &str,
    ) -> Result<Value, String> {
        let hex = value
            .strip_prefix(&format!("{prefix}:"))
            .ok_or_else(|| format!("{prefix} ref has invalid prefix"))?;
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("{prefix} ref has invalid digest"));
        }
        let artifact_ref = format!("{hex}.json");
        let byte_len = fs::metadata(self.payload_root.join(&artifact_ref))
            .map_err(|error| error.to_string())?
            .len();
        DurableWorkPayloadStore::new(&self.payload_root)
            .read_json(&WorkPayloadRef::Artifact {
                payload_type: payload_type.to_owned(),
                artifact_ref,
                sha256: format!("sha256:{hex}"),
                byte_len,
            })
            .map_err(|error| error.to_string())
    }

    fn ensure_work(&self, input: WorkEnsureInput<'_>) -> Result<(), String> {
        DurableWorkEnqueuer::open(&self.event_root)
            .map_err(|error| error.to_string())?
            .enqueue(DurableWorkEnqueueInput {
                work_id: input.work_id,
                work_kind: input.work_kind,
                session_key: input.session_id.to_owned(),
                turn_id: input.turn_id.map(str::to_owned),
                effect_id: input.effect_id,
                payload_ref: input.payload_ref,
                dedupe_hint: input.dedupe_hint,
                next_wake_at_ms: input.next_wake_at_ms,
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn replay_work_state(&self) -> Result<DurableWorkReplayState, String> {
        let store = DurableEventStore::open(&self.event_root).map_err(|error| error.to_string())?;
        let mut state = DurableWorkReplayState::default();
        let mut reducer_error = None;
        store
            .visit_from_sequence(0, |event| {
                if reducer_error.is_none() {
                    if let Err(error) = apply_persisted_durable_work_event(&mut state, event) {
                        reducer_error = Some(error.to_string());
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        if let Some(error) = reducer_error {
            return Err(error);
        }
        Ok(state)
    }

    fn append(
        &self,
        event: ChildEventAppend<'_>,
        data: Value,
    ) -> Result<DurableEventRecord, DurableEventError> {
        let mut store = DurableEventStore::open(&self.event_root)?;
        let mut input = DurableEventInput::new(
            event.session_id,
            event.kind,
            DurableEventPayload::inline(event.payload_type, data.clone()),
        );
        input.turn_id = event.turn_id.map(str::to_owned);
        input.causation_id = event.causation_id.map(str::to_owned);
        input.correlation_id = event.correlation_id.map(str::to_owned);
        let record = store.append(input)?;
        self.append_trace_after_commit(&record, &data);
        Ok(record)
    }

    fn append_trace_after_commit(&self, record: &DurableEventRecord, data: &Value) {
        let Ok(store) = DurableTraceStore::open(&self.trace_root) else {
            return;
        };
        let child_task_id = data
            .get("child_task_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                data.get("work_id")
                    .and_then(Value::as_str)
                    .and_then(|work_id| {
                        work_id
                            .strip_prefix("child-run-")
                            .or_else(|| work_id.strip_prefix("child-reentry-"))
                    })
                    .map(str::to_owned)
            });
        let mut input = DurableTraceInput::new(
            "durable_child.event_committed",
            DurableTraceSeverity::Info,
            json!({
                "event_kind": record.kind,
                "payload_type": "durable_child",
                "child_ref": child_task_id.as_deref().map(|value| opaque_trace_ref("child", value)),
                "work_ref": data.get("work_id").and_then(Value::as_str).map(|value| opaque_trace_ref("work", value)),
            }),
        );
        input.event_sequence = Some(record.sequence);
        input.active_recovery = matches!(
            record.kind.as_str(),
            CHILD_SPAWNED
                | CHILD_RUNNING
                | CHILD_CANCEL_REQUESTED
                | WORK_ENQUEUED
                | WORK_LEASED
                | WORK_CANCEL_REQUESTED
        );
        input.correlation = DurableTraceCorrelation {
            session_id: Some(record.session_id.clone()),
            turn_id: record.turn_id.clone(),
            effect_id: record.causation_id.clone(),
            event_id: Some(record.event_id.clone()),
            child_task_id,
            service_correlation_id: record.correlation_id.clone(),
            ..DurableTraceCorrelation::default()
        };
        let _ = store.append(input);
    }
}

struct WorkEnsureInput<'a> {
    session_id: &'a str,
    turn_id: Option<&'a str>,
    work_id: String,
    work_kind: String,
    payload_ref: WorkPayloadRef,
    dedupe_hint: Option<String>,
    effect_id: Option<String>,
    next_wake_at_ms: Option<u64>,
}

pub fn apply_durable_child_event(
    state: &mut DurableChildReplayState,
    event: &DurableEventRecord,
) -> Result<bool, DurableChildReducerError> {
    match event.kind.as_str() {
        CHILD_SPAWNED => apply_spawned(state, event)?,
        CHILD_RUNNING => apply_running(state, event)?,
        CHILD_CANCEL_REQUESTED => apply_cancel_requested(state, event)?,
        CHILD_RESULT_RECORDED => apply_result(state, event)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn apply_spawned(
    state: &mut DurableChildReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableChildReducerError> {
    let payload = payload::<ChildSpawned>(event)?;
    let parent_turn_id =
        event
            .turn_id
            .clone()
            .ok_or_else(|| DurableChildReducerError::InvalidPayload {
                sequence: event.sequence,
                reason: "child spawn is missing turn identity".to_owned(),
            })?;
    let spawn_effect_id =
        event
            .causation_id
            .clone()
            .ok_or_else(|| DurableChildReducerError::InvalidPayload {
                sequence: event.sequence,
                reason: "child spawn is missing effect identity".to_owned(),
            })?;
    let correlation_id =
        event
            .correlation_id
            .clone()
            .ok_or_else(|| DurableChildReducerError::InvalidPayload {
                sequence: event.sequence,
                reason: "child spawn is missing correlation identity".to_owned(),
            })?;
    if payload
        .run_ref
        .as_deref()
        .is_some_and(|run_ref| !valid_child_ref_prefix("child-run", run_ref))
    {
        return Err(DurableChildReducerError::InvalidPayload {
            sequence: event.sequence,
            reason: "run_ref is not an opaque child-run digest".to_owned(),
        });
    }
    if state.items.contains_key(&payload.child_task_id) {
        return Err(DurableChildReducerError::DuplicateSpawn {
            sequence: event.sequence,
            child_task_id: payload.child_task_id,
        });
    }
    state.items.insert(
        payload.child_task_id.clone(),
        ReplayChildTask {
            child_task_id: payload.child_task_id,
            session_id: event.session_id.clone(),
            parent_turn_id,
            spawn_effect_id,
            correlation_id,
            idempotency_key: payload.idempotency_key,
            run_ref: payload.run_ref,
            state: ReplayChildTaskState::Spawned,
            attempt: payload.attempt,
            spawned_sequence: event.sequence,
            spawned_at_ms: payload.spawned_at_ms,
            started_sequence: None,
            started_at_ms: None,
            cancellation_requested_sequence: None,
            cancellation_requested_at_ms: None,
            terminal_sequence: None,
            finished_at_ms: None,
            result_ref: None,
        },
    );
    Ok(())
}

fn apply_running(
    state: &mut DurableChildReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableChildReducerError> {
    let payload = payload::<ChildRunning>(event)?;
    let item = state.items.get(&payload.child_task_id).ok_or_else(|| {
        DurableChildReducerError::MissingChild {
            sequence: event.sequence,
            child_task_id: payload.child_task_id.clone(),
        }
    })?;
    validate_transition_identity(item, event)?;
    let item = child_mut(state, event.sequence, &payload.child_task_id)?;
    if item.state != ReplayChildTaskState::Spawned {
        return Err(DurableChildReducerError::InvalidTransition {
            sequence: event.sequence,
            child_task_id: payload.child_task_id,
        });
    }
    item.state = ReplayChildTaskState::Running;
    item.started_sequence = Some(event.sequence);
    item.started_at_ms = Some(payload.started_at_ms);
    Ok(())
}

fn apply_cancel_requested(
    state: &mut DurableChildReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableChildReducerError> {
    let payload = payload::<ChildCancelRequested>(event)?;
    let item = state.items.get(&payload.child_task_id).ok_or_else(|| {
        DurableChildReducerError::MissingChild {
            sequence: event.sequence,
            child_task_id: payload.child_task_id.clone(),
        }
    })?;
    validate_transition_identity(item, event)?;
    let item = child_mut(state, event.sequence, &payload.child_task_id)?;
    if item.state.is_terminal() || item.cancellation_requested_sequence.is_some() {
        return Err(DurableChildReducerError::InvalidTransition {
            sequence: event.sequence,
            child_task_id: payload.child_task_id,
        });
    }
    item.cancellation_requested_sequence = Some(event.sequence);
    item.cancellation_requested_at_ms = Some(payload.requested_at_ms);
    Ok(())
}

fn validate_transition_identity(
    item: &ReplayChildTask,
    event: &DurableEventRecord,
) -> Result<(), DurableChildReducerError> {
    if item.session_id != event.session_id
        || event.turn_id.as_deref() != Some(item.parent_turn_id.as_str())
        || event.causation_id.as_deref() != Some(item.spawn_effect_id.as_str())
        || event.correlation_id.as_deref() != Some(item.correlation_id.as_str())
    {
        return Err(DurableChildReducerError::CorrelationMismatch {
            sequence: event.sequence,
            child_task_id: item.child_task_id.clone(),
        });
    }
    Ok(())
}

fn apply_result(
    state: &mut DurableChildReplayState,
    event: &DurableEventRecord,
) -> Result<(), DurableChildReducerError> {
    let payload = payload::<ChildResultRecorded>(event)?;
    if !valid_result_ref(&payload.result_ref) {
        return Err(DurableChildReducerError::InvalidPayload {
            sequence: event.sequence,
            reason: "result_ref is not an opaque child-result digest".to_owned(),
        });
    }
    let terminal_state = match (payload.decision, payload.terminal_state) {
        (ChildResultDecisionKind::Accepted, Some(terminal_state))
            if terminal_state.is_terminal() =>
        {
            let item = state.items.get(&payload.child_task_id).ok_or_else(|| {
                DurableChildReducerError::MissingChild {
                    sequence: event.sequence,
                    child_task_id: payload.child_task_id.clone(),
                }
            })?;
            if item.state.is_terminal() {
                return Err(DurableChildReducerError::InvalidTransition {
                    sequence: event.sequence,
                    child_task_id: payload.child_task_id,
                });
            }
            if item.session_id != event.session_id
                || event.turn_id.as_deref() != Some(item.parent_turn_id.as_str())
                || event.causation_id.as_deref() != Some(item.spawn_effect_id.as_str())
                || event.correlation_id.as_deref() != Some(item.correlation_id.as_str())
                || item.idempotency_key != payload.idempotency_key
            {
                return Err(DurableChildReducerError::CorrelationMismatch {
                    sequence: event.sequence,
                    child_task_id: payload.child_task_id,
                });
            }
            Some(terminal_state)
        }
        (ChildResultDecisionKind::Accepted, _)
        | (ChildResultDecisionKind::Duplicate, Some(_))
        | (ChildResultDecisionKind::Late, Some(_))
        | (ChildResultDecisionKind::Stale, Some(_)) => {
            return Err(DurableChildReducerError::InvalidTransition {
                sequence: event.sequence,
                child_task_id: payload.child_task_id,
            });
        }
        (_, None) => None,
    };
    state.decisions.push(ReplayChildDecision {
        sequence: event.sequence,
        child_task_id: payload.child_task_id.clone(),
        session_id: event.session_id.clone(),
        parent_turn_id: payload.parent_turn_id.clone(),
        spawn_effect_id: payload.spawn_effect_id.clone(),
        decision: payload.decision,
        result_ref: payload.result_ref.clone(),
    });
    bound_decisions(state);
    if let Some(terminal_state) = terminal_state {
        let item = child_mut(state, event.sequence, &payload.child_task_id)?;
        item.state = terminal_state;
        item.terminal_sequence = Some(event.sequence);
        item.finished_at_ms = Some(payload.finished_at_ms);
        item.result_ref = Some(payload.result_ref);
        bound_terminal_items(state);
    }
    Ok(())
}

fn valid_result_ref(value: &str) -> bool {
    value.strip_prefix("child-result:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn valid_child_ref_prefix(prefix: &str, value: &str) -> bool {
    value
        .strip_prefix(&format!("{prefix}:"))
        .is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn child_ref_from_payload(prefix: &str, payload_ref: &WorkPayloadRef) -> Result<String, String> {
    let WorkPayloadRef::Artifact { sha256, .. } = payload_ref else {
        return Err("child artifact must be stored out-of-line".to_owned());
    };
    let hex = sha256
        .strip_prefix("sha256:")
        .ok_or_else(|| "child artifact digest must use sha256".to_owned())?;
    Ok(format!("{prefix}:{hex}"))
}

fn default_payload_root(event_root: &Path) -> PathBuf {
    if event_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "durable-events")
    {
        return event_root
            .parent()
            .map(|parent| parent.join("work-payloads"))
            .unwrap_or_else(|| event_root.join("work-payloads"));
    }
    event_root.join("work-payloads")
}

fn default_trace_root(event_root: &Path) -> PathBuf {
    if event_root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "durable-events")
    {
        return event_root
            .parent()
            .map(|parent| parent.join("durable-diagnostics"))
            .unwrap_or_else(|| event_root.join("durable-diagnostics"));
    }
    event_root.join("durable-diagnostics")
}

fn child_run_work_id(child_task_id: &str) -> String {
    format!("child-run-{child_task_id}")
}

fn child_reentry_work_id(child_task_id: &str) -> String {
    format!("child-reentry-{child_task_id}")
}

fn payload<T: for<'de> Deserialize<'de>>(
    event: &DurableEventRecord,
) -> Result<T, DurableChildReducerError> {
    let DurableEventPayload::Inline { data, .. } = &event.payload else {
        return Err(DurableChildReducerError::InvalidPayload {
            sequence: event.sequence,
            reason: "artifact payload is not supported".to_owned(),
        });
    };
    serde_json::from_value(data.clone()).map_err(|error| DurableChildReducerError::InvalidPayload {
        sequence: event.sequence,
        reason: error.to_string(),
    })
}

fn child_mut<'a>(
    state: &'a mut DurableChildReplayState,
    sequence: u64,
    child_task_id: &str,
) -> Result<&'a mut ReplayChildTask, DurableChildReducerError> {
    state
        .items
        .get_mut(child_task_id)
        .ok_or_else(|| DurableChildReducerError::MissingChild {
            sequence,
            child_task_id: child_task_id.to_owned(),
        })
}

fn bound_decisions(state: &mut DurableChildReplayState) {
    if state.decisions.len() <= MAX_RETAINED_CHILD_DECISIONS {
        return;
    }
    let remove = state.decisions.len() - MAX_RETAINED_CHILD_DECISIONS;
    state.decisions.drain(..remove);
    state.decision_evicted_count = state.decision_evicted_count.saturating_add(remove as u64);
}

fn bound_terminal_items(state: &mut DurableChildReplayState) {
    let mut terminal = state
        .items
        .values()
        .filter(|item| item.state.is_terminal())
        .map(|item| {
            (
                item.terminal_sequence.unwrap_or(u64::MAX),
                item.child_task_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    if terminal.len() <= MAX_RETAINED_TERMINAL_CHILDREN {
        return;
    }
    terminal.sort();
    let remove = terminal.len() - MAX_RETAINED_TERMINAL_CHILDREN;
    for (_, child_task_id) in terminal.into_iter().take(remove) {
        state.items.remove(&child_task_id);
    }
    state.terminal_evicted_count = state.terminal_evicted_count.saturating_add(remove as u64);
}

pub fn child_recovery_counts(state: &DurableChildReplayState) -> Value {
    let mut spawned = 0_u64;
    let mut recovery_needed = 0_u64;
    let mut terminal = 0_u64;
    let mut cancellation_requested = 0_u64;
    for item in state.items.values() {
        match item.state {
            ReplayChildTaskState::Spawned => spawned = spawned.saturating_add(1),
            ReplayChildTaskState::Running => recovery_needed = recovery_needed.saturating_add(1),
            _ => terminal = terminal.saturating_add(1),
        }
        if item.cancellation_requested_sequence.is_some() && !item.state.is_terminal() {
            cancellation_requested = cancellation_requested.saturating_add(1);
        }
    }
    json!({
        "spawned": spawned,
        "recovery_needed": recovery_needed,
        "cancellation_requested": cancellation_requested,
        "terminal": terminal,
        "stale_decisions": state.decisions.iter().filter(|item| item.decision == ChildResultDecisionKind::Stale).count(),
        "duplicate_decisions": state.decisions.iter().filter(|item| item.decision == ChildResultDecisionKind::Duplicate).count(),
        "late_decisions": state.decisions.iter().filter(|item| item.decision == ChildResultDecisionKind::Late).count(),
        "terminal_evicted": state.terminal_evicted_count,
        "decision_evicted": state.decision_evicted_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_terminal_and_decision_retention_are_bounded() {
        let mut state = DurableChildReplayState::default();
        for sequence in 1..=(MAX_RETAINED_TERMINAL_CHILDREN + 1) as u64 {
            let child_task_id = format!("child-{sequence}");
            state.items.insert(
                child_task_id.clone(),
                ReplayChildTask {
                    child_task_id: child_task_id.clone(),
                    session_id: "session".to_owned(),
                    parent_turn_id: "turn".to_owned(),
                    spawn_effect_id: format!("spawn:{child_task_id}"),
                    correlation_id: format!("correlation:{child_task_id}"),
                    idempotency_key: format!("idempotency:{child_task_id}"),
                    run_ref: None,
                    state: ReplayChildTaskState::Completed,
                    attempt: 1,
                    spawned_sequence: sequence,
                    spawned_at_ms: sequence,
                    started_sequence: None,
                    started_at_ms: None,
                    cancellation_requested_sequence: None,
                    cancellation_requested_at_ms: None,
                    terminal_sequence: Some(sequence),
                    finished_at_ms: Some(sequence),
                    result_ref: Some(format!("child-result:{sequence:064}")),
                },
            );
            state.decisions.push(ReplayChildDecision {
                sequence,
                child_task_id,
                session_id: "session".to_owned(),
                parent_turn_id: "turn".to_owned(),
                spawn_effect_id: format!("spawn:child-{sequence}"),
                decision: ChildResultDecisionKind::Accepted,
                result_ref: format!("child-result:{sequence:064}"),
            });
        }

        bound_terminal_items(&mut state);
        bound_decisions(&mut state);

        assert_eq!(state.items.len(), MAX_RETAINED_TERMINAL_CHILDREN);
        assert_eq!(state.decisions.len(), MAX_RETAINED_CHILD_DECISIONS);
        assert_eq!(state.terminal_evicted_count, 1);
        assert_eq!(state.decision_evicted_count, 1);
    }
}
