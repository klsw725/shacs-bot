use super::*;
use shacs_eval::completion_boundary::EvaluatorRoute;
use shacs_projection::{
    Spec033AutomationFact, Spec033AutomationJobStatus, Spec033DeliveryStatus,
    Spec033HookConfirmationFact,
};
use shacs_session::durable_work::{DurableWorkPayloadStore, ReplayWorkItem, ReplayWorkState};

pub(super) fn populate(snapshot: &mut Spec033Snapshot, data_dir: &Path, session_id: &str) {
    let Some(work) = latest(data_dir, session_id) else {
        return;
    };
    let Some(envelope) = envelope(&work, data_dir) else {
        return;
    };
    let terminal = work.terminal_facts.as_ref();
    let lifecycle = terminal.and_then(|facts| facts.get("lifecycle"));
    let run = lifecycle.and_then(|value| value.get("run"));
    let gate = lifecycle.and_then(|value| value.get("gate"));
    let mut evidence = if let Some(sequence) = work.terminal_sequence {
        vec![format!("durable_work:{}:terminal:{sequence}", work.work_id)]
    } else {
        vec![format!("durable_work:{}", work.work_id)]
    };
    let task_outcome = terminal.and_then(|facts| facts.get("task_outcome"));
    evidence.extend(task_outcome_refs(task_outcome));
    snapshot.automation = Spec033OwnerFact::available(
        Spec033Owner::Automation,
        Spec033EvidenceSource::DurableStore,
        Spec033AutomationFact {
            work_id: work.work_id,
            job_id: envelope.event.job_id,
            run_id: run
                .and_then(|value| value.get("run_id"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or(work.effect_id)
                .unwrap_or_default(),
            turn_id: work.turn_id,
            snapshot_id: gate
                .and_then(|value| value.get("snapshot_id"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    envelope
                        .enqueue_provenance_snapshot
                        .as_ref()
                        .map(|value| value.snapshot_id.clone())
                }),
            snapshot_digest: gate
                .and_then(|value| value.get("snapshot_digest"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    envelope
                        .enqueue_provenance_snapshot
                        .as_ref()
                        .map(|value| value.provenance_digest.clone())
                }),
            checkpoint_id: None,
            artifact_refs: refs(
                terminal
                    .and_then(|facts| facts.get("process_receipt"))
                    .and_then(|receipt| receipt.get("artifact_refs")),
            ),
            job_status: lifecycle
                .and_then(lifecycle_status)
                .or_else(|| terminal.and_then(job_status))
                .unwrap_or_else(|| state_status(work.state)),
            delivery_status: task_outcome
                .and_then(route_delivery_status)
                .or_else(|| terminal.and_then(delivery_status))
                .unwrap_or(Spec033DeliveryStatus::NotRequested),
        },
        evidence.clone(),
    );
    if let Some(fact) = snapshot.automation.fact.as_ref() {
        snapshot.diagnostics.automation_job_id =
            shacs_projection::Spec033DiagnosticLink::available(&fact.job_id);
        if let Some(turn_id) = fact.turn_id.as_ref() {
            snapshot.diagnostics.turn_id =
                shacs_projection::Spec033DiagnosticLink::available(turn_id);
        }
        if let Some(snapshot_id) = fact.snapshot_id.as_ref() {
            snapshot.diagnostics.execution_snapshot_id =
                shacs_projection::Spec033DiagnosticLink::available(snapshot_id);
        }
        if let Some(snapshot_digest) = fact.snapshot_digest.as_ref() {
            snapshot.diagnostics.execution_snapshot_digest =
                shacs_projection::Spec033DiagnosticLink::available(snapshot_digest);
        }
        if !fact.artifact_refs.is_empty() {
            snapshot.diagnostics.safe_artifact_refs =
                shacs_projection::Spec033DiagnosticLinks::available(fact.artifact_refs.clone());
        }
    }
    if let Some(fact) = terminal.and_then(hook_fact) {
        snapshot.hook_confirmation = Spec033OwnerFact::available(
            Spec033Owner::HookConfirmation,
            Spec033EvidenceSource::DurableStore,
            fact,
            evidence,
        );
    }
}

fn latest(data_dir: &Path, session_id: &str) -> Option<ReplayWorkItem> {
    evaluate_durable_recovery(
        data_dir.join("runtime/durable-events"),
        data_dir.join("runtime/durable-checkpoints"),
    )
    .state?
    .work
    .items
    .into_values()
    .filter(|item| {
        item.session_key == session_id && item.work_kind == super::super::AUTOMATION_WORK_KIND
    })
    .max_by_key(|item| item.updated_sequence)
}

fn envelope(
    work: &ReplayWorkItem,
    data_dir: &Path,
) -> Option<super::super::AutomationWorkEnvelope> {
    let data = DurableWorkPayloadStore::new(data_dir.join("runtime/work-payloads"))
        .read_json(&work.payload_ref)
        .ok()?;
    serde_json::from_value::<super::super::automation_payload::AutomationWorkPayload>(data)
        .ok()?
        .into_envelope()
        .ok()
}

const fn state_status(state: ReplayWorkState) -> Spec033AutomationJobStatus {
    match state {
        ReplayWorkState::Pending | ReplayWorkState::Leased | ReplayWorkState::WaitingRetry => {
            Spec033AutomationJobStatus::Pending
        }
        ReplayWorkState::Cancelled => Spec033AutomationJobStatus::Cancelled,
        ReplayWorkState::Terminal => Spec033AutomationJobStatus::Failed,
    }
}

fn job_status(value: &serde_json::Value) -> Option<Spec033AutomationJobStatus> {
    match value.get("job_result")?.get("kind")?.as_str()? {
        "pending" => Some(Spec033AutomationJobStatus::Pending),
        "succeeded" => Some(Spec033AutomationJobStatus::Succeeded),
        "failed" => Some(Spec033AutomationJobStatus::Failed),
        "timed_out" => Some(Spec033AutomationJobStatus::TimedOut),
        "cancelled" => Some(Spec033AutomationJobStatus::Cancelled),
        _ => None,
    }
}

fn lifecycle_status(value: &serde_json::Value) -> Option<Spec033AutomationJobStatus> {
    match value.get("run")?.get("state")?.as_str()? {
        "suppressed" => Some(Spec033AutomationJobStatus::Suppressed),
        "queued" | "running" => Some(Spec033AutomationJobStatus::Pending),
        "succeeded" => Some(Spec033AutomationJobStatus::Succeeded),
        "failed" => Some(Spec033AutomationJobStatus::Failed),
        "timed_out" => Some(Spec033AutomationJobStatus::TimedOut),
        "cancelled" => Some(Spec033AutomationJobStatus::Cancelled),
        _ => None,
    }
}

fn delivery_status(value: &serde_json::Value) -> Option<Spec033DeliveryStatus> {
    match value.get("delivery_result")?.get("kind")?.as_str()? {
        "not_requested" => Some(Spec033DeliveryStatus::NotRequested),
        "pending" => Some(Spec033DeliveryStatus::Pending),
        "succeeded" => Some(Spec033DeliveryStatus::Succeeded),
        "failed" => Some(Spec033DeliveryStatus::Failed),
        _ => None,
    }
}

fn hook_fact(value: &serde_json::Value) -> Option<Spec033HookConfirmationFact> {
    let gate = value.get("lifecycle")?.get("gate")?;
    match gate.get("denial").and_then(serde_json::Value::as_str) {
        Some("hook_veto") => Some(Spec033HookConfirmationFact::Vetoed),
        Some("hook_failed") | Some("missing_hook_evidence") => {
            Some(Spec033HookConfirmationFact::Failed)
        }
        _ => match gate.get("confirmation")?.as_str()? {
            "not_required" => Some(Spec033HookConfirmationFact::NotRequired),
            "confirmed" => Some(Spec033HookConfirmationFact::Confirmed),
            "denied" => Some(Spec033HookConfirmationFact::Denied),
            "headless_denied" => Some(Spec033HookConfirmationFact::HeadlessDenied),
            _ => None,
        },
    }
}

fn refs(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .map_or_else(Vec::new, |values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
}

fn task_outcome_refs(value: Option<&serde_json::Value>) -> Vec<String> {
    value.map_or_else(Vec::new, |value| {
        ["evaluator_evidence_ref", "owner_evidence_ref"]
            .into_iter()
            .filter_map(|key| value.get(key).and_then(serde_json::Value::as_str))
            .map(str::to_owned)
            .collect()
    })
}

fn route_delivery_status(value: &serde_json::Value) -> Option<Spec033DeliveryStatus> {
    let route = serde_json::from_value::<EvaluatorRoute>(value.get("route")?.clone()).ok()?;
    Some(match route {
        EvaluatorRoute::Notify | EvaluatorRoute::Escalate => Spec033DeliveryStatus::Pending,
        EvaluatorRoute::Suppress
        | EvaluatorRoute::Continue
        | EvaluatorRoute::Verify
        | EvaluatorRoute::RollbackCandidate => Spec033DeliveryStatus::NotRequested,
    })
}
