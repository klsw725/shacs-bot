use crate::runtime::{PermissionSecretRefEvidence, PermissionedAction, PolicySafetySnapshotRef};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessAdapterKind {
    ExecTool,
    PluginHook,
    PluginTool,
    PluginCommand,
    McpStdio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub process_ref: String,
    pub session_id: String,
    pub turn_id: String,
}

impl ProcessIdentity {
    pub fn new(
        process_ref: impl Into<String>,
        session_id: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> Self {
        Self {
            process_ref: process_ref.into(),
            session_id: session_id.into(),
            turn_id: turn_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRedactedCommand {
    pub command_family: String,
    pub redacted_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted_targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessExecutionEnvelopeInput {
    pub identity: ProcessIdentity,
    pub adapter: ProcessAdapterKind,
    pub action: PermissionedAction,
    pub required_secret_ref_count: usize,
    pub redacted_command: ProcessRedactedCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExecutionEnvelope {
    pub envelope_id: String,
    pub identity: ProcessIdentity,
    pub adapter: ProcessAdapterKind,
    pub action: PermissionedAction,
    pub policy_safety_snapshot_ref: PolicySafetySnapshotRef,
    pub secret_ref_evidence: Vec<PermissionSecretRefEvidence>,
    pub redacted_command: ProcessRedactedCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessEnvelopeError {
    MissingPolicySafetySnapshotRef,
    MissingSecretRefs,
    SessionMismatch,
    TurnMismatch,
}

impl fmt::Display for ProcessEnvelopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ProcessEnvelopeError {}

impl ProcessExecutionEnvelope {
    pub fn try_from_input(
        input: ProcessExecutionEnvelopeInput,
    ) -> Result<Self, ProcessEnvelopeError> {
        if input.identity.session_id != input.action.session_id {
            return Err(ProcessEnvelopeError::SessionMismatch);
        }
        if input.identity.turn_id != input.action.turn_id {
            return Err(ProcessEnvelopeError::TurnMismatch);
        }
        let policy_safety_snapshot_ref = input
            .action
            .policy_safety_snapshot_ref
            .clone()
            .ok_or(ProcessEnvelopeError::MissingPolicySafetySnapshotRef)?;
        if input.action.secret_ref_evidence.len() < input.required_secret_ref_count {
            return Err(ProcessEnvelopeError::MissingSecretRefs);
        }
        Ok(Self {
            envelope_id: envelope_id(&input.identity, &input.action.action_digest),
            identity: input.identity,
            adapter: input.adapter,
            secret_ref_evidence: input.action.secret_ref_evidence.clone(),
            action: input.action,
            policy_safety_snapshot_ref,
            redacted_command: input.redacted_command,
        })
    }
}

fn envelope_id(identity: &ProcessIdentity, action_digest: &str) -> String {
    format!(
        "process:{}:{}:{}",
        identity.session_id, identity.turn_id, action_digest
    )
}
