use chrono::{SecondsFormat, Utc};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_redaction::{redact_string, redact_value};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::SystemTime;

use crate::durable_work::{
    apply_durable_work_event, apply_persisted_durable_work_event,
    durable_work_append_requires_normalization, normalize_durable_work_append,
    DurableWorkReplayState,
};

pub const CURRENT_DURABLE_EVENT_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_DURABLE_EVENT_FRAME_VERSION: u32 = 1;
pub const MAX_INLINE_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_DURABLE_EVENT_FRAME_BYTES: usize = 128 * 1024;

const MAX_IDENTIFIER_BYTES: usize = 1024;
const MAX_PAYLOAD_TYPE_BYTES: usize = 256;
const MAX_ARTIFACT_REF_BYTES: usize = 4096;
const MAX_SKILL_PROVENANCE_ENTRIES: usize = 256;
const TOOL_RESULT_ARTIFACT_PREFIX: &str = ".nanobot/tool-results/";

pub const SESSION_TURN_ACCEPTED: &str = "session.turn_accepted";
pub const SESSION_TURN_COMPLETED: &str = "session.turn_completed";
pub const SESSION_TURN_FAILED: &str = "session.turn_failed";
pub const WORKFLOW_PLANNED: &str = "workflow.planned";
pub const WORKFLOW_COMPLETED: &str = "workflow.completed";
pub const WORKFLOW_FAILED: &str = "workflow.failed";
pub const WORK_ENQUEUED: &str = "work.enqueued";
pub const WORK_LEASED: &str = "work.leased";
pub const WORK_RETRY_SCHEDULED: &str = "work.retry_scheduled";
pub const WORK_REQUEUED: &str = "work.requeued";
pub const WORK_CANCEL_REQUESTED: &str = "work.cancel_requested";
pub const WORK_CANCELLED: &str = "work.cancelled";
pub const WORK_TERMINAL: &str = "work.terminal";
pub const CHILD_SPAWNED: &str = "child.spawned";
pub const CHILD_RUNNING: &str = "child.running";
pub const CHILD_CANCEL_REQUESTED: &str = "child.cancel_requested";
pub const CHILD_RESULT_RECORDED: &str = "child.result_recorded";
pub const RUNTIME_STOP_REQUESTED: &str = "runtime.stop_requested";
pub const RUNTIME_RESTART_REQUESTED: &str = "runtime.restart_requested";
pub const RUNTIME_OWNER_LIFECYCLE: &str = "runtime.owner_lifecycle";
pub const RUNTIME_SUPERVISION_RECORDED: &str = "runtime.supervision_recorded";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum DurableEventPayload {
    Inline {
        payload_type: String,
        data: Value,
    },
    Artifact {
        payload_type: String,
        artifact_ref: String,
    },
}

impl DurableEventPayload {
    pub fn inline(payload_type: impl Into<String>, data: Value) -> Self {
        Self::Inline {
            payload_type: payload_type.into(),
            data,
        }
    }

    pub fn artifact(payload_type: impl Into<String>, artifact_ref: impl Into<String>) -> Self {
        Self::Artifact {
            payload_type: payload_type.into(),
            artifact_ref: artifact_ref.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableExecutionIdentityRef {
    pub session_id: String,
    pub turn_id: String,
    pub effect_id: String,
    pub attempt_id: String,
    pub correlation_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableEventProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_registry_hash: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub skill_body_hashes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_identity: Option<DurableExecutionIdentityRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableEventRecord {
    pub schema_version: u32,
    pub event_id: String,
    pub sequence: u64,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub kind: String,
    pub payload: DurableEventPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<DurableEventProvenance>,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableEventInput {
    pub session_id: String,
    pub turn_id: Option<String>,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub kind: String,
    pub payload: DurableEventPayload,
    pub provenance: Option<DurableEventProvenance>,
}

impl DurableEventInput {
    pub fn new(
        session_id: impl Into<String>,
        kind: impl Into<String>,
        payload: DurableEventPayload,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            turn_id: None,
            causation_id: None,
            correlation_id: None,
            kind: kind.into(),
            payload,
            provenance: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableEventCompatibility {
    Current,
    UnsupportedFrameVersion { found: u32 },
    UnsupportedSchemaVersion { found: u32 },
    UnsupportedKind { kind: String, schema_version: u32 },
}

impl DurableEventCompatibility {
    pub fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

#[derive(Debug, Clone)]
pub struct DurableEventSchemaRegistry {
    kinds: BTreeMap<String, BTreeSet<u32>>,
}

impl Default for DurableEventSchemaRegistry {
    fn default() -> Self {
        let mut registry = Self {
            kinds: BTreeMap::new(),
        };
        for kind in [
            SESSION_TURN_ACCEPTED,
            SESSION_TURN_COMPLETED,
            SESSION_TURN_FAILED,
            WORKFLOW_PLANNED,
            WORKFLOW_COMPLETED,
            WORKFLOW_FAILED,
            WORK_ENQUEUED,
            WORK_LEASED,
            WORK_RETRY_SCHEDULED,
            WORK_REQUEUED,
            WORK_CANCEL_REQUESTED,
            WORK_CANCELLED,
            WORK_TERMINAL,
            CHILD_SPAWNED,
            CHILD_RUNNING,
            CHILD_CANCEL_REQUESTED,
            CHILD_RESULT_RECORDED,
            RUNTIME_STOP_REQUESTED,
            RUNTIME_RESTART_REQUESTED,
            RUNTIME_OWNER_LIFECYCLE,
            RUNTIME_SUPERVISION_RECORDED,
        ] {
            registry.register(kind, CURRENT_DURABLE_EVENT_SCHEMA_VERSION);
        }
        registry
    }
}

impl DurableEventSchemaRegistry {
    pub fn register(&mut self, kind: impl Into<String>, schema_version: u32) {
        self.kinds
            .entry(kind.into())
            .or_default()
            .insert(schema_version);
    }

    pub fn compatibility(&self, kind: &str, schema_version: u32) -> DurableEventCompatibility {
        if schema_version != CURRENT_DURABLE_EVENT_SCHEMA_VERSION {
            return DurableEventCompatibility::UnsupportedSchemaVersion {
                found: schema_version,
            };
        }
        if self
            .kinds
            .get(kind)
            .is_some_and(|versions| versions.contains(&schema_version))
        {
            DurableEventCompatibility::Current
        } else {
            DurableEventCompatibility::UnsupportedKind {
                kind: kind.to_owned(),
                schema_version,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableEventScan {
    pub records: Vec<DurableEventRecord>,
    pub compatibility: DurableEventCompatibility,
    pub incomplete_tail: bool,
    pub truncated: bool,
    pub last_sequence: Option<u64>,
    pub bytes_scanned: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableEventVisitSummary {
    pub compatibility: DurableEventCompatibility,
    pub incomplete_tail: bool,
    pub last_sequence: Option<u64>,
    pub visited: usize,
    pub bytes_scanned: u64,
}

#[derive(Debug)]
pub enum DurableEventError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    Validation(String),
    Corruption { offset: u64, reason: String },
    ReadOnly(DurableEventCompatibility),
    IncompleteTail,
}

impl fmt::Display for DurableEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "durable event I/O failed: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "durable event serialization failed: {error}")
            }
            Self::Validation(reason) => {
                write!(formatter, "durable event validation failed: {reason}")
            }
            Self::Corruption { offset, reason } => {
                write!(
                    formatter,
                    "durable event corruption at byte {offset}: {reason}"
                )
            }
            Self::ReadOnly(compatibility) => {
                write!(
                    formatter,
                    "durable event store is not writable: {compatibility:?}"
                )
            }
            Self::IncompleteTail => write!(formatter, "durable event store has an incomplete tail"),
        }
    }
}

impl Error for DurableEventError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DurableEventError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DurableEventError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[derive(Debug)]
pub(crate) enum DurableEventAppendError {
    DefinitelyNotCommitted(DurableEventError),
    CommitUnknown(DurableEventError),
}

#[derive(Debug)]
pub(crate) struct DurableEventFrameDurable;

impl fmt::Display for DurableEventAppendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefinitelyNotCommitted(error) => {
                write!(formatter, "durable event was not committed: {error}")
            }
            Self::CommitUnknown(error) => {
                write!(formatter, "durable event commit is unknown: {error}")
            }
        }
    }
}

impl Error for DurableEventAppendError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DefinitelyNotCommitted(error) | Self::CommitUnknown(error) => Some(error),
        }
    }
}

impl DurableEventAppendError {
    fn into_error(self) -> DurableEventError {
        match self {
            Self::DefinitelyNotCommitted(error) | Self::CommitUnknown(error) => error,
        }
    }
}

impl From<DurableEventError> for DurableEventAppendError {
    fn from(error: DurableEventError) -> Self {
        Self::DefinitelyNotCommitted(error)
    }
}

impl From<std::io::Error> for DurableEventAppendError {
    fn from(error: std::io::Error) -> Self {
        Self::DefinitelyNotCommitted(DurableEventError::Io(error))
    }
}

impl From<serde_json::Error> for DurableEventAppendError {
    fn from(error: serde_json::Error) -> Self {
        Self::DefinitelyNotCommitted(DurableEventError::Serialization(error))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DurableEventFrame {
    frame_version: u32,
    record_length: u64,
    checksum: String,
    record: DurableEventRecord,
}

#[derive(Debug)]
pub struct DurableEventStore {
    root: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
    registry: DurableEventSchemaRegistry,
    next_sequence: u64,
    compatibility: DurableEventCompatibility,
    incomplete_tail: bool,
    process_lock: Arc<Mutex<ProcessStoreState>>,
}

#[derive(Debug, Default)]
struct ProcessStoreState {
    verified: bool,
    stamp: Option<FileStamp>,
    work_state: Option<DurableWorkReplayState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    length: u64,
    modified: Option<SystemTime>,
    digest: [u8; 32],
}

impl DurableEventStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, DurableEventError> {
        Self::open_with_registry(root, DurableEventSchemaRegistry::default())
    }

    pub fn open_with_registry(
        root: impl AsRef<Path>,
        registry: DurableEventSchemaRegistry,
    ) -> Result<Self, DurableEventError> {
        let root = root.as_ref().to_path_buf();
        reject_symlink(&root)?;
        fs::create_dir_all(&root)?;
        reject_symlink(&root)?;
        let root = fs::canonicalize(root)?;
        let path = root.join("events.log");
        let lock_path = root.join("events.lock");
        let process_lock = process_lock_for(&path);
        let mut guard = recover_lock(&process_lock);
        let _file_lock = acquire_file_lock(&lock_path)?;
        reject_symlink(&path)?;
        let created = match create_event_file(&path) {
            Ok(file) => {
                file.sync_all()?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => return Err(error.into()),
        };
        if created {
            sync_dir(&root)?;
        }
        let current_stamp = file_stamp(&path)?;
        let unchanged = guard.stamp.as_ref() == Some(&current_stamp);
        if !unchanged {
            guard.work_state = None;
        }
        let (compatibility, incomplete_tail, last_sequence) =
            if guard.verified && unchanged && !created {
                let tail = read_tail_state(&path, &registry)?;
                (tail.compatibility, tail.incomplete_tail, tail.last_sequence)
            } else {
                let scan = scan_path(&path, &registry, 0)?;
                guard.verified = !scan.incomplete_tail && scan.compatibility.is_current();
                guard.stamp = Some(current_stamp);
                (scan.compatibility, scan.incomplete_tail, scan.last_sequence)
            };
        drop(guard);
        Ok(Self {
            root,
            path,
            lock_path,
            registry,
            next_sequence: last_sequence.unwrap_or(0).saturating_add(1),
            compatibility,
            incomplete_tail,
            process_lock,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn compatibility(&self) -> &DurableEventCompatibility {
        &self.compatibility
    }

    pub fn is_writable(&self) -> bool {
        self.compatibility.is_current() && !self.incomplete_tail
    }

    pub fn append(
        &mut self,
        input: DurableEventInput,
    ) -> Result<DurableEventRecord, DurableEventError> {
        self.append_classified(input)
            .map_err(DurableEventAppendError::into_error)
    }

    pub(crate) fn append_classified(
        &mut self,
        input: DurableEventInput,
    ) -> Result<DurableEventRecord, DurableEventAppendError> {
        self.append_with_writer(input, write_frame)
    }

    pub(crate) fn append_with_writer(
        &mut self,
        mut input: DurableEventInput,
        writer: impl FnOnce(&Path, &[u8]) -> std::io::Result<DurableEventFrameDurable>,
    ) -> Result<DurableEventRecord, DurableEventAppendError> {
        if self.incomplete_tail {
            return Err(DurableEventError::IncompleteTail.into());
        }
        if !self.compatibility.is_current() {
            return Err(DurableEventError::ReadOnly(self.compatibility.clone()).into());
        }
        let mut guard = recover_lock(&self.process_lock);
        let _file_lock = acquire_file_lock(&self.lock_path)?;
        let current = read_tail_state(&self.path, &self.registry)?;
        if !file_metadata_matches_stamp(&self.path, guard.stamp.as_ref())? {
            guard.work_state = None;
        }
        self.compatibility = current.compatibility;
        self.incomplete_tail = current.incomplete_tail;
        self.next_sequence = current.last_sequence.unwrap_or(0).saturating_add(1);
        if self.incomplete_tail {
            return Err(DurableEventError::IncompleteTail.into());
        }
        if !self.compatibility.is_current() {
            return Err(DurableEventError::ReadOnly(self.compatibility.clone()).into());
        }
        validate_input(&input, &self.registry)?;
        if let DurableEventPayload::Inline { data, .. } = &mut input.payload {
            *data = redact_value(data);
        }
        let sequence = self.next_sequence;
        let mut record = DurableEventRecord {
            schema_version: CURRENT_DURABLE_EVENT_SCHEMA_VERSION,
            event_id: format!("event-{sequence:020}"),
            sequence,
            session_id: input.session_id,
            turn_id: input.turn_id,
            causation_id: input.causation_id,
            correlation_id: input.correlation_id,
            kind: input.kind,
            payload: input.payload,
            provenance: input.provenance,
            recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        let mut appended_work_state = None;
        if record.kind.starts_with("work.") {
            let mut work = match guard.work_state.clone() {
                Some(work) => work,
                None => replay_persisted_work(&self.path, &self.registry)?,
            };
            if durable_work_append_requires_normalization(&record)
                .map_err(|error| DurableEventError::Validation(error.to_string()))?
            {
                normalize_durable_work_append(&work, &mut record)
                    .map_err(|error| DurableEventError::Validation(error.to_string()))?;
            }
            if !apply_durable_work_event(&mut work, &record)
                .map_err(|error| DurableEventError::Validation(error.to_string()))?
            {
                return Err(DurableEventError::Validation(format!(
                    "unregistered durable work event kind {}",
                    record.kind
                ))
                .into());
            }
            appended_work_state = Some(work);
        }
        let record_bytes = canonical_record_bytes(&record)?;
        if record_bytes.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
            return Err(DurableEventError::Validation(format!(
                "event record exceeds {MAX_DURABLE_EVENT_FRAME_BYTES} bytes"
            ))
            .into());
        }
        let frame = DurableEventFrame {
            frame_version: CURRENT_DURABLE_EVENT_FRAME_VERSION,
            record_length: record_bytes.len() as u64,
            checksum: checksum(&record_bytes),
            record: record.clone(),
        };
        let mut frame_bytes = serde_json::to_vec(&frame)?;
        if frame_bytes.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
            return Err(DurableEventError::Validation(format!(
                "event frame exceeds {MAX_DURABLE_EVENT_FRAME_BYTES} bytes"
            ))
            .into());
        }
        frame_bytes.push(b'\n');
        let frame_start = self.path.metadata()?.len();
        let write_result = writer(&self.path, &frame_bytes);
        if let Err(error) = write_result {
            match appended_frame_readback(&self.path, frame_start, &frame_bytes) {
                AppendedFrameReadback::ExactFrame => {
                    guard.verified = false;
                    guard.stamp = None;
                    guard.work_state = None;
                    return Err(DurableEventAppendError::CommitUnknown(
                        DurableEventError::Io(error),
                    ));
                }
                AppendedFrameReadback::DefinitelyNotPresent => {
                    match read_tail_state(&self.path, &self.registry) {
                        Ok(current) => {
                            self.compatibility = current.compatibility;
                            self.incomplete_tail = current.incomplete_tail;
                            self.next_sequence =
                                current.last_sequence.unwrap_or(0).saturating_add(1);
                            let verified = !self.incomplete_tail && self.compatibility.is_current();
                            refresh_process_stamp(&mut guard, &self.path, verified);
                        }
                        Err(_) => {
                            self.incomplete_tail = true;
                            guard.verified = false;
                            guard.stamp = None;
                            guard.work_state = None;
                        }
                    }
                    return Err(DurableEventAppendError::DefinitelyNotCommitted(
                        DurableEventError::Io(error),
                    ));
                }
                AppendedFrameReadback::Unknown(readback_error) => {
                    guard.verified = false;
                    guard.stamp = None;
                    guard.work_state = None;
                    return Err(DurableEventAppendError::CommitUnknown(
                        DurableEventError::Io(readback_error),
                    ));
                }
            }
        }
        refresh_process_stamp(&mut guard, &self.path, true);
        if let Some(work) = appended_work_state {
            guard.work_state = Some(work);
        }
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(record)
    }

    pub fn scan(&self, max_records: usize) -> Result<DurableEventScan, DurableEventError> {
        let mut guard = recover_lock(&self.process_lock);
        let _file_lock = acquire_file_lock(&self.lock_path)?;
        let scan = scan_path(&self.path, &self.registry, max_records)?;
        let stamp = file_stamp(&self.path)?;
        if guard.stamp.as_ref() != Some(&stamp) {
            guard.work_state = None;
        }
        guard.verified = !scan.incomplete_tail && scan.compatibility.is_current();
        guard.stamp = Some(stamp);
        Ok(scan)
    }

    pub fn visit_from_sequence(
        &self,
        after_sequence: u64,
        visitor: impl FnMut(&DurableEventRecord),
    ) -> Result<DurableEventVisitSummary, DurableEventError> {
        let mut guard = recover_lock(&self.process_lock);
        let _file_lock = acquire_file_lock(&self.lock_path)?;
        let summary = visit_path(&self.path, &self.registry, after_sequence, visitor)?;
        let stamp = file_stamp(&self.path)?;
        if guard.stamp.as_ref() != Some(&stamp) {
            guard.work_state = None;
        }
        guard.verified = !summary.incomplete_tail && summary.compatibility.is_current();
        guard.stamp = Some(stamp);
        Ok(summary)
    }
}

fn replay_persisted_work(
    path: &Path,
    registry: &DurableEventSchemaRegistry,
) -> Result<DurableWorkReplayState, DurableEventError> {
    let mut work = DurableWorkReplayState::default();
    let mut reducer_error = None;
    visit_path(path, registry, 0, |persisted| {
        if reducer_error.is_none() {
            reducer_error = apply_persisted_durable_work_event(&mut work, persisted).err();
        }
    })?;
    if let Some(error) = reducer_error {
        return Err(DurableEventError::Validation(error.to_string()));
    }
    Ok(work)
}

fn validate_input(
    input: &DurableEventInput,
    registry: &DurableEventSchemaRegistry,
) -> Result<(), DurableEventError> {
    validate_identifier("session_id", &input.session_id)?;
    for (name, value) in [
        ("turn_id", input.turn_id.as_deref()),
        ("causation_id", input.causation_id.as_deref()),
        ("correlation_id", input.correlation_id.as_deref()),
    ] {
        if let Some(value) = value {
            validate_identifier(name, value)?;
        }
    }
    match registry.compatibility(&input.kind, CURRENT_DURABLE_EVENT_SCHEMA_VERSION) {
        DurableEventCompatibility::Current => {}
        compatibility => return Err(DurableEventError::ReadOnly(compatibility)),
    }
    match &input.payload {
        DurableEventPayload::Inline { payload_type, data } => {
            validate_payload_type(payload_type)?;
            let bytes = serde_json::to_vec(data)?;
            if bytes.len() > MAX_INLINE_EVENT_PAYLOAD_BYTES {
                return Err(DurableEventError::Validation(format!(
                    "inline payload exceeds {MAX_INLINE_EVENT_PAYLOAD_BYTES} bytes"
                )));
            }
        }
        DurableEventPayload::Artifact {
            payload_type,
            artifact_ref,
        } => {
            validate_payload_type(payload_type)?;
            if artifact_ref.trim().is_empty() {
                return Err(DurableEventError::Validation(
                    "artifact_ref must not be empty".to_owned(),
                ));
            }
            if artifact_ref.len() > MAX_ARTIFACT_REF_BYTES {
                return Err(DurableEventError::Validation(format!(
                    "artifact_ref exceeds {MAX_ARTIFACT_REF_BYTES} bytes"
                )));
            }
            if redact_string(artifact_ref) != *artifact_ref {
                return Err(DurableEventError::Validation(
                    "artifact_ref contains secret-like content".to_owned(),
                ));
            }
            let artifact_path = Path::new(artifact_ref);
            if !artifact_ref.starts_with(TOOL_RESULT_ARTIFACT_PREFIX)
                || artifact_ref.len() == TOOL_RESULT_ARTIFACT_PREFIX.len()
                || artifact_path.is_absolute()
                || artifact_ref.contains('\\')
                || artifact_path.components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
            {
                return Err(DurableEventError::Validation(
                    "artifact_ref must be a runtime-managed tool result locator".to_owned(),
                ));
            }
        }
    }
    validate_provenance(input.provenance.as_ref())?;
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<(), DurableEventError> {
    if value.trim().is_empty() {
        return Err(DurableEventError::Validation(format!(
            "{name} must not be empty"
        )));
    }
    if value.len() > MAX_IDENTIFIER_BYTES {
        return Err(DurableEventError::Validation(format!(
            "{name} exceeds {MAX_IDENTIFIER_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_payload_type(payload_type: &str) -> Result<(), DurableEventError> {
    if payload_type.trim().is_empty() {
        return Err(DurableEventError::Validation(
            "payload_type must not be empty".to_owned(),
        ));
    }
    if payload_type.len() > MAX_PAYLOAD_TYPE_BYTES
        || !payload_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(DurableEventError::Validation(
            "payload_type must be a bounded stable identifier".to_owned(),
        ));
    }
    Ok(())
}

fn validate_provenance(
    provenance: Option<&DurableEventProvenance>,
) -> Result<(), DurableEventError> {
    let Some(provenance) = provenance else {
        return Ok(());
    };
    if provenance.skill_body_hashes.len() > MAX_SKILL_PROVENANCE_ENTRIES {
        return Err(DurableEventError::Validation(format!(
            "skill provenance exceeds {MAX_SKILL_PROVENANCE_ENTRIES} entries"
        )));
    }
    if let Some(hash) = provenance.skill_registry_hash.as_deref() {
        validate_identifier("skill_registry_hash", hash)?;
    }
    for (name, hash) in &provenance.skill_body_hashes {
        validate_identifier("skill_name", name)?;
        validate_identifier("skill_body_hash", hash)?;
    }
    if let Some(identity) = provenance.execution_identity.as_ref() {
        for (name, value) in [
            ("execution.session_id", identity.session_id.as_str()),
            ("execution.turn_id", identity.turn_id.as_str()),
            ("execution.effect_id", identity.effect_id.as_str()),
            ("execution.attempt_id", identity.attempt_id.as_str()),
            ("execution.correlation_id", identity.correlation_id.as_str()),
        ] {
            validate_identifier(name, value)?;
        }
    }
    Ok(())
}

fn scan_path(
    path: &Path,
    registry: &DurableEventSchemaRegistry,
    max_records: usize,
) -> Result<DurableEventScan, DurableEventError> {
    reject_symlink(path)?;
    let mut reader = BufReader::new(open_read_file(path)?);
    let mut records = Vec::new();
    let mut compatibility = DurableEventCompatibility::Current;
    let mut incomplete_tail = false;
    let mut truncated = false;
    let mut expected_sequence = 1_u64;
    let mut last_sequence = None;
    let mut offset = 0_u64;
    loop {
        let mut bytes = Vec::new();
        let mut limited =
            Read::by_ref(&mut reader).take((MAX_DURABLE_EVENT_FRAME_BYTES + 2) as u64);
        let read = limited.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        let frame_offset = offset;
        offset = offset.saturating_add(read as u64);
        if bytes.last() != Some(&b'\n') {
            if bytes.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
                return Err(corruption(frame_offset, "frame exceeds maximum size"));
            }
            incomplete_tail = true;
            break;
        }
        bytes.pop();
        let (frame, frame_compatibility) = decode_frame(&bytes, frame_offset, registry)?;
        if frame.record.sequence != expected_sequence {
            return Err(corruption(
                frame_offset,
                format!(
                    "expected sequence {expected_sequence}, found {}",
                    frame.record.sequence
                ),
            ));
        }
        expected_sequence = expected_sequence.saturating_add(1);
        last_sequence = Some(frame.record.sequence);
        if compatibility.is_current() && !frame_compatibility.is_current() {
            compatibility = frame_compatibility.clone();
        }
        if frame_compatibility.is_current() {
            if records.len() < max_records {
                records.push(frame.record);
            } else {
                truncated = true;
            }
        }
    }
    Ok(DurableEventScan {
        records,
        compatibility,
        incomplete_tail,
        truncated,
        last_sequence,
        bytes_scanned: offset,
    })
}

fn visit_path(
    path: &Path,
    registry: &DurableEventSchemaRegistry,
    after_sequence: u64,
    mut visitor: impl FnMut(&DurableEventRecord),
) -> Result<DurableEventVisitSummary, DurableEventError> {
    reject_symlink(path)?;
    let mut reader = BufReader::new(open_read_file(path)?);
    let mut compatibility = DurableEventCompatibility::Current;
    let mut incomplete_tail = false;
    let mut expected_sequence = 1_u64;
    let mut last_sequence = None;
    let mut visited = 0_usize;
    let mut offset = 0_u64;
    loop {
        let mut bytes = Vec::new();
        let mut limited =
            Read::by_ref(&mut reader).take((MAX_DURABLE_EVENT_FRAME_BYTES + 2) as u64);
        let read = limited.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        let frame_offset = offset;
        offset = offset.saturating_add(read as u64);
        if bytes.last() != Some(&b'\n') {
            if bytes.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
                return Err(corruption(frame_offset, "frame exceeds maximum size"));
            }
            incomplete_tail = true;
            break;
        }
        bytes.pop();
        let (frame, frame_compatibility) = decode_frame(&bytes, frame_offset, registry)?;
        if frame.record.sequence != expected_sequence {
            return Err(corruption(
                frame_offset,
                format!(
                    "expected sequence {expected_sequence}, found {}",
                    frame.record.sequence
                ),
            ));
        }
        expected_sequence = expected_sequence.saturating_add(1);
        last_sequence = Some(frame.record.sequence);
        if compatibility.is_current() && !frame_compatibility.is_current() {
            compatibility = frame_compatibility.clone();
        }
        if frame_compatibility.is_current() && frame.record.sequence > after_sequence {
            visitor(&frame.record);
            visited = visited.saturating_add(1);
        }
    }
    Ok(DurableEventVisitSummary {
        compatibility,
        incomplete_tail,
        last_sequence,
        visited,
        bytes_scanned: offset,
    })
}

#[derive(Debug)]
struct DurableEventTailState {
    compatibility: DurableEventCompatibility,
    incomplete_tail: bool,
    last_sequence: Option<u64>,
}

fn read_tail_state(
    path: &Path,
    registry: &DurableEventSchemaRegistry,
) -> Result<DurableEventTailState, DurableEventError> {
    let mut file = open_read_file(path)?;
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(DurableEventTailState {
            compatibility: DurableEventCompatibility::Current,
            incomplete_tail: false,
            last_sequence: None,
        });
    }
    file.seek(SeekFrom::End(-1))?;
    let mut final_byte = [0_u8; 1];
    file.read_exact(&mut final_byte)?;
    if final_byte[0] != b'\n' {
        return Ok(DurableEventTailState {
            compatibility: DurableEventCompatibility::Current,
            incomplete_tail: true,
            last_sequence: None,
        });
    }
    let window = length.min((MAX_DURABLE_EVENT_FRAME_BYTES + 2) as u64);
    file.seek(SeekFrom::End(-(window as i64)))?;
    let mut bytes = Vec::with_capacity(window as usize);
    file.read_to_end(&mut bytes)?;
    bytes.pop();
    let frame_start = match bytes.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => index + 1,
        None if window == length => 0,
        None => {
            return Err(corruption(
                length - window,
                "tail frame exceeds maximum size",
            ))
        }
    };
    let frame_bytes = &bytes[frame_start..];
    let frame_offset = length - window + frame_start as u64;
    let (frame, compatibility) = decode_frame(frame_bytes, frame_offset, registry)?;
    Ok(DurableEventTailState {
        compatibility,
        incomplete_tail: false,
        last_sequence: Some(frame.record.sequence),
    })
}

fn decode_frame(
    bytes: &[u8],
    offset: u64,
    registry: &DurableEventSchemaRegistry,
) -> Result<(DurableEventFrame, DurableEventCompatibility), DurableEventError> {
    if bytes.is_empty() {
        return Err(corruption(offset, "empty frame"));
    }
    if bytes.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
        return Err(corruption(offset, "frame exceeds maximum size"));
    }
    let frame = serde_json::from_slice::<DurableEventFrame>(bytes)
        .map_err(|error| corruption(offset, format!("invalid completed frame: {error}")))?;
    let record_bytes = canonical_record_bytes(&frame.record)?;
    if frame.record_length != record_bytes.len() as u64 {
        return Err(corruption(offset, "record length mismatch"));
    }
    if frame.checksum != checksum(&record_bytes) {
        return Err(corruption(offset, "checksum mismatch"));
    }
    let compatibility = if frame.frame_version != CURRENT_DURABLE_EVENT_FRAME_VERSION {
        DurableEventCompatibility::UnsupportedFrameVersion {
            found: frame.frame_version,
        }
    } else {
        registry.compatibility(&frame.record.kind, frame.record.schema_version)
    };
    Ok((frame, compatibility))
}

fn checksum(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn canonical_record_bytes(record: &DurableEventRecord) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&serde_json::to_value(record)?)
}

fn corruption(offset: u64, reason: impl Into<String>) -> DurableEventError {
    DurableEventError::Corruption {
        offset,
        reason: reason.into(),
    }
}

fn acquire_file_lock(path: &Path) -> std::io::Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    let file = open_regular_file(path, options)?;
    FileExt::lock(&file)?;
    Ok(file)
}

fn create_event_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    open_regular_file(path, options)
}

fn open_read_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    open_regular_file(path, options)
}

fn open_append_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.append(true);
    open_regular_file(path, options)
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
            format!(
                "durable event path is not a regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(file)
}

fn write_frame(path: &Path, bytes: &[u8]) -> std::io::Result<DurableEventFrameDurable> {
    let mut file = open_append_file(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    Ok(DurableEventFrameDurable)
}

#[derive(Debug)]
enum AppendedFrameReadback {
    ExactFrame,
    DefinitelyNotPresent,
    Unknown(std::io::Error),
}

fn appended_frame_readback(
    path: &Path,
    frame_start: u64,
    expected: &[u8],
) -> AppendedFrameReadback {
    let mut file = match open_read_file(path) {
        Ok(file) => file,
        Err(error) => return AppendedFrameReadback::Unknown(error),
    };
    let expected_end = frame_start.saturating_add(expected.len() as u64);
    let actual_end = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => return AppendedFrameReadback::Unknown(error),
    };
    if actual_end < expected_end {
        return AppendedFrameReadback::DefinitelyNotPresent;
    }
    if actual_end > expected_end {
        return AppendedFrameReadback::Unknown(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "durable event append readback contains unexpected trailing bytes",
        ));
    }
    if let Err(error) = file.seek(SeekFrom::Start(frame_start)) {
        return AppendedFrameReadback::Unknown(error);
    }
    let mut actual = vec![0_u8; expected.len()];
    if let Err(error) = file.read_exact(&mut actual) {
        return AppendedFrameReadback::Unknown(error);
    }
    if actual == expected {
        AppendedFrameReadback::ExactFrame
    } else {
        AppendedFrameReadback::Unknown(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "durable event append readback does not match the expected frame",
        ))
    }
}

fn refresh_process_stamp(state: &mut ProcessStoreState, path: &Path, verified: bool) {
    match file_stamp(path) {
        Ok(stamp) => {
            state.verified = verified;
            state.stamp = Some(stamp);
        }
        Err(_) => {
            state.verified = false;
            state.stamp = None;
            state.work_state = None;
        }
    }
}

fn file_stamp(path: &Path) -> std::io::Result<FileStamp> {
    let metadata = fs::metadata(path)?;
    let mut reader = BufReader::new(open_read_file(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(FileStamp {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        digest: hasher.finalize().into(),
    })
}

fn file_metadata_matches_stamp(path: &Path, stamp: Option<&FileStamp>) -> std::io::Result<bool> {
    let Some(stamp) = stamp else {
        return Ok(false);
    };
    let metadata = fs::metadata(path)?;
    Ok(metadata.len() == stamp.length && metadata.modified().ok() == stamp.modified)
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("durable event path is a symlink: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    OpenOptions::new().read(true).open(path)?.sync_all()
}

fn process_lock_for(path: &Path) -> Arc<Mutex<ProcessStoreState>> {
    static LOCKS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<ProcessStoreState>>>>> =
        OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut locks = recover_lock(locks);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(Mutex::new(ProcessStoreState::default()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(message: &str) -> DurableEventInput {
        DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::inline("turn_fact", json!({"message": message})),
        )
    }

    #[test]
    fn append_failure_injection_distinguishes_unwritten_partial_and_committed_frames(
    ) -> Result<(), Box<dyn Error>> {
        let unwritten_root = tempfile::tempdir()?;
        let mut unwritten = DurableEventStore::open(unwritten_root.path())?;
        unwritten.append(input("committed"))?;
        let error = unwritten
            .append_with_writer(input("not-written"), |_path, _bytes| {
                Err(std::io::Error::other("injected before write"))
            })
            .err()
            .ok_or("expected injected write failure")?;
        assert!(matches!(
            error,
            DurableEventAppendError::DefinitelyNotCommitted(DurableEventError::Io(_))
        ));
        drop(unwritten);
        let unwritten = DurableEventStore::open(unwritten_root.path())?;
        let scan = unwritten.scan(10)?;
        assert_eq!(scan.records.len(), 1);
        assert!(!scan.incomplete_tail);
        assert!(unwritten.is_writable());

        let partial_root = tempfile::tempdir()?;
        let mut partial = DurableEventStore::open(partial_root.path())?;
        partial.append(input("committed"))?;
        let error = partial
            .append_with_writer(input("partial"), |path, bytes| {
                let mut file = open_append_file(path)?;
                file.write_all(&bytes[..bytes.len() / 2])?;
                file.sync_all()?;
                Err(std::io::Error::other("injected partial write"))
            })
            .err()
            .ok_or("expected injected partial failure")?;
        assert!(matches!(
            error,
            DurableEventAppendError::DefinitelyNotCommitted(DurableEventError::Io(_))
        ));
        drop(partial);
        let partial = DurableEventStore::open(partial_root.path())?;
        let scan = partial.scan(10)?;
        assert_eq!(scan.records.len(), 1);
        assert!(scan.incomplete_tail);
        assert!(!partial.is_writable());

        let committed_root = tempfile::tempdir()?;
        let mut committed = DurableEventStore::open(committed_root.path())?;
        committed.append(input("first"))?;
        let record = committed.append_with_writer(input("second"), write_frame)?;
        assert_eq!(record.sequence, 2);
        drop(committed);
        let committed = DurableEventStore::open(committed_root.path())?;
        let scan = committed.scan(10)?;
        assert_eq!(scan.records.len(), 2);
        assert!(!scan.incomplete_tail);
        assert!(committed.is_writable());
        Ok(())
    }

    #[test]
    fn append_sync_failure_with_exact_readback_is_commit_unknown() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let mut store = DurableEventStore::open(root.path())?;

        let error = store
            .append_with_writer(input("sync-unknown"), |path, bytes| {
                let mut file = open_append_file(path)?;
                file.write_all(bytes)?;
                file.flush()?;
                Err(std::io::Error::other("injected sync_all failure"))
            })
            .err()
            .ok_or("expected sync failure to leave commit unknown")?;

        assert!(matches!(
            error,
            DurableEventAppendError::CommitUnknown(DurableEventError::Io(_))
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn append_succeeds_when_process_stamp_refresh_fails_after_commit() -> Result<(), Box<dyn Error>>
    {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir()?;
        let mut store = DurableEventStore::open(root.path())?;
        let path = store.path().to_path_buf();

        let result = store.append_with_writer(input("committed"), |path, bytes| {
            let durable = write_frame(path, bytes)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o200))?;
            Ok(durable)
        });
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let record = result?;

        assert_eq!(record.sequence, 1);
        drop(store);
        let reopened = DurableEventStore::open(root.path())?;
        assert_eq!(reopened.scan(10)?.records, vec![record]);
        Ok(())
    }
}
