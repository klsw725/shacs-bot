use crate::durable_child::{
    ChildResultDecisionKind, ChildResultRecorded, ChildSpawned, ReplayChildTaskState,
    CHILD_RESULT_REENTRY_PAYLOAD_TYPE, CHILD_RUN_PAYLOAD_TYPE,
};
use crate::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, CHILD_SPAWNED,
    SESSION_TURN_ACCEPTED, WORK_ENQUEUED, WORK_LEASED, WORK_RETRY_SCHEDULED,
};
use crate::durable_replay::{evaluate_durable_recovery, DurableCheckpointStore};
use crate::durable_trace::{DurableTraceInput, DurableTraceSeverity, DurableTraceStore};
use crate::durable_work::{WorkEnqueued, WorkLeased, WorkPayloadRef, WorkRetryScheduled};
use crate::{Session, SessionManager};
use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CURRENT_STORED_DATA_SCHEMA_VERSION: u32 = 1;
pub const MIGRATION_LEDGER_FILE: &str = "migration-ledger.json";
pub const MIGRATION_LOCK_FILE: &str = "migration.lock";
const V0_FIXTURE_DIR: &str = "migration-fixtures/v0";
const MAX_MIGRATION_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_MIGRATION_BACKUP_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableMigrationFamily {
    SessionMetadata,
    Event,
    Checkpoint,
    Queue,
    Scheduler,
    Channel,
    Child,
    Trace,
    DiagnosticsArtifact,
}

impl DurableMigrationFamily {
    pub fn all() -> [Self; 9] {
        [
            Self::SessionMetadata,
            Self::Event,
            Self::Checkpoint,
            Self::Queue,
            Self::Scheduler,
            Self::Channel,
            Self::Child,
            Self::Trace,
            Self::DiagnosticsArtifact,
        ]
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionMetadata => "session_metadata",
            Self::Event => "event",
            Self::Checkpoint => "checkpoint",
            Self::Queue => "queue",
            Self::Scheduler => "scheduler",
            Self::Channel => "channel",
            Self::Child => "child",
            Self::Trace => "trace",
            Self::DiagnosticsArtifact => "diagnostics_artifact",
        }
    }

    fn fixture_name(self) -> String {
        format!("{}.json", self.as_str())
    }

    fn physical_resource(self) -> DurableMigrationResource {
        match self {
            Self::SessionMetadata => DurableMigrationResource::WorkspaceSessions,
            Self::Event | Self::Queue | Self::Scheduler | Self::Child => {
                DurableMigrationResource::DurableEventsLog
            }
            Self::Checkpoint => DurableMigrationResource::DurableCheckpoints,
            Self::Channel => DurableMigrationResource::ChannelWorkerMetadata,
            Self::Trace | Self::DiagnosticsArtifact => DurableMigrationResource::DurableDiagnostics,
        }
    }

    fn primary_path(self, roots: &MigrationRoots) -> PathBuf {
        match self {
            Self::SessionMetadata => roots.workspace.join("sessions"),
            Self::Event => runtime_root(&roots.data_dir)
                .join("durable-events")
                .join("events.log"),
            Self::Checkpoint => runtime_root(&roots.data_dir).join("durable-checkpoints"),
            Self::Queue | Self::Scheduler | Self::Child => runtime_root(&roots.data_dir)
                .join("durable-events")
                .join("events.log"),
            Self::Channel => runtime_root(&roots.data_dir)
                .join("channels")
                .join("worker-metadata"),
            Self::Trace => runtime_root(&roots.data_dir)
                .join("durable-diagnostics")
                .join("diagnostics.log"),
            Self::DiagnosticsArtifact => runtime_root(&roots.data_dir).join("durable-diagnostics"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableMigrationResource {
    WorkspaceSessions,
    DurableEventsLog,
    DurableCheckpoints,
    ChannelWorkerMetadata,
    DurableDiagnostics,
}

impl DurableMigrationResource {
    fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceSessions => "workspace_sessions",
            Self::DurableEventsLog => "durable_events_log",
            Self::DurableCheckpoints => "durable_checkpoints",
            Self::ChannelWorkerMetadata => "channel_worker_metadata",
            Self::DurableDiagnostics => "durable_diagnostics",
        }
    }

    fn path(self, roots: &MigrationRoots) -> PathBuf {
        match self {
            Self::WorkspaceSessions => roots.workspace.join("sessions"),
            Self::DurableEventsLog => runtime_root(&roots.data_dir)
                .join("durable-events")
                .join("events.log"),
            Self::DurableCheckpoints => runtime_root(&roots.data_dir).join("durable-checkpoints"),
            Self::ChannelWorkerMetadata => runtime_root(&roots.data_dir)
                .join("channels")
                .join("worker-metadata"),
            Self::DurableDiagnostics => runtime_root(&roots.data_dir).join("durable-diagnostics"),
        }
    }
}

impl fmt::Display for DurableMigrationResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
struct MigrationRoots {
    data_dir: PathBuf,
    workspace: PathBuf,
}

impl MigrationRoots {
    fn new(data_dir: impl AsRef<Path>, workspace: impl AsRef<Path>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
            workspace: workspace.as_ref().to_path_buf(),
        }
    }
}

impl fmt::Display for DurableMigrationFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableMigrationAction {
    NoOp,
    Transform,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableMigrationResultStatus {
    InProgress,
    Skipped,
    NoOp,
    Transformed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableMigrationRunMode {
    DryRun,
    Apply,
    Resume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableConfigCompatibility {
    Readable,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMigrationInventoryEntry {
    pub family: DurableMigrationFamily,
    pub source_version: u32,
    pub target_version: u32,
    pub path_ref: String,
    pub evidence_digest: String,
    #[serde(default)]
    pub logical_source_digest: String,
    #[serde(default)]
    pub physical_resource: Option<DurableMigrationResource>,
    #[serde(default)]
    pub physical_precondition_digest: String,
    pub exists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMigrationPlanEntry {
    pub order: usize,
    pub family: DurableMigrationFamily,
    pub source_version: u32,
    pub target_version: u32,
    pub action: DurableMigrationAction,
    pub precondition_digest: String,
    #[serde(default)]
    pub logical_source_digest: String,
    #[serde(default)]
    pub physical_resource: Option<DurableMigrationResource>,
    #[serde(default)]
    pub physical_precondition_digest: String,
    pub rollback_capability: String,
    pub detail_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMigrationFamilyResult {
    pub family: DurableMigrationFamily,
    pub status: DurableMigrationResultStatus,
    pub source_version: u32,
    pub target_version: u32,
    pub precondition_digest: String,
    #[serde(default)]
    pub physical_resource: Option<DurableMigrationResource>,
    #[serde(default)]
    pub physical_precondition_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    pub detail_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMigrationPlan {
    pub inventory: Vec<DurableMigrationInventoryEntry>,
    pub entries: Vec<DurableMigrationPlanEntry>,
    pub blocked: bool,
    pub config_compatibility: DurableConfigCompatibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMigrationLedger {
    pub schema_version: u32,
    pub run_id: String,
    pub phase: String,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at_ms: Option<u64>,
    pub plan_digest: String,
    pub plan_entries: Vec<DurableMigrationPlanEntry>,
    pub initial_resource_preconditions: Vec<DurableMigrationResourcePrecondition>,
    pub backup_ref: String,
    pub results: Vec<DurableMigrationFamilyResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableMigrationResourcePrecondition {
    pub resource: DurableMigrationResource,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableMigrationReport {
    pub data_dir: PathBuf,
    pub ledger_path: PathBuf,
    pub plan: DurableMigrationPlan,
    pub ledger: Option<DurableMigrationLedger>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default)]
pub struct DurableMigrationOptions {
    pub interrupt: Option<DurableMigrationInterrupt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableMigrationInterrupt {
    Before(DurableMigrationFamily),
    AfterInProgress(DurableMigrationFamily),
    During(DurableMigrationFamily),
    AfterMutation(DurableMigrationFamily),
    BeforeResultCommit(DurableMigrationFamily),
    BeforeFixtureDeletion(DurableMigrationFamily),
    After(DurableMigrationFamily),
}

#[derive(Debug)]
pub enum DurableMigrationError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    Blocked(String),
    Validation(String),
    Interrupted(String),
}

impl fmt::Display for DurableMigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "durable migration I/O failed: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "durable migration serialization failed: {error}")
            }
            Self::Blocked(reason) => write!(formatter, "durable migration blocked: {reason}"),
            Self::Validation(reason) => {
                write!(formatter, "durable migration validation failed: {reason}")
            }
            Self::Interrupted(reason) => {
                write!(formatter, "durable migration interrupted: {reason}")
            }
        }
    }
}

impl Error for DurableMigrationError {}

impl From<std::io::Error> for DurableMigrationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DurableMigrationError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

pub fn durable_migration_ledger_path(data_dir: impl AsRef<Path>) -> PathBuf {
    runtime_root(data_dir.as_ref()).join(MIGRATION_LEDGER_FILE)
}

pub fn read_durable_migration_ledger(
    data_dir: impl AsRef<Path>,
) -> Result<Option<DurableMigrationLedger>, DurableMigrationError> {
    let path = durable_migration_ledger_path(data_dir);
    if !path.exists() {
        return Ok(None);
    }
    reject_symlink(&path)?;
    let bytes = read_limited(&path)?;
    let ledger: DurableMigrationLedger = serde_json::from_slice(&bytes)?;
    if ledger.schema_version != CURRENT_STORED_DATA_SCHEMA_VERSION {
        return Err(DurableMigrationError::Blocked(
            "stored-data migration ledger schema is not current; inspect-only manual recovery required"
                .to_owned(),
        ));
    }
    Ok(Some(ledger))
}

pub fn durable_migration_blocks_writable_runtime(
    data_dir: impl AsRef<Path>,
    config_compatibility: DurableConfigCompatibility,
) -> Result<Option<String>, DurableMigrationError> {
    durable_migration_blocks_writable_runtime_for_roots(
        data_dir.as_ref(),
        data_dir.as_ref(),
        config_compatibility,
    )
}

pub fn durable_migration_blocks_writable_runtime_for_roots(
    data_dir: impl AsRef<Path>,
    workspace: impl AsRef<Path>,
    config_compatibility: DurableConfigCompatibility,
) -> Result<Option<String>, DurableMigrationError> {
    if config_compatibility == DurableConfigCompatibility::Incompatible {
        return Ok(Some("config compatibility is incompatible".to_owned()));
    }
    let data_dir = data_dir.as_ref();
    let workspace = workspace.as_ref();
    if let Some(ledger) = read_durable_migration_ledger(data_dir)? {
        if ledger.phase != "complete" {
            return Ok(Some(format!(
                "stored-data migration ledger phase is {}",
                ledger.phase
            )));
        }
        if ledger.results.iter().any(|result| {
            matches!(
                result.status,
                DurableMigrationResultStatus::Failed | DurableMigrationResultStatus::Blocked
            )
        }) {
            return Ok(Some(
                "stored-data migration ledger requires manual recovery".to_owned(),
            ));
        }
    }
    let plan = plan_durable_migration_for_roots(data_dir, workspace, config_compatibility)?;
    if plan.blocked {
        return Ok(Some("stored-data migration plan is blocked".to_owned()));
    }
    if plan
        .entries
        .iter()
        .any(|entry| entry.action == DurableMigrationAction::Transform)
    {
        return Ok(Some(
            "stored-data migration requires explicit `runtime migrate --apply`".to_owned(),
        ));
    }
    Ok(None)
}

pub fn plan_durable_migration(
    data_dir: impl AsRef<Path>,
    config_compatibility: DurableConfigCompatibility,
) -> Result<DurableMigrationPlan, DurableMigrationError> {
    plan_durable_migration_for_roots(data_dir.as_ref(), data_dir.as_ref(), config_compatibility)
}

pub fn plan_durable_migration_for_roots(
    data_dir: impl AsRef<Path>,
    workspace: impl AsRef<Path>,
    config_compatibility: DurableConfigCompatibility,
) -> Result<DurableMigrationPlan, DurableMigrationError> {
    let roots = MigrationRoots::new(data_dir, workspace);
    let mut inventory = Vec::new();
    let mut entries = Vec::new();
    let mut blocked = config_compatibility == DurableConfigCompatibility::Incompatible;
    for (index, family) in DurableMigrationFamily::all().into_iter().enumerate() {
        let entry = inventory_family(&roots, family).unwrap_or_else(|error| {
            let issue = redact_migration_issue(&error.to_string());
            let resource = family.physical_resource();
            DurableMigrationInventoryEntry {
                family,
                source_version: CURRENT_STORED_DATA_SCHEMA_VERSION,
                target_version: CURRENT_STORED_DATA_SCHEMA_VERSION,
                path_ref: opaque_ref(
                    "path",
                    &format!("{}:{}", family, family.primary_path(&roots).display()),
                ),
                evidence_digest: digest_text(&format!("inventory-error:{family}:{issue}")),
                logical_source_digest: digest_text(&format!("logical-error:{family}:{issue}")),
                physical_resource: Some(resource),
                physical_precondition_digest: digest_text(&format!(
                    "physical-error:{resource}:{issue}"
                )),
                exists: true,
                issue: Some(issue),
            }
        });
        blocked |=
            entry.issue.is_some() || entry.source_version > CURRENT_STORED_DATA_SCHEMA_VERSION;
        let action = if entry.issue.is_some()
            || entry.source_version > CURRENT_STORED_DATA_SCHEMA_VERSION
            || (entry.source_version < CURRENT_STORED_DATA_SCHEMA_VERSION
                && !fixture_path(&roots.data_dir, family).exists())
        {
            DurableMigrationAction::Blocked
        } else if entry.source_version < CURRENT_STORED_DATA_SCHEMA_VERSION {
            DurableMigrationAction::Transform
        } else {
            DurableMigrationAction::NoOp
        };
        blocked |= action == DurableMigrationAction::Blocked;
        entries.push(DurableMigrationPlanEntry {
            order: index + 1,
            family,
            source_version: entry.source_version,
            target_version: CURRENT_STORED_DATA_SCHEMA_VERSION,
            action,
            precondition_digest: entry.evidence_digest.clone(),
            logical_source_digest: entry.logical_source_digest.clone(),
            physical_resource: entry.physical_resource,
            physical_precondition_digest: entry.physical_precondition_digest.clone(),
            rollback_capability: if action == DurableMigrationAction::Transform {
                "bounded_backup_until_complete".to_owned()
            } else {
                "not_needed".to_owned()
            },
            detail_ref: opaque_ref(
                "migration-plan",
                &format!("{}:{}", family, entry.evidence_digest),
            ),
        });
        inventory.push(entry);
    }
    Ok(DurableMigrationPlan {
        inventory,
        entries,
        blocked,
        config_compatibility,
    })
}

pub fn run_durable_migration(
    data_dir: impl AsRef<Path>,
    mode: DurableMigrationRunMode,
    config_compatibility: DurableConfigCompatibility,
    options: DurableMigrationOptions,
) -> Result<DurableMigrationReport, DurableMigrationError> {
    run_durable_migration_for_roots(
        data_dir.as_ref(),
        data_dir.as_ref(),
        mode,
        config_compatibility,
        options,
    )
}

pub fn run_durable_migration_for_roots(
    data_dir: impl AsRef<Path>,
    workspace: impl AsRef<Path>,
    mode: DurableMigrationRunMode,
    config_compatibility: DurableConfigCompatibility,
    options: DurableMigrationOptions,
) -> Result<DurableMigrationReport, DurableMigrationError> {
    let roots = MigrationRoots::new(data_dir, workspace);
    let data_dir = roots.data_dir.clone();
    let ledger_path = durable_migration_ledger_path(&data_dir);
    if mode == DurableMigrationRunMode::DryRun {
        let plan = plan_durable_migration_for_roots(
            &roots.data_dir,
            &roots.workspace,
            config_compatibility,
        )?;
        return Ok(DurableMigrationReport {
            data_dir,
            ledger_path,
            plan,
            ledger: None,
            dry_run: true,
        });
    }
    let _lock = acquire_migration_lock(&roots.data_dir)?;
    let mut plan =
        plan_durable_migration_for_roots(&roots.data_dir, &roots.workspace, config_compatibility)?;
    if plan.blocked {
        return Err(DurableMigrationError::Blocked(
            "migration plan contains blocked family or incompatible config".to_owned(),
        ));
    }
    fs::create_dir_all(runtime_root(&data_dir))?;
    let plan_digest = digest_json(&plan)?;
    let existing = read_durable_migration_ledger(&data_dir)?;
    let mut ledger = match (mode, existing) {
        (DurableMigrationRunMode::Apply, Some(ledger)) if ledger.phase != "complete" => {
            return Err(DurableMigrationError::Blocked(
                "partial migration exists; use resume".to_owned(),
            ))
        }
        (DurableMigrationRunMode::Resume, Some(ledger)) if ledger.phase != "complete" => ledger,
        (DurableMigrationRunMode::Resume, Some(ledger)) => ledger,
        (DurableMigrationRunMode::Resume, None) => {
            return Err(DurableMigrationError::Blocked(
                "resume requested without partial migration ledger".to_owned(),
            ))
        }
        (_, _) => new_ledger(&roots, &plan, &plan_digest)?,
    };
    if ledger.results.iter().any(|result| {
        matches!(
            result.status,
            DurableMigrationResultStatus::Failed | DurableMigrationResultStatus::Blocked
        )
    }) {
        return Err(DurableMigrationError::Blocked(
            "migration ledger requires manual recovery before resume".to_owned(),
        ));
    }
    if ledger.plan_entries.is_empty() {
        ledger.plan_entries = plan.entries.clone();
    }
    if ledger.initial_resource_preconditions.is_empty() {
        ledger.initial_resource_preconditions = initial_resource_preconditions(&plan);
    }
    if ledger.phase == "complete" {
        return Ok(DurableMigrationReport {
            data_dir,
            ledger_path,
            plan,
            ledger: Some(ledger),
            dry_run: false,
        });
    }
    plan.entries = ledger.plan_entries.clone();
    if ledger.results.is_empty() {
        write_ledger(&ledger_path, &ledger)?;
    }
    let mut completed = ledger
        .results
        .iter()
        .filter(|result| {
            matches!(
                result.status,
                DurableMigrationResultStatus::NoOp
                    | DurableMigrationResultStatus::Transformed
                    | DurableMigrationResultStatus::Skipped
            )
        })
        .map(|result| result.family)
        .collect::<BTreeSet<_>>();
    for entry in ledger.plan_entries.clone() {
        if completed.contains(&entry.family) {
            verify_completed_resource(&roots, &entry, &ledger)?;
            finalize_leftover_fixture(&roots, &entry)?;
            verify_completed_family(&roots, &entry, &ledger)?;
            continue;
        }
        if let Some(in_progress_index) = ledger.results.iter().position(|result| {
            result.family == entry.family
                && result.status == DurableMigrationResultStatus::InProgress
        }) {
            verify_in_progress_retry_safe(&roots, &entry, &ledger)?;
            ledger.results.remove(in_progress_index);
            ledger.phase = "partial".to_owned();
            write_ledger(&ledger_path, &ledger)?;
        }
        if let Err(error) = interrupt(
            options.interrupt,
            DurableMigrationInterrupt::Before(entry.family),
        ) {
            ledger.phase = "partial".to_owned();
            write_ledger(&ledger_path, &ledger)?;
            return Err(error);
        }
        let mut result = DurableMigrationFamilyResult {
            family: entry.family,
            status: DurableMigrationResultStatus::InProgress,
            source_version: entry.source_version,
            target_version: entry.target_version,
            precondition_digest: entry.precondition_digest.clone(),
            physical_resource: entry.physical_resource,
            physical_precondition_digest: entry.physical_precondition_digest.clone(),
            output_digest: None,
            detail_ref: entry.detail_ref.clone(),
        };
        if entry.action == DurableMigrationAction::Transform {
            ledger.results.push(result.clone());
            ledger.phase = "partial".to_owned();
            write_ledger(&ledger_path, &ledger)?;
            interrupt(
                options.interrupt,
                DurableMigrationInterrupt::AfterInProgress(entry.family),
            )?;
        }
        let step = (|| -> Result<(), DurableMigrationError> {
            verify_pending_preconditions(&roots, &entry, &ledger)?;
            match entry.action {
                DurableMigrationAction::NoOp => {
                    result.output_digest = entry
                        .physical_resource
                        .map(|resource| resource_digest(&roots, resource))
                        .transpose()?;
                    result.status = DurableMigrationResultStatus::NoOp;
                }
                DurableMigrationAction::Blocked => {
                    result.status = DurableMigrationResultStatus::Blocked;
                    return Err(DurableMigrationError::Blocked(format!(
                        "{} plan entry is blocked and requires manual recovery",
                        entry.family
                    )));
                }
                DurableMigrationAction::Transform => {
                    backup_family(&roots, &ledger.run_id, entry.family)?;
                    interrupt(
                        options.interrupt,
                        DurableMigrationInterrupt::During(entry.family),
                    )?;
                    transform_v0_family(&roots, entry.family)?;
                    interrupt(
                        options.interrupt,
                        DurableMigrationInterrupt::AfterMutation(entry.family),
                    )?;
                    let resource = entry
                        .physical_resource
                        .unwrap_or_else(|| entry.family.physical_resource());
                    let output_digest = resource_digest(&roots, resource)?;
                    if output_digest == entry.physical_precondition_digest {
                        result.status = DurableMigrationResultStatus::Failed;
                        return Err(DurableMigrationError::Validation(format!(
                            "{} did not change its physical migration resource",
                            entry.family
                        )));
                    }
                    result.output_digest = Some(output_digest);
                    result.status = DurableMigrationResultStatus::Transformed;
                }
            }
            Ok(())
        })();
        if let Err(error) = step {
            if !matches!(result.status, DurableMigrationResultStatus::Blocked) {
                result.status = if matches!(error, DurableMigrationError::Blocked(_)) {
                    DurableMigrationResultStatus::Blocked
                } else {
                    DurableMigrationResultStatus::Failed
                };
            }
            result.output_digest = entry
                .physical_resource
                .and_then(|resource| resource_digest(&roots, resource).ok());
            replace_family_result(&mut ledger, result);
            ledger.phase = "partial".to_owned();
            write_ledger(&ledger_path, &ledger)?;
            return Err(error);
        }
        interrupt(
            options.interrupt,
            DurableMigrationInterrupt::BeforeResultCommit(entry.family),
        )?;
        replace_family_result(&mut ledger, result.clone());
        ledger.phase = "partial".to_owned();
        write_ledger(&ledger_path, &ledger)?;
        if result.status == DurableMigrationResultStatus::Transformed {
            interrupt(
                options.interrupt,
                DurableMigrationInterrupt::BeforeFixtureDeletion(entry.family),
            )?;
            remove_fixture_if_matching(&roots, &entry)?;
        }
        completed.insert(entry.family);
        if let Err(error) = interrupt(
            options.interrupt,
            DurableMigrationInterrupt::After(entry.family),
        ) {
            ledger.phase = "partial".to_owned();
            write_ledger(&ledger_path, &ledger)?;
            return Err(error);
        }
    }
    verify_completed_resources(&roots, &ledger)?;
    plan =
        plan_durable_migration_for_roots(&roots.data_dir, &roots.workspace, config_compatibility)?;
    if plan.blocked
        || plan
            .entries
            .iter()
            .any(|entry| entry.action != DurableMigrationAction::NoOp)
    {
        ledger.phase = "partial".to_owned();
        write_ledger(&ledger_path, &ledger)?;
        return Err(DurableMigrationError::Validation(
            "post-migration verification did not reach no-op plan".to_owned(),
        ));
    }
    ledger.phase = "complete".to_owned();
    ledger.completed_at_ms = Some(now_ms());
    write_ledger(&ledger_path, &ledger)?;
    cleanup_backup(&data_dir, &ledger.run_id)?;
    Ok(DurableMigrationReport {
        data_dir,
        ledger_path,
        plan,
        ledger: Some(ledger),
        dry_run: false,
    })
}

fn inventory_family(
    roots: &MigrationRoots,
    family: DurableMigrationFamily,
) -> Result<DurableMigrationInventoryEntry, DurableMigrationError> {
    let fixture = fixture_path(&roots.data_dir, family);
    let mut issue = None;
    let (source_version, logical_source_digest) = if fixture.exists() {
        let value = read_json(&fixture)?;
        let raw_version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                DurableMigrationError::Validation(format!(
                    "{} fixture has no schema_version",
                    family
                ))
            })?;
        let version = u32::try_from(raw_version).map_err(|_| {
            DurableMigrationError::Validation(format!(
                "{} fixture schema_version exceeds u32",
                family
            ))
        })?;
        if version < CURRENT_STORED_DATA_SCHEMA_VERSION
            && value.get("family").and_then(Value::as_str) != Some(family.as_str())
        {
            issue = Some("fixture family mismatch".to_owned());
        }
        (version, digest_file(&fixture)?)
    } else {
        (
            CURRENT_STORED_DATA_SCHEMA_VERSION,
            digest_text("no-legacy-fixture"),
        )
    };
    let primary = family.primary_path(roots);
    let exists = primary.exists() || fixture.exists();
    let resource = family.physical_resource();
    let physical_precondition_digest = resource_digest(roots, resource)?;
    let evidence_digest = digest_text(&format!(
        "{}:{}:{}",
        logical_source_digest, resource, physical_precondition_digest
    ));
    Ok(DurableMigrationInventoryEntry {
        family,
        source_version,
        target_version: CURRENT_STORED_DATA_SCHEMA_VERSION,
        path_ref: opaque_ref("path", &format!("{}:{}", family, primary.display())),
        evidence_digest,
        logical_source_digest,
        physical_resource: Some(resource),
        physical_precondition_digest,
        exists,
        issue,
    })
}

fn transform_v0_family(
    roots: &MigrationRoots,
    family: DurableMigrationFamily,
) -> Result<(), DurableMigrationError> {
    let fixture = fixture_path(&roots.data_dir, family);
    let value = read_json(&fixture)?;
    let raw_version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            DurableMigrationError::Validation("fixture has no schema_version".to_owned())
        })?;
    let version = u32::try_from(raw_version).map_err(|_| {
        DurableMigrationError::Validation("fixture schema_version exceeds u32".to_owned())
    })?;
    if version != 0 {
        return Err(DurableMigrationError::Blocked(format!(
            "{} has no v0->v1 migration path",
            family
        )));
    }
    match family {
        DurableMigrationFamily::SessionMetadata => transform_session_metadata(roots, &value)?,
        DurableMigrationFamily::Event
        | DurableMigrationFamily::Queue
        | DurableMigrationFamily::Scheduler
        | DurableMigrationFamily::Child => transform_event_family(roots, family, &value)?,
        DurableMigrationFamily::Checkpoint => transform_checkpoint(roots)?,
        DurableMigrationFamily::Channel => transform_channel(roots, &value)?,
        DurableMigrationFamily::Trace => transform_trace(roots, &value)?,
        DurableMigrationFamily::DiagnosticsArtifact => {
            transform_diagnostics_artifact(roots, &value)?
        }
    }
    Ok(())
}

fn transform_session_metadata(
    roots: &MigrationRoots,
    value: &Value,
) -> Result<(), DurableMigrationError> {
    let key = value
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("migration:v0");
    let created_at = value
        .get("created_at")
        .and_then(Value::as_str)
        .unwrap_or("1970-01-01T00:00:00Z");
    let updated_at = value
        .get("updated_at")
        .and_then(Value::as_str)
        .unwrap_or(created_at);
    let mut session = Session::new(key);
    session.created_at = created_at.to_owned();
    session.updated_at = updated_at.to_owned();
    let mut manager = SessionManager::new(&roots.workspace)?;
    manager.save_with_fsync(&session)?;
    Ok(())
}

fn transform_event_family(
    roots: &MigrationRoots,
    family: DurableMigrationFamily,
    value: &Value,
) -> Result<(), DurableMigrationError> {
    let mut store = DurableEventStore::open(runtime_root(&roots.data_dir).join("durable-events"))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("migration");
    match family {
        DurableMigrationFamily::Queue => append_queue_fixture(&mut store, session_id)?,
        DurableMigrationFamily::Scheduler => append_scheduler_fixture(&mut store, session_id)?,
        DurableMigrationFamily::Child => append_child_fixture(&mut store, session_id)?,
        _ => {
            let payload = value
                .get("payload")
                .cloned()
                .unwrap_or_else(|| json!({"content_hash":"sha256:migration"}));
            let mut input = DurableEventInput::new(
                session_id,
                SESSION_TURN_ACCEPTED,
                DurableEventPayload::inline("migration.v0", payload),
            );
            input.turn_id = Some(
                value
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .unwrap_or("migration-turn")
                    .to_owned(),
            );
            store
                .append(input)
                .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
        }
    }
    Ok(())
}

fn append_queue_fixture(
    store: &mut DurableEventStore,
    session_id: &str,
) -> Result<(), DurableMigrationError> {
    let payload_ref = WorkPayloadRef::inline("migration.work.v1", json!({"summary":"queued"}))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    let mut input = DurableEventInput::new(
        session_id,
        WORK_ENQUEUED,
        DurableEventPayload::inline(
            "durable_work",
            serde_json::to_value(WorkEnqueued {
                work_id: "migration-queue-work".to_owned(),
                work_kind: "migration.queue".to_owned(),
                payload_ref,
                dedupe_hint: None,
                next_wake_at_ms: None,
                effect_id: Some("migration-effect".to_owned()),
            })?,
        ),
    );
    input.turn_id = Some("migration-turn".to_owned());
    input.causation_id = Some("migration-effect".to_owned());
    store
        .append(input)
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    Ok(())
}

fn append_scheduler_fixture(
    store: &mut DurableEventStore,
    session_id: &str,
) -> Result<(), DurableMigrationError> {
    let payload_ref = WorkPayloadRef::inline("migration.work.v1", json!({"summary":"scheduled"}))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    let work_id = "migration-scheduler-work".to_owned();
    let mut enqueued = DurableEventInput::new(
        session_id,
        WORK_ENQUEUED,
        DurableEventPayload::inline(
            "durable_work",
            serde_json::to_value(WorkEnqueued {
                work_id: work_id.clone(),
                work_kind: "migration.scheduler".to_owned(),
                payload_ref,
                dedupe_hint: None,
                next_wake_at_ms: None,
                effect_id: Some("migration-scheduler-effect".to_owned()),
            })?,
        ),
    );
    enqueued.turn_id = Some("migration-turn".to_owned());
    enqueued.causation_id = Some("migration-scheduler-effect".to_owned());
    store
        .append(enqueued)
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    store
        .append(DurableEventInput::new(
            session_id,
            WORK_LEASED,
            DurableEventPayload::inline(
                "durable_work",
                serde_json::to_value(WorkLeased {
                    work_id: work_id.clone(),
                    lease_id: "migration-lease".to_owned(),
                    lease_owner_ref: "migration-owner".to_owned(),
                    attempt: 1,
                    leased_at_ms: 1,
                    lease_expires_at_ms: 10,
                })?,
            ),
        ))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    store
        .append(DurableEventInput::new(
            session_id,
            WORK_RETRY_SCHEDULED,
            DurableEventPayload::inline(
                "durable_work",
                serde_json::to_value(WorkRetryScheduled {
                    work_id,
                    attempt: 1,
                    next_wake_at_ms: 10_000,
                    backoff_ms: 1_000,
                    reason_ref: "migration-retry".to_owned(),
                })?,
            ),
        ))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    Ok(())
}

fn append_child_fixture(
    store: &mut DurableEventStore,
    session_id: &str,
) -> Result<(), DurableMigrationError> {
    let child = ChildSpawned {
        child_task_id: "migration-child".to_owned(),
        parent_turn_id: "migration-turn".to_owned(),
        spawn_effect_id: "migration-child-effect".to_owned(),
        correlation_id: "migration-child-correlation".to_owned(),
        idempotency_key: "migration-child-idempotency".to_owned(),
        run_ref: None,
        attempt: 1,
        spawned_at_ms: 1,
    };
    let mut input = DurableEventInput::new(
        session_id,
        CHILD_SPAWNED,
        DurableEventPayload::inline(CHILD_RUN_PAYLOAD_TYPE, serde_json::to_value(child)?),
    );
    input.turn_id = Some("migration-turn".to_owned());
    input.causation_id = Some("migration-child-effect".to_owned());
    input.correlation_id = Some("migration-child-correlation".to_owned());
    store
        .append(input)
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    let result = ChildResultRecorded {
        child_task_id: "migration-child".to_owned(),
        parent_turn_id: "migration-turn".to_owned(),
        spawn_effect_id: "migration-child-effect".to_owned(),
        correlation_id: "migration-child-correlation".to_owned(),
        idempotency_key: "migration-child-idempotency".to_owned(),
        decision: ChildResultDecisionKind::Accepted,
        terminal_state: Some(ReplayChildTaskState::Cancelled),
        result_ref: format!("child-result:{}", "0".repeat(64)),
        finished_at_ms: 2,
    };
    let mut terminal = DurableEventInput::new(
        session_id,
        crate::durable_event::CHILD_RESULT_RECORDED,
        DurableEventPayload::inline(
            CHILD_RESULT_REENTRY_PAYLOAD_TYPE,
            serde_json::to_value(result)?,
        ),
    );
    terminal.turn_id = Some("migration-turn".to_owned());
    terminal.causation_id = Some("migration-child-effect".to_owned());
    terminal.correlation_id = Some("migration-child-correlation".to_owned());
    store
        .append(terminal)
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    Ok(())
}

fn transform_checkpoint(roots: &MigrationRoots) -> Result<(), DurableMigrationError> {
    let recovery = evaluate_durable_recovery(
        runtime_root(&roots.data_dir).join("durable-events"),
        runtime_root(&roots.data_dir).join("durable-checkpoints"),
    );
    let state = recovery
        .state
        .unwrap_or_else(crate::durable_replay::DurableReplayState::event_zero);
    DurableCheckpointStore::open(runtime_root(&roots.data_dir).join("durable-checkpoints"))
        .and_then(|store| store.write(&state))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    Ok(())
}

fn transform_channel(roots: &MigrationRoots, value: &Value) -> Result<(), DurableMigrationError> {
    let channel = value
        .get("channel")
        .and_then(Value::as_str)
        .unwrap_or("migration");
    let root = runtime_root(&roots.data_dir)
        .join("channels")
        .join("worker-metadata");
    fs::create_dir_all(&root)?;
    let path = root.join(format!("{}.json", safe_file_component(channel)));
    let out = json!({"schema_version":1,"channel":channel,"restart_state":{"delivery":"unknown"}});
    write_atomic(&path, serde_json::to_vec(&out)?.as_slice())
}

fn transform_trace(roots: &MigrationRoots, value: &Value) -> Result<(), DurableMigrationError> {
    let store = DurableTraceStore::open(runtime_root(&roots.data_dir).join("durable-diagnostics"))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    let detail = value
        .get("detail")
        .cloned()
        .unwrap_or_else(|| json!({"migration":"v0"}));
    store
        .append(DurableTraceInput::new(
            "migration.v0",
            DurableTraceSeverity::Info,
            detail,
        ))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    Ok(())
}

fn transform_diagnostics_artifact(
    roots: &MigrationRoots,
    value: &Value,
) -> Result<(), DurableMigrationError> {
    let store = DurableTraceStore::open(runtime_root(&roots.data_dir).join("durable-diagnostics"))
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    let payload = value
        .get("artifact")
        .cloned()
        .unwrap_or_else(|| json!({"migration":"v0"}));
    store
        .append_artifact_backed(
            DurableTraceInput::new(
                "migration.diagnostics_artifact.v0",
                DurableTraceSeverity::Info,
                json!({"migration":"diagnostics_artifact_v0"}),
            ),
            payload,
        )
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
    Ok(())
}

fn verify_completed_family(
    roots: &MigrationRoots,
    entry: &DurableMigrationPlanEntry,
    ledger: &DurableMigrationLedger,
) -> Result<(), DurableMigrationError> {
    if !ledger
        .results
        .iter()
        .any(|result| result.family == entry.family)
    {
        return Err(DurableMigrationError::Validation(format!(
            "{} ledger result missing",
            entry.family
        )));
    }
    let inventory = inventory_family(roots, entry.family)?;
    if inventory.source_version != CURRENT_STORED_DATA_SCHEMA_VERSION {
        return Err(DurableMigrationError::Blocked(format!(
            "{} completed evidence no longer verifies",
            entry.family
        )));
    }
    Ok(())
}

fn verify_completed_resource(
    roots: &MigrationRoots,
    entry: &DurableMigrationPlanEntry,
    ledger: &DurableMigrationLedger,
) -> Result<(), DurableMigrationError> {
    let result = ledger
        .results
        .iter()
        .find(|result| result.family == entry.family)
        .ok_or_else(|| {
            DurableMigrationError::Validation(format!("{} ledger result missing", entry.family))
        })?;
    let Some(resource) = result.physical_resource else {
        return Ok(());
    };
    let output_digest = latest_resource_digest(ledger, resource)
        .or_else(|| initial_resource_digest(ledger, resource))
        .ok_or_else(|| {
            DurableMigrationError::Validation(format!(
                "migration ledger has no completed digest for resource {resource}"
            ))
        })?;
    let current = resource_digest(roots, resource)?;
    if current != output_digest {
        return Err(DurableMigrationError::Blocked(format!(
            "{} completed resource digest changed and requires manual recovery",
            entry.family
        )));
    }
    Ok(())
}

fn replace_family_result(
    ledger: &mut DurableMigrationLedger,
    result: DurableMigrationFamilyResult,
) {
    if let Some(existing) = ledger
        .results
        .iter_mut()
        .find(|existing| existing.family == result.family)
    {
        *existing = result;
    } else {
        ledger.results.push(result);
    }
}

fn verify_in_progress_retry_safe(
    roots: &MigrationRoots,
    entry: &DurableMigrationPlanEntry,
    ledger: &DurableMigrationLedger,
) -> Result<(), DurableMigrationError> {
    let Some(resource) = entry.physical_resource else {
        return Ok(());
    };
    let current = resource_digest(roots, resource)?;
    let expected = latest_resource_digest(ledger, resource)
        .or_else(|| initial_resource_digest(ledger, resource))
        .unwrap_or_else(|| entry.physical_precondition_digest.clone());
    if current != expected {
        return Err(DurableMigrationError::Blocked(format!(
            "{} in-progress migration changed physical resource; manual recovery required",
            entry.family
        )));
    }
    Ok(())
}

fn finalize_leftover_fixture(
    roots: &MigrationRoots,
    entry: &DurableMigrationPlanEntry,
) -> Result<(), DurableMigrationError> {
    if entry.action == DurableMigrationAction::Transform {
        remove_fixture_if_matching(roots, entry)?;
    }
    Ok(())
}

fn remove_fixture_if_matching(
    roots: &MigrationRoots,
    entry: &DurableMigrationPlanEntry,
) -> Result<(), DurableMigrationError> {
    let fixture = fixture_path(&roots.data_dir, entry.family);
    if !fixture.exists() {
        return Ok(());
    }
    let digest = digest_file(&fixture)?;
    if digest != entry.logical_source_digest && digest != entry.precondition_digest {
        return Err(DurableMigrationError::Blocked(format!(
            "{} leftover fixture changed; manual recovery required",
            entry.family
        )));
    }
    fs::remove_file(&fixture)?;
    sync_parent_dir(&fixture)?;
    Ok(())
}

fn new_ledger(
    roots: &MigrationRoots,
    plan: &DurableMigrationPlan,
    plan_digest: &str,
) -> Result<DurableMigrationLedger, DurableMigrationError> {
    let run_id = format!("migration-{}", now_ms());
    Ok(DurableMigrationLedger {
        schema_version: CURRENT_STORED_DATA_SCHEMA_VERSION,
        run_id: run_id.clone(),
        phase: "started".to_owned(),
        started_at_ms: now_ms(),
        completed_at_ms: None,
        plan_digest: plan_digest.to_owned(),
        plan_entries: plan.entries.clone(),
        initial_resource_preconditions: initial_resource_preconditions(plan),
        backup_ref: opaque_ref(
            "backup",
            &runtime_root(&roots.data_dir)
                .join("migration-backups")
                .join(&run_id)
                .display()
                .to_string(),
        ),
        results: Vec::new(),
    })
}

fn write_ledger(path: &Path, ledger: &DurableMigrationLedger) -> Result<(), DurableMigrationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(path, serde_json::to_vec_pretty(ledger)?.as_slice())
}

fn acquire_migration_lock(data_dir: &Path) -> Result<File, DurableMigrationError> {
    let root = runtime_root(data_dir);
    fs::create_dir_all(&root)?;
    reject_symlink(&root)?;
    let path = root.join(MIGRATION_LOCK_FILE);
    reject_symlink(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(DurableMigrationError::Validation(
            "migration lock is not a regular file".to_owned(),
        ));
    }
    FileExt::lock(&file)?;
    Ok(file)
}

fn backup_family(
    roots: &MigrationRoots,
    run_id: &str,
    family: DurableMigrationFamily,
) -> Result<(), DurableMigrationError> {
    let resource = family.physical_resource();
    let source = resource.path(roots);
    let backup_run_root = runtime_root(&roots.data_dir)
        .join("migration-backups")
        .join(run_id);
    let root = backup_run_root.join(resource.as_str());
    let complete = root.join("backup.complete");
    if complete.exists() {
        return Ok(());
    }
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    if !source.exists() {
        write_atomic(&complete, b"complete")?;
        return Ok(());
    }
    let mut copied = backup_bytes(&backup_run_root)?;
    for file in collect_files(&source)? {
        let byte_len = file.metadata()?.len();
        copied = copied.saturating_add(byte_len);
        if copied > MAX_MIGRATION_BACKUP_BYTES {
            return Err(DurableMigrationError::Blocked(
                "migration backup exceeds bounded limit".to_owned(),
            ));
        }
        let bytes = fs::read(&file)?;
        let rel = file
            .strip_prefix(source.parent().unwrap_or(&source))
            .unwrap_or(&file);
        let dest = root.join(safe_relative(rel)?);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        write_atomic(&dest, &bytes)?;
    }
    write_atomic(&complete, b"complete")?;
    Ok(())
}

fn backup_bytes(root: &Path) -> Result<u64, DurableMigrationError> {
    if !root.exists() {
        return Ok(0);
    }
    collect_files(root)?
        .into_iter()
        .try_fold(0_u64, |total, file| {
            Ok(total.saturating_add(file.metadata()?.len()))
        })
}

fn cleanup_backup(data_dir: &Path, run_id: &str) -> Result<(), DurableMigrationError> {
    let path = runtime_root(data_dir)
        .join("migration-backups")
        .join(run_id);
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn initial_resource_preconditions(
    plan: &DurableMigrationPlan,
) -> Vec<DurableMigrationResourcePrecondition> {
    let mut out = Vec::new();
    for entry in &plan.entries {
        let Some(resource) = entry.physical_resource else {
            continue;
        };
        if out
            .iter()
            .any(|item: &DurableMigrationResourcePrecondition| item.resource == resource)
        {
            continue;
        }
        out.push(DurableMigrationResourcePrecondition {
            resource,
            digest: entry.physical_precondition_digest.clone(),
        });
    }
    out
}

fn verify_pending_preconditions(
    roots: &MigrationRoots,
    entry: &DurableMigrationPlanEntry,
    ledger: &DurableMigrationLedger,
) -> Result<(), DurableMigrationError> {
    if entry.action == DurableMigrationAction::Transform {
        let logical = digest_file(&fixture_path(&roots.data_dir, entry.family))?;
        if logical != entry.logical_source_digest && logical != entry.precondition_digest {
            return Err(DurableMigrationError::Blocked(format!(
                "{} legacy source changed since migration plan; manual recovery required",
                entry.family
            )));
        }
    }
    let Some(resource) = entry.physical_resource else {
        return Ok(());
    };
    let current = resource_digest(roots, resource)?;
    let expected = latest_resource_digest(ledger, resource)
        .or_else(|| initial_resource_digest(ledger, resource))
        .unwrap_or_else(|| entry.physical_precondition_digest.clone());
    if current != expected {
        return Err(DurableMigrationError::Blocked(format!(
            "{} physical resource changed since last migration step; manual recovery required",
            resource
        )));
    }
    Ok(())
}

fn verify_completed_resources(
    roots: &MigrationRoots,
    ledger: &DurableMigrationLedger,
) -> Result<(), DurableMigrationError> {
    let mut resources = BTreeSet::new();
    for result in &ledger.results {
        if let Some(resource) = result.physical_resource {
            resources.insert(resource);
        }
    }
    for resource in resources {
        let expected = latest_resource_digest(ledger, resource)
            .or_else(|| initial_resource_digest(ledger, resource))
            .ok_or_else(|| {
                DurableMigrationError::Validation(format!(
                    "migration ledger has no digest for resource {resource}"
                ))
            })?;
        let current = resource_digest(roots, resource)?;
        if current != expected {
            return Err(DurableMigrationError::Blocked(format!(
                "completed migration resource {resource} changed; manual recovery required"
            )));
        }
    }
    Ok(())
}

fn latest_resource_digest(
    ledger: &DurableMigrationLedger,
    resource: DurableMigrationResource,
) -> Option<String> {
    ledger
        .results
        .iter()
        .rev()
        .find(|result| {
            result.physical_resource == Some(resource)
                && result.output_digest.is_some()
                && matches!(
                    result.status,
                    DurableMigrationResultStatus::NoOp
                        | DurableMigrationResultStatus::Skipped
                        | DurableMigrationResultStatus::Transformed
                )
        })
        .and_then(|result| result.output_digest.clone())
}

fn initial_resource_digest(
    ledger: &DurableMigrationLedger,
    resource: DurableMigrationResource,
) -> Option<String> {
    ledger
        .initial_resource_preconditions
        .iter()
        .find(|entry| entry.resource == resource)
        .map(|entry| entry.digest.clone())
}

fn resource_digest(
    roots: &MigrationRoots,
    resource: DurableMigrationResource,
) -> Result<String, DurableMigrationError> {
    if resource == DurableMigrationResource::DurableDiagnostics {
        return durable_diagnostics_digest(roots);
    }
    let mut hasher = Sha256::new();
    hasher.update(resource.as_str().as_bytes());
    let path = resource.path(roots);
    if path.exists() {
        for file in collect_files(&path)? {
            hasher.update(display_relative(&roots.data_dir, &file).as_bytes());
            hash_file_into(&file, &mut hasher)?;
        }
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn durable_diagnostics_digest(roots: &MigrationRoots) -> Result<String, DurableMigrationError> {
    let mut hasher = Sha256::new();
    let root = DurableMigrationResource::DurableDiagnostics.path(roots);
    hasher.update(
        DurableMigrationResource::DurableDiagnostics
            .as_str()
            .as_bytes(),
    );
    let log = root.join("diagnostics.log");
    if log.exists() {
        hasher.update(display_relative(&roots.data_dir, &log).as_bytes());
        hash_file_into(&log, &mut hasher)?;
        let scan = DurableTraceStore::scan_existing(
            &root,
            crate::durable_trace::MAX_RETAINED_TRACE_RECORDS + 1,
        )
        .map_err(|error| DurableMigrationError::Validation(error.to_string()))?;
        if scan.corrupt_tail {
            return Err(DurableMigrationError::Validation(
                scan.issue
                    .unwrap_or_else(|| "diagnostics tail corrupt".to_owned()),
            ));
        }
        if scan.truncated {
            return Err(DurableMigrationError::Blocked(
                "durable diagnostics evidence exceeds bounded migration scan; manual recovery required"
                    .to_owned(),
            ));
        }
        let mut refs = scan
            .records
            .iter()
            .flat_map(|record| record.artifact_refs.iter())
            .map(|artifact| artifact.artifact_ref.clone())
            .collect::<Vec<_>>();
        refs.sort();
        refs.dedup();
        for artifact_ref in refs {
            let relative = safe_relative(Path::new(&artifact_ref))?;
            let path = root.join(relative);
            hasher.update(artifact_ref.as_bytes());
            hash_file_into(&path, &mut hasher)?;
        }
    } else if root.exists() {
        reject_symlink(&root)?;
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn digest_file(path: &Path) -> Result<String, DurableMigrationError> {
    let mut hasher = Sha256::new();
    hash_file_into(path, &mut hasher)?;
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn digest_text(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn hash_file_into(path: &Path, hasher: &mut Sha256) -> Result<(), DurableMigrationError> {
    reject_symlink(path)?;
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(DurableMigrationError::Validation(
            "migration path is not a regular file".to_owned(),
        ));
    }
    let mut reader = BufReader::new(file);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(())
}

fn collect_files(path: &Path) -> Result<Vec<PathBuf>, DurableMigrationError> {
    reject_symlink(path)?;
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    collect_files_inner(path, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_files_inner(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), DurableMigrationError> {
    reject_symlink(path)?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        reject_symlink(&path)?;
        if path.is_dir() {
            collect_files_inner(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn fixture_path(data_dir: &Path, family: DurableMigrationFamily) -> PathBuf {
    runtime_root(data_dir)
        .join(V0_FIXTURE_DIR)
        .join(family.fixture_name())
}

fn runtime_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime")
}

fn read_json(path: &Path) -> Result<Value, DurableMigrationError> {
    Ok(serde_json::from_slice(&read_limited(path)?)?)
}

fn redact_migration_issue(value: &str) -> String {
    format!("inventory unavailable ({})", opaque_ref("detail", value))
}

fn read_limited(path: &Path) -> Result<Vec<u8>, DurableMigrationError> {
    reject_symlink(path)?;
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(DurableMigrationError::Validation(
            "migration path is not a regular file".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((MAX_MIGRATION_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MIGRATION_FILE_BYTES {
        return Err(DurableMigrationError::Validation(
            "migration file exceeds maximum size".to_owned(),
        ));
    }
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DurableMigrationError> {
    let parent = path.parent().ok_or_else(|| {
        DurableMigrationError::Validation("migration write path has no parent".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    reject_symlink(parent)?;
    reject_symlink(path)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("migration");
    let temp = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        now_ms()
    ));
    let result = (|| -> std::io::Result<()> {
        reject_symlink(&temp)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
        }
        let mut file = options.open(&temp)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "migration temp path is not a regular file",
            ));
        }
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        reject_symlink(path)?;
        if !File::open(path)?.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "migration target path is not a regular file",
            ));
        }
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    Ok(result?)
}

fn sync_parent_dir(path: &Path) -> Result<(), DurableMigrationError> {
    let parent = path.parent().ok_or_else(|| {
        DurableMigrationError::Validation("migration path has no parent".to_owned())
    })?;
    Ok(sync_dir(parent)?)
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    reject_symlink(path)?;
    OpenOptions::new().read(true).open(path)?.sync_all()
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "durable migration path is a symlink",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn digest_json(value: &impl Serialize) -> Result<String, DurableMigrationError> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(value)?)
    ))
}

fn opaque_ref(kind: &str, value: &str) -> String {
    format!(
        "{kind}:{}",
        &format!("{:x}", Sha256::digest(value.as_bytes()))[..16]
    )
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn safe_file_component(value: &str) -> String {
    let text = value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => character,
            _ => '_',
        })
        .collect::<String>();
    if text.is_empty() || text == "." || text == ".." {
        "migration".to_owned()
    } else {
        text
    }
}

fn safe_relative(path: &Path) -> Result<PathBuf, DurableMigrationError> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => out.push(value),
            _ => {
                return Err(DurableMigrationError::Validation(
                    "unsafe backup relative path".to_owned(),
                ))
            }
        }
    }
    Ok(out)
}

fn interrupt(
    configured: Option<DurableMigrationInterrupt>,
    current: DurableMigrationInterrupt,
) -> Result<(), DurableMigrationError> {
    if configured == Some(current) {
        return Err(DurableMigrationError::Interrupted(format!(
            "injected at {current:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(root: &Path, family: DurableMigrationFamily) -> Result<(), Box<dyn Error>> {
        let path = fixture_path(root, family);
        fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        let payload = json!({"schema_version":0,"family":family.as_str(),"payload":{"content_hash":"abc"},"key":"cli:test","channel":"telegram"});
        write_atomic(&path, serde_json::to_vec(&payload)?.as_slice())?;
        Ok(())
    }

    #[test]
    fn no_op_plan_has_deterministic_family_inventory() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let plan = plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?;
        assert_eq!(plan.entries.len(), DurableMigrationFamily::all().len());
        assert!(!plan.blocked);
        assert!(plan
            .entries
            .iter()
            .all(|entry| entry.action == DurableMigrationAction::NoOp));
        assert_eq!(
            plan.entries[0].family,
            DurableMigrationFamily::SessionMetadata
        );
        Ok(())
    }

    #[test]
    fn dry_run_does_not_create_ledger_or_remove_fixture() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::SessionMetadata)?;
        let before = fs::read(fixture_path(
            root.path(),
            DurableMigrationFamily::SessionMetadata,
        ))?;
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::DryRun,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert!(report.dry_run);
        assert!(!durable_migration_ledger_path(root.path()).exists());
        assert_eq!(
            before,
            fs::read(fixture_path(
                root.path(),
                DurableMigrationFamily::SessionMetadata
            ))?
        );
        Ok(())
    }

    #[test]
    fn transforms_single_and_multiple_families_then_resumes_noop() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::SessionMetadata)?;
        write_fixture(root.path(), DurableMigrationFamily::Trace)?;
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        let ledger = report.ledger.ok_or("missing ledger")?;
        assert_eq!(ledger.phase, "complete");
        assert!(root.path().join("sessions").read_dir()?.next().is_some());
        let scan = DurableTraceStore::scan_existing(
            runtime_root(root.path()).join("durable-diagnostics"),
            10,
        )?;
        assert_eq!(scan.records.len(), 1);
        let resumed = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(
            resumed
                .ledger
                .as_ref()
                .ok_or("missing resumed ledger")?
                .phase,
            "complete"
        );
        assert!(resumed
            .plan
            .entries
            .iter()
            .all(|entry| entry.action == DurableMigrationAction::NoOp));
        Ok(())
    }

    #[test]
    fn session_metadata_uses_workspace_sessions_not_data_dir_sessions() -> Result<(), Box<dyn Error>>
    {
        let data = tempfile::tempdir()?;
        let workspace = tempfile::tempdir()?;
        write_fixture(data.path(), DurableMigrationFamily::SessionMetadata)?;
        let report = run_durable_migration_for_roots(
            data.path(),
            workspace.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(report.ledger.ok_or("missing ledger")?.phase, "complete");
        assert!(workspace
            .path()
            .join("sessions")
            .read_dir()?
            .next()
            .is_some());
        assert!(!data.path().join("sessions").exists());
        let manager = SessionManager::new(workspace.path())?;
        assert!(manager.session_path("cli:test").exists());
        Ok(())
    }

    #[test]
    fn interruption_leaves_partial_and_resume_continues() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::SessionMetadata)?;
        write_fixture(root.path(), DurableMigrationFamily::Trace)?;
        let error = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::After(
                    DurableMigrationFamily::SessionMetadata,
                )),
            },
        )
        .expect_err("expected interruption");
        assert!(matches!(error, DurableMigrationError::Interrupted(_)));
        assert_eq!(
            read_durable_migration_ledger(root.path())?
                .ok_or("missing ledger")?
                .phase,
            "partial"
        );
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(report.ledger.ok_or("missing ledger")?.phase, "complete");
        Ok(())
    }

    #[test]
    fn unknown_newer_missing_path_and_config_incompatible_block() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let path = fixture_path(root.path(), DurableMigrationFamily::Event);
        fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        write_atomic(&path, br#"{"schema_version":2,"family":"event"}"#)?;
        assert!(plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?.blocked);
        fs::remove_file(&path)?;
        write_atomic(&path, br#"{"schema_version":0,"family":"wrong"}"#)?;
        assert!(plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?.blocked);
        assert!(durable_migration_blocks_writable_runtime(
            root.path(),
            DurableConfigCompatibility::Incompatible
        )?
        .is_some());
        Ok(())
    }

    #[test]
    fn malformed_and_symlinked_inventory_projects_blocked_family() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let path = fixture_path(root.path(), DurableMigrationFamily::Event);
        fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        write_atomic(&path, b"not-json")?;
        let plan = plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?;
        assert!(plan.blocked);
        assert!(plan
            .inventory
            .iter()
            .any(|entry| entry.family == DurableMigrationFamily::Event && entry.issue.is_some()));
        #[cfg(unix)]
        {
            fs::remove_file(&path)?;
            std::os::unix::fs::symlink("/tmp", &path)?;
            let plan = plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?;
            assert!(plan.blocked);
        }
        Ok(())
    }

    #[test]
    fn all_family_transform_outputs_remain_durable_compatible() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        for family in DurableMigrationFamily::all() {
            write_fixture(root.path(), family)?;
        }
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(report.ledger.ok_or("missing ledger")?.phase, "complete");
        assert!(durable_migration_blocks_writable_runtime(
            root.path(),
            DurableConfigCompatibility::Readable
        )?
        .is_none());
        let recovery = evaluate_durable_recovery(
            runtime_root(root.path()).join("durable-events"),
            runtime_root(root.path()).join("durable-checkpoints"),
        );
        assert!(recovery.writable, "{recovery:?}");
        let trace = DurableTraceStore::scan_existing(
            runtime_root(root.path()).join("durable-diagnostics"),
            20,
        )?;
        assert!(!trace.corrupt_tail);
        assert!(runtime_root(root.path())
            .join("durable-diagnostics")
            .read_dir()?
            .next()
            .is_some());
        Ok(())
    }

    #[test]
    fn diagnostics_artifact_transform_records_referenced_artifact() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::DiagnosticsArtifact)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        let diagnostics_root = runtime_root(root.path()).join("durable-diagnostics");
        let scan = DurableTraceStore::scan_existing(&diagnostics_root, 10)?;
        let record = scan.records.first().ok_or("missing diagnostics record")?;
        let artifact = record.artifact_refs.first().ok_or("missing artifact ref")?;
        assert!(diagnostics_root.join(&artifact.artifact_ref).exists());
        Ok(())
    }

    #[test]
    fn current_log_larger_than_four_mib_inventories_with_streaming_digest(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let log = runtime_root(root.path())
            .join("durable-events")
            .join("events.log");
        fs::create_dir_all(log.parent().ok_or("missing parent")?)?;
        let mut file = File::create(&log)?;
        file.write_all(&vec![b'x'; MAX_MIGRATION_FILE_BYTES + 1024])?;
        file.sync_all()?;
        let plan = plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?;
        assert!(!plan.blocked);
        assert!(plan
            .inventory
            .iter()
            .any(|entry| entry.family == DurableMigrationFamily::Event));
        Ok(())
    }

    #[test]
    fn apply_waits_for_exclusive_migration_lock() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::Trace)?;
        fs::create_dir_all(runtime_root(root.path()))?;
        let lock = acquire_migration_lock(root.path())?;
        let root_path = root.path().to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let result = run_durable_migration(
                &root_path,
                DurableMigrationRunMode::Apply,
                DurableConfigCompatibility::Readable,
                DurableMigrationOptions::default(),
            );
            sender
                .send(result.map(|report| report.ledger.map(|ledger| ledger.phase)))
                .ok();
        });
        assert!(receiver
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err());
        drop(lock);
        let phase = receiver.recv_timeout(std::time::Duration::from_secs(5))??;
        assert_eq!(phase.as_deref(), Some("complete"));
        handle.join().map_err(|_| "migration thread panicked")?;
        Ok(())
    }

    #[test]
    fn pending_logical_precondition_mismatch_blocks_resume() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::SessionMetadata)?;
        write_fixture(root.path(), DurableMigrationFamily::Trace)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::After(
                    DurableMigrationFamily::SessionMetadata,
                )),
            },
        )
        .expect_err("expected interruption");
        write_atomic(
            &fixture_path(root.path(), DurableMigrationFamily::Trace),
            br#"{"schema_version":0,"family":"trace","detail":{"summary":"changed"}}"#,
        )?;
        let error = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )
        .expect_err("expected precondition block");
        assert!(error.to_string().contains("legacy source changed"));
        Ok(())
    }

    #[test]
    fn shared_resource_latest_output_mismatch_blocks_resume() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::Queue)?;
        write_fixture(root.path(), DurableMigrationFamily::Scheduler)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::After(
                    DurableMigrationFamily::Queue,
                )),
            },
        )
        .expect_err("expected interruption");
        let log = runtime_root(root.path())
            .join("durable-events")
            .join("events.log");
        OpenOptions::new()
            .append(true)
            .open(&log)?
            .write_all(b"external-change\n")?;
        let error = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )
        .expect_err("expected shared resource block");
        assert!(error.to_string().contains("resource"), "{error}");
        Ok(())
    }

    #[test]
    fn failed_transform_ledger_blocks_auto_resume() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::Trace)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::During(
                    DurableMigrationFamily::Trace,
                )),
            },
        )
        .expect_err("expected injected failure");
        let ledger = read_durable_migration_ledger(root.path())?.ok_or("missing ledger")?;
        assert_eq!(ledger.phase, "partial");
        assert!(ledger
            .results
            .iter()
            .any(|result| result.status == DurableMigrationResultStatus::Failed));
        let error = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )
        .expect_err("expected manual recovery block");
        assert!(error.to_string().contains("manual recovery"));
        Ok(())
    }

    #[test]
    fn crash_before_result_commit_leaves_in_progress_fixture_and_blocks_if_resource_changed(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::Queue)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::BeforeResultCommit(
                    DurableMigrationFamily::Queue,
                )),
            },
        )
        .expect_err("expected crash before result commit");
        let ledger = read_durable_migration_ledger(root.path())?.ok_or("missing ledger")?;
        assert!(ledger.results.iter().any(|result| {
            result.family == DurableMigrationFamily::Queue
                && result.status == DurableMigrationResultStatus::InProgress
        }));
        assert!(fixture_path(root.path(), DurableMigrationFamily::Queue).exists());
        assert!(runtime_root(root.path())
            .join("migration-backups")
            .join(&ledger.run_id)
            .join(DurableMigrationResource::DurableEventsLog.as_str())
            .join("backup.complete")
            .exists());
        let error = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )
        .expect_err("expected manual recovery block");
        assert!(error.to_string().contains("manual recovery"));
        assert!(fixture_path(root.path(), DurableMigrationFamily::Queue).exists());
        Ok(())
    }

    #[test]
    fn in_progress_before_mutation_is_removed_and_retried_on_resume() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::Trace)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::AfterInProgress(
                    DurableMigrationFamily::Trace,
                )),
            },
        )
        .expect_err("expected in-progress crash");
        let ledger = read_durable_migration_ledger(root.path())?.ok_or("missing ledger")?;
        assert!(ledger.results.iter().any(|result| {
            result.family == DurableMigrationFamily::Trace
                && result.status == DurableMigrationResultStatus::InProgress
        }));
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(report.ledger.ok_or("missing ledger")?.phase, "complete");
        Ok(())
    }

    #[test]
    fn transformed_commit_then_fixture_delete_crash_finalizes_idempotently(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::SessionMetadata)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::BeforeFixtureDeletion(
                    DurableMigrationFamily::SessionMetadata,
                )),
            },
        )
        .expect_err("expected fixture deletion crash");
        assert!(fixture_path(root.path(), DurableMigrationFamily::SessionMetadata).exists());
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(report.ledger.ok_or("missing ledger")?.phase, "complete");
        assert!(!fixture_path(root.path(), DurableMigrationFamily::SessionMetadata).exists());
        Ok(())
    }

    #[test]
    fn completed_resource_mismatch_preserves_leftover_fixture() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::SessionMetadata)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions {
                interrupt: Some(DurableMigrationInterrupt::BeforeFixtureDeletion(
                    DurableMigrationFamily::SessionMetadata,
                )),
            },
        )
        .expect_err("expected fixture deletion crash");
        let fixture = fixture_path(root.path(), DurableMigrationFamily::SessionMetadata);
        assert!(fixture.exists());
        let sessions = root.path().join("sessions");
        fs::write(sessions.join("tampered.jsonl"), b"tampered")?;

        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )
        .expect_err("changed completed resource must require manual recovery");
        assert!(fixture.exists());
        Ok(())
    }

    #[test]
    fn future_or_malformed_ledger_blocks_inspect_only() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let ledger = durable_migration_ledger_path(root.path());
        fs::create_dir_all(ledger.parent().ok_or("missing parent")?)?;
        write_atomic(
            &ledger,
            br#"{"schema_version":999,"run_id":"x","phase":"complete","started_at_ms":1,"plan_digest":"x","backup_ref":"x","results":[]}"#,
        )?;
        assert!(durable_migration_blocks_writable_runtime(
            root.path(),
            DurableConfigCompatibility::Readable
        )
        .is_err());
        write_atomic(&ledger, b"not-json")?;
        assert!(durable_migration_blocks_writable_runtime(
            root.path(),
            DurableConfigCompatibility::Readable
        )
        .is_err());
        write_atomic(
            &ledger,
            br#"{"schema_version":1,"run_id":"x","phase":"partial","started_at_ms":1,"plan_digest":"x","backup_ref":"x","results":[]}"#,
        )?;
        assert!(read_durable_migration_ledger(root.path()).is_err());
        Ok(())
    }

    #[test]
    fn fixture_schema_version_overflow_projects_blocked() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let path = fixture_path(root.path(), DurableMigrationFamily::Trace);
        fs::create_dir_all(path.parent().ok_or("missing parent")?)?;
        write_atomic(
            &path,
            format!("{{\"schema_version\":{},\"family\":\"trace\"}}", u64::MAX).as_bytes(),
        )?;
        let plan = plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?;
        assert!(plan.blocked);
        Ok(())
    }

    #[test]
    fn write_atomic_does_not_follow_precreated_deterministic_temp_symlink(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let target = root.path().join("ledger.json");
        #[cfg(unix)]
        {
            let old_temp = target.with_extension("tmp");
            std::os::unix::fs::symlink("/tmp/should-not-write", &old_temp)?;
            write_atomic(&target, b"safe")?;
            assert_eq!(fs::read(&target)?, b"safe");
            assert!(std::fs::symlink_metadata(old_temp)?
                .file_type()
                .is_symlink());
        }
        Ok(())
    }

    #[test]
    fn diagnostics_digest_blocks_truncated_bounded_scan() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let diagnostics = runtime_root(root.path()).join("durable-diagnostics");
        fs::create_dir_all(diagnostics.join("artifacts"))?;
        let mut bytes = Vec::new();
        for index in 0..(crate::durable_trace::MAX_RETAINED_TRACE_RECORDS + 2) {
            let record = crate::durable_trace::DurableTraceRecord {
                schema_family: crate::durable_trace::DURABLE_TRACE_SCHEMA_FAMILY.to_owned(),
                schema_version: crate::durable_trace::CURRENT_DURABLE_TRACE_SCHEMA_VERSION,
                trace_id: format!("trace-{index}"),
                kind: "migration.test".to_owned(),
                severity: DurableTraceSeverity::Info,
                event_sequence: None,
                correlation: Default::default(),
                redaction_status: crate::durable_trace::DurableTraceRedactionStatus::Applied,
                detail_preview: Some("{}".to_owned()),
                artifact_refs: Vec::new(),
                active_recovery: false,
                timestamp_ms: index as u64,
            };
            bytes.extend(crate::durable_trace::frame_for_test(&record)?);
        }
        write_atomic(&diagnostics.join("diagnostics.log"), &bytes)?;
        let plan = plan_durable_migration(root.path(), DurableConfigCompatibility::Readable)?;
        assert!(plan.blocked);
        Ok(())
    }

    #[test]
    fn complete_resume_ignores_post_complete_runtime_resource_changes() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        write_fixture(root.path(), DurableMigrationFamily::Queue)?;
        run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Apply,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        OpenOptions::new()
            .append(true)
            .open(
                runtime_root(root.path())
                    .join("durable-events")
                    .join("events.log"),
            )?
            .write_all(b"post-complete-write\n")?;
        let report = run_durable_migration(
            root.path(),
            DurableMigrationRunMode::Resume,
            DurableConfigCompatibility::Readable,
            DurableMigrationOptions::default(),
        )?;
        assert_eq!(report.ledger.ok_or("missing ledger")?.phase, "complete");
        Ok(())
    }

    #[test]
    fn interruptions_before_during_and_after_each_family_leave_resumable_partial(
    ) -> Result<(), Box<dyn Error>> {
        for family in DurableMigrationFamily::all() {
            for interrupt in [
                DurableMigrationInterrupt::Before(family),
                DurableMigrationInterrupt::During(family),
                DurableMigrationInterrupt::After(family),
            ] {
                let root = tempfile::tempdir()?;
                write_fixture(root.path(), family)?;
                let error = run_durable_migration(
                    root.path(),
                    DurableMigrationRunMode::Apply,
                    DurableConfigCompatibility::Readable,
                    DurableMigrationOptions {
                        interrupt: Some(interrupt),
                    },
                )
                .expect_err("expected interruption");
                assert!(matches!(error, DurableMigrationError::Interrupted(_)));
                assert_eq!(
                    read_durable_migration_ledger(root.path())?
                        .ok_or("missing ledger")?
                        .phase,
                    "partial"
                );
                if interrupt == DurableMigrationInterrupt::During(family) {
                    assert!(run_durable_migration(
                        root.path(),
                        DurableMigrationRunMode::Resume,
                        DurableConfigCompatibility::Readable,
                        DurableMigrationOptions::default(),
                    )
                    .is_err());
                } else {
                    let resumed = run_durable_migration(
                        root.path(),
                        DurableMigrationRunMode::Resume,
                        DurableConfigCompatibility::Readable,
                        DurableMigrationOptions::default(),
                    )?;
                    assert_eq!(resumed.ledger.ok_or("missing ledger")?.phase, "complete");
                }
            }
        }
        Ok(())
    }
}
