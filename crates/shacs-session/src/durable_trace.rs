use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use shacs_redaction::{redact_string, redact_value, REDACTED};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

pub const DURABLE_TRACE_SCHEMA_FAMILY: &str = "shacs.durable_diagnostics_evidence";
pub const CURRENT_DURABLE_TRACE_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_DURABLE_TRACE_FRAME_VERSION: u32 = 1;
pub const MAX_TRACE_DETAIL_PREVIEW_BYTES: usize = 2048;
pub const MAX_TRACE_DETAIL_ARTIFACT_BYTES: usize = 64 * 1024;
pub const MAX_DURABLE_TRACE_FRAME_BYTES: usize = 128 * 1024;
pub const MAX_RETAINED_TRACE_RECORDS: usize = 512;
pub const MAX_TRACE_ARTIFACT_STORE_BYTES: u64 = 64 * 1024 * 1024;

const ARTIFACT_DIR: &str = "artifacts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableTraceSeverity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableTraceRedactionStatus {
    Applied,
    RejectedUnsafeInput,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableTraceCorrelation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_process_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableTraceArtifactRef {
    pub artifact_ref: String,
    pub sha256: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableTraceRecord {
    pub schema_family: String,
    pub schema_version: u32,
    pub trace_id: String,
    pub kind: String,
    pub severity: DurableTraceSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_sequence: Option<u64>,
    #[serde(default)]
    pub correlation: DurableTraceCorrelation,
    pub redaction_status: DurableTraceRedactionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_preview: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<DurableTraceArtifactRef>,
    pub active_recovery: bool,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableTraceInput {
    pub kind: String,
    pub severity: DurableTraceSeverity,
    pub event_sequence: Option<u64>,
    pub correlation: DurableTraceCorrelation,
    pub detail: Value,
    pub active_recovery: bool,
}

impl DurableTraceInput {
    pub fn new(kind: impl Into<String>, severity: DurableTraceSeverity, detail: Value) -> Self {
        Self {
            kind: kind.into(),
            severity,
            event_sequence: None,
            correlation: DurableTraceCorrelation::default(),
            detail,
            active_recovery: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DurableTraceScan {
    pub records: Vec<DurableTraceRecord>,
    pub corrupt_tail: bool,
    pub missing: bool,
    pub issue: Option<String>,
    pub truncated: bool,
}

#[derive(Debug)]
pub enum DurableTraceError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    Validation(String),
    Corruption(String),
}

impl fmt::Display for DurableTraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "durable trace I/O failed: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "durable trace serialization failed: {error}")
            }
            Self::Validation(reason) => {
                write!(formatter, "durable trace validation failed: {reason}")
            }
            Self::Corruption(reason) => write!(formatter, "durable trace corruption: {reason}"),
        }
    }
}

impl Error for DurableTraceError {}

impl From<std::io::Error> for DurableTraceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DurableTraceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct DurableTraceFrame {
    frame_version: u32,
    record_length: u64,
    checksum: String,
    record: DurableTraceRecord,
}

#[derive(Debug, Clone)]
pub struct DurableTraceStore {
    root: PathBuf,
    path: PathBuf,
    artifact_root: PathBuf,
    lock_path: PathBuf,
}

impl DurableTraceStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, DurableTraceError> {
        let root = root.as_ref().to_path_buf();
        reject_symlink(&root)?;
        fs::create_dir_all(root.join(ARTIFACT_DIR))?;
        reject_symlink(&root)?;
        let root = fs::canonicalize(root)?;
        let artifact_root = root.join(ARTIFACT_DIR);
        reject_symlink(&artifact_root)?;
        let path = root.join("diagnostics.log");
        let lock_path = root.join("diagnostics.lock");
        Ok(Self {
            root,
            path,
            artifact_root,
            lock_path,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn append(
        &self,
        input: DurableTraceInput,
    ) -> Result<DurableTraceRecord, DurableTraceError> {
        self.append_with_writer(input, write_frame)
    }

    pub fn append_artifact_backed(
        &self,
        input: DurableTraceInput,
        artifact_detail: Value,
    ) -> Result<DurableTraceRecord, DurableTraceError> {
        self.append_artifact_backed_with_writer(input, artifact_detail, write_frame)
    }

    fn append_artifact_backed_with_writer(
        &self,
        input: DurableTraceInput,
        artifact_detail: Value,
        writer: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
    ) -> Result<DurableTraceRecord, DurableTraceError> {
        validate_kind(&input.kind)?;
        let _lock = acquire_lock(&self.lock_path)?;
        if !self.path.exists() {
            create_new_regular_file(&self.path)?.sync_all()?;
            sync_dir(&self.root)?;
        }
        let scan = self.scan_unlocked(MAX_RETAINED_TRACE_RECORDS.saturating_add(1))?;
        if scan.corrupt_tail {
            return Err(DurableTraceError::Corruption(
                scan.issue
                    .unwrap_or_else(|| "trace tail is corrupt".to_owned()),
            ));
        }
        let (persisted_detail, redaction_status) = safe_trace_detail(input.detail);
        let detail_bytes = serde_json::to_vec(&persisted_detail)?;
        let preview = bounded_preview(&detail_bytes);
        let (persisted_artifact, artifact_redaction_status) = safe_trace_detail(artifact_detail);
        let artifact_bytes = serde_json::to_vec(&persisted_artifact)?;
        let timestamp_ms = current_time_ms();
        let correlation = sanitize_correlation(input.correlation);
        let trace_identity = serde_json::to_vec(&serde_json::json!({
            "previous_trace_id": scan.records.last().map(|record| record.trace_id.as_str()),
            "kind": input.kind,
            "severity": input.severity,
            "event_sequence": input.event_sequence,
            "correlation": correlation,
            "detail_sha256": checksum(&detail_bytes),
            "artifact_sha256": checksum(&artifact_bytes),
            "active_recovery": input.active_recovery,
            "timestamp_ms": timestamp_ms,
        }))?;
        let trace_id = format!("trace-{:x}", Sha256::digest(trace_identity));
        let record = DurableTraceRecord {
            schema_family: DURABLE_TRACE_SCHEMA_FAMILY.to_owned(),
            schema_version: CURRENT_DURABLE_TRACE_SCHEMA_VERSION,
            trace_id,
            kind: input.kind,
            severity: input.severity,
            event_sequence: input.event_sequence,
            correlation,
            redaction_status: if redaction_status == DurableTraceRedactionStatus::Applied {
                artifact_redaction_status
            } else {
                redaction_status
            },
            detail_preview: Some(preview),
            artifact_refs: vec![self.write_artifact(&artifact_bytes)?],
            active_recovery: input.active_recovery,
            timestamp_ms,
        };
        validate_record(&record)?;
        let frame = frame_for_record(&record)?;
        writer(&self.path, &frame)?;
        self.retain_unlocked()?;
        Ok(record)
    }

    fn append_with_writer(
        &self,
        input: DurableTraceInput,
        writer: impl FnOnce(&Path, &[u8]) -> std::io::Result<()>,
    ) -> Result<DurableTraceRecord, DurableTraceError> {
        validate_kind(&input.kind)?;
        let _lock = acquire_lock(&self.lock_path)?;
        if !self.path.exists() {
            create_new_regular_file(&self.path)?.sync_all()?;
            sync_dir(&self.root)?;
        }
        let scan = self.scan_unlocked(MAX_RETAINED_TRACE_RECORDS.saturating_add(1))?;
        if scan.corrupt_tail {
            return Err(DurableTraceError::Corruption(
                scan.issue
                    .unwrap_or_else(|| "trace tail is corrupt".to_owned()),
            ));
        }
        let (persisted_detail, redaction_status) = safe_trace_detail(input.detail);
        let detail_bytes = serde_json::to_vec(&persisted_detail)?;
        let preview = bounded_preview(&detail_bytes);
        let artifact_refs = if detail_bytes.len() > MAX_TRACE_DETAIL_PREVIEW_BYTES {
            vec![self.write_artifact(&detail_bytes)?]
        } else {
            Vec::new()
        };
        let timestamp_ms = current_time_ms();
        let correlation = sanitize_correlation(input.correlation);
        let trace_identity = serde_json::to_vec(&serde_json::json!({
            "previous_trace_id": scan.records.last().map(|record| record.trace_id.as_str()),
            "kind": input.kind,
            "severity": input.severity,
            "event_sequence": input.event_sequence,
            "correlation": correlation,
            "detail_sha256": checksum(&detail_bytes),
            "active_recovery": input.active_recovery,
            "timestamp_ms": timestamp_ms,
        }))?;
        let trace_id = format!("trace-{:x}", Sha256::digest(trace_identity));
        let record = DurableTraceRecord {
            schema_family: DURABLE_TRACE_SCHEMA_FAMILY.to_owned(),
            schema_version: CURRENT_DURABLE_TRACE_SCHEMA_VERSION,
            trace_id,
            kind: input.kind,
            severity: input.severity,
            event_sequence: input.event_sequence,
            correlation,
            redaction_status,
            detail_preview: Some(preview),
            artifact_refs,
            active_recovery: input.active_recovery,
            timestamp_ms,
        };
        validate_record(&record)?;
        let frame = frame_for_record(&record)?;
        writer(&self.path, &frame)?;
        self.retain_unlocked()?;
        Ok(record)
    }

    pub fn scan(&self, max_records: usize) -> Result<DurableTraceScan, DurableTraceError> {
        let _lock = acquire_lock(&self.lock_path)?;
        self.scan_unlocked(max_records)
    }

    pub fn scan_existing(
        root: impl AsRef<Path>,
        max_records: usize,
    ) -> Result<DurableTraceScan, DurableTraceError> {
        let root = root.as_ref();
        if !root.exists() || !root.join("diagnostics.log").exists() {
            return Ok(missing_scan());
        }
        reject_symlink(root)?;
        let root = fs::canonicalize(root)?;
        let store = Self {
            path: root.join("diagnostics.log"),
            lock_path: root.join("diagnostics.lock"),
            artifact_root: root.join(ARTIFACT_DIR),
            root,
        };
        let _lock = if store.lock_path.exists() {
            Some(acquire_existing_lock(&store.lock_path)?)
        } else {
            None
        };
        store.scan_unlocked(max_records)
    }

    fn scan_unlocked(&self, max_records: usize) -> Result<DurableTraceScan, DurableTraceError> {
        if !self.path.exists() {
            return Ok(missing_scan());
        }
        reject_symlink(&self.path)?;
        let mut records = Vec::new();
        let mut truncated = false;
        let mut reader = BufReader::new(open_read_file(&self.path)?);
        loop {
            let mut bytes = Vec::new();
            let mut limited =
                Read::by_ref(&mut reader).take((MAX_DURABLE_TRACE_FRAME_BYTES + 2) as u64);
            let read = limited.read_until(b'\n', &mut bytes)?;
            if read == 0 {
                break;
            }
            if bytes.last() != Some(&b'\n') {
                return Ok(corrupt_scan(
                    records,
                    "incomplete diagnostics evidence tail",
                ));
            }
            bytes.pop();
            let frame = match decode_frame(&bytes) {
                Ok(frame) => frame,
                Err(error) => return Ok(corrupt_scan(records, error.to_string())),
            };
            if records.len() < max_records {
                records.push(frame.record);
            } else {
                truncated = true;
            }
        }
        Ok(DurableTraceScan {
            records,
            corrupt_tail: false,
            missing: false,
            issue: None,
            truncated,
        })
    }

    fn write_artifact(&self, bytes: &[u8]) -> Result<DurableTraceArtifactRef, DurableTraceError> {
        if bytes.len() > MAX_TRACE_DETAIL_ARTIFACT_BYTES {
            return Err(DurableTraceError::Validation(format!(
                "trace detail exceeds {MAX_TRACE_DETAIL_ARTIFACT_BYTES} bytes"
            )));
        }
        reject_symlink(&self.artifact_root)?;
        let sha256 = checksum(bytes);
        let file_name = format!("{}.json", sha256.trim_start_matches("sha256:"));
        let path = self.artifact_root.join(&file_name);
        if !path.exists()
            && stored_artifact_bytes(&self.artifact_root)?.saturating_add(bytes.len() as u64)
                > MAX_TRACE_ARTIFACT_STORE_BYTES
        {
            return Err(DurableTraceError::Validation(format!(
                "durable trace artifact store exceeds {MAX_TRACE_ARTIFACT_STORE_BYTES} bytes"
            )));
        }
        write_content_addressed(&path, bytes)?;
        Ok(DurableTraceArtifactRef {
            artifact_ref: format!("{ARTIFACT_DIR}/{file_name}"),
            sha256,
            byte_len: bytes.len() as u64,
        })
    }

    fn retain_unlocked(&self) -> Result<(), DurableTraceError> {
        let scan = self.scan_unlocked(usize::MAX)?;
        if scan.corrupt_tail {
            return Ok(());
        }
        if scan.records.len() <= MAX_RETAINED_TRACE_RECORDS {
            return self.gc_artifacts(&scan.records);
        }
        let latest_by_subject = scan
            .records
            .iter()
            .enumerate()
            .filter_map(|(index, record)| recovery_subject(record).map(|subject| (subject, index)))
            .collect::<BTreeMap<_, _>>();
        let mut active = scan
            .records
            .iter()
            .enumerate()
            .filter(|(index, record)| {
                let latest = recovery_subject(record)
                    .and_then(|subject| latest_by_subject.get(&subject).copied());
                record.active_recovery
                    && match latest {
                        Some(latest) => latest == *index,
                        None => true,
                    }
            })
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>();
        let active_ids = active
            .iter()
            .map(|record| record.trace_id.clone())
            .collect::<BTreeSet<_>>();
        let mut terminal = scan
            .records
            .into_iter()
            .filter(|record| !active_ids.contains(&record.trace_id))
            .collect::<Vec<_>>();
        active.sort_by_key(|record| record.timestamp_ms);
        terminal.sort_by_key(|record| record.timestamp_ms);
        let mut keep = Vec::new();
        keep.extend(active.into_iter().rev().take(MAX_RETAINED_TRACE_RECORDS));
        let remaining = MAX_RETAINED_TRACE_RECORDS.saturating_sub(keep.len());
        keep.extend(terminal.into_iter().rev().take(remaining));
        keep.sort_by_key(|record| record.timestamp_ms);
        let mut bytes = Vec::new();
        for record in &keep {
            bytes.extend(frame_for_record(record)?);
        }
        write_atomic(&self.path, &bytes)?;
        self.gc_artifacts(&keep)
    }

    fn gc_artifacts(&self, retained: &[DurableTraceRecord]) -> Result<(), DurableTraceError> {
        let retained = retained
            .iter()
            .flat_map(|record| record.artifact_refs.iter())
            .filter_map(|artifact| Path::new(&artifact.artifact_ref).file_name())
            .map(|name| name.to_os_string())
            .collect::<BTreeSet<_>>();
        reject_symlink(&self.artifact_root)?;
        for entry in fs::read_dir(&self.artifact_root)? {
            let entry = entry?;
            let path = entry.path();
            reject_symlink(&path)?;
            if entry.file_type()?.is_file() && !retained.contains(&entry.file_name()) {
                fs::remove_file(path)?;
            }
        }
        sync_dir(&self.artifact_root)?;
        Ok(())
    }
}

fn missing_scan() -> DurableTraceScan {
    DurableTraceScan {
        records: Vec::new(),
        corrupt_tail: false,
        missing: true,
        issue: None,
        truncated: false,
    }
}

fn recovery_subject(record: &DurableTraceRecord) -> Option<String> {
    record
        .correlation
        .child_task_id
        .as_ref()
        .map(|value| format!("child:{value}"))
        .or_else(|| {
            record
                .correlation
                .service_correlation_id
                .as_ref()
                .map(|value| format!("service:{value}"))
        })
        .or_else(|| {
            record
                .correlation
                .effect_id
                .as_ref()
                .map(|value| format!("effect:{value}"))
        })
}

fn corrupt_scan(records: Vec<DurableTraceRecord>, issue: impl Into<String>) -> DurableTraceScan {
    DurableTraceScan {
        records,
        corrupt_tail: true,
        missing: false,
        issue: Some(issue.into()),
        truncated: false,
    }
}

fn frame_for_record(record: &DurableTraceRecord) -> Result<Vec<u8>, DurableTraceError> {
    let record_bytes = canonical_record_bytes(record)?;
    if record_bytes.len() > MAX_DURABLE_TRACE_FRAME_BYTES {
        return Err(DurableTraceError::Validation(format!(
            "trace record exceeds {MAX_DURABLE_TRACE_FRAME_BYTES} bytes"
        )));
    }
    let frame = DurableTraceFrame {
        frame_version: CURRENT_DURABLE_TRACE_FRAME_VERSION,
        record_length: record_bytes.len() as u64,
        checksum: checksum(&record_bytes),
        record: record.clone(),
    };
    let mut frame_bytes = serde_json::to_vec(&frame)?;
    if frame_bytes.len() > MAX_DURABLE_TRACE_FRAME_BYTES {
        return Err(DurableTraceError::Validation(format!(
            "trace frame exceeds {MAX_DURABLE_TRACE_FRAME_BYTES} bytes"
        )));
    }
    frame_bytes.push(b'\n');
    Ok(frame_bytes)
}

#[cfg(test)]
pub(crate) fn frame_for_test(record: &DurableTraceRecord) -> Result<Vec<u8>, DurableTraceError> {
    frame_for_record(record)
}

fn decode_frame(bytes: &[u8]) -> Result<DurableTraceFrame, DurableTraceError> {
    if bytes.len() > MAX_DURABLE_TRACE_FRAME_BYTES {
        return Err(DurableTraceError::Corruption(
            "frame exceeds maximum size".to_owned(),
        ));
    }
    let frame = serde_json::from_slice::<DurableTraceFrame>(bytes)?;
    if frame.frame_version != CURRENT_DURABLE_TRACE_FRAME_VERSION {
        return Err(DurableTraceError::Corruption(
            "unsupported trace frame version".to_owned(),
        ));
    }
    validate_record(&frame.record)?;
    let record_bytes = canonical_record_bytes(&frame.record)?;
    if frame.record_length != record_bytes.len() as u64 || frame.checksum != checksum(&record_bytes)
    {
        return Err(DurableTraceError::Corruption(
            "trace frame checksum mismatch".to_owned(),
        ));
    }
    Ok(frame)
}

fn validate_record(record: &DurableTraceRecord) -> Result<(), DurableTraceError> {
    if record.schema_family != DURABLE_TRACE_SCHEMA_FAMILY
        || record.schema_version != CURRENT_DURABLE_TRACE_SCHEMA_VERSION
    {
        return Err(DurableTraceError::Corruption(
            "trace schema is incompatible".to_owned(),
        ));
    }
    validate_kind(&record.kind)?;
    validate_identifier("trace_id", &record.trace_id)?;
    validate_correlation(&record.correlation)?;
    for artifact in &record.artifact_refs {
        validate_artifact_ref(artifact)?;
    }
    Ok(())
}

fn validate_kind(value: &str) -> Result<(), DurableTraceError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(DurableTraceError::Validation(
            "trace kind must be a bounded stable identifier".to_owned(),
        ));
    }
    Ok(())
}

fn validate_correlation(correlation: &DurableTraceCorrelation) -> Result<(), DurableTraceError> {
    for (name, value) in [
        ("session_id", correlation.session_id.as_deref()),
        ("turn_id", correlation.turn_id.as_deref()),
        ("effect_id", correlation.effect_id.as_deref()),
        ("event_id", correlation.event_id.as_deref()),
        (
            "approval_request_id",
            correlation.approval_request_id.as_deref(),
        ),
        ("child_task_id", correlation.child_task_id.as_deref()),
        ("app_id", correlation.app_id.as_deref()),
        ("app_process_id", correlation.app_process_id.as_deref()),
        ("device_id", correlation.device_id.as_deref()),
        ("port_id", correlation.port_id.as_deref()),
        ("channel_id", correlation.channel_id.as_deref()),
        (
            "service_correlation_id",
            correlation.service_correlation_id.as_deref(),
        ),
    ] {
        if let Some(value) = value {
            validate_identifier(name, value)?;
        }
    }
    Ok(())
}

fn validate_identifier(name: &str, value: &str) -> Result<(), DurableTraceError> {
    if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
        return Err(DurableTraceError::Validation(format!(
            "{name} is empty, oversized, or contains control characters"
        )));
    }
    Ok(())
}

fn validate_artifact_ref(value: &DurableTraceArtifactRef) -> Result<(), DurableTraceError> {
    if !value.sha256.starts_with("sha256:") || value.sha256.len() != "sha256:".len() + 64 {
        return Err(DurableTraceError::Validation(
            "trace artifact digest is malformed".to_owned(),
        ));
    }
    let path = Path::new(&value.artifact_ref);
    if path.is_absolute()
        || path.components().count() != 2
        || !value.artifact_ref.starts_with("artifacts/")
        || value.artifact_ref.contains('\\')
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || redact_string(&value.artifact_ref) != value.artifact_ref
    {
        return Err(DurableTraceError::Validation(
            "trace artifact reference is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn sanitize_correlation(mut correlation: DurableTraceCorrelation) -> DurableTraceCorrelation {
    for value in [
        &mut correlation.session_id,
        &mut correlation.turn_id,
        &mut correlation.effect_id,
        &mut correlation.event_id,
        &mut correlation.approval_request_id,
        &mut correlation.child_task_id,
        &mut correlation.app_id,
        &mut correlation.app_process_id,
        &mut correlation.device_id,
        &mut correlation.port_id,
        &mut correlation.channel_id,
        &mut correlation.service_correlation_id,
    ] {
        if let Some(text) = value.as_mut() {
            *text = redact_string(text);
            if string_contains_host_path(text) {
                *text = REDACTED.to_owned();
            }
        }
    }
    correlation.session_id = correlation
        .session_id
        .map(|value| opaque_trace_ref("session", &value));
    correlation.turn_id = correlation
        .turn_id
        .map(|value| opaque_trace_ref("turn", &value));
    correlation.effect_id = correlation
        .effect_id
        .map(|value| opaque_trace_ref("effect", &value));
    correlation.event_id = correlation
        .event_id
        .map(|value| opaque_trace_ref("event", &value));
    correlation.approval_request_id = correlation
        .approval_request_id
        .map(|value| opaque_trace_ref("approval", &value));
    correlation.child_task_id = correlation
        .child_task_id
        .map(|value| opaque_trace_ref("child", &value));
    correlation.app_id = correlation
        .app_id
        .map(|value| opaque_trace_ref("app", &value));
    correlation.app_process_id = correlation
        .app_process_id
        .map(|value| opaque_trace_ref("app-process", &value));
    correlation.device_id = correlation
        .device_id
        .map(|value| opaque_trace_ref("device", &value));
    correlation.port_id = correlation
        .port_id
        .map(|value| opaque_trace_ref("port", &value));
    correlation.service_correlation_id = correlation
        .service_correlation_id
        .map(|value| opaque_trace_ref("service", &value));
    correlation
}

pub fn opaque_trace_ref(kind: &str, value: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    format!("{kind}:{}", &digest[..16])
}

fn detail_has_forbidden_projection(value: &Value) -> bool {
    match value {
        Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "provider_payload"
                    | "tool_payload"
                    | "channel_payload"
                    | "process_handle"
                    | "raw_child_identity"
            ) || detail_has_forbidden_projection(value)
        }),
        Value::Array(values) => values.iter().any(detail_has_forbidden_projection),
        Value::String(value) => string_contains_host_path(value),
        _ => false,
    }
}

fn safe_trace_detail(value: Value) -> (Value, DurableTraceRedactionStatus) {
    let redacted_detail = redact_value(&value);
    if detail_has_forbidden_projection(&redacted_detail) {
        (
            serde_json::json!({ "payload": REDACTED, "reason": "unsafe diagnostic payload rejected" }),
            DurableTraceRedactionStatus::RejectedUnsafeInput,
        )
    } else {
        (redacted_detail, DurableTraceRedactionStatus::Applied)
    }
}

fn string_contains_host_path(value: &str) -> bool {
    value.split_whitespace().any(|part| {
        let candidate = trim_path_punctuation(part);
        is_absolute_host_path(candidate)
            || candidate
                .split_once('=')
                .is_some_and(|(_, value)| is_absolute_host_path(trim_path_punctuation(value)))
            || candidate.split_once(':').is_some_and(|(label, value)| {
                label.len() != 1 && is_absolute_host_path(trim_path_punctuation(value))
            })
    })
}

fn trim_path_punctuation(value: &str) -> &str {
    value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    })
}

fn is_absolute_host_path(value: &str) -> bool {
    Path::new(value).is_absolute()
        || (value.len() >= 3
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'\\' | b'/'))
}

fn bounded_preview(bytes: &[u8]) -> String {
    let take = bytes.len().min(MAX_TRACE_DETAIL_PREVIEW_BYTES);
    String::from_utf8_lossy(&bytes[..take]).into_owned()
}

fn canonical_record_bytes(record: &DurableTraceRecord) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&serde_json::to_value(record)?)
}

fn checksum(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn current_time_ms() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or(0)
}

fn acquire_lock(path: &Path) -> std::io::Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    let file = open_regular_file(path, options)?;
    FileExt::lock(&file)?;
    Ok(file)
}

fn acquire_existing_lock(path: &Path) -> std::io::Result<File> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    let file = open_regular_file(path, options)?;
    FileExt::lock(&file)?;
    Ok(file)
}

fn create_new_regular_file(path: &Path) -> std::io::Result<File> {
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
            "durable trace path is not a regular file",
        ));
    }
    Ok(file)
}

fn write_frame(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = open_append_file(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), DurableTraceError> {
    let parent = path
        .parent()
        .ok_or_else(|| DurableTraceError::Validation("trace path has no parent".to_owned()))?;
    let temp = parent.join(".diagnostics.log.tmp");
    let result = (|| -> std::io::Result<()> {
        reject_symlink(&temp)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        let mut file = open_regular_file(&temp, options)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    Ok(result?)
}

fn write_content_addressed(path: &Path, bytes: &[u8]) -> Result<(), DurableTraceError> {
    reject_symlink(path)?;
    if path.exists() {
        let mut existing = Vec::new();
        open_read_file(path)?.read_to_end(&mut existing)?;
        if existing != bytes {
            return Err(DurableTraceError::Validation(
                "content-addressed trace artifact collision".to_owned(),
            ));
        }
        return Ok(());
    }
    let temp = path.with_extension("json.tmp");
    reject_symlink(&temp)?;
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = open_regular_file(&temp, options)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(&temp, path)?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn stored_artifact_bytes(root: &Path) -> Result<u64, DurableTraceError> {
    reject_symlink(root)?;
    let mut total = 0_u64;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

fn reject_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "durable trace path is a symlink",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_dir(path: &Path) -> std::io::Result<()> {
    OpenOptions::new().read(true).open(path)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_before_persist_and_records_full_optional_correlation() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let store = DurableTraceStore::open(root.path())?;
        let mut input = DurableTraceInput::new(
            "work.transition",
            DurableTraceSeverity::Info,
            json!({
                "api_key": "sk-secret",
                "summary": "safe"
            }),
        );
        input.event_sequence = Some(7);
        input.correlation = DurableTraceCorrelation {
            session_id: Some("session-1".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            effect_id: Some("effect-1".to_owned()),
            event_id: Some("event-0007".to_owned()),
            approval_request_id: Some("approval-1".to_owned()),
            child_task_id: Some("child-1".to_owned()),
            app_id: Some("app-1".to_owned()),
            app_process_id: Some("app-process-1".to_owned()),
            device_id: Some("device-1".to_owned()),
            port_id: Some("port-1".to_owned()),
            channel_id: Some("telegram".to_owned()),
            service_correlation_id: Some("service-1".to_owned()),
        };
        let record = store.append(input)?;

        assert_eq!(record.schema_family, DURABLE_TRACE_SCHEMA_FAMILY);
        assert_eq!(record.event_sequence, Some(7));
        assert!(record
            .correlation
            .approval_request_id
            .as_deref()
            .is_some_and(|value| value.starts_with("approval:")));
        assert!(record
            .correlation
            .child_task_id
            .as_deref()
            .is_some_and(|value| value.starts_with("child:")));
        assert_ne!(record.correlation.child_task_id.as_deref(), Some("child-1"));
        let persisted = fs::read_to_string(root.path().join("diagnostics.log"))?;
        assert!(!persisted.contains("sk-secret"));
        assert!(persisted.contains(REDACTED));
        Ok(())
    }

    #[test]
    fn oversized_detail_uses_bounded_preview_and_artifact_ref() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let store = DurableTraceStore::open(root.path())?;
        let detail = json!({ "detail": "x".repeat(MAX_TRACE_DETAIL_PREVIEW_BYTES + 128) });
        let record = store.append(DurableTraceInput::new(
            "runtime.detail",
            DurableTraceSeverity::Warning,
            detail,
        ))?;

        assert!(record.detail_preview.unwrap_or_default().len() <= MAX_TRACE_DETAIL_PREVIEW_BYTES);
        assert_eq!(record.artifact_refs.len(), 1);
        assert!(root
            .path()
            .join(&record.artifact_refs[0].artifact_ref)
            .exists());
        Ok(())
    }

    #[test]
    fn corrupt_tail_is_evidence_only_and_append_failure_preserves_prior_records(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let store = DurableTraceStore::open(root.path())?;
        store.append(DurableTraceInput::new(
            "runtime.ok",
            DurableTraceSeverity::Info,
            json!({"ok": true}),
        ))?;
        let error = store.append_with_writer(
            DurableTraceInput::new(
                "runtime.fail",
                DurableTraceSeverity::Error,
                json!({"ok": false}),
            ),
            |_path, _bytes| Err(std::io::Error::other("injected trace failure")),
        );
        assert!(matches!(error, Err(DurableTraceError::Io(_))));
        assert_eq!(store.scan(10)?.records.len(), 1);

        let mut file = open_append_file(&root.path().join("diagnostics.log"))?;
        file.write_all(b"{bad")?;
        file.sync_all()?;
        let scan = store.scan(10)?;
        assert!(scan.corrupt_tail);
        assert_eq!(scan.records.len(), 1);
        Ok(())
    }

    #[test]
    fn retention_keeps_active_recovery_before_terminal_evidence() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let store = DurableTraceStore::open(root.path())?;
        let orphaned = store.append(DurableTraceInput::new(
            "runtime.evicted_artifact",
            DurableTraceSeverity::Info,
            json!({"detail": "x".repeat(MAX_TRACE_DETAIL_PREVIEW_BYTES + 128)}),
        ))?;
        let orphaned_path = root.path().join(&orphaned.artifact_refs[0].artifact_ref);
        for index in 0..(MAX_RETAINED_TRACE_RECORDS + 10) {
            let mut input = DurableTraceInput::new(
                "runtime.retention",
                DurableTraceSeverity::Info,
                json!({"index": index}),
            );
            input.active_recovery = index < 10;
            store.append(input)?;
        }
        let scan = store.scan(usize::MAX)?;
        assert_eq!(scan.records.len(), MAX_RETAINED_TRACE_RECORDS);
        assert_eq!(
            scan.records
                .iter()
                .filter(|record| record.active_recovery)
                .count(),
            10
        );
        let before = scan
            .records
            .last()
            .ok_or("missing retained trace")?
            .trace_id
            .clone();
        let after = store
            .append(DurableTraceInput::new(
                "runtime.retention",
                DurableTraceSeverity::Info,
                json!({"index": "same"}),
            ))?
            .trace_id;
        assert_ne!(before, after);
        assert!(!orphaned_path.exists());
        Ok(())
    }

    #[test]
    fn rejects_generic_host_paths_and_process_handles_before_persist() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let store = DurableTraceStore::open(root.path())?;
        let record = store.append(DurableTraceInput::new(
            "runtime.unsafe",
            DurableTraceSeverity::Warning,
            json!({
                "message": "cwd:/opt/private/runtime.db file=C:\\Users\\alice\\runtime.db",
                "process_handle": 42,
            }),
        ))?;
        assert_eq!(
            record.redaction_status,
            DurableTraceRedactionStatus::RejectedUnsafeInput
        );
        let persisted = fs::read_to_string(root.path().join("diagnostics.log"))?;
        assert!(!persisted.contains("/opt/private/runtime.db"));
        assert!(!persisted.contains("Users\\\\alice"));
        assert!(!persisted.contains("process_handle"));
        Ok(())
    }

    #[test]
    fn read_only_scan_does_not_create_a_missing_store() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?.path().join("missing-diagnostics");
        let scan = DurableTraceStore::scan_existing(&root, 10)?;
        assert!(scan.missing);
        assert!(!root.exists());
        Ok(())
    }

    #[test]
    fn concurrent_first_append_does_not_drop_evidence() -> Result<(), Box<dyn Error>> {
        use std::sync::{Arc, Barrier};

        let temp = tempfile::tempdir()?;
        let root = temp.path().join("diagnostics");
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for index in 0..2 {
            let root = root.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || -> Result<(), String> {
                let store = DurableTraceStore::open(root).map_err(|error| error.to_string())?;
                barrier.wait();
                store
                    .append(DurableTraceInput::new(
                        "runtime.concurrent_first_append",
                        DurableTraceSeverity::Info,
                        json!({"index": index}),
                    ))
                    .map_err(|error| error.to_string())?;
                Ok(())
            }));
        }
        for handle in handles {
            handle
                .join()
                .map_err(|_| "trace append thread panicked")??;
        }
        assert_eq!(DurableTraceStore::scan_existing(root, 10)?.records.len(), 2);
        Ok(())
    }
}
