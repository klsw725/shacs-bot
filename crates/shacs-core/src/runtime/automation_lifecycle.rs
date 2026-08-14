use super::{
    coordinate_automation_run, AdapterSandboxRef, AutomationSourceEvent,
    AutomationSuppressionReason, ExecutionSnapshot, PluginHookDispatchRecord,
};
use serde::{Deserialize, Serialize};
use shacs_eval::evaluator::{
    automation_run_state_projection_status, AutomationDeliveryRecord, AutomationExecutionMode,
    AutomationRunRequest, AutomationRunState, AutomationRunStateRecord, ProjectionSurface,
};
use shacs_projection::{CredentialStatus, HookDenialReason};
use shacs_session::durable_work::ReplayWorkItem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationScheduleKind {
    OneShot,
    Recurring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationConfirmationFact {
    NotRequired,
    Confirmed,
    Denied,
    HeadlessDenied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationExecutionRequirements {
    pub execution_sensitive: bool,
    pub credential_required: bool,
    pub sandbox_required: bool,
    pub confirmation: AutomationConfirmationFact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationJobResult {
    Pending,
    Succeeded { result_ref: String },
    Failed { reason_ref: String },
    TimedOut { timeout_ref: String },
    Cancelled { reason_ref: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationDeliveryResult {
    NotRequested,
    Pending {
        target: ProjectionSurface,
    },
    Succeeded {
        target: ProjectionSurface,
    },
    Failed {
        target: ProjectionSurface,
        reason_ref: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationNoDispatchReason {
    Duplicate,
    Superseded,
    RecursionGuard,
    MalformedSource,
    InvalidDurableLineage,
    SnapshotMissing,
    SnapshotMismatch,
    MissingHookEvidence,
    HookVeto,
    HookFailed,
    ConfirmationDenied,
    HeadlessConfirmationDenied,
    SandboxUnsupported,
    SandboxFailed,
    CredentialUnavailable,
    AdapterUnsupported,
    TerminalResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationGateRecord {
    pub snapshot_id: Option<String>,
    pub snapshot_digest: Option<String>,
    pub trusted_runtime_ref: Option<String>,
    pub sandbox: Vec<AdapterSandboxRef>,
    pub credential_status: Option<CredentialStatus>,
    pub hook_evidence: Option<Vec<PluginHookDispatchRecord>>,
    pub hook_denial: Option<HookDenialReason>,
    pub confirmation: AutomationConfirmationFact,
    pub denial: Option<AutomationNoDispatchReason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLifecycleRecord {
    pub schedule: AutomationScheduleKind,
    pub execution_mode: AutomationExecutionMode,
    pub run: Option<AutomationRunStateRecord>,
    pub gate: AutomationGateRecord,
}

pub struct AutomationLifecycleInput<'a> {
    pub event: &'a AutomationSourceEvent,
    pub schedule: AutomationScheduleKind,
    pub existing_runs: &'a [AutomationRunStateRecord],
    pub durable_work: Option<&'a ReplayWorkItem>,
    pub execution_snapshot: Option<&'a ExecutionSnapshot>,
    pub expected_snapshot_digest: &'a str,
    pub hook_evidence: Option<&'a [PluginHookDispatchRecord]>,
    pub hook_denial: Option<HookDenialReason>,
    pub requirements: AutomationExecutionRequirements,
    pub job_result: AutomationJobResult,
    pub delivery_result: AutomationDeliveryResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLifecycleOutcome {
    pub lifecycle: AutomationLifecycleRecord,
    pub dispatch_request: Option<AutomationRunRequest>,
    pub no_dispatch_reason: Option<AutomationNoDispatchReason>,
    pub job_result: AutomationJobResult,
    pub delivery_result: AutomationDeliveryResult,
    pub delivery_record: Option<AutomationDeliveryRecord>,
}

pub fn own_automation_lifecycle(input: AutomationLifecycleInput<'_>) -> AutomationLifecycleOutcome {
    let coordinated = coordinate_automation_run(input.event, input.existing_runs);
    let coordination_denial = coordinated.suppression.map(|reason| match reason {
        AutomationSuppressionReason::InactiveHeartbeat
        | AutomationSuppressionReason::MalformedSource => {
            AutomationNoDispatchReason::MalformedSource
        }
        AutomationSuppressionReason::Duplicate => AutomationNoDispatchReason::Duplicate,
        AutomationSuppressionReason::RecursionGuard { .. } => {
            AutomationNoDispatchReason::RecursionGuard
        }
    });
    let denial = coordination_denial
        .or_else(|| {
            super::automation_gates::durable_denial(
                input.durable_work,
                &coordinated.coordinated_request,
            )
        })
        .or_else(|| super::automation_gates::snapshot_denial(&input))
        .or_else(|| super::automation_gates::trusted_gate_denial(&input))
        .or_else(|| terminal_denial(&input.job_result));
    let snapshot = input.execution_snapshot;
    let gate = AutomationGateRecord {
        snapshot_id: snapshot.map(|value| value.snapshot_id.clone()),
        snapshot_digest: snapshot.map(|value| value.provenance_digest.clone()),
        trusted_runtime_ref: snapshot.map(|value| value.trusted_runtime.profile_ref.clone()),
        sandbox: snapshot.map_or_else(Vec::new, |value| value.sandbox.clone()),
        credential_status: snapshot.map(|value| value.credential.status),
        hook_evidence: input
            .hook_evidence
            .map(<[PluginHookDispatchRecord]>::to_vec),
        hook_denial: input.hook_denial,
        confirmation: input.requirements.confirmation,
        denial,
    };
    let run = lifecycle_run(coordinated.run_state_record, denial, &input.job_result);
    AutomationLifecycleOutcome {
        lifecycle: AutomationLifecycleRecord {
            schedule: input.schedule,
            execution_mode: input.event.execution_mode.clone(),
            run,
            gate,
        },
        dispatch_request: if denial.is_none() {
            coordinated.request
        } else {
            None
        },
        no_dispatch_reason: denial,
        job_result: input.job_result,
        delivery_result: input.delivery_result,
        delivery_record: coordinated.delivery_record,
    }
}

fn lifecycle_run(
    run: Option<AutomationRunStateRecord>,
    denial: Option<AutomationNoDispatchReason>,
    job_result: &AutomationJobResult,
) -> Option<AutomationRunStateRecord> {
    let mut run = run?;
    run.state = match job_result {
        AutomationJobResult::Succeeded { .. } => AutomationRunState::Succeeded,
        AutomationJobResult::Failed { .. } => AutomationRunState::Failed,
        AutomationJobResult::TimedOut { .. } => AutomationRunState::TimedOut,
        AutomationJobResult::Cancelled { .. } => AutomationRunState::Cancelled,
        AutomationJobResult::Pending if denial.is_some() => AutomationRunState::Suppressed,
        AutomationJobResult::Pending => AutomationRunState::Queued,
    };
    run.projection_status = automation_run_state_projection_status(&run.state);
    if denial.is_some() && run.suppress_reason.is_none() {
        run.suppress_reason = denial.map(|reason| format!("{reason:?}"));
    }
    Some(run)
}

fn terminal_denial(result: &AutomationJobResult) -> Option<AutomationNoDispatchReason> {
    match result {
        AutomationJobResult::Pending => None,
        AutomationJobResult::Succeeded { .. }
        | AutomationJobResult::Failed { .. }
        | AutomationJobResult::TimedOut { .. }
        | AutomationJobResult::Cancelled { .. } => Some(AutomationNoDispatchReason::TerminalResult),
    }
}
