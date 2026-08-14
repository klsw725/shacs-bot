use super::{
    AutomationConfirmationFact, AutomationJobResult, AutomationLifecycleInput,
    AutomationNoDispatchReason, AutomationProcessCleanupFact, PluginHookDispatchEffect,
    PluginHookDispatchStatus, PluginHookEvent, ProcessExecutionReceipt, ProcessTerminalOutcome,
    SandboxMode,
};
use shacs_eval::evaluator::{automation_run_idempotency_key, AutomationRunRequest};
use shacs_projection::{CredentialStatus, HookDenialReason, ProcessAdapterKind};
use shacs_session::durable_work::{ReplayWorkItem, WorkTerminalKind};

pub(super) fn durable_denial(
    work: Option<&ReplayWorkItem>,
    request: &AutomationRunRequest,
) -> Option<AutomationNoDispatchReason> {
    let work = work?;
    if work.terminal_kind == Some(WorkTerminalKind::Superseded) {
        return Some(AutomationNoDispatchReason::Superseded);
    }
    let expected_dedupe = automation_run_idempotency_key(&request.job_id, &request.trigger_ref);
    if work.work_kind != super::AUTOMATION_WORK_KIND
        || request.session_id.as_deref() != Some(work.session_key.as_str())
        || work.dedupe_hint.as_deref() != Some(expected_dedupe.as_str())
    {
        return Some(AutomationNoDispatchReason::InvalidDurableLineage);
    }
    None
}

pub(super) fn snapshot_denial(
    input: &AutomationLifecycleInput<'_>,
) -> Option<AutomationNoDispatchReason> {
    let Some(snapshot) = input.execution_snapshot else {
        return Some(AutomationNoDispatchReason::SnapshotMissing);
    };
    if snapshot.validate_provenance().is_err()
        || snapshot.semantic_compatibility_digest().ok().as_deref()
            != Some(input.expected_snapshot_digest)
    {
        Some(AutomationNoDispatchReason::SnapshotMismatch)
    } else {
        None
    }
}

pub(super) fn trusted_gate_denial(
    input: &AutomationLifecycleInput<'_>,
) -> Option<AutomationNoDispatchReason> {
    let snapshot = input.execution_snapshot?;
    if input.requirements.execution_sensitive {
        if let Some(denial) = input.hook_denial {
            return Some(match denial {
                HookDenialReason::ExtensionBlocked
                | HookDenialReason::UserDenied
                | HookDenialReason::HeadlessConfirmationDenied => {
                    AutomationNoDispatchReason::HookVeto
                }
                HookDenialReason::HookFailed => AutomationNoDispatchReason::HookFailed,
            });
        }
        let Some(records) = input.hook_evidence else {
            return Some(AutomationNoDispatchReason::MissingHookEvidence);
        };
        if records
            .iter()
            .any(|record| record.effect == Some(PluginHookDispatchEffect::Blocked))
        {
            return Some(AutomationNoDispatchReason::HookVeto);
        }
        if records.is_empty()
            || records.iter().any(|record| {
                record.event != PluginHookEvent::ToolBefore
                    || record.status != PluginHookDispatchStatus::Succeeded
            })
        {
            return Some(AutomationNoDispatchReason::HookFailed);
        }
    }
    match input.requirements.confirmation {
        AutomationConfirmationFact::Denied => {
            return Some(AutomationNoDispatchReason::ConfirmationDenied)
        }
        AutomationConfirmationFact::HeadlessDenied => {
            return Some(AutomationNoDispatchReason::HeadlessConfirmationDenied)
        }
        AutomationConfirmationFact::NotRequired | AutomationConfirmationFact::Confirmed => {}
    }
    if input.requirements.credential_required
        && snapshot.credential.status != CredentialStatus::Resolved
    {
        return Some(AutomationNoDispatchReason::CredentialUnavailable);
    }
    if !input.requirements.sandbox_required {
        return None;
    }
    let sandbox = snapshot.sandbox.iter().find(|fact| {
        fact.adapter == ProcessAdapterKind::GenericExec || fact.adapter == ProcessAdapterKind::Bash
    });
    match sandbox.map(|fact| &fact.mode) {
        Some(SandboxMode::Active) => None,
        Some(SandboxMode::Failed) => Some(AutomationNoDispatchReason::SandboxFailed),
        Some(SandboxMode::Disabled)
        | Some(SandboxMode::Unsupported)
        | Some(SandboxMode::Unknown)
        | None => Some(AutomationNoDispatchReason::SandboxUnsupported),
    }
}

pub(super) fn process_job_result(
    current: AutomationJobResult,
    process: Option<&ProcessExecutionReceipt>,
    cleanup: &AutomationProcessCleanupFact,
) -> AutomationJobResult {
    if let AutomationProcessCleanupFact::Failed { reason_ref } = cleanup {
        return AutomationJobResult::Failed {
            reason_ref: reason_ref.clone(),
        };
    }
    let Some(process) = process else {
        return current;
    };
    match process.terminal_outcome {
        ProcessTerminalOutcome::Succeeded => current,
        ProcessTerminalOutcome::TimedOut => AutomationJobResult::TimedOut {
            timeout_ref: process.receipt_id.clone(),
        },
        ProcessTerminalOutcome::Cancelled | ProcessTerminalOutcome::Interrupted => {
            AutomationJobResult::Cancelled {
                reason_ref: process.receipt_id.clone(),
            }
        }
        ProcessTerminalOutcome::Failed
        | ProcessTerminalOutcome::Denied
        | ProcessTerminalOutcome::ReplaySkipped => AutomationJobResult::Failed {
            reason_ref: process.receipt_id.clone(),
        },
    }
}
