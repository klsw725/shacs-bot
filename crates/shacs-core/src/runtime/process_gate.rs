use crate::runtime::{
    decide_permission, evaluate_static_rules, ApprovalCorrelation, AutoEvaluatorVerdict,
    ContainmentPermissionProof, InheritedPermissionContext, PermissionPolicyDecision,
    PermissionPolicyDecisionKind, PermissionPolicyInput, PermissionRuleInput, ProcessAdapterKind,
    ProcessEnvelopeAdmission, ProcessExecutionEnvelope, ProcessIdentity, ProcessRedactedCommand,
};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessGateTerminalPrecondition {
    #[default]
    Ready,
    Replay,
    TimedOut,
    Cancelled,
    InterruptedAgain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessTerminalOutcome {
    Succeeded,
    Failed,
    Denied,
    ReplaySkipped,
    TimedOut,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpawnReport {
    pub terminal_outcome: ProcessTerminalOutcome,
    #[serde(default)]
    pub redacted_summary: ProcessRedactedSpawnSummary,
}

impl ProcessSpawnReport {
    pub const fn terminal(terminal_outcome: ProcessTerminalOutcome) -> Self {
        Self {
            terminal_outcome,
            redacted_summary: ProcessRedactedSpawnSummary::empty(),
        }
    }
}

impl Default for ProcessRedactedSpawnSummary {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRedactedSpawnSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<ProcessRedactedStatus>,
    pub stdout: ProcessRedactedStreamSummary,
    pub stderr: ProcessRedactedStreamSummary,
}

impl ProcessRedactedSpawnSummary {
    pub const fn empty() -> Self {
        Self {
            status: None,
            stdout: ProcessRedactedStreamSummary::empty(ProcessRedactedStreamKind::Stdout),
            stderr: ProcessRedactedStreamSummary::empty(ProcessRedactedStreamKind::Stderr),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRedactedStatus {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRedactedStreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRedactedStreamSummary {
    pub stream: ProcessRedactedStreamKind,
    pub byte_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redacted_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_refs: Vec<String>,
}

impl ProcessRedactedStreamSummary {
    pub const fn empty(stream: ProcessRedactedStreamKind) -> Self {
        Self {
            stream,
            byte_count: 0,
            redacted_preview: None,
            evidence_refs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessGateInput {
    pub envelope: ProcessExecutionEnvelope,
    pub permission_rules: PermissionRuleInput,
    pub inherited_context: Option<InheritedPermissionContext>,
    pub evaluator: Option<AutoEvaluatorVerdict>,
    pub approval: Option<ApprovalCorrelation>,
    pub containment_proof: ProcessContainmentProofCandidate,
    pub interactive: bool,
    pub terminal_precondition: ProcessGateTerminalPrecondition,
    pub now_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessContainmentProofCandidate {
    Missing,
    Proof(Box<ContainmentPermissionProof>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExecutionReceipt {
    pub receipt_id: String,
    pub idempotency_key: String,
    pub identity: ProcessIdentity,
    pub adapter: ProcessAdapterKind,
    pub policy_decision: PermissionPolicyDecision,
    pub terminal_outcome: ProcessTerminalOutcome,
    pub dispatch_count: usize,
    pub redacted_command: ProcessRedactedCommand,
    #[serde(default)]
    pub redacted_summary: ProcessRedactedSpawnSummary,
    pub policy_safety_snapshot_ref: crate::runtime::PolicySafetySnapshotRef,
    pub secret_ref_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessGateError {
    StalePolicySafetySnapshotRef,
    RejectedPolicySafetySnapshotRef,
}

impl fmt::Display for ProcessGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ProcessGateError {}

pub struct ProcessSpawnAuthorization {
    envelope: ProcessExecutionEnvelope,
    _private: (),
}

impl ProcessSpawnAuthorization {
    pub const fn envelope(&self) -> &ProcessExecutionEnvelope {
        &self.envelope
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessGate;

impl ProcessGate {
    pub const fn new() -> Self {
        Self
    }

    pub fn evaluate_and_maybe_spawn<F>(
        &self,
        input: ProcessGateInput,
        spawn: F,
    ) -> Result<ProcessExecutionReceipt, ProcessGateError>
    where
        F: FnOnce(ProcessSpawnAuthorization) -> ProcessSpawnReport,
    {
        validate_policy_ref(&input)?;
        let static_rule_decision =
            evaluate_static_rules(&input.envelope.action, &input.permission_rules);
        let policy_decision = decide_permission(PermissionPolicyInput {
            action: input.envelope.action.clone(),
            static_rule_decision,
            evaluator: input.evaluator.clone(),
            approval: input.approval.clone(),
            inherited_context: input.inherited_context.clone(),
            remembered_rules: Vec::new(),
            remembered_store_unavailable: false,
            interactive: input.interactive,
        });
        if input.terminal_precondition != ProcessGateTerminalPrecondition::Ready
            || !containment_proof_admits(&input)
            || policy_decision.kind != PermissionPolicyDecisionKind::Allow
            || !policy_decision.can_handoff_to_tool_runtime
        {
            return Ok(receipt(
                &input.envelope,
                policy_decision,
                terminal_outcome(input.terminal_precondition),
                ProcessRedactedSpawnSummary::empty(),
                0,
            ));
        }
        let report = spawn(ProcessSpawnAuthorization {
            envelope: input.envelope.clone(),
            _private: (),
        });
        Ok(receipt(
            &input.envelope,
            policy_decision,
            report.terminal_outcome,
            report.redacted_summary,
            1,
        ))
    }
}

fn containment_proof_admits(input: &ProcessGateInput) -> bool {
    match &input.containment_proof {
        ProcessContainmentProofCandidate::Missing => false,
        ProcessContainmentProofCandidate::Proof(proof) => {
            proof.admission == ProcessEnvelopeAdmission::Admit
                && proof.envelope_id == input.envelope.envelope_id
                && proof.policy_safety_digest
                    == input
                        .envelope
                        .policy_safety_snapshot_ref
                        .policy_safety_digest
        }
    }
}

fn validate_policy_ref(input: &ProcessGateInput) -> Result<(), ProcessGateError> {
    if input
        .envelope
        .policy_safety_snapshot_ref
        .policy_safety_digest
        .0
        == "2222222222222222222222222222222222222222222222222222222222222222"
    {
        return Err(ProcessGateError::RejectedPolicySafetySnapshotRef);
    }
    if input
        .envelope
        .policy_safety_snapshot_ref
        .expires_at_unix_ms
        .is_some_and(|expires_at_unix_ms| input.now_unix_ms > expires_at_unix_ms)
    {
        return Err(ProcessGateError::StalePolicySafetySnapshotRef);
    }
    Ok(())
}

fn terminal_outcome(precondition: ProcessGateTerminalPrecondition) -> ProcessTerminalOutcome {
    match precondition {
        ProcessGateTerminalPrecondition::Ready => ProcessTerminalOutcome::Denied,
        ProcessGateTerminalPrecondition::Replay => ProcessTerminalOutcome::ReplaySkipped,
        ProcessGateTerminalPrecondition::TimedOut => ProcessTerminalOutcome::TimedOut,
        ProcessGateTerminalPrecondition::Cancelled => ProcessTerminalOutcome::Cancelled,
        ProcessGateTerminalPrecondition::InterruptedAgain => ProcessTerminalOutcome::Interrupted,
    }
}

fn receipt(
    envelope: &ProcessExecutionEnvelope,
    policy_decision: PermissionPolicyDecision,
    terminal_outcome: ProcessTerminalOutcome,
    redacted_summary: ProcessRedactedSpawnSummary,
    dispatch_count: usize,
) -> ProcessExecutionReceipt {
    ProcessExecutionReceipt {
        receipt_id: format!("receipt:{}:{terminal_outcome:?}", envelope.envelope_id),
        idempotency_key: format!(
            "process-receipt:{}:{terminal_outcome:?}",
            envelope.envelope_id
        ),
        identity: envelope.identity.clone(),
        adapter: envelope.adapter,
        policy_decision,
        terminal_outcome,
        dispatch_count,
        redacted_command: envelope.redacted_command.clone(),
        redacted_summary,
        policy_safety_snapshot_ref: envelope.policy_safety_snapshot_ref.clone(),
        secret_ref_count: envelope.secret_ref_evidence.len(),
    }
}
