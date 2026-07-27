use crate::{SessionRuntimeExecutionProjection, SessionUxDiagnostics};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDiagnosticsAggregate {
    pub schema_id: &'static str,
    #[serde(serialize_with = "serialize_safe_string")]
    pub session_ref: String,
    pub exists: bool,
    pub message_count: usize,
    pub last_consolidated: usize,
    pub metadata_key_count: usize,
    #[serde(serialize_with = "serialize_safe_string_vec")]
    pub recovery_markers: Vec<String>,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub checkpoint_phase: Option<String>,
    pub diagnostics_ref_count: usize,
    #[serde(serialize_with = "serialize_safe_string_vec")]
    pub diagnostics_refs: Vec<String>,
    pub legal_start: usize,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub workflow_state: Option<String>,
    #[serde(serialize_with = "serialize_optional_safe_string")]
    pub workflow_blocked_reason: Option<String>,
    pub runtime_execution: Option<SessionRuntimeExecutionDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRuntimeExecutionDiagnostics {
    pub pending_count: u64,
    pub outcome_count: u64,
    pub accepted_count: u64,
    pub stale_count: u64,
    pub safe_artifact_ref_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionDiagnosticsError {
    Serialization,
    RawDiagnosticMaterial,
}

impl fmt::Display for SessionDiagnosticsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization => formatter.write_str("session diagnostics serialization failed"),
            Self::RawDiagnosticMaterial => formatter.write_str("raw diagnostic material rejected"),
        }
    }
}

impl Error for SessionDiagnosticsError {}

const REDACTED: &str = "[REDACTED]";

fn serialize_safe_string<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&safe_string(value))
}

fn serialize_safe_string_vec<S>(values: &[String], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    values
        .iter()
        .map(|value| safe_string(value))
        .collect::<Vec<_>>()
        .serialize(serializer)
}

fn serialize_optional_safe_string<S>(
    value: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    value
        .as_ref()
        .map(|item| safe_string(item))
        .serialize(serializer)
}

fn safe_string(value: &str) -> String {
    if raw_text(value) {
        REDACTED.to_owned()
    } else {
        value.to_owned()
    }
}

pub fn build_session_diagnostics_aggregate(
    diagnostics: &SessionUxDiagnostics,
) -> Result<SessionDiagnosticsAggregate, SessionDiagnosticsError> {
    reject_input_raw_material(diagnostics)?;
    let aggregate = SessionDiagnosticsAggregate {
        schema_id: "spec030_session_diagnostics.v1",
        session_ref: opaque_ref("session", &diagnostics.key),
        exists: diagnostics.exists,
        message_count: diagnostics.message_count,
        last_consolidated: diagnostics.last_consolidated,
        metadata_key_count: diagnostics.metadata_keys.len(),
        recovery_markers: diagnostics.recovery_markers.clone(),
        checkpoint_phase: diagnostics.checkpoint_phase.clone(),
        diagnostics_ref_count: diagnostics.diagnostics_refs.len(),
        diagnostics_refs: diagnostics
            .diagnostics_refs
            .iter()
            .map(|reference| opaque_ref("diagnostics", reference))
            .collect(),
        legal_start: diagnostics.legal_start,
        workflow_state: diagnostics
            .runtime_workflow
            .as_ref()
            .and_then(|workflow| workflow.state.clone()),
        workflow_blocked_reason: diagnostics
            .runtime_workflow
            .as_ref()
            .and_then(|workflow| workflow.blocked_reason.clone()),
        runtime_execution: diagnostics
            .runtime_execution
            .as_ref()
            .map(runtime_execution),
    };
    reject_output_raw_material(&aggregate)?;
    Ok(aggregate)
}

fn runtime_execution(
    execution: &SessionRuntimeExecutionProjection,
) -> SessionRuntimeExecutionDiagnostics {
    SessionRuntimeExecutionDiagnostics {
        pending_count: execution.pending_count,
        outcome_count: execution.outcome_count,
        accepted_count: execution.decisions.accepted,
        stale_count: execution.decisions.stale,
        safe_artifact_ref_count: execution.safe_artifact_ref_count,
    }
}

fn reject_input_raw_material(
    diagnostics: &SessionUxDiagnostics,
) -> Result<(), SessionDiagnosticsError> {
    if diagnostics
        .metadata_keys
        .iter()
        .any(|value| raw_text(value))
        || diagnostics
            .recovery_markers
            .iter()
            .any(|value| raw_text(value))
        || diagnostics
            .checkpoint_phase
            .as_ref()
            .is_some_and(|value| raw_text(value))
        || diagnostics
            .diagnostics_refs
            .iter()
            .any(|value| raw_text(value))
    {
        Err(SessionDiagnosticsError::RawDiagnosticMaterial)
    } else {
        Ok(())
    }
}

fn reject_output_raw_material(
    aggregate: &SessionDiagnosticsAggregate,
) -> Result<(), SessionDiagnosticsError> {
    let value =
        serde_json::to_value(aggregate).map_err(|_| SessionDiagnosticsError::Serialization)?;
    if value_contains_raw_material(&value) {
        Err(SessionDiagnosticsError::RawDiagnosticMaterial)
    } else {
        Ok(())
    }
}

fn value_contains_raw_material(value: &Value) -> bool {
    match value {
        Value::String(text) => raw_text(text),
        Value::Array(items) => items.iter().any(value_contains_raw_material),
        Value::Object(map) => map.values().any(value_contains_raw_material),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn raw_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let normalized: String = text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    lower.contains("sk-")
        || lower.contains("bearer ")
        || lower.contains("private key")
        || lower.contains("-----begin private key-----")
        || text.contains("RAW_")
        || text.contains("/Users/")
        || text.contains("/home/")
        || text.starts_with('/')
        || text.starts_with("\\\\")
        || contains_windows_drive_path(text)
        || lower.contains("provider-secret")
        || lower.contains("process_handle")
        || normalized.contains("processhandle")
        || normalized.contains("rawstdout")
        || normalized.contains("rawstderr")
        || normalized.contains("standardoutputraw")
        || normalized.contains("rawproviderpayload")
}

fn contains_windows_drive_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    })
}

fn opaque_ref(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{prefix}:sha256:{:x}", digest)[..prefix.len() + 8 + 16].to_owned()
}
