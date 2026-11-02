use chrono::{SecondsFormat, Utc};
use rusqlite::config::DbConfig;
use rusqlite::types::ValueRef;
use rusqlite::{
    params, params_from_iter, Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_redaction::{redact_string, redact_value};
use std::cell::{Cell, OnceCell};
use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use crate::durable_work::{
    apply_durable_work_event, apply_persisted_durable_work_event,
    durable_work_append_requires_normalization, normalize_durable_work_append,
    DurableWorkReducerError, DurableWorkReplayState,
};

pub const CURRENT_DURABLE_EVENT_SCHEMA_VERSION: u32 = 1;
/// Storage format version of the durable event database, kept in `PRAGMA user_version`.
/// A database with another version is reported as `UnsupportedFrameVersion`.
pub const CURRENT_DURABLE_EVENT_FRAME_VERSION: u32 = 1;
pub const MAX_INLINE_EVENT_PAYLOAD_BYTES: usize = 64 * 1024;
/// Largest canonical record a durable event may store.
pub const MAX_DURABLE_EVENT_FRAME_BYTES: usize = 128 * 1024;
pub const DURABLE_EVENT_DATABASE_FILE: &str = "events.sqlite3";
/// The append-only log that preceded the SQLite store. It is never read or migrated.
pub const LEGACY_DURABLE_EVENT_LOG_FILE: &str = "events.log";

const MAX_IDENTIFIER_BYTES: usize = 1024;
const MAX_PAYLOAD_TYPE_BYTES: usize = 256;
const MAX_ARTIFACT_REF_BYTES: usize = 4096;
const MAX_SKILL_PROVENANCE_ENTRIES: usize = 256;
const TOOL_RESULT_ARTIFACT_PREFIX: &str = ".nanobot/tool-results/";
const BUSY_TIMEOUT: Duration = Duration::from_millis(5000);
const EVENT_COLUMNS: &str = "sequence, event_id, session_id, turn_id, kind, work_id, \
     schema_version, recorded_at, record, checksum";
/// Every schema object the store owns, ordered by name as `sqlite_master` lists them.
/// `events_reject_overwrite` stops `INSERT OR REPLACE`, whose implicit delete fires no delete
/// trigger.
const SCHEMA: [(&str, &str); 6] = [
    (
        "events",
        "CREATE TABLE events (sequence INTEGER PRIMARY KEY NOT NULL, \
         event_id TEXT NOT NULL UNIQUE, session_id TEXT NOT NULL, turn_id TEXT, \
         kind TEXT NOT NULL, work_id TEXT, schema_version INTEGER NOT NULL, \
         recorded_at TEXT NOT NULL, record TEXT NOT NULL, checksum TEXT NOT NULL) STRICT",
    ),
    (
        "events_by_kind",
        "CREATE INDEX events_by_kind ON events (kind, sequence)",
    ),
    (
        "events_by_session_work",
        "CREATE INDEX events_by_session_work ON events (session_id, work_id, sequence)",
    ),
    (
        "events_reject_delete",
        "CREATE TRIGGER events_reject_delete BEFORE DELETE ON events \
         BEGIN SELECT RAISE(ABORT, 'durable events are append-only'); END",
    ),
    (
        "events_reject_overwrite",
        "CREATE TRIGGER events_reject_overwrite BEFORE INSERT ON events \
         WHEN EXISTS (SELECT 1 FROM events \
         WHERE sequence = NEW.sequence OR event_id = NEW.event_id) \
         BEGIN SELECT RAISE(ABORT, 'durable events are append-only'); END",
    ),
    (
        "events_reject_update",
        "CREATE TRIGGER events_reject_update BEFORE UPDATE ON events \
         BEGIN SELECT RAISE(ABORT, 'durable events are append-only'); END",
    ),
];

pub const SESSION_TURN_ACCEPTED: &str = "session.turn_accepted";
pub const SESSION_TURN_COMPLETED: &str = "session.turn_completed";
pub const SESSION_TURN_FAILED: &str = "session.turn_failed";
pub const ENTRUSTED_NOTIFICATION_FACT: &str = "entrusted.notification_fact";
pub const WEB_WATCH_NOTIFICATION_FACT: &str = "web_watch.notification_fact";
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

/// The kinds the durable work reducer acts on. It leaves the work state unchanged for every
/// other kind, so the work state is built from these events alone.
pub(crate) const WORK_STATE_KINDS: [&str; 9] = [
    WORK_ENQUEUED,
    WORK_LEASED,
    WORK_RETRY_SCHEDULED,
    WORK_REQUEUED,
    WORK_CANCEL_REQUESTED,
    WORK_CANCELLED,
    WORK_TERMINAL,
    RUNTIME_STOP_REQUESTED,
    RUNTIME_RESTART_REQUESTED,
];

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

#[derive(Debug, Clone, PartialEq, Eq)]
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
            ENTRUSTED_NOTIFICATION_FACT,
            WEB_WATCH_NOTIFICATION_FACT,
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

/// Result of reading every stored event in sequence order.
///
/// `incomplete_tail` is always false: SQLite transactions never expose a partially written
/// event. `bytes_scanned` counts the bytes of the canonical records that were read.
#[derive(Debug, Clone, PartialEq)]
pub struct DurableEventScan {
    pub records: Vec<DurableEventRecord>,
    pub compatibility: DurableEventCompatibility,
    pub incomplete_tail: bool,
    pub truncated: bool,
    pub last_sequence: Option<u64>,
    pub bytes_scanned: u64,
}

/// Result of visiting stored events; see [`DurableEventScan`] for the meaning of
/// `incomplete_tail` and `bytes_scanned`. `last_sequence` is the last sequence of the whole
/// store even when no event was visited. `decoded` counts every event the call decoded and
/// checked: the events new since this process last verified the store, then the events read
/// for the visit.
#[derive(Debug, Clone, PartialEq)]
pub struct DurableEventVisitSummary {
    pub compatibility: DurableEventCompatibility,
    pub incomplete_tail: bool,
    pub last_sequence: Option<u64>,
    pub visited: usize,
    pub bytes_scanned: u64,
    pub decoded: u64,
}

/// Events a filtered read selects. A condition left `None` holds for every event; `kinds` of
/// `Some` holds for the listed kinds only. `work_id_prefix` holds only for an inline payload
/// whose `work_id` is a string that starts with the prefix.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DurableEventFilter {
    pub session_id: Option<String>,
    pub kinds: Option<Vec<String>>,
    pub work_id_prefix: Option<String>,
}

impl DurableEventFilter {
    fn matches(&self, record: &DurableEventRecord) -> bool {
        self.session_id
            .as_ref()
            .is_none_or(|session_id| *session_id == record.session_id)
            && self
                .kinds
                .as_ref()
                .is_none_or(|kinds| kinds.contains(&record.kind))
            && self.work_id_prefix.as_ref().is_none_or(|prefix| {
                work_id_of(&record.payload)
                    .is_some_and(|work_id| work_id.starts_with(prefix.as_str()))
            })
    }
}

/// Errors of the durable event store.
///
/// `Corruption::offset` is the sequence of the damaged event, or 0 when the store as a whole
/// is damaged (for example a schema that does not match).
#[derive(Debug)]
pub enum DurableEventError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    Validation(String),
    Corruption { offset: u64, reason: String },
    ReadOnly(DurableEventCompatibility),
    IncompleteTail,
    LegacyLogPresent,
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
                    "durable event corruption at sequence {offset}: {reason}"
                )
            }
            Self::ReadOnly(compatibility) => {
                write!(
                    formatter,
                    "durable event store is not writable: {compatibility:?}"
                )
            }
            Self::IncompleteTail => write!(formatter, "durable event store has an incomplete tail"),
            Self::LegacyLogPresent => write!(
                formatter,
                "a legacy durable event log ({LEGACY_DURABLE_EVENT_LOG_FILE}) is present in the \
                 durable event directory; delete it or move it elsewhere, then start the runtime \
                 again"
            ),
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

impl From<rusqlite::Error> for DurableEventError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
                Self::Corruption {
                    offset: 0,
                    reason: error.to_string(),
                }
            }
            _ => Self::Io(std::io::Error::other(error)),
        }
    }
}

#[derive(Debug)]
pub enum DurableWorkStateError {
    Event(DurableEventError),
    Reducer(DurableWorkReducerError),
}

impl fmt::Display for DurableWorkStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Event(error) => error.fmt(formatter),
            Self::Reducer(error) => error.fmt(formatter),
        }
    }
}

impl Error for DurableWorkStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Event(error) => Some(error),
            Self::Reducer(error) => Some(error),
        }
    }
}

impl From<DurableEventError> for DurableWorkStateError {
    fn from(error: DurableEventError) -> Self {
        Self::Event(error)
    }
}

impl From<rusqlite::Error> for DurableWorkStateError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Event(error.into())
    }
}

impl From<std::io::Error> for DurableWorkStateError {
    fn from(error: std::io::Error) -> Self {
        Self::Event(error.into())
    }
}

#[derive(Debug)]
pub(crate) enum DurableEventAppendError {
    DefinitelyNotCommitted(DurableEventError),
    CommitUnknown(DurableEventError),
}

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
    pub(crate) fn into_error(self) -> DurableEventError {
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

impl From<rusqlite::Error> for DurableEventAppendError {
    fn from(error: rusqlite::Error) -> Self {
        Self::DefinitelyNotCommitted(error.into())
    }
}

/// Points of an append where a test can inject a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppendStage {
    /// The event row is inserted but the transaction is not committed.
    BeforeCommit,
    /// The transaction is committed but the caller has not been told.
    AfterCommit,
    /// The process cache is about to learn the new store identity.
    CacheRefresh,
}

/// What the `decide` of a decided append asks for, having seen the work state inside the
/// write transaction.
pub(crate) enum WorkDecision<T> {
    /// Insert this event; it is validated and normalized as `append` does.
    Append(Box<DurableEventInput>),
    /// Insert nothing; the caller gets the value.
    Settled(T),
}

pub(crate) enum DecidedAppend<T> {
    Appended(Box<DurableEventRecord>),
    Settled(T),
}

#[derive(Debug)]
pub(crate) enum DecidedAppendError<E> {
    /// `decide` refused; nothing was inserted.
    Decision(E),
    Append(DurableEventAppendError),
}

impl<E> From<DurableEventAppendError> for DecidedAppendError<E> {
    fn from(error: DurableEventAppendError) -> Self {
        Self::Append(error)
    }
}

impl<E> From<DurableEventError> for DecidedAppendError<E> {
    fn from(error: DurableEventError) -> Self {
        Self::Append(error.into())
    }
}

impl<E> From<std::io::Error> for DecidedAppendError<E> {
    fn from(error: std::io::Error) -> Self {
        Self::Append(error.into())
    }
}

impl<E> From<serde_json::Error> for DecidedAppendError<E> {
    fn from(error: serde_json::Error) -> Self {
        Self::Append(error.into())
    }
}

impl<E> From<rusqlite::Error> for DecidedAppendError<E> {
    fn from(error: rusqlite::Error) -> Self {
        Self::Append(error.into())
    }
}

/// What the `decide` of a decided append can read inside the write transaction that will
/// insert its event.
pub(crate) struct WorkAppendContext<'a> {
    reader: &'a EventReader<'a>,
    refreshed: &'a Refreshed,
    cache: &'a StoreCache,
    root: &'a Path,
    work: OnceCell<DurableWorkReplayState>,
    /// Set once reading or reducing stored events failed. The append then leaves this process
    /// nothing it verified, so that the next operation checks every event again.
    failed: Cell<bool>,
}

impl WorkAppendContext<'_> {
    /// The work state through the newest event, caught up inside this transaction.
    pub(crate) fn work(&self) -> Result<&DurableWorkReplayState, DurableWorkStateError> {
        if let Some(work) = self.work.get() {
            return Ok(work);
        }
        let work =
            self.noting_failure(work_state_at_head(self.reader, self.refreshed, self.cache))?;
        Ok(self.work.get_or_init(|| work))
    }

    /// The current event stored at `sequence`, checked like every read.
    pub(crate) fn event_at(
        &self,
        sequence: u64,
    ) -> Result<Option<DurableEventRecord>, DurableEventError> {
        Ok(self
            .noting_failure(self.reader.event_at(sequence))?
            .filter(|decoded| decoded.compatibility.is_current())
            .map(|decoded| decoded.record))
    }

    /// Bytes the store occupies on disk, as [`DurableEventStore::storage_bytes`].
    pub(crate) fn storage_bytes(&self) -> std::io::Result<u64> {
        storage_bytes_of(self.root)
    }

    fn noting_failure<T, E>(&self, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.failed.set(true);
        }
        result
    }

    fn into_work(self) -> Result<DurableWorkReplayState, DurableWorkStateError> {
        match self.work.into_inner() {
            Some(work) => Ok(work),
            None => work_state_at_head(self.reader, self.refreshed, self.cache),
        }
    }
}

#[derive(Debug)]
pub struct DurableEventStore {
    root: PathBuf,
    path: PathBuf,
    registry: DurableEventSchemaRegistry,
    compatibility: DurableEventCompatibility,
    process_state: Arc<Mutex<ProcessStoreState>>,
}

/// What this process knows about one store, shared by every handle to it.
#[derive(Debug, Default)]
struct ProcessStoreState {
    cache: StoreCache,
    connection: Option<StoreConnection>,
    /// Events this process has decoded from the store; never reset.
    decoded: Cell<u64>,
}

impl ProcessStoreState {
    fn reset(&mut self) {
        self.cache = StoreCache::default();
        self.connection = None;
    }

    /// The kept connection when it still belongs to the database file at `path` and allows
    /// `access`, otherwise a new one. The caller puts it back after a successful operation.
    fn take_connection(
        &mut self,
        path: &Path,
        access: Access,
    ) -> Result<StoreConnection, DurableEventError> {
        let file = file_identity(path)?;
        match self.connection.take() {
            Some(kept) if kept.file == file && kept.allows(access) => Ok(kept),
            _ => StoreConnection::open(path, access, file),
        }
    }

    /// A write connection is not kept for a store whose format is not admitted.
    fn put_back(&mut self, kept: StoreConnection, identity: &StoreIdentity) {
        if kept.access == Access::ReadOnly || identity.has_current_format() {
            self.connection = Some(kept);
        }
    }
}

impl Drop for ProcessStoreState {
    fn drop(&mut self) {
        if let Some(kept) = self.connection.take() {
            kept.close_emptying_wal();
        }
    }
}

#[derive(Debug, Default)]
struct StoreCache {
    verified: Option<VerifiedStore>,
    work_state: Option<CachedWorkState>,
}

impl StoreCache {
    fn publish(&mut self, refreshed: &Refreshed) {
        if !refreshed.reused {
            self.work_state = None;
        }
        self.verified = Some(refreshed.verified.clone());
    }
}

/// The connection this process keeps to one store between operations. Were the last connection
/// closed after every operation, SQLite would checkpoint and delete the WAL each time.
#[derive(Debug)]
struct StoreConnection {
    connection: Connection,
    access: Access,
    path: PathBuf,
    /// Device and inode of the database file, the same before and after the connection opened
    /// it.
    file: (u64, u64),
}

impl StoreConnection {
    /// A write connection gets the settings that change the database file only after it has
    /// itself read an admitted store version and schema.
    fn open(path: &Path, access: Access, file: (u64, u64)) -> Result<Self, DurableEventError> {
        let connection = open_connection(path, access)?;
        if file_identity(path)? != file {
            return Err(DurableEventError::Io(std::io::Error::other(
                "durable event database was replaced while it was being opened",
            )));
        }
        if access == Access::ReadWrite {
            let user_version = read_user_version(&connection)?;
            if user_version != i64::from(CURRENT_DURABLE_EVENT_FRAME_VERSION) {
                return Err(DurableEventError::ReadOnly(unsupported_format(
                    user_version,
                )));
            }
            verify_schema(&connection)?;
            configure_writes(&connection)?;
        }
        Ok(Self {
            connection,
            access,
            path: path.to_path_buf(),
            file,
        })
    }

    fn allows(&self, access: Access) -> bool {
        self.access == Access::ReadWrite || access == Access::ReadOnly
    }

    /// When nothing else uses the store, moves the WAL into the database and empties it, so
    /// that the next process does not rebuild its WAL index from a WAL that never resets. Never
    /// waits and never acts through a path: the checkpoint writes through this connection's
    /// open files, and only into the admitted store it opened.
    fn close_emptying_wal(self) {
        if self.access != Access::ReadWrite || !self.still_the_admitted_store() {
            return;
        }
        if self.connection.busy_timeout(Duration::ZERO).is_ok() {
            let _ = self
                .connection
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        }
    }

    fn still_the_admitted_store(&self) -> bool {
        let root = self.path.parent().unwrap_or(&self.path);
        cfg!(unix)
            && matches!(ensure_no_legacy_log(root), Ok(()))
            && matches!(regular_file_identity(&self.path), Ok(Some(file)) if file == self.file)
            && matches!(read_user_version(&self.connection), Ok(version)
                if version == i64::from(CURRENT_DURABLE_EVENT_FRAME_VERSION))
            && verify_schema(&self.connection).is_ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoreIdentity {
    device: u64,
    inode: u64,
    schema_version: i64,
    user_version: i64,
}

impl StoreIdentity {
    fn has_current_format(&self) -> bool {
        self.user_version == i64::from(CURRENT_DURABLE_EVENT_FRAME_VERSION)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoreHead {
    sequence: u64,
    checksum: String,
}

#[derive(Debug, Clone)]
struct VerifiedStore {
    identity: StoreIdentity,
    /// The registry that judged `compatibility`; another registry may judge the rows otherwise.
    registry: DurableEventSchemaRegistry,
    compatibility: DurableEventCompatibility,
    head: Option<StoreHead>,
}

/// A verification of the store, and whether it extended this process's earlier one. Only then
/// is the cached work state still a state of this store.
struct Refreshed {
    verified: VerifiedStore,
    reused: bool,
}

#[derive(Debug, Clone)]
struct CachedWorkState {
    state: DurableWorkReplayState,
    through: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Creation {
    CreateIfMissing,
    ExistingOnly,
}

impl DurableEventStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, DurableEventError> {
        Self::open_with_registry(root, DurableEventSchemaRegistry::default())
    }

    pub fn open_with_registry(
        root: impl AsRef<Path>,
        registry: DurableEventSchemaRegistry,
    ) -> Result<Self, DurableEventError> {
        Self::open_inner(root.as_ref(), registry, Creation::CreateIfMissing)?.ok_or_else(|| {
            DurableEventError::Io(std::io::Error::other("durable event store was not created"))
        })
    }

    /// Whether `root` holds any entry of a store: the database, a SQLite sidecar file or a
    /// legacy event log. Nothing is opened.
    pub fn entries_present(root: &Path) -> std::io::Result<bool> {
        for path in [
            root.join(DURABLE_EVENT_DATABASE_FILE),
            sidecar_path(root, "-wal"),
            sidecar_path(root, "-shm"),
            root.join(LEGACY_DURABLE_EVENT_LOG_FILE),
        ] {
            if entry_present(&path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Opens the store only when its database already exists; never creates files.
    pub fn open_existing(root: &Path) -> Result<Option<Self>, DurableEventError> {
        Self::open_inner(
            root,
            DurableEventSchemaRegistry::default(),
            Creation::ExistingOnly,
        )
    }

    fn open_inner(
        root: &Path,
        registry: DurableEventSchemaRegistry,
        creation: Creation,
    ) -> Result<Option<Self>, DurableEventError> {
        reject_symlink(root)?;
        match creation {
            Creation::CreateIfMissing => fs::create_dir_all(root)?,
            Creation::ExistingOnly => {
                if !entry_present(root)? {
                    return Ok(None);
                }
            }
        }
        reject_symlink(root)?;
        let root = fs::canonicalize(root)?;
        let path = root.join(DURABLE_EVENT_DATABASE_FILE);
        let process_state = process_state_for(&path)?;
        let mut state = lock(&process_state)?;
        ensure_no_legacy_log(&root)?;
        let files = inspect_store_files(&root)?;
        if !files.database {
            if files.sidecar {
                return Err(corruption(
                    0,
                    "SQLite sidecar files exist without the durable event database",
                ));
            }
            if creation == Creation::ExistingOnly {
                return Ok(None);
            }
            create_database(&root, &path)?;
            state.reset();
        }
        let mut kept = state.take_connection(&path, Access::ReadOnly)?;
        let file = kept.file;
        let transaction = kept
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let reader = EventReader::new(&transaction, &registry, &state.decoded);
        let refreshed = match refresh_verified(&reader, file, &state.cache) {
            Ok(refreshed) => refreshed,
            Err(error) => {
                state.reset();
                return Err(error);
            }
        };
        transaction.finish()?;
        state.cache.publish(&refreshed);
        let verified = refreshed.verified;
        state.put_back(kept, &verified.identity);
        if verified.identity.has_current_format() {
            restrict_permissions(&root)?;
        }
        drop(state);
        Ok(Some(Self {
            root,
            path,
            registry,
            compatibility: verified.compatibility,
            process_state,
        }))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The SQLite database file of this store.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn compatibility(&self) -> &DurableEventCompatibility {
        &self.compatibility
    }

    pub fn is_writable(&self) -> bool {
        self.compatibility.is_current()
    }

    /// Bytes the store occupies on disk: the database and its SQLite sidecar files.
    pub fn storage_bytes(&self) -> std::io::Result<u64> {
        storage_bytes_of(&self.root)
    }

    /// Events this process has decoded and checked from this store, by any of its handles,
    /// since the process first opened it while no other handle was open.
    pub fn decoded_event_count(&self) -> Result<u64, DurableEventError> {
        Ok(lock(&self.process_state)?.decoded.get())
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
        self.append_with_hook(input, |_stage| Ok(()))
    }

    pub(crate) fn append_with_hook(
        &mut self,
        input: DurableEventInput,
        hook: impl FnMut(AppendStage) -> std::io::Result<()>,
    ) -> Result<DurableEventRecord, DurableEventAppendError> {
        let appended = self.append_deciding_with_hook(
            |_context| Ok::<_, Infallible>(WorkDecision::<Infallible>::Append(Box::new(input))),
            hook,
        );
        match appended {
            Ok(DecidedAppend::Appended(record)) => Ok(*record),
            Ok(DecidedAppend::Settled(never)) => match never {},
            Err(DecidedAppendError::Decision(never)) => match never {},
            Err(DecidedAppendError::Append(error)) => Err(error),
        }
    }

    /// Appends the event `decide` asks for, or nothing, after `decide` has judged the work
    /// state caught up inside the same write transaction; see [`WorkAppendContext`].
    pub(crate) fn append_deciding<T, E>(
        &mut self,
        decide: impl FnOnce(&WorkAppendContext<'_>) -> Result<WorkDecision<T>, E>,
    ) -> Result<DecidedAppend<T>, DecidedAppendError<E>> {
        self.append_deciding_with_hook(decide, |_stage| Ok(()))
    }

    /// Appends inside one write transaction: verify the store, decide, build work state,
    /// validate, assign the next sequence, insert, commit. Caches change only after the commit.
    pub(crate) fn append_deciding_with_hook<T, E>(
        &mut self,
        decide: impl FnOnce(&WorkAppendContext<'_>) -> Result<WorkDecision<T>, E>,
        mut hook: impl FnMut(AppendStage) -> std::io::Result<()>,
    ) -> Result<DecidedAppend<T>, DecidedAppendError<E>> {
        if !self.compatibility.is_current() {
            return Err(DurableEventError::ReadOnly(self.compatibility.clone()).into());
        }
        let mut guard = lock(&self.process_state)?;
        let state = &mut *guard;
        ensure_no_legacy_log(&self.root)?;
        let mut kept = state.take_connection(&self.path, Access::ReadOnly)?;
        let user_version = read_user_version(&kept.connection)?;
        if user_version != i64::from(CURRENT_DURABLE_EVENT_FRAME_VERSION) {
            self.compatibility = unsupported_format(user_version);
            return Err(DurableEventError::ReadOnly(self.compatibility.clone()).into());
        }
        if !kept.allows(Access::ReadWrite) {
            let writer =
                StoreConnection::open(&self.path, Access::ReadWrite, file_identity(&self.path)?);
            kept = match writer {
                Ok(writer) => writer,
                Err(DurableEventError::ReadOnly(compatibility)) => {
                    self.compatibility = compatibility.clone();
                    return Err(DurableEventError::ReadOnly(compatibility).into());
                }
                Err(error) => return Err(error.into()),
            };
        }
        let appended = append_on(
            &mut kept,
            &mut state.cache,
            AppendScope {
                decoded: &state.decoded,
                registry: &self.registry,
                root: &self.root,
            },
            &mut self.compatibility,
            decide,
            &mut hook,
        );
        let format_admitted = !matches!(
            self.compatibility,
            DurableEventCompatibility::UnsupportedFrameVersion { .. }
        );
        match &appended {
            Err(DecidedAppendError::Append(DurableEventAppendError::CommitUnknown(_))) => {
                state.reset()
            }
            _ if format_admitted => state.connection = Some(kept),
            _ => {}
        }
        appended
    }

    /// Verifies every stored event and returns the first `max_records` current events.
    pub fn scan(&self, max_records: usize) -> Result<DurableEventScan, DurableEventError> {
        let mut guard = lock(&self.process_state)?;
        let state = &mut *guard;
        ensure_no_legacy_log(&self.root)?;
        let mut kept = state.take_connection(&self.path, Access::ReadOnly)?;
        let file = kept.file;
        let transaction = kept
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let reader = EventReader::new(&transaction, &self.registry, &state.decoded);
        let scanned = scan_store(&reader, file, max_records).and_then(|(scan, verified)| {
            let reused = reusable_verification(
                &transaction,
                &state.cache,
                &verified.identity,
                &verified.registry,
                verified.head.as_ref(),
            )?
            .is_some();
            Ok((scan, Refreshed { verified, reused }))
        });
        match scanned {
            Ok((scan, refreshed)) => {
                transaction.finish()?;
                state.put_back(kept, &refreshed.verified.identity);
                state.cache.publish(&refreshed);
                Ok(scan)
            }
            Err(error) => {
                state.reset();
                Err(error)
            }
        }
    }

    /// Visits every current event whose sequence is greater than `after_sequence`.
    pub fn visit_from_sequence(
        &self,
        after_sequence: u64,
        mut visitor: impl FnMut(&DurableEventRecord),
    ) -> Result<DurableEventVisitSummary, DurableEventError> {
        self.visit(|reader, verified| {
            visit_verified(reader, verified, after_sequence, &mut visitor)
        })
    }

    /// Visits, in sequence order and once each, every current event that one of `filters`
    /// selects. Only those events are read and checked; the rest stay as the verification of
    /// new events left them.
    pub fn visit_matching(
        &self,
        filters: &[DurableEventFilter],
        mut visitor: impl FnMut(&DurableEventRecord),
    ) -> Result<DurableEventVisitSummary, DurableEventError> {
        self.visit(|reader, verified| {
            visit_matching_verified(reader, verified, filters, &mut visitor)
        })
    }

    fn visit(
        &self,
        read: impl FnOnce(
            &EventReader<'_>,
            &VerifiedStore,
        ) -> Result<DurableEventVisitSummary, DurableEventError>,
    ) -> Result<DurableEventVisitSummary, DurableEventError> {
        let mut guard = lock(&self.process_state)?;
        let state = &mut *guard;
        ensure_no_legacy_log(&self.root)?;
        let mut kept = state.take_connection(&self.path, Access::ReadOnly)?;
        let file = kept.file;
        let transaction = kept
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let decoded_before = state.decoded.get();
        let reader = EventReader::new(&transaction, &self.registry, &state.decoded);
        let visited = refresh_verified(&reader, file, &state.cache).and_then(|refreshed| {
            let summary = read(&reader, &refreshed.verified)?;
            Ok((refreshed, summary))
        });
        match visited {
            Ok((refreshed, mut summary)) => {
                summary.decoded = state.decoded.get().saturating_sub(decoded_before);
                transaction.finish()?;
                state.cache.publish(&refreshed);
                state.put_back(kept, &refreshed.verified.identity);
                Ok(summary)
            }
            Err(error) => {
                state.reset();
                Err(error)
            }
        }
    }

    /// The durable work state through the newest event. When this process's cached work state
    /// is still a state of this store, only the work events after it are read; otherwise every
    /// work event is. Unsupported events are left out, as from every read.
    pub fn work_state(&self) -> Result<DurableWorkReplayState, DurableWorkStateError> {
        let mut guard = lock(&self.process_state)?;
        let state = &mut *guard;
        ensure_no_legacy_log(&self.root)?;
        let mut kept = state.take_connection(&self.path, Access::ReadOnly)?;
        let file = kept.file;
        let transaction = kept
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let reader = EventReader::new(&transaction, &self.registry, &state.decoded);
        let built = refresh_verified(&reader, file, &state.cache)
            .map_err(DurableWorkStateError::Event)
            .and_then(|refreshed| {
                let work = work_state_at_head(&reader, &refreshed, &state.cache)?;
                Ok((refreshed, work))
            });
        let (refreshed, work) = match built {
            Ok(built) => built,
            Err(error) => {
                state.reset();
                return Err(error);
            }
        };
        transaction.finish()?;
        state.cache.publish(&refreshed);
        let verified = &refreshed.verified;
        if verified.identity.has_current_format() {
            state.cache.work_state = Some(CachedWorkState {
                state: work.clone(),
                through: verified.head.as_ref().map(|head| head.sequence),
            });
        }
        state.put_back(kept, &verified.identity);
        Ok(work)
    }

    /// SHA-256 over the store version, the schema and every value of every table, taken in one
    /// read transaction. `None` when no database exists.
    pub fn logical_digest_existing(root: &Path) -> Result<Option<String>, DurableEventError> {
        if !entry_present(root)? {
            return Ok(None);
        }
        reject_symlink(root)?;
        let root = fs::canonicalize(root)?;
        ensure_no_legacy_log(&root)?;
        let files = inspect_store_files(&root)?;
        if !files.database {
            if files.sidecar {
                return Err(corruption(
                    0,
                    "SQLite sidecar files exist without the durable event database",
                ));
            }
            return Ok(None);
        }
        let path = root.join(DURABLE_EVENT_DATABASE_FILE);
        let mut reader = StoreConnection::open(&path, Access::ReadOnly, file_identity(&path)?)?;
        let transaction = reader
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred)?;
        let mut hasher = Sha256::new();
        hasher.update(b"shacs.durable_events.logical.v1\n");
        let user_version = read_user_version(&transaction)?;
        hash_field(&mut hasher, Some(user_version.to_string().as_bytes()));
        for (name, sql) in read_schema(&transaction)? {
            hash_field(&mut hasher, Some(name.as_bytes()));
            hash_field(&mut hasher, sql.as_deref().map(str::as_bytes));
        }
        hash_table_contents(&transaction, &mut hasher)?;
        transaction.finish()?;
        Ok(Some(format!("sha256:{:x}", hasher.finalize())))
    }
}

struct AppendScope<'a> {
    decoded: &'a Cell<u64>,
    registry: &'a DurableEventSchemaRegistry,
    root: &'a Path,
}

fn append_on<T, E>(
    kept: &mut StoreConnection,
    cache: &mut StoreCache,
    scope: AppendScope<'_>,
    compatibility: &mut DurableEventCompatibility,
    decide: impl FnOnce(&WorkAppendContext<'_>) -> Result<WorkDecision<T>, E>,
    hook: &mut impl FnMut(AppendStage) -> std::io::Result<()>,
) -> Result<DecidedAppend<T>, DecidedAppendError<E>> {
    let file = kept.file;
    let registry = scope.registry;
    let transaction = kept
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let reader = EventReader::new(&transaction, registry, scope.decoded);
    let refreshed = match refresh_verified(&reader, file, cache) {
        Ok(refreshed) => refreshed,
        Err(error) => {
            *cache = StoreCache::default();
            return Err(error.into());
        }
    };
    *compatibility = refreshed.verified.compatibility.clone();
    if !compatibility.is_current() {
        return Err(DurableEventError::ReadOnly(compatibility.clone()).into());
    }
    let previous = refreshed.verified.head.as_ref().map(|head| head.sequence);
    let context = WorkAppendContext {
        reader: &reader,
        refreshed: &refreshed,
        cache,
        root: scope.root,
        work: OnceCell::new(),
        failed: Cell::new(false),
    };
    let decided = decide(&context);
    let read_failed = context.failed.get();
    let mut input = match decided {
        Err(error) => {
            if read_failed {
                *cache = StoreCache::default();
            }
            return Err(DecidedAppendError::Decision(error));
        }
        Ok(WorkDecision::Settled(value)) => {
            let work = context.work.into_inner();
            transaction.finish()?;
            if read_failed {
                *cache = StoreCache::default();
            } else {
                cache.publish(&refreshed);
                if let Some(work) = work {
                    cache.work_state = Some(CachedWorkState {
                        state: work,
                        through: previous,
                    });
                }
            }
            return Ok(DecidedAppend::Settled(value));
        }
        Ok(WorkDecision::Append(input)) => *input,
    };
    validate_input(&input, registry)?;
    if let DurableEventPayload::Inline { data, .. } = &mut input.payload {
        *data = redact_value(data);
    }
    let sequence = previous.unwrap_or(0).checked_add(1).ok_or_else(|| {
        DurableEventError::Validation("durable event sequence is exhausted".to_owned())
    })?;
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
    let mut decided_work_state = None;
    if record.kind.starts_with("work.") {
        let mut work = match context.into_work() {
            Ok(work) => work,
            Err(error) => {
                *cache = StoreCache::default();
                return Err(match error {
                    DurableWorkStateError::Event(error) => error,
                    DurableWorkStateError::Reducer(error) => {
                        DurableEventError::Validation(error.to_string())
                    }
                }
                .into());
            }
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
    } else {
        decided_work_state = context.work.into_inner();
    }
    let record_bytes = canonical_record_bytes(&record)?;
    if record_bytes.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
        return Err(DurableEventError::Validation(format!(
            "event record exceeds {MAX_DURABLE_EVENT_FRAME_BYTES} bytes"
        ))
        .into());
    }
    let record_text = String::from_utf8(record_bytes).map_err(|error| {
        DurableEventError::Validation(format!("event record is not UTF-8: {error}"))
    })?;
    let record_checksum = checksum(record_text.as_bytes());
    transaction.execute(
        &format!(
            "INSERT INTO events ({EVENT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        ),
        params![
            sequence_to_sql(sequence)?,
            record.event_id,
            record.session_id,
            record.turn_id,
            record.kind,
            work_id_of(&record.payload),
            i64::from(record.schema_version),
            record.recorded_at,
            record_text,
            record_checksum,
        ],
    )?;
    if let Err(error) = hook(AppendStage::BeforeCommit) {
        return Err(
            DurableEventAppendError::DefinitelyNotCommitted(DurableEventError::Io(error)).into(),
        );
    }
    if let Err(error) = transaction.commit() {
        return Err(DurableEventAppendError::CommitUnknown(error.into()).into());
    }
    if let Err(error) = hook(AppendStage::AfterCommit) {
        return Err(DurableEventAppendError::CommitUnknown(DurableEventError::Io(error)).into());
    }
    let identity = hook(AppendStage::CacheRefresh)
        .map_err(DurableEventError::Io)
        .and_then(|()| read_identity(&kept.connection, file));
    match identity {
        Ok(identity) if !read_failed => {
            let work_state = match appended_work_state {
                Some(work) => Some(CachedWorkState {
                    state: work,
                    through: Some(sequence),
                }),
                None => decided_work_state
                    .map(|state| CachedWorkState {
                        state,
                        through: previous,
                    })
                    .or_else(|| {
                        cache
                            .work_state
                            .take()
                            .filter(|cached| refreshed.reused && cached.through <= previous)
                    })
                    .and_then(|cached| work_state_after(cached, previous, &record)),
            };
            cache.verified = Some(VerifiedStore {
                identity,
                registry: registry.clone(),
                compatibility: DurableEventCompatibility::Current,
                head: Some(StoreHead {
                    sequence,
                    checksum: record_checksum,
                }),
            });
            cache.work_state = work_state;
        }
        _ => *cache = StoreCache::default(),
    }
    Ok(DecidedAppend::Appended(Box::new(record)))
}

/// The cached work state once `record`, a kind other than `work.*`, follows `previous`. A
/// state from before `previous` stays as it is: a later read catches it up past `record`.
fn work_state_after(
    mut cached: CachedWorkState,
    previous: Option<u64>,
    record: &DurableEventRecord,
) -> Option<CachedWorkState> {
    if cached.through != previous {
        return Some(cached);
    }
    apply_persisted_durable_work_event(&mut cached.state, record).ok()?;
    cached.through = Some(record.sequence);
    Some(cached)
}

fn visit_verified(
    reader: &EventReader<'_>,
    verified: &VerifiedStore,
    after_sequence: u64,
    visitor: &mut impl FnMut(&DurableEventRecord),
) -> Result<DurableEventVisitSummary, DurableEventError> {
    let mut summary = visit_summary(verified);
    if !verified.identity.has_current_format() {
        return Ok(summary);
    }
    let mut expected = after_sequence.saturating_add(1);
    reader.each_after(after_sequence, |decoded| {
        ensure_next_sequence(expected, decoded.record.sequence)?;
        expected = expected.saturating_add(1);
        summary.bytes_scanned = summary.bytes_scanned.saturating_add(decoded.bytes);
        if decoded.compatibility.is_current() {
            visitor(&decoded.record);
            summary.visited = summary.visited.saturating_add(1);
        }
        Ok(())
    })?;
    Ok(summary)
}

fn visit_matching_verified(
    reader: &EventReader<'_>,
    verified: &VerifiedStore,
    filters: &[DurableEventFilter],
    visitor: &mut impl FnMut(&DurableEventRecord),
) -> Result<DurableEventVisitSummary, DurableEventError> {
    let mut summary = visit_summary(verified);
    if !verified.identity.has_current_format() {
        return Ok(summary);
    }
    reader.each_matching(filters, 0, |decoded| {
        summary.bytes_scanned = summary.bytes_scanned.saturating_add(decoded.bytes);
        if decoded.compatibility.is_current()
            && filters.iter().any(|filter| filter.matches(&decoded.record))
        {
            visitor(&decoded.record);
            summary.visited = summary.visited.saturating_add(1);
        }
        Ok(())
    })?;
    Ok(summary)
}

fn visit_summary(verified: &VerifiedStore) -> DurableEventVisitSummary {
    DurableEventVisitSummary {
        compatibility: verified.compatibility.clone(),
        incomplete_tail: false,
        last_sequence: verified.head.as_ref().map(|head| head.sequence),
        visited: 0,
        bytes_scanned: 0,
        decoded: 0,
    }
}

struct StoreFiles {
    database: bool,
    sidecar: bool,
}

struct StoredEvent {
    sequence: i64,
    event_id: String,
    session_id: String,
    turn_id: Option<String>,
    kind: String,
    work_id: Option<String>,
    schema_version: i64,
    recorded_at: String,
    record: String,
    checksum: String,
}

impl StoredEvent {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            sequence: row.get(0)?,
            event_id: row.get(1)?,
            session_id: row.get(2)?,
            turn_id: row.get(3)?,
            kind: row.get(4)?,
            work_id: row.get(5)?,
            schema_version: row.get(6)?,
            recorded_at: row.get(7)?,
            record: row.get(8)?,
            checksum: row.get(9)?,
        })
    }
}

struct DecodedEvent {
    record: DurableEventRecord,
    compatibility: DurableEventCompatibility,
    checksum: String,
    bytes: u64,
}

fn scan_store(
    reader: &EventReader<'_>,
    file: (u64, u64),
    max_records: usize,
) -> Result<(DurableEventScan, VerifiedStore), DurableEventError> {
    let (connection, registry) = (reader.connection, reader.registry);
    let identity = read_identity(connection, file)?;
    if !identity.has_current_format() {
        let compatibility = unsupported_format(identity.user_version);
        let scan = DurableEventScan {
            records: Vec::new(),
            compatibility: compatibility.clone(),
            incomplete_tail: false,
            truncated: false,
            last_sequence: None,
            bytes_scanned: 0,
        };
        let verified = VerifiedStore {
            identity,
            registry: registry.clone(),
            compatibility,
            head: None,
        };
        return Ok((scan, verified));
    }
    verify_schema(connection)?;
    let mut records = Vec::new();
    let mut compatibility = DurableEventCompatibility::Current;
    let mut truncated = false;
    let mut bytes_scanned = 0_u64;
    let mut head = None;
    let mut expected = 1_u64;
    reader.each_after(0, |decoded| {
        ensure_next_sequence(expected, decoded.record.sequence)?;
        expected = expected.saturating_add(1);
        bytes_scanned = bytes_scanned.saturating_add(decoded.bytes);
        if compatibility.is_current() && !decoded.compatibility.is_current() {
            compatibility = decoded.compatibility.clone();
        }
        head = Some(StoreHead {
            sequence: decoded.record.sequence,
            checksum: decoded.checksum,
        });
        if decoded.compatibility.is_current() {
            if records.len() < max_records {
                records.push(decoded.record);
            } else {
                truncated = true;
            }
        }
        Ok(())
    })?;
    let scan = DurableEventScan {
        records,
        compatibility: compatibility.clone(),
        incomplete_tail: false,
        truncated,
        last_sequence: head.as_ref().map(|head| head.sequence),
        bytes_scanned,
    };
    let verified = VerifiedStore {
        identity,
        registry: registry.clone(),
        compatibility,
        head,
    };
    Ok((scan, verified))
}

/// Verifies only the events after this process's earlier verification when that one can be
/// reused (see `reusable_verification`), and every event otherwise. Changes nothing: the caller
/// decides when the result becomes this process's cache.
fn refresh_verified(
    reader: &EventReader<'_>,
    file: (u64, u64),
    cache: &StoreCache,
) -> Result<Refreshed, DurableEventError> {
    let (connection, registry) = (reader.connection, reader.registry);
    let identity = read_identity(connection, file)?;
    if !identity.has_current_format() {
        return Ok(Refreshed {
            verified: VerifiedStore {
                compatibility: unsupported_format(identity.user_version),
                identity,
                registry: registry.clone(),
                head: None,
            },
            reused: false,
        });
    }
    verify_schema(connection)?;
    let current_head = read_head(connection)?;
    let reusable = reusable_verification(
        connection,
        cache,
        &identity,
        registry,
        current_head.as_ref(),
    )?;
    let reused = reusable.is_some();
    let (start, mut compatibility) = match reusable {
        Some(cached) => (cached.head.clone(), cached.compatibility.clone()),
        None => (None, DurableEventCompatibility::Current),
    };
    let mut head = start.clone();
    let mut expected = start
        .as_ref()
        .map_or(1, |head| head.sequence.saturating_add(1));
    let after = start.as_ref().map_or(0, |head| head.sequence);
    reader.each_after(after, |decoded| {
        ensure_next_sequence(expected, decoded.record.sequence)?;
        expected = expected.saturating_add(1);
        if compatibility.is_current() && !decoded.compatibility.is_current() {
            compatibility = decoded.compatibility.clone();
        }
        head = Some(StoreHead {
            sequence: decoded.record.sequence,
            checksum: decoded.checksum,
        });
        Ok(())
    })?;
    Ok(Refreshed {
        verified: VerifiedStore {
            identity,
            registry: registry.clone(),
            compatibility,
            head,
        },
        reused,
    })
}

/// This process's earlier verification, when the store is the same file with the same format
/// and schema, was judged by the same registry, and still holds the verified head row
/// unchanged: events were then only appended since.
fn reusable_verification<'a>(
    connection: &Connection,
    cache: &'a StoreCache,
    identity: &StoreIdentity,
    registry: &DurableEventSchemaRegistry,
    current_head: Option<&StoreHead>,
) -> Result<Option<&'a VerifiedStore>, DurableEventError> {
    match cache.verified.as_ref() {
        Some(cached) if cached.identity == *identity && cached.registry == *registry => {
            Ok(head_extends(connection, cached.head.as_ref(), current_head)?.then_some(cached))
        }
        _ => Ok(None),
    }
}

fn head_extends(
    connection: &Connection,
    cached: Option<&StoreHead>,
    current: Option<&StoreHead>,
) -> Result<bool, DurableEventError> {
    let Some(cached) = cached else {
        return Ok(true);
    };
    let Some(current) = current else {
        return Ok(false);
    };
    if current.sequence < cached.sequence {
        return Ok(false);
    }
    let stored_checksum: Option<String> = connection
        .query_row(
            "SELECT checksum FROM events WHERE sequence = ?1",
            params![sequence_to_sql(cached.sequence)?],
            |row| row.get(0),
        )
        .optional()?;
    Ok(stored_checksum.as_deref() == Some(cached.checksum.as_str()))
}

fn read_head(connection: &Connection) -> Result<Option<StoreHead>, DurableEventError> {
    let head: Option<(i64, String)> = connection
        .query_row(
            "SELECT sequence, checksum FROM events ORDER BY sequence DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match head {
        Some((sequence, checksum)) => Ok(Some(StoreHead {
            sequence: sequence_from_sql(sequence)?,
            checksum,
        })),
        None => Ok(None),
    }
}

/// The work state through the verified head: this process's cached state caught up with the
/// work events after it while it is still a state of this store, otherwise one built from every
/// work event.
fn work_state_at_head(
    reader: &EventReader<'_>,
    refreshed: &Refreshed,
    cache: &StoreCache,
) -> Result<DurableWorkReplayState, DurableWorkStateError> {
    let verified = &refreshed.verified;
    if !verified.identity.has_current_format() {
        return Ok(DurableWorkReplayState::default());
    }
    let head = verified.head.as_ref().map(|head| head.sequence);
    let cached = cache
        .work_state
        .as_ref()
        .filter(|cached| refreshed.reused && cached.through <= head);
    let (mut work, after) = match cached {
        Some(cached) => (cached.state.clone(), cached.through.unwrap_or(0)),
        None => (DurableWorkReplayState::default(), 0),
    };
    if after == head.unwrap_or(0) {
        return Ok(work);
    }
    let work_events = [DurableEventFilter {
        kinds: Some(WORK_STATE_KINDS.map(str::to_owned).to_vec()),
        ..DurableEventFilter::default()
    }];
    let mut reducer_error = None;
    reader.each_matching(&work_events, after, |decoded| {
        if reducer_error.is_none() && decoded.compatibility.is_current() {
            reducer_error = apply_persisted_durable_work_event(&mut work, &decoded.record).err();
        }
        Ok(())
    })?;
    match reducer_error {
        Some(error) => Err(DurableWorkStateError::Reducer(error)),
        None => Ok(work),
    }
}

/// Reads and checks the stored events of one transaction, counting each event it decodes in
/// the process state of the store.
struct EventReader<'a> {
    connection: &'a Connection,
    registry: &'a DurableEventSchemaRegistry,
    decoded: &'a Cell<u64>,
}

impl<'a> EventReader<'a> {
    fn new(
        connection: &'a Connection,
        registry: &'a DurableEventSchemaRegistry,
        decoded: &'a Cell<u64>,
    ) -> Self {
        Self {
            connection,
            registry,
            decoded,
        }
    }

    fn decode(&self, stored: StoredEvent) -> Result<DecodedEvent, DurableEventError> {
        self.decoded.set(self.decoded.get().saturating_add(1));
        decode_event(stored, self.registry)
    }

    fn each_after(
        &self,
        after_sequence: u64,
        mut each: impl FnMut(DecodedEvent) -> Result<(), DurableEventError>,
    ) -> Result<(), DurableEventError> {
        for_each_event_after(self.connection, after_sequence, |stored| {
            each(self.decode(stored)?)
        })
    }

    /// Events after `after_sequence` that one of `filters` selects, in sequence order and once
    /// each. Each filter is one `SELECT sequence` that SQLite answers from an index. From 0 no
    /// lower bound applies, so that a selected row whose sequence is below 1 is found as
    /// corruption.
    fn each_matching(
        &self,
        filters: &[DurableEventFilter],
        after_sequence: u64,
        mut each: impl FnMut(DecodedEvent) -> Result<(), DurableEventError>,
    ) -> Result<(), DurableEventError> {
        if filters.is_empty() {
            return Ok(());
        }
        let after = match after_sequence {
            0 => None,
            after => match i64::try_from(after) {
                Ok(after) => Some(after),
                Err(_) => return Ok(()),
            },
        };
        let mut values = Vec::new();
        let selects = filters
            .iter()
            .map(|filter| filter_select(filter, after, &mut values))
            .collect::<Vec<_>>()
            .join(" UNION ");
        let mut statement = self.connection.prepare(&format!(
            "SELECT {EVENT_COLUMNS} FROM events WHERE sequence IN ({selects}) ORDER BY sequence"
        ))?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            each(self.decode(StoredEvent::from_row(row)?)?)?;
        }
        Ok(())
    }

    fn event_at(&self, sequence: u64) -> Result<Option<DecodedEvent>, DurableEventError> {
        let stored = self
            .connection
            .query_row(
                &format!("SELECT {EVENT_COLUMNS} FROM events WHERE sequence = ?1"),
                params![sequence_to_sql(sequence)?],
                StoredEvent::from_row,
            )
            .optional()?;
        stored.map(|stored| self.decode(stored)).transpose()
    }
}

fn filter_select(
    filter: &DurableEventFilter,
    after: Option<i64>,
    values: &mut Vec<rusqlite::types::Value>,
) -> String {
    let mut bind = |value: rusqlite::types::Value| {
        values.push(value);
        format!("?{}", values.len())
    };
    let mut conditions = Vec::new();
    if let Some(after) = after {
        conditions.push(format!("sequence > {}", bind(after.into())));
    }
    if let Some(session_id) = &filter.session_id {
        conditions.push(format!("session_id = {}", bind(session_id.clone().into())));
    }
    if let Some(kinds) = &filter.kinds {
        let kinds: Vec<String> = kinds.iter().map(|kind| bind(kind.clone().into())).collect();
        conditions.push(if kinds.is_empty() {
            "0".to_owned()
        } else {
            format!("kind IN ({})", kinds.join(", "))
        });
    }
    if let Some(prefix) = &filter.work_id_prefix {
        conditions.push(format!("work_id >= {}", bind(prefix.clone().into())));
        if let Some(bound) = prefix_upper_bound(prefix) {
            conditions.push(format!("work_id < {}", bind(bound.into())));
        }
    }
    if conditions.is_empty() {
        "SELECT sequence FROM events".to_owned()
    } else {
        format!(
            "SELECT sequence FROM events WHERE {}",
            conditions.join(" AND ")
        )
    }
}

/// The least string above every string that starts with `prefix`, in the byte order SQLite
/// compares text by, or `None` when no string is. UTF-8 orders strings by their code points, so
/// the bound replaces the last character that has a successor by that successor and drops the
/// rest.
fn prefix_upper_bound(prefix: &str) -> Option<String> {
    let mut characters: Vec<char> = prefix.chars().collect();
    while let Some(last) = characters.pop() {
        let next = match last {
            '\u{D7FF}' => Some('\u{E000}'),
            _ => char::from_u32(u32::from(last) + 1),
        };
        if let Some(next) = next {
            characters.push(next);
            return Some(characters.into_iter().collect());
        }
    }
    None
}

/// From 0 every row is read, so that a row whose sequence is below 1 is found as corruption.
fn for_each_event_after(
    connection: &Connection,
    after_sequence: u64,
    mut each: impl FnMut(StoredEvent) -> Result<(), DurableEventError>,
) -> Result<(), DurableEventError> {
    let first = if after_sequence == 0 {
        i64::MIN
    } else {
        match i64::try_from(after_sequence)
            .ok()
            .and_then(|after| after.checked_add(1))
        {
            Some(first) => first,
            None => return Ok(()),
        }
    };
    let mut statement = connection.prepare(&format!(
        "SELECT {EVENT_COLUMNS} FROM events WHERE sequence >= ?1 ORDER BY sequence"
    ))?;
    let mut rows = statement.query(params![first])?;
    while let Some(row) = rows.next()? {
        each(StoredEvent::from_row(row)?)?;
    }
    Ok(())
}

/// Checks one stored event: the checksum of the stored record, its canonical form, and that
/// every indexed column equals the value taken from the record.
fn decode_event(
    stored: StoredEvent,
    registry: &DurableEventSchemaRegistry,
) -> Result<DecodedEvent, DurableEventError> {
    let sequence = sequence_from_sql(stored.sequence)?;
    if stored.record.len() > MAX_DURABLE_EVENT_FRAME_BYTES {
        return Err(corruption(sequence, "stored record exceeds maximum size"));
    }
    if checksum(stored.record.as_bytes()) != stored.checksum {
        return Err(corruption(sequence, "checksum mismatch"));
    }
    let record: DurableEventRecord = serde_json::from_str(&stored.record)
        .map_err(|error| corruption(sequence, format!("invalid stored record: {error}")))?;
    if canonical_record_bytes(&record)? != stored.record.as_bytes() {
        return Err(corruption(sequence, "stored record is not canonical"));
    }
    let columns_match = record.sequence == sequence
        && record.event_id == stored.event_id
        && record.session_id == stored.session_id
        && record.turn_id == stored.turn_id
        && record.kind == stored.kind
        && i64::from(record.schema_version) == stored.schema_version
        && record.recorded_at == stored.recorded_at
        && work_id_of(&record.payload) == stored.work_id.as_deref();
    if !columns_match {
        return Err(corruption(
            sequence,
            "indexed columns do not match the stored record",
        ));
    }
    let compatibility = registry.compatibility(&record.kind, record.schema_version);
    Ok(DecodedEvent {
        record,
        compatibility,
        checksum: stored.checksum,
        bytes: stored.record.len() as u64,
    })
}

fn ensure_next_sequence(expected: u64, found: u64) -> Result<(), DurableEventError> {
    if found == expected {
        Ok(())
    } else {
        Err(corruption(
            found,
            format!("expected sequence {expected}, found {found}"),
        ))
    }
}

/// Every value of every row of every stored table but SQLite's own, tables in name order and
/// rows in key order, so that the digest follows the content whatever the store version.
fn hash_table_contents(
    connection: &Connection,
    hasher: &mut Sha256,
) -> Result<(), DurableEventError> {
    let mut statement = connection.prepare(
        "SELECT name, wr FROM pragma_table_list WHERE schema = 'main' \
         AND type IN ('table', 'shadow') AND name NOT GLOB 'sqlite_*' ORDER BY name",
    )?;
    let tables: Vec<(String, bool)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (table, without_rowid) in tables {
        hash_field(hasher, Some(table.as_bytes()));
        let order = row_order(connection, &table, without_rowid)?;
        let mut rows_statement = connection.prepare(&format!(
            "SELECT * FROM {} ORDER BY {order}",
            quoted_identifier(&table)
        ))?;
        let columns = rows_statement.column_count();
        let mut rows = rows_statement.query([])?;
        while let Some(row) = rows.next()? {
            for index in 0..columns {
                hash_value(hasher, row.get_ref(index)?);
            }
        }
    }
    Ok(())
}

/// A total order of the table's rows: the rowid, under a name no column hides, or the primary
/// key of a `WITHOUT ROWID` table.
fn row_order(
    connection: &Connection,
    table: &str,
    without_rowid: bool,
) -> Result<String, DurableEventError> {
    let mut statement = connection.prepare("SELECT name, pk FROM pragma_table_info(?1)")?;
    let mut columns: Vec<(String, i64)> = statement
        .query_map([table], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let order = if without_rowid {
        columns.retain(|(_, key_position)| *key_position > 0);
        columns.sort_by_key(|(_, key_position)| *key_position);
        let key: Vec<String> = columns
            .iter()
            .map(|(name, _)| quoted_identifier(name))
            .collect();
        (!key.is_empty()).then(|| key.join(", "))
    } else {
        ["rowid", "_rowid_", "oid"]
            .into_iter()
            .find(|alias| {
                !columns
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(alias))
            })
            .map(str::to_owned)
    };
    order.ok_or_else(|| corruption(0, format!("table {table} has no key to order its rows by")))
}

fn quoted_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn hash_value(hasher: &mut Sha256, value: ValueRef<'_>) {
    match value {
        ValueRef::Null => hasher.update([0_u8]),
        ValueRef::Integer(integer) => {
            hasher.update([1_u8]);
            hasher.update(integer.to_be_bytes());
        }
        ValueRef::Real(real) => {
            hasher.update([2_u8]);
            hasher.update(real.to_bits().to_be_bytes());
        }
        ValueRef::Text(text) => hash_tagged_bytes(hasher, 3, text),
        ValueRef::Blob(blob) => hash_tagged_bytes(hasher, 4, blob),
    }
}

fn hash_tagged_bytes(hasher: &mut Sha256, tag: u8, bytes: &[u8]) {
    hasher.update([tag]);
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn hash_field(hasher: &mut Sha256, value: Option<&[u8]>) {
    match value {
        Some(bytes) => {
            hasher.update([1_u8]);
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
        None => hasher.update([0_u8]),
    }
}

fn work_id_of(payload: &DurableEventPayload) -> Option<&str> {
    match payload {
        DurableEventPayload::Inline { data, .. } => data.get("work_id").and_then(Value::as_str),
        DurableEventPayload::Artifact { .. } => None,
    }
}

/// `path` must be canonical: `SQLITE_OPEN_NOFOLLOW` refuses a symbolic link in any component.
fn open_connection(path: &Path, access: Access) -> Result<Connection, DurableEventError> {
    let mode = match access {
        Access::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
        Access::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
    };
    let connection = Connection::open_with_flags(
        path,
        mode | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    // SQLite's own close-time checkpoint deletes the WAL by path, which may by then belong to
    // a replaced database. It stays off for the life of every connection; see
    // `StoreConnection::close_emptying_wal`.
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    #[cfg(test)]
    {
        if access == Access::ReadWrite {
            tests::record_write_connection();
        }
    }
    Ok(connection)
}

/// Settings that change the database file. Only for a store whose format was admitted.
fn configure_writes(connection: &Connection) -> Result<(), DurableEventError> {
    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(DurableEventError::Io(std::io::Error::other(format!(
            "durable event store journal mode is {journal_mode}, not wal"
        ))));
    }
    connection.pragma_update(None, "synchronous", "FULL")?;
    if cfg!(target_vendor = "apple") {
        connection.pragma_update(None, "fullfsync", true)?;
        connection.pragma_update(None, "checkpoint_fullfsync", true)?;
    }
    Ok(())
}

fn initialize_schema(connection: &mut Connection) -> Result<(), DurableEventError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for (_, sql) in SCHEMA {
        transaction.execute_batch(sql)?;
    }
    transaction.pragma_update(
        None,
        "user_version",
        i64::from(CURRENT_DURABLE_EVENT_FRAME_VERSION),
    )?;
    transaction.commit()?;
    Ok(())
}

fn read_user_version(connection: &Connection) -> Result<i64, DurableEventError> {
    Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

fn read_identity(
    connection: &Connection,
    (device, inode): (u64, u64),
) -> Result<StoreIdentity, DurableEventError> {
    let schema_version: i64 =
        connection.pragma_query_value(None, "schema_version", |row| row.get(0))?;
    Ok(StoreIdentity {
        device,
        inode,
        schema_version,
        user_version: read_user_version(connection)?,
    })
}

fn read_schema(
    connection: &Connection,
) -> Result<Vec<(String, Option<String>)>, DurableEventError> {
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let schema: Vec<(String, Option<String>)> = rows.collect::<Result<_, _>>()?;
    Ok(schema)
}

fn verify_schema(connection: &Connection) -> Result<(), DurableEventError> {
    let actual = read_schema(connection)?;
    let matches = actual.len() == SCHEMA.len()
        && actual
            .iter()
            .zip(SCHEMA)
            .all(|((name, sql), (expected_name, expected_sql))| {
                name == expected_name
                    && sql.as_deref().map(normalize_sql) == Some(normalize_sql(expected_sql))
            });
    if matches {
        Ok(())
    } else {
        Err(corruption(
            0,
            "durable event store schema does not match the expected definition",
        ))
    }
}

fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn unsupported_format(user_version: i64) -> DurableEventCompatibility {
    DurableEventCompatibility::UnsupportedFrameVersion {
        found: u32::try_from(user_version).unwrap_or(u32::MAX),
    }
}

fn sequence_to_sql(sequence: u64) -> Result<i64, DurableEventError> {
    i64::try_from(sequence).map_err(|_| {
        DurableEventError::Validation(format!("sequence {sequence} exceeds the store range"))
    })
}

fn sequence_from_sql(sequence: i64) -> Result<u64, DurableEventError> {
    match u64::try_from(sequence) {
        Ok(sequence) if sequence > 0 => Ok(sequence),
        _ => Err(corruption(
            0,
            format!("stored sequence {sequence} is not positive"),
        )),
    }
}

fn file_identity(path: &Path) -> std::io::Result<(u64, u64)> {
    Ok(identity_of(&fs::symlink_metadata(path)?))
}

fn regular_file_identity(path: &Path) -> std::io::Result<Option<(u64, u64)>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(identity_of(&metadata))),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn identity_of(metadata: &fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        (0, 0)
    }
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

fn ensure_no_legacy_log(root: &Path) -> Result<(), DurableEventError> {
    if entry_present(&root.join(LEGACY_DURABLE_EVENT_LOG_FILE))? {
        Err(DurableEventError::LegacyLogPresent)
    } else {
        Ok(())
    }
}

fn inspect_store_files(root: &Path) -> Result<StoreFiles, DurableEventError> {
    let database = regular_file_present(&root.join(DURABLE_EVENT_DATABASE_FILE))?;
    let wal = regular_file_present(&sidecar_path(root, "-wal"))?;
    let shm = regular_file_present(&sidecar_path(root, "-shm"))?;
    Ok(StoreFiles {
        database,
        sidecar: wal || shm,
    })
}

fn sidecar_path(root: &Path, suffix: &str) -> PathBuf {
    root.join(format!("{DURABLE_EVENT_DATABASE_FILE}{suffix}"))
}

fn storage_bytes_of(root: &Path) -> std::io::Result<u64> {
    let mut total = 0_u64;
    for path in [
        root.join(DURABLE_EVENT_DATABASE_FILE),
        sidecar_path(root, "-wal"),
        sidecar_path(root, "-shm"),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) => total = total.saturating_add(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(total)
}

fn entry_present(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn regular_file_present(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("durable event path is a symlink: {}", path.display()),
        )),
        Ok(metadata) if !metadata.is_file() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "durable event path is not a regular file: {}",
                path.display()
            ),
        )),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Builds the database complete under a staging name and links it into place, so that no
/// process ever finds a half-made database at the path; an empty database found there was
/// never made by this store. When another process links its database first, that one stays.
fn create_database(root: &Path, path: &Path) -> Result<(), DurableEventError> {
    static STAGED: AtomicU64 = AtomicU64::new(0);
    let staging = root.join(format!(
        ".{DURABLE_EVENT_DATABASE_FILE}.creating-{}-{}",
        std::process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    ));
    let linked =
        build_staged_database(&staging).and_then(|()| match fs::hard_link(&staging, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(DurableEventError::Io(error)),
        });
    let removed = remove_if_present(&staging);
    linked?;
    removed?;
    Ok(sync_dir(root)?)
}

fn build_staged_database(staging: &Path) -> Result<(), DurableEventError> {
    // Same process id and counter: left by a process that has since died.
    let mut journal = staging.as_os_str().to_owned();
    journal.push("-journal");
    remove_if_present(Path::new(&journal))?;
    remove_if_present(staging)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    drop(options.open(staging)?);
    let mut connection = Connection::open_with_flags(
        staging,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    initialize_schema(&mut connection)?;
    connection.close().map_err(|(_, error)| error)?;
    OpenOptions::new().read(true).open(staging)?.sync_all()?;
    Ok(())
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Changed by path, never through a descriptor of our own: closing a second descriptor of the
/// database would release the SQLite locks this process holds on it.
fn restrict_permissions(root: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        for path in [
            root.join(DURABLE_EVENT_DATABASE_FILE),
            sidecar_path(root, "-wal"),
            sidecar_path(root, "-shm"),
        ] {
            match fs::symlink_metadata(&path) {
                Ok(metadata)
                    if metadata.is_file() && metadata.permissions().mode() & 0o777 != 0o600 =>
                {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
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

fn process_state_for(path: &Path) -> Result<Arc<Mutex<ProcessStoreState>>, DurableEventError> {
    static STATES: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<ProcessStoreState>>>>> =
        OnceLock::new();
    let states = STATES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut states = lock(states)?;
    if let Some(state) = states.get(path).and_then(Weak::upgrade) {
        return Ok(state);
    }
    states.retain(|_, state| state.strong_count() > 0);
    let state = Arc::new(Mutex::new(ProcessStoreState::default()));
    states.insert(path.to_path_buf(), Arc::downgrade(&state));
    Ok(state)
}

fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, DurableEventError> {
    mutex.lock().map_err(|_| {
        DurableEventError::Io(std::io::Error::other(
            "durable event store lock poisoned by a panicking thread",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    thread_local! {
        static WRITE_CONNECTIONS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn record_write_connection() {
        WRITE_CONNECTIONS.with(|count| count.set(count.get().saturating_add(1)));
    }

    fn write_connections() -> usize {
        WRITE_CONNECTIONS.with(Cell::get)
    }

    fn input(message: &str) -> DurableEventInput {
        DurableEventInput::new(
            "session-1",
            SESSION_TURN_ACCEPTED,
            DurableEventPayload::inline("turn_fact", json!({"message": message})),
        )
    }

    #[test]
    fn append_failure_injection_distinguishes_uncommitted_and_unknown_commits(
    ) -> Result<(), Box<dyn Error>> {
        let uncommitted_root = tempfile::tempdir()?;
        let mut uncommitted = DurableEventStore::open(uncommitted_root.path())?;
        uncommitted.append(input("committed"))?;
        let error = uncommitted
            .append_with_hook(input("not-committed"), |stage| {
                if stage == AppendStage::BeforeCommit {
                    Err(std::io::Error::other("injected before commit"))
                } else {
                    Ok(())
                }
            })
            .err()
            .ok_or("expected injected failure before commit")?;
        assert!(matches!(
            error,
            DurableEventAppendError::DefinitelyNotCommitted(DurableEventError::Io(_))
        ));
        drop(uncommitted);
        let uncommitted = DurableEventStore::open(uncommitted_root.path())?;
        let scan = uncommitted.scan(10)?;
        assert_eq!(scan.records.len(), 1);
        assert!(!scan.incomplete_tail);
        assert!(uncommitted.is_writable());

        let unknown_root = tempfile::tempdir()?;
        let mut unknown = DurableEventStore::open(unknown_root.path())?;
        let error = unknown
            .append_with_hook(input("commit-unknown"), |stage| {
                if stage == AppendStage::AfterCommit {
                    Err(std::io::Error::other("injected after commit"))
                } else {
                    Ok(())
                }
            })
            .err()
            .ok_or("expected commit to remain unknown")?;
        assert!(matches!(
            error,
            DurableEventAppendError::CommitUnknown(DurableEventError::Io(_))
        ));
        drop(unknown);
        let unknown = DurableEventStore::open(unknown_root.path())?;
        let scan = unknown.scan(10)?;
        assert_eq!(scan.records.len(), 1);
        assert!(unknown.is_writable());
        Ok(())
    }

    #[test]
    fn the_work_reducer_acts_on_exactly_the_work_state_kinds() {
        let registry = DurableEventSchemaRegistry::default();
        for kind in WORK_STATE_KINDS {
            assert!(registry.kinds.contains_key(kind), "{kind}");
        }
        for (sequence, kind) in (1..).zip(registry.kinds.keys()) {
            let record = DurableEventRecord {
                schema_version: CURRENT_DURABLE_EVENT_SCHEMA_VERSION,
                event_id: format!("event-{sequence:020}"),
                sequence,
                session_id: "session-1".to_owned(),
                turn_id: None,
                causation_id: None,
                correlation_id: None,
                kind: kind.clone(),
                payload: DurableEventPayload::inline("durable_work", json!({"work_id": "work-1"})),
                provenance: None,
                recorded_at: "2026-11-01T00:00:00.000Z".to_owned(),
            };
            let mut state = DurableWorkReplayState::default();

            let applied = apply_persisted_durable_work_event(&mut state, &record);

            assert_eq!(
                !matches!(applied, Ok(false)),
                WORK_STATE_KINDS.contains(&kind.as_str()),
                "{kind}: {applied:?}"
            );
            if matches!(applied, Ok(false)) {
                assert_eq!(state, DurableWorkReplayState::default(), "{kind}");
            }
        }
    }

    #[test]
    fn a_prefix_bound_is_the_least_string_above_every_string_with_the_prefix() {
        for (prefix, bound) in [
            ("", None),
            ("web-watch:a:", Some("web-watch:a;")),
            ("가", Some("각")),
            ("a\u{10FFFF}", Some("b")),
            ("\u{10FFFF}\u{10FFFF}", None),
            ("x\u{D7FF}", Some("x\u{E000}")),
        ] {
            assert_eq!(prefix_upper_bound(prefix).as_deref(), bound, "{prefix:?}");
        }
    }

    #[test]
    fn append_succeeds_when_cache_refresh_fails_after_commit() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let mut store = DurableEventStore::open(root.path())?;

        let record = store.append_with_hook(input("committed"), |stage| {
            if stage == AppendStage::CacheRefresh {
                Err(std::io::Error::other("injected cache refresh failure"))
            } else {
                Ok(())
            }
        })?;

        assert_eq!(record.sequence, 1);
        assert_eq!(store.append(input("next"))?.sequence, 2);
        drop(store);
        let reopened = DurableEventStore::open(root.path())?;
        assert_eq!(reopened.scan(10)?.records.first(), Some(&record));
        Ok(())
    }

    #[test]
    fn a_write_connection_is_not_kept_once_the_store_version_is_unsupported(
    ) -> Result<(), Box<dyn Error>> {
        let kept_access = |store: &DurableEventStore| -> Result<Option<Access>, Box<dyn Error>> {
            Ok(lock(&store.process_state)?
                .connection
                .as_ref()
                .map(|kept| kept.access))
        };
        type Operation = fn(&mut DurableEventStore) -> Result<(), DurableEventError>;
        let operations: [(&str, Operation); 4] = [
            ("open", |store| {
                DurableEventStore::open(store.root()).map(drop)
            }),
            ("scan", |store| store.scan(10).map(drop)),
            ("visit", |store| {
                store.visit_from_sequence(0, |_| {}).map(drop)
            }),
            ("append", |store| match store.append(input("refused")) {
                Err(DurableEventError::ReadOnly(_)) => Ok(()),
                Err(error) => Err(error),
                Ok(_) => Err(DurableEventError::Validation(
                    "an unsupported store version must refuse the append".to_owned(),
                )),
            }),
        ];
        for (label, operation) in operations {
            let root = tempfile::tempdir()?;
            let mut store = DurableEventStore::open(root.path())?;
            store.append(input("first"))?;
            assert_eq!(kept_access(&store)?, Some(Access::ReadWrite), "{label}");
            Connection::open(store.path())?.pragma_update(None, "user_version", 2_i64)?;

            operation(&mut store)?;

            assert_ne!(kept_access(&store)?, Some(Access::ReadWrite), "{label}");
        }
        Ok(())
    }

    #[test]
    fn a_write_connection_refuses_an_unadmitted_store_before_changing_it(
    ) -> Result<(), Box<dyn Error>> {
        let journal_mode = |path: &Path| -> rusqlite::Result<String> {
            Connection::open(path)?.query_row("PRAGMA journal_mode", [], |row| row.get(0))
        };
        type Damage = fn(&Connection) -> rusqlite::Result<()>;
        type Refusal = fn(&DurableEventError) -> bool;
        let cases: [(&str, Damage, Refusal); 2] = [
            (
                "version",
                |connection| connection.pragma_update(None, "user_version", 2_i64),
                |error| {
                    matches!(
                        error,
                        DurableEventError::ReadOnly(
                            DurableEventCompatibility::UnsupportedFrameVersion { found: 2 }
                        )
                    )
                },
            ),
            (
                "schema",
                |connection| {
                    connection
                        .execute_batch("CREATE INDEX events_by_recorded_at ON events (recorded_at)")
                },
                |error| matches!(error, DurableEventError::Corruption { offset: 0, .. }),
            ),
        ];
        for (label, damage, refusal) in cases {
            let root = tempfile::tempdir()?;
            let path = DurableEventStore::open(root.path())?.path().to_path_buf();
            damage(&Connection::open(&path)?)?;
            let before = journal_mode(&path)?;
            assert!(!before.eq_ignore_ascii_case("wal"), "{label}");

            let opened = StoreConnection::open(&path, Access::ReadWrite, file_identity(&path)?);

            assert!(opened.as_ref().err().is_some_and(refusal), "{label}");
            assert_eq!(journal_mode(&path)?, before, "{label}");
        }
        Ok(())
    }

    #[test]
    fn an_uncommitted_append_leaves_the_process_cache_unchanged() -> Result<(), Box<dyn Error>> {
        type CacheView = (
            Option<(StoreIdentity, Option<StoreHead>)>,
            Option<Option<u64>>,
        );
        let cache_view = |store: &DurableEventStore| -> Result<CacheView, Box<dyn Error>> {
            let state = lock(&store.process_state)?;
            Ok((
                state
                    .cache
                    .verified
                    .as_ref()
                    .map(|verified| (verified.identity.clone(), verified.head.clone())),
                state.cache.work_state.as_ref().map(|work| work.through),
            ))
        };
        let root = tempfile::tempdir()?;
        let mut store = DurableEventStore::open(root.path())?;
        let enqueued = crate::durable_work::WorkEnqueued {
            work_id: "work-1".to_owned(),
            work_kind: "test.work".to_owned(),
            payload_ref: crate::durable_work::WorkPayloadRef::inline(
                "test.work.v1",
                json!({"value": 1}),
            )?,
            dedupe_hint: None,
            next_wake_at_ms: None,
            effect_id: None,
        };
        store.append(DurableEventInput::new(
            "session-1",
            WORK_ENQUEUED,
            DurableEventPayload::inline("durable_work", serde_json::to_value(&enqueued)?),
        ))?;
        let before = cache_view(&store)?;
        assert!(before.1.is_some());
        let changer = Connection::open(store.path())?;
        let trigger: String = changer.query_row(
            "SELECT sql FROM sqlite_master WHERE name = 'events_reject_delete'",
            [],
            |row| row.get(0),
        )?;
        changer.execute_batch(&format!("DROP TRIGGER events_reject_delete; {trigger};"))?;
        drop(changer);

        let error = store
            .append_with_hook(input("not committed"), |stage| {
                if stage == AppendStage::BeforeCommit {
                    Err(std::io::Error::other("injected before commit"))
                } else {
                    Ok(())
                }
            })
            .err()
            .ok_or("expected injected failure before commit")?;

        assert!(matches!(
            error,
            DurableEventAppendError::DefinitelyNotCommitted(DurableEventError::Io(_))
        ));
        assert_eq!(cache_view(&store)?, before);
        Ok(())
    }

    #[test]
    fn unsupported_store_version_is_read_without_a_write_connection() -> Result<(), Box<dyn Error>>
    {
        let data_dir = tempfile::tempdir()?;
        let event_root = data_dir.path().join("runtime").join("durable-events");
        let mut store = DurableEventStore::open(&event_root)?;
        store.append(input("first"))?;
        let path = store.path().to_path_buf();
        drop(store);
        {
            let connection = Connection::open(&path)?;
            connection.pragma_update(None, "user_version", 2_i64)?;
        }
        let journal_mode_before: String =
            Connection::open(&path)?.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        let digest_before = DurableEventStore::logical_digest_existing(&event_root)?;

        let writes_before = write_connections();
        let reopened = DurableEventStore::open(&event_root)?;
        let scan = reopened.scan(10)?;
        let visit = reopened.visit_from_sequence(0, |_| {})?;
        crate::durable_migration::plan_durable_migration_for_roots(
            data_dir.path(),
            data_dir.path(),
            crate::durable_migration::DurableConfigCompatibility::Readable,
        )?;
        let admission = crate::durable_replay::evaluate_durable_recovery(
            &event_root,
            data_dir.path().join("runtime").join("durable-checkpoints"),
        );

        assert_eq!(write_connections(), writes_before);
        assert!(!admission.writable);
        assert_eq!(
            DurableEventStore::logical_digest_existing(&event_root)?,
            digest_before
        );
        assert_eq!(
            reopened.compatibility(),
            &DurableEventCompatibility::UnsupportedFrameVersion { found: 2 }
        );
        assert!(!reopened.is_writable());
        assert!(scan.records.is_empty());
        assert_eq!(visit.visited, 0);
        let check = Connection::open(&path)?;
        let user_version: i64 = check.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let rows: i64 = check.query_row("SELECT count(*) FROM events", [], |row| row.get(0))?;
        let journal_mode_after: String =
            check.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!(user_version, 2);
        assert_eq!(rows, 1);
        assert_eq!(journal_mode_after, journal_mode_before);
        Ok(())
    }
}
