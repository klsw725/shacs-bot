use crate::redaction::{redact_value, REDACTED};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsSeverity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsKind {
    Runtime,
    Configuration,
    Session,
    Provider,
    Tool,
    Subagent,
    Api,
    Crash,
    Recovery,
    Redaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Started,
    Ok,
    Error,
    Waiting,
    Skipped,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DiagnosticsCorrelation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_correlation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalLogRecord {
    pub timestamp_ms: u64,
    pub severity: DiagnosticsSeverity,
    pub kind: DiagnosticsKind,
    pub message: String,
    #[serde(default)]
    pub correlation: DiagnosticsCorrelation,
    #[serde(default)]
    pub fields: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceRecord {
    pub timestamp_ms: u64,
    pub name: String,
    pub status: TraceStatus,
    #[serde(default)]
    pub correlation: DiagnosticsCorrelation,
    #[serde(default)]
    pub fields: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticsRecord {
    pub timestamp_ms: u64,
    pub severity: DiagnosticsSeverity,
    pub kind: DiagnosticsKind,
    pub summary: String,
    #[serde(default)]
    pub correlation: DiagnosticsCorrelation,
    #[serde(default)]
    pub detail: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrashEvidence {
    pub timestamp_ms: u64,
    pub summary: String,
    #[serde(default)]
    pub correlation: DiagnosticsCorrelation,
    #[serde(default)]
    pub fields: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveryEvidence {
    pub timestamp_ms: u64,
    pub status: TraceStatus,
    pub summary: String,
    #[serde(default)]
    pub correlation: DiagnosticsCorrelation,
    #[serde(default)]
    pub fields: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    pub generated_at_ms: u64,
    pub runtime: Value,
    #[serde(default)]
    pub operational_logs: Vec<OperationalLogRecord>,
    #[serde(default)]
    pub traces: Vec<TraceRecord>,
    #[serde(default)]
    pub diagnostics: Vec<DiagnosticsRecord>,
    #[serde(default)]
    pub crash_evidence: Vec<CrashEvidence>,
    #[serde(default)]
    pub recovery_evidence: Vec<RecoveryEvidence>,
    #[serde(default)]
    pub provider_progress: Vec<Value>,
    #[serde(default)]
    pub tool_progress: Vec<Value>,
    #[serde(default)]
    pub subagent_progress: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsBundleManifest {
    pub generated_at_ms: u64,
    pub format_version: u8,
    pub files: Vec<String>,
    pub redaction: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsBundleOutcome {
    pub path: PathBuf,
    pub manifest: DiagnosticsBundleManifest,
}

impl OperationalLogRecord {
    pub fn new(
        severity: DiagnosticsSeverity,
        kind: DiagnosticsKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_ms: current_time_ms(),
            severity,
            kind,
            message: message.into(),
            correlation: DiagnosticsCorrelation::default(),
            fields: Value::Object(Default::default()),
        }
    }
}

impl DiagnosticsRecord {
    pub fn new(
        severity: DiagnosticsSeverity,
        kind: DiagnosticsKind,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_ms: current_time_ms(),
            severity,
            kind,
            summary: summary.into(),
            correlation: DiagnosticsCorrelation::default(),
            detail: Value::Object(Default::default()),
        }
    }

    pub fn safe_rejected_payload(summary: impl Into<String>) -> Self {
        let mut record = Self::new(
            DiagnosticsSeverity::Warning,
            DiagnosticsKind::Redaction,
            summary,
        );
        record.detail =
            json!({ "payload": REDACTED, "reason": "unsafe diagnostic payload rejected" });
        record
    }
}

impl DiagnosticsSnapshot {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            generated_at_ms: current_time_ms(),
            runtime: json!({ "status": "unavailable" }),
            operational_logs: Vec::new(),
            traces: Vec::new(),
            diagnostics: vec![DiagnosticsRecord::new(
                DiagnosticsSeverity::Info,
                DiagnosticsKind::Runtime,
                reason,
            )],
            crash_evidence: Vec::new(),
            recovery_evidence: Vec::new(),
            provider_progress: Vec::new(),
            tool_progress: Vec::new(),
            subagent_progress: Vec::new(),
        }
    }

    pub fn redacted_value(&self) -> Value {
        match serde_json::to_value(self) {
            Ok(value) => redact_value(&value),
            Err(error) => json!({
                "generated_at_ms": current_time_ms(),
                "runtime": { "status": "serialization_error" },
                "diagnostics": [{
                    "timestamp_ms": current_time_ms(),
                    "severity": "error",
                    "kind": "redaction",
                    "summary": "diagnostics snapshot could not be serialized safely",
                    "correlation": {},
                    "detail": { "error": error.to_string() }
                }]
            }),
        }
    }
}

pub fn write_diagnostics_bundle(
    path: impl AsRef<Path>,
    snapshot: &DiagnosticsSnapshot,
) -> io::Result<DiagnosticsBundleOutcome> {
    let path = path.as_ref();
    let manifest = DiagnosticsBundleManifest {
        generated_at_ms: current_time_ms(),
        format_version: 1,
        files: vec!["manifest.json".to_owned(), "snapshot.json".to_owned()],
        redaction: "all files are serialized from redacted JSON projections only".to_owned(),
    };
    let file = File::create(path)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive
        .start_file("manifest.json", options)
        .map_err(zip_io_error)?;
    archive.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    archive
        .start_file("snapshot.json", options)
        .map_err(zip_io_error)?;
    archive.write_all(&serde_json::to_vec_pretty(&snapshot.redacted_value())?)?;
    archive.finish().map_err(zip_io_error)?;
    Ok(DiagnosticsBundleOutcome {
        path: path.to_path_buf(),
        manifest,
    })
}

fn zip_io_error(error: zip::result::ZipError) -> io::Error {
    io::Error::other(error)
}

pub fn current_time_ms() -> u64 {
    let timestamp = Utc::now().timestamp_millis();
    u64::try_from(timestamp).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redaction::REDACTED;
    use std::io::Read;

    #[test]
    fn diagnostics_record_classifies_kind_severity_and_correlation() {
        let mut record = DiagnosticsRecord::new(
            DiagnosticsSeverity::Error,
            DiagnosticsKind::Provider,
            "provider failed",
        );
        record.correlation = DiagnosticsCorrelation {
            session_id: Some("session-1".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            effect_id: Some("effect-1".to_owned()),
            child_task_id: Some("child-1".to_owned()),
            service_correlation_id: Some("svc-1".to_owned()),
        };

        assert_eq!(record.severity, DiagnosticsSeverity::Error);
        assert_eq!(record.kind, DiagnosticsKind::Provider);
        assert_eq!(record.correlation.session_id.as_deref(), Some("session-1"));
        assert_eq!(record.correlation.child_task_id.as_deref(), Some("child-1"));
    }

    #[test]
    fn diagnostics_redaction_masks_secret_path_env_and_payload_before_serialization() {
        let snapshot = DiagnosticsSnapshot {
            generated_at_ms: 1,
            runtime: json!({
                "config_path": "/Users/me/.config/shacs/config.json",
                "credential_path": "/Users/me/.config/shacs/credentials.json",
                "env": "OPENAI_API_KEY=sk-secret normal=value"
            }),
            operational_logs: vec![OperationalLogRecord {
                timestamp_ms: 1,
                severity: DiagnosticsSeverity::Info,
                kind: DiagnosticsKind::Runtime,
                message: "ok".to_owned(),
                correlation: DiagnosticsCorrelation::default(),
                fields: json!({ "authorization": "Bearer secret" }),
            }],
            traces: Vec::new(),
            diagnostics: Vec::new(),
            crash_evidence: Vec::new(),
            recovery_evidence: Vec::new(),
            provider_progress: vec![json!({ "payload": { "refresh_token": "secret" } })],
            tool_progress: Vec::new(),
            subagent_progress: Vec::new(),
        };

        let serialized = serde_json::to_string(&snapshot.redacted_value()).unwrap_or_default();

        assert!(!serialized.contains("sk-secret"));
        assert!(!serialized.contains("Bearer secret"));
        assert!(!serialized.contains("credentials.json"));
        assert!(serialized.contains(REDACTED));
    }

    #[test]
    fn redaction_failure_records_safe_diagnostic_without_original_payload() {
        let record = DiagnosticsRecord::safe_rejected_payload("unsafe payload rejected");
        let serialized = serde_json::to_string(&record).unwrap_or_default();

        assert!(serialized.contains(REDACTED));
        assert!(!serialized.contains("original-secret"));
    }

    #[test]
    fn diagnostics_bundle_contains_only_redacted_json_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("shacs-diagnostics-test-{}", current_time_ms()));
        std::fs::create_dir_all(&temp_dir).unwrap_or(());
        let path = temp_dir.join("bundle.zip");
        let snapshot = DiagnosticsSnapshot {
            generated_at_ms: 1,
            runtime: json!({ "api_key": "sk-secret" }),
            operational_logs: Vec::new(),
            traces: Vec::new(),
            diagnostics: Vec::new(),
            crash_evidence: Vec::new(),
            recovery_evidence: Vec::new(),
            provider_progress: Vec::new(),
            tool_progress: Vec::new(),
            subagent_progress: Vec::new(),
        };

        let outcome = write_diagnostics_bundle(&path, &snapshot).unwrap_or_else(|error| {
            panic!("bundle should be written: {error}");
        });
        assert_eq!(outcome.manifest.files.len(), 2);
        let file = File::open(&path).unwrap_or_else(|error| panic!("bundle open failed: {error}"));
        let mut archive = zip::ZipArchive::new(file)
            .unwrap_or_else(|error| panic!("bundle read failed: {error}"));
        let mut snapshot_json = String::new();
        let mut file = archive
            .by_name("snapshot.json")
            .unwrap_or_else(|error| panic!("snapshot entry read failed: {error}"));
        file.read_to_string(&mut snapshot_json)
            .unwrap_or_else(|error| panic!("snapshot read failed: {error}"));

        assert!(!snapshot_json.contains("sk-secret"));
        assert!(snapshot_json.contains(REDACTED));
    }
}
