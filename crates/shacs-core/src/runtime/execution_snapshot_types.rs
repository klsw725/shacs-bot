use super::super::context_handoff::{ContextArtifactPriority, ContextBudgetDecision};
use serde::{Deserialize, Serialize};
use shacs_projection::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, DataSurface,
    ProcessAdapterKind, SandboxFallback,
};
use std::fmt;

pub const EXECUTION_SNAPSHOT_SCHEMA_V1: &str = "shacs.execution-diagnostic.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigMigrationState {
    Legacy,
    Current,
    RecoveryRequired,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextInclusion {
    Included,
    Truncated,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    Active,
    Disabled,
    Unsupported,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSnapshotRef {
    pub source_ref: String,
    pub schema_version: u32,
    pub migration_state: ConfigMigrationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSelectionSnapshot {
    pub provider: Option<String>,
    pub trusted_runtime: Option<String>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedRuntimeFactRef {
    pub schema_version: u32,
    pub profile_ref: String,
    pub projection_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterSandboxRef {
    pub adapter: ProcessAdapterKind,
    pub mode: SandboxMode,
    pub fallback: SandboxFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialSnapshotRef {
    pub source_kind: Option<CredentialSource>,
    pub status: CredentialStatus,
    pub fingerprint_status: CredentialFingerprintStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceSnapshot {
    pub source_ref: String,
    pub content_digest: String,
    pub inclusion: ContextInclusion,
    pub original_bytes: u64,
    pub included_bytes: u64,
    pub precedence: ContextArtifactPriority,
    pub decision: ContextBudgetDecision,
    pub estimated_tokens: u64,
    pub included_tokens: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedIdentitySnapshot {
    pub identity: String,
    pub activation_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceIdentitySnapshot {
    pub identity: String,
    pub content_digest: Option<String>,
    pub activation_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderInputSnapshot {
    pub provider: String,
    pub model: String,
    pub shaping_version: String,
    pub messages_digest: String,
    pub tools_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenBudgetSnapshot {
    pub tokenizer: String,
    pub estimator_uncertainty_percent: u8,
    pub budget_tokens: u64,
    pub reserved_tokens: u64,
    pub used_context_tokens: u64,
    pub estimated_input_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataDisclosureWarning {
    pub raw_content_possible: bool,
    pub surfaces: Vec<DataSurface>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec030ExecutionRefs {
    pub trusted_runtime: TrustedRuntimeFactRef,
    pub sandbox: Vec<AdapterSandboxRef>,
    pub credential: CredentialSnapshotRef,
    pub resources: Vec<ResourceIdentitySnapshot>,
    pub disclosure: DataDisclosureWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayContract {
    pub diagnostic_only: bool,
    pub live_execution_authorized: bool,
    pub replay_authorized: bool,
    pub current_source_truth: bool,
}

impl ReplayContract {
    pub const fn diagnostic_only() -> Self {
        Self {
            diagnostic_only: true,
            live_execution_authorized: false,
            replay_authorized: false,
            current_source_truth: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionSnapshotError {
    Malformed(String),
    UnknownSchema(String),
    InvalidReplayContract,
    MissingField(&'static str),
    ProvenanceMismatch,
}

impl fmt::Display for ExecutionSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(detail) => write!(formatter, "malformed execution snapshot: {detail}"),
            Self::UnknownSchema(schema) => {
                write!(formatter, "unknown execution snapshot schema: {schema}")
            }
            Self::InvalidReplayContract => {
                formatter.write_str("snapshot replay contract must be diagnostic-only")
            }
            Self::MissingField(field) => {
                write!(formatter, "execution snapshot field is empty: {field}")
            }
            Self::ProvenanceMismatch => {
                formatter.write_str("execution snapshot provenance digest mismatch")
            }
        }
    }
}

impl std::error::Error for ExecutionSnapshotError {}
