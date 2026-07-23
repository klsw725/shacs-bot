use shacs_bus::{InboundMessage, MessageBus, MessageBusError};
use shacs_session::durable_event::{
    DurableEventError, DurableEventInput, DurableEventPayload, DurableEventRecord,
    DurableEventStore, WORK_CANCELLED, WORK_CANCEL_REQUESTED, WORK_ENQUEUED, WORK_LEASED,
    WORK_REQUEUED, WORK_RETRY_SCHEDULED, WORK_TERMINAL,
};
use shacs_session::durable_trace::{
    opaque_trace_ref, DurableTraceCorrelation, DurableTraceInput, DurableTraceSeverity,
    DurableTraceStore,
};
use shacs_session::durable_work::{
    DurableWorkAdmission, DurableWorkError, DurableWorkPayloadStore, DurableWorkReplayState,
    ReplayWorkItem, ReplayWorkState, RuntimeControlRequested, WorkCancellation, WorkEnqueued,
    WorkLeased, WorkPayloadRef, WorkRequeued, WorkRetryScheduled, WorkTerminal, WorkTerminalKind,
    MAX_DURABLE_WORK_ATTEMPTS, MAX_WORK_PAYLOAD_BYTES,
};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

const INBOUND_WORK_KIND: &str = "agent.inbound_turn";
const INBOUND_PAYLOAD_TYPE: &str = "shacs.inbound_message.v1";
const DURABLE_WORK_ID_METADATA: &str = "durable_work_id";
const MAX_DURABLE_WORK_EVENT_LOG_BYTES: u64 = 512 * 1024 * 1024;
const PROCESS_LOCAL_BUS_RETRY_MS: u64 = 250;

#[derive(Debug)]
pub enum DurableDispatchError {
    Event(DurableEventError),
    Work(DurableWorkError),
    Serialization(serde_json::Error),
    Bus(MessageBusError),
    MissingWork(String),
    InvalidWork(String),
    UnsupportedWorkKind(String),
}

impl fmt::Display for DurableDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event(error) => error.fmt(formatter),
            Self::Work(error) => error.fmt(formatter),
            Self::Serialization(error) => write!(formatter, "durable dispatch failed: {error}"),
            Self::Bus(error) => error.fmt(formatter),
            Self::MissingWork(work_id) => write!(formatter, "durable work {work_id} is missing"),
            Self::InvalidWork(message) => formatter.write_str(message),
            Self::UnsupportedWorkKind(kind) => {
                write!(formatter, "durable work kind {kind} is not dispatchable")
            }
        }
    }
}

impl Error for DurableDispatchError {}

impl From<DurableEventError> for DurableDispatchError {
    fn from(error: DurableEventError) -> Self {
        Self::Event(error)
    }
}

impl From<DurableWorkError> for DurableDispatchError {
    fn from(error: DurableWorkError) -> Self {
        Self::Work(error)
    }
}

impl From<serde_json::Error> for DurableDispatchError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

impl From<MessageBusError> for DurableDispatchError {
    fn from(error: MessageBusError) -> Self {
        Self::Bus(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableDispatchSummary {
    pub leased_work_ids: Vec<String>,
    pub retry_scheduled_work_ids: Vec<String>,
    pub exhausted_work_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableStaleRecoverySummary {
    pub requeued_work_ids: Vec<String>,
    pub cancelled_work_ids: Vec<String>,
}

enum DispatchOutcome {
    Leased,
    RetryScheduled,
    Exhausted,
}

pub struct DurableWorkDispatcher {
    events: DurableEventStore,
    trace_root: PathBuf,
    payloads: DurableWorkPayloadStore,
    bus: MessageBus,
    lease_owner_ref: String,
    lease_duration_ms: u64,
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

impl DurableWorkDispatcher {
    pub fn open(
        event_root: impl AsRef<Path>,
        payload_root: impl AsRef<Path>,
        bus: MessageBus,
        lease_owner_ref: impl Into<String>,
        lease_duration_ms: u64,
    ) -> Result<Self, DurableDispatchError> {
        if lease_duration_ms == 0 {
            return Err(DurableWorkError::Validation(
                "durable work lease duration must be positive".to_owned(),
            )
            .into());
        }
        let event_root = event_root.as_ref().to_path_buf();
        Ok(Self {
            trace_root: default_trace_root(&event_root),
            events: DurableEventStore::open(event_root)?,
            payloads: DurableWorkPayloadStore::open(payload_root)?,
            bus,
            lease_owner_ref: lease_owner_ref.into(),
            lease_duration_ms,
        })
    }

    pub fn enqueue_inbound(
        &mut self,
        work_id: impl Into<String>,
        message: &InboundMessage,
        dedupe_hint: Option<String>,
        next_wake_at_ms: Option<u64>,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        if self
            .events
            .path()
            .metadata()
            .map_err(DurableWorkError::from)?
            .len()
            >= MAX_DURABLE_WORK_EVENT_LOG_BYTES
        {
            return Err(DurableWorkError::Validation(format!(
                "durable event log exceeds {MAX_DURABLE_WORK_EVENT_LOG_BYTES} bytes"
            ))
            .into());
        }
        let work_id = work_id.into();
        let message_value = serde_json::to_value(message)?;
        if serde_json::to_vec(&message_value)?.len() > MAX_WORK_PAYLOAD_BYTES {
            return Err(DurableWorkError::Validation(format!(
                "inbound work payload exceeds {MAX_WORK_PAYLOAD_BYTES} bytes"
            ))
            .into());
        }
        let payload_ref = self
            .payloads
            .write_json(INBOUND_PAYLOAD_TYPE, &message_value)?;
        let payload = WorkEnqueued {
            work_id,
            work_kind: INBOUND_WORK_KIND.to_owned(),
            payload_ref,
            dedupe_hint,
            next_wake_at_ms,
            effect_id: None,
        };
        let record = self.append(message.session_key(), None, WORK_ENQUEUED, &payload)?;
        self.append_channel_trace_after_commit(&record, &message.channel, &payload.work_id);
        Ok(record)
    }

    pub fn enqueue_work(
        &mut self,
        input: DurableWorkEnqueueInput,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        let payload = WorkEnqueued {
            work_id: input.work_id,
            work_kind: input.work_kind,
            payload_ref: input.payload_ref,
            dedupe_hint: input.dedupe_hint,
            next_wake_at_ms: input.next_wake_at_ms,
            effect_id: input.effect_id,
        };
        self.append(input.session_key, input.turn_id, WORK_ENQUEUED, &payload)
    }

    pub fn dispatch_due(
        &mut self,
        state: &DurableWorkReplayState,
        admission: &DurableWorkAdmission,
        now_ms: u64,
    ) -> Result<DurableDispatchSummary, DurableDispatchError> {
        let mut leased_work_ids = Vec::new();
        let mut retry_scheduled_work_ids = Vec::new();
        let mut exhausted_work_ids = Vec::new();
        let mut unavailable_sessions = state
            .items
            .values()
            .filter(|item| item.state == ReplayWorkState::Leased)
            .map(|item| item.session_key.clone())
            .collect::<BTreeSet<_>>();
        for work_id in &admission.due_work_ids {
            let item = state
                .items
                .get(work_id)
                .ok_or_else(|| DurableDispatchError::MissingWork(work_id.clone()))?;
            if item.work_kind != INBOUND_WORK_KIND {
                continue;
            }
            if !unavailable_sessions.insert(item.session_key.clone()) {
                continue;
            }
            match self.lease_and_publish(item, now_ms)? {
                DispatchOutcome::Leased => leased_work_ids.push(work_id.clone()),
                DispatchOutcome::RetryScheduled => retry_scheduled_work_ids.push(work_id.clone()),
                DispatchOutcome::Exhausted => exhausted_work_ids.push(work_id.clone()),
            }
        }
        Ok(DurableDispatchSummary {
            leased_work_ids,
            retry_scheduled_work_ids,
            exhausted_work_ids,
        })
    }

    pub fn dispatch_priority(
        &mut self,
        item: &ReplayWorkItem,
        now_ms: u64,
    ) -> Result<DurableDispatchSummary, DurableDispatchError> {
        if !matches!(
            item.state,
            ReplayWorkState::Pending | ReplayWorkState::WaitingRetry
        ) {
            return Err(DurableDispatchError::InvalidWork(format!(
                "durable priority work {} is not dispatchable from state {:?}",
                item.work_id, item.state
            )));
        }
        let mut summary = DurableDispatchSummary {
            leased_work_ids: Vec::new(),
            retry_scheduled_work_ids: Vec::new(),
            exhausted_work_ids: Vec::new(),
        };
        match self.lease_and_publish(item, now_ms)? {
            DispatchOutcome::Leased => summary.leased_work_ids.push(item.work_id.clone()),
            DispatchOutcome::RetryScheduled => {
                summary.retry_scheduled_work_ids.push(item.work_id.clone())
            }
            DispatchOutcome::Exhausted => summary.exhausted_work_ids.push(item.work_id.clone()),
        }
        Ok(summary)
    }

    pub fn schedule_retry(
        &mut self,
        item: &ReplayWorkItem,
        next_wake_at_ms: u64,
        backoff_ms: u64,
        reason_ref: impl Into<String>,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        if item.cancellation_requested_sequence.is_some() {
            return Err(DurableDispatchError::InvalidWork(format!(
                "durable work {} cannot retry after cancellation was requested",
                item.work_id
            )));
        }
        self.append_for_item(
            item,
            WORK_RETRY_SCHEDULED,
            &WorkRetryScheduled {
                work_id: item.work_id.clone(),
                attempt: item.attempt,
                next_wake_at_ms,
                backoff_ms,
                reason_ref: reason_ref.into(),
            },
        )
    }

    pub fn request_cancellation(
        &mut self,
        item: &ReplayWorkItem,
        reason: impl Into<String>,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        self.append_for_item(
            item,
            WORK_CANCEL_REQUESTED,
            &WorkCancellation {
                work_id: item.work_id.clone(),
                reason: reason.into(),
            },
        )
    }

    pub fn record_cancelled(
        &mut self,
        item: &ReplayWorkItem,
        reason: impl Into<String>,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        self.append_for_item(
            item,
            WORK_CANCELLED,
            &WorkCancellation {
                work_id: item.work_id.clone(),
                reason: reason.into(),
            },
        )
    }

    pub fn record_terminal(
        &mut self,
        item: &ReplayWorkItem,
        terminal_kind: WorkTerminalKind,
        outcome_ref: impl Into<String>,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        self.append_for_item(
            item,
            WORK_TERMINAL,
            &WorkTerminal {
                work_id: item.work_id.clone(),
                terminal_kind,
                outcome_ref: outcome_ref.into(),
            },
        )
    }

    pub fn requeue_stale(
        &mut self,
        state: &DurableWorkReplayState,
        admission: &DurableWorkAdmission,
    ) -> Result<DurableStaleRecoverySummary, DurableDispatchError> {
        let mut summary = DurableStaleRecoverySummary {
            requeued_work_ids: Vec::new(),
            cancelled_work_ids: Vec::new(),
        };
        for work_id in &admission.stale_lease_work_ids {
            let item = state
                .items
                .get(work_id)
                .ok_or_else(|| DurableDispatchError::MissingWork(work_id.clone()))?;
            if item.cancellation_requested_sequence.is_some() {
                self.record_cancelled(item, "stale_lease_after_cancellation")?;
                summary.cancelled_work_ids.push(work_id.clone());
            } else {
                self.requeue(item, "stale_lease")?;
                summary.requeued_work_ids.push(work_id.clone());
            }
        }
        Ok(summary)
    }

    pub fn requeue(
        &mut self,
        item: &ReplayWorkItem,
        reason: impl Into<String>,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        self.append_for_item(
            item,
            WORK_REQUEUED,
            &WorkRequeued {
                work_id: item.work_id.clone(),
                reason: reason.into(),
            },
        )
    }

    pub fn lease_owner_ref(&self) -> &str {
        &self.lease_owner_ref
    }

    fn lease_and_publish(
        &mut self,
        item: &ReplayWorkItem,
        now_ms: u64,
    ) -> Result<DispatchOutcome, DurableDispatchError> {
        if item.work_kind != INBOUND_WORK_KIND {
            return Err(DurableDispatchError::UnsupportedWorkKind(
                item.work_kind.clone(),
            ));
        }
        let mut message: InboundMessage =
            serde_json::from_value(self.payloads.read_json(&item.payload_ref)?)?;
        message.session_key_override = Some(item.session_key.clone());
        message.metadata.insert(
            DURABLE_WORK_ID_METADATA.to_owned(),
            serde_json::Value::String(item.work_id.clone()),
        );
        let attempt = item.attempt.saturating_add(1);
        self.append_for_item(
            item,
            WORK_LEASED,
            &WorkLeased {
                work_id: item.work_id.clone(),
                lease_id: format!("lease-{}-{attempt}-{now_ms}", item.work_id),
                lease_owner_ref: self.lease_owner_ref.clone(),
                attempt,
                leased_at_ms: now_ms,
                lease_expires_at_ms: now_ms.saturating_add(self.lease_duration_ms),
            },
        )?;
        if self.bus.try_publish_inbound(message).is_ok() {
            return Ok(DispatchOutcome::Leased);
        }
        if attempt >= MAX_DURABLE_WORK_ATTEMPTS {
            self.append_for_item(
                item,
                WORK_TERMINAL,
                &WorkTerminal {
                    work_id: item.work_id.clone(),
                    terminal_kind: WorkTerminalKind::Exhausted,
                    outcome_ref: "process_local_bus_attempt_limit".to_owned(),
                },
            )?;
            return Ok(DispatchOutcome::Exhausted);
        }
        self.append_for_item(
            item,
            WORK_RETRY_SCHEDULED,
            &WorkRetryScheduled {
                work_id: item.work_id.clone(),
                attempt,
                next_wake_at_ms: now_ms.saturating_add(PROCESS_LOCAL_BUS_RETRY_MS),
                backoff_ms: PROCESS_LOCAL_BUS_RETRY_MS,
                reason_ref: "process_local_bus_full".to_owned(),
            },
        )?;
        Ok(DispatchOutcome::RetryScheduled)
    }

    fn append_for_item<T: serde::Serialize>(
        &mut self,
        item: &ReplayWorkItem,
        kind: &str,
        payload: &T,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        self.append(
            item.session_key.clone(),
            item.turn_id.clone(),
            kind,
            payload,
        )
    }

    fn append<T: serde::Serialize>(
        &mut self,
        session_id: String,
        turn_id: Option<String>,
        kind: &str,
        payload: &T,
    ) -> Result<DurableEventRecord, DurableDispatchError> {
        let payload = serde_json::to_value(payload)?;
        let mut input = DurableEventInput::new(
            session_id,
            kind,
            DurableEventPayload::inline("durable_work", payload.clone()),
        );
        input.turn_id = turn_id;
        input.causation_id = payload
            .get("effect_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let record = self.events.append(input)?;
        self.append_trace_after_commit(&record, &payload);
        Ok(record)
    }

    fn append_trace_after_commit(&self, record: &DurableEventRecord, payload: &serde_json::Value) {
        let Ok(store) = DurableTraceStore::open(&self.trace_root) else {
            return;
        };
        let mut input = DurableTraceInput::new(
            "durable_work.event_committed",
            DurableTraceSeverity::Info,
            serde_json::json!({
                "event_kind": record.kind,
                "payload_type": "durable_work",
                "work_ref": payload.get("work_id").and_then(serde_json::Value::as_str).map(|value| opaque_trace_ref("work", value)),
            }),
        );
        input.event_sequence = Some(record.sequence);
        input.active_recovery = matches!(
            record.kind.as_str(),
            WORK_ENQUEUED
                | WORK_LEASED
                | WORK_REQUEUED
                | WORK_RETRY_SCHEDULED
                | WORK_CANCEL_REQUESTED
        );
        input.correlation = DurableTraceCorrelation {
            session_id: Some(record.session_id.clone()),
            turn_id: record.turn_id.clone(),
            effect_id: record.causation_id.clone(),
            event_id: Some(record.event_id.clone()),
            service_correlation_id: payload
                .get("work_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| record.correlation_id.clone()),
            ..DurableTraceCorrelation::default()
        };
        let _ = store.append(input);
    }

    fn append_channel_trace_after_commit(
        &self,
        record: &DurableEventRecord,
        channel: &str,
        work_id: &str,
    ) {
        let Ok(store) = DurableTraceStore::open(&self.trace_root) else {
            return;
        };
        let mut input = DurableTraceInput::new(
            "durable_channel.inbound_committed",
            DurableTraceSeverity::Info,
            serde_json::json!({
                "event_kind": record.kind,
                "channel": channel,
                "work_ref": opaque_trace_ref("work", work_id),
            }),
        );
        input.event_sequence = Some(record.sequence);
        input.active_recovery = true;
        input.correlation = DurableTraceCorrelation {
            session_id: Some(record.session_id.clone()),
            event_id: Some(record.event_id.clone()),
            channel_id: Some(channel.to_owned()),
            service_correlation_id: Some(work_id.to_owned()),
            ..DurableTraceCorrelation::default()
        };
        let _ = store.append(input);
    }
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

pub fn runtime_control_payload(requested_at_ms: u64) -> RuntimeControlRequested {
    RuntimeControlRequested {
        requested_at_ms,
        request_id: None,
        target_owner_id: None,
    }
}

pub fn inline_control_payload(
    payload_type: impl Into<String>,
    data: serde_json::Value,
) -> Result<WorkPayloadRef, DurableDispatchError> {
    Ok(WorkPayloadRef::inline(payload_type, data)?)
}
