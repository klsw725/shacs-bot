use super::{
    own_automation_lifecycle, AutomationDeliveryResult, AutomationDispatchRequest,
    AutomationExecutionReceipt, AutomationExecutionRequirements, AutomationExecutor,
    AutomationGateResolution, AutomationGateResolver, AutomationJobResult,
    AutomationLifecycleInput, AutomationLifecycleOutcome, AutomationProcessCleanupFact,
    AutomationScheduleKind, AutomationSourceEvent, DurableDispatchError, DurableWorkDispatcher,
    ExecutionSnapshot, PluginHookDispatchRecord, ProcessExecutionReceipt,
};
use serde::{Deserialize, Serialize};
use shacs_eval::evaluator::{
    automation_run_state_projection_status, AutomationRunRequest, AutomationRunState,
    AutomationRunStateRecord,
};
use shacs_session::durable_work::{
    DurableWorkAdmission, DurableWorkEnqueueJsonInput, DurableWorkReplayState, ReplayWorkItem,
    WorkTerminalKind,
};

pub const AUTOMATION_WORK_KIND: &str = "automation.run";
const AUTOMATION_PAYLOAD_TYPE: &str = "shacs.automation_work.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationWorkEnvelope {
    pub event: AutomationSourceEvent,
    pub schedule: AutomationScheduleKind,
    pub existing_runs: Vec<AutomationRunStateRecord>,
    pub enqueue_provenance_snapshot: Option<ExecutionSnapshot>,
    pub expected_current_facts_digest: String,
    pub hook_evidence: Option<Vec<PluginHookDispatchRecord>>,
    pub requirements: AutomationExecutionRequirements,
    pub instruction: Option<String>,
    pub outcome_policy: super::AutomationOutcomePolicy,
}

#[derive(Debug, Clone)]
pub struct AutomationWorkEnqueueInput {
    pub work_id: String,
    pub envelope: AutomationWorkEnvelope,
    pub next_wake_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct AutomationTerminalFacts {
    lifecycle: super::AutomationLifecycleRecord,
    job_result: AutomationJobResult,
    terminal_fact: super::AutomationExecutionTerminalFact,
    delivery_result: AutomationDeliveryResult,
    process_receipt: Option<ProcessExecutionReceipt>,
    process_cleanup: AutomationProcessCleanupFact,
    task_outcome: Option<super::AutomationTaskOutcomeRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationDispatchSummary {
    pub consumed_work_ids: Vec<String>,
    pub suppressed_work_ids: Vec<String>,
}

impl DurableWorkDispatcher {
    pub fn enqueue_automation(
        &mut self,
        input: AutomationWorkEnqueueInput,
    ) -> Result<(), DurableDispatchError> {
        let coordinated =
            super::coordinate_automation_run(&input.envelope.event, &input.envelope.existing_runs);
        let request = coordinated.coordinated_request;
        let idempotency_key = coordinated
            .run_state_record
            .map(|run| run.idempotency_key)
            .unwrap_or_else(|| {
                shacs_eval::evaluator::automation_run_idempotency_key(
                    &request.job_id,
                    &request.trigger_ref,
                )
            });
        let session_key = request.session_id.clone().ok_or_else(|| {
            DurableDispatchError::InvalidWork("automation run requires a session id".to_owned())
        })?;
        let payload =
            super::automation_payload::AutomationWorkPayload::from_envelope(&input.envelope)?;
        self.enqueue_json_work(
            AUTOMATION_PAYLOAD_TYPE,
            &payload,
            DurableWorkEnqueueJsonInput {
                work_id: input.work_id,
                work_kind: AUTOMATION_WORK_KIND.to_owned(),
                session_key,
                turn_id: None,
                effect_id: Some(request.run_id),
                dedupe_hint: Some(idempotency_key),
                next_wake_at_ms: input.next_wake_at_ms,
            },
        )?;
        Ok(())
    }

    pub fn dispatch_due_automation<G: AutomationGateResolver, E: AutomationExecutor>(
        &mut self,
        state: &DurableWorkReplayState,
        admission: &DurableWorkAdmission,
        now_ms: u64,
        gate_resolver: &mut G,
        executor: &mut E,
    ) -> Result<AutomationDispatchSummary, DurableDispatchError> {
        let mut summary = AutomationDispatchSummary {
            consumed_work_ids: Vec::new(),
            suppressed_work_ids: Vec::new(),
        };
        for work_id in &admission.due_work_ids {
            let item = state
                .items
                .get(work_id)
                .ok_or_else(|| DurableDispatchError::MissingWork(work_id.clone()))?;
            if item.work_kind != AUTOMATION_WORK_KIND {
                continue;
            }
            self.consume_automation(item, now_ms, gate_resolver, executor, &mut summary)?;
        }
        Ok(summary)
    }

    fn consume_automation<G: AutomationGateResolver, E: AutomationExecutor>(
        &mut self,
        item: &ReplayWorkItem,
        now_ms: u64,
        gate_resolver: &mut G,
        executor: &mut E,
        summary: &mut AutomationDispatchSummary,
    ) -> Result<(), DurableDispatchError> {
        let payload: super::automation_payload::AutomationWorkPayload =
            self.read_work_payload(item)?;
        let envelope = payload.into_envelope()?;
        let coordinated =
            super::coordinate_automation_run(&envelope.event, &envelope.existing_runs);
        let run = coordinated.coordinated_request;
        let request = normalized_request(item, run, &envelope)?;
        let gates = gate_resolver.resolve(&request);
        let initial = lifecycle(
            &envelope,
            item,
            &gates,
            AutomationJobResult::Pending,
            AutomationDeliveryResult::NotRequested,
        );
        if initial.dispatch_request.is_none() {
            self.record_automation_terminal(item, WorkTerminalKind::Blocked, &initial)?;
            summary.suppressed_work_ids.push(item.work_id.clone());
            return Ok(());
        };
        let control = execution_control(&request.run.timeout_policy_ref)?;
        self.lease_work(item, now_ms)?;
        let receipt = normalize_execution_receipt(
            executor.execute(request, control.clone()),
            &control,
            self.cancellation_requested(item)?,
        );
        let terminal = lifecycle(
            &envelope,
            item,
            &gates,
            receipt.job_result.clone(),
            receipt.delivery_result.clone(),
        );
        let facts = AutomationTerminalFacts {
            lifecycle: terminal.lifecycle,
            job_result: terminal.job_result,
            terminal_fact: receipt.terminal_fact,
            delivery_result: terminal.delivery_result,
            process_receipt: receipt.process_receipt,
            process_cleanup: receipt.process_cleanup,
            task_outcome: receipt.task_outcome,
        };
        self.record_automation_terminal(item, terminal_kind(&facts.job_result), &facts)?;
        summary.consumed_work_ids.push(item.work_id.clone());
        Ok(())
    }
}

fn lifecycle(
    envelope: &AutomationWorkEnvelope,
    item: &ReplayWorkItem,
    gates: &AutomationGateResolution,
    job_result: AutomationJobResult,
    delivery_result: AutomationDeliveryResult,
) -> AutomationLifecycleOutcome {
    let mut outcome = own_automation_lifecycle(AutomationLifecycleInput {
        event: &envelope.event,
        schedule: envelope.schedule.clone(),
        existing_runs: &envelope.existing_runs,
        durable_work: Some(item),
        execution_snapshot: gates.execution_snapshot.as_ref(),
        expected_snapshot_digest: &envelope.expected_current_facts_digest,
        hook_evidence: Some(&gates.hook_evidence),
        hook_denial: gates.hook_denial,
        requirements: gates.requirements.clone(),
        job_result,
        delivery_result,
    });
    if !gates.adapter_supported && outcome.dispatch_request.is_some() {
        let denial = super::AutomationNoDispatchReason::AdapterUnsupported;
        outcome.dispatch_request = None;
        outcome.no_dispatch_reason = Some(denial);
        outcome.lifecycle.gate.denial = Some(denial);
        if let Some(run) = outcome.lifecycle.run.as_mut() {
            run.state = AutomationRunState::Suppressed;
            run.projection_status = automation_run_state_projection_status(&run.state);
            if run.suppress_reason.is_none() {
                run.suppress_reason = Some(format!("{denial:?}"));
            }
        }
    }
    outcome
}

fn normalized_request(
    item: &ReplayWorkItem,
    run: AutomationRunRequest,
    envelope: &AutomationWorkEnvelope,
) -> Result<AutomationDispatchRequest, DurableDispatchError> {
    let dedupe_key = item.dedupe_hint.clone().ok_or_else(|| {
        DurableDispatchError::InvalidWork("automation work lacks dedupe lineage".to_owned())
    })?;
    Ok(AutomationDispatchRequest {
        work_id: item.work_id.clone(),
        session_key: item.session_key.clone(),
        work_kind: item.work_kind.clone(),
        idempotency_key: dedupe_key.clone(),
        dedupe_key,
        run,
        requirements: envelope.requirements.clone(),
        instruction: envelope.instruction.clone(),
        outcome_policy: envelope.outcome_policy,
        owner_target_ref: match &envelope.event.source {
            super::AutomationSourceEventKind::AppTaskResult { app_task_id, .. } => {
                app_task_id.clone()
            }
            super::AutomationSourceEventKind::Heartbeat
            | super::AutomationSourceEventKind::Cron { .. }
            | super::AutomationSourceEventKind::SubagentResult { .. }
            | super::AutomationSourceEventKind::ChannelEvent { .. }
            | super::AutomationSourceEventKind::LocalApiBackground { .. }
            | super::AutomationSourceEventKind::ManualResume { .. } => None,
        },
    })
}

fn normalize_execution_receipt(
    mut receipt: AutomationExecutionReceipt,
    control: &super::AutomationExecutionControl,
    durable_cancellation_requested: bool,
) -> AutomationExecutionReceipt {
    if control.deadline_elapsed() {
        receipt.job_result = AutomationJobResult::TimedOut {
            timeout_ref: control.timeout_ref().to_owned(),
        };
        receipt.terminal_fact = super::AutomationExecutionTerminalFact::TimedOut {
            timeout_ref: control.timeout_ref().to_owned(),
        };
    } else if control.is_cancelled() || durable_cancellation_requested {
        receipt.job_result = AutomationJobResult::Cancelled {
            reason_ref: "automation:cancelled".to_owned(),
        };
        receipt.terminal_fact = super::AutomationExecutionTerminalFact::Cancelled {
            reason_ref: "automation:cancelled".to_owned(),
        };
    }
    receipt.job_result = super::automation_gates::process_job_result(
        receipt.job_result,
        receipt.process_receipt.as_ref(),
        &receipt.process_cleanup,
    );
    receipt
}

fn execution_control(
    timeout_policy_ref: &str,
) -> Result<super::AutomationExecutionControl, DurableDispatchError> {
    match timeout_policy_ref {
        "runtime-default" => Ok(super::AutomationExecutionControl::runtime_default()),
        unknown => Err(DurableDispatchError::InvalidWork(format!(
            "unknown automation timeout policy: {unknown}"
        ))),
    }
}

fn terminal_kind(result: &AutomationJobResult) -> WorkTerminalKind {
    match result {
        AutomationJobResult::Succeeded { .. } => WorkTerminalKind::Succeeded,
        AutomationJobResult::Pending
        | AutomationJobResult::Failed { .. }
        | AutomationJobResult::TimedOut { .. }
        | AutomationJobResult::Cancelled { .. } => WorkTerminalKind::Failed,
    }
}
