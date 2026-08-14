use super::{
    now_millis, runtime_durable_event_root, runtime_durable_work_payload_root,
    AgentLoopChatCompletionAdapter, DURABLE_WORK_LEASE_DURATION_MS,
};
use sha2::{Digest, Sha256};
use shacs_channels::OwnerAcceptedAutomationResult;
use shacs_core::runtime::{
    enqueue_production_automation, AutomationConfirmationFact, AutomationExecutionRequirements,
    AutomationOutcomePolicy, AutomationProductionJob, AutomationScheduleKind,
    AutomationSourceEvent, AutomationSourceEventKind, AutomationWorkEnvelope,
    DurableWorkDispatcher, InboundMessage, MessageBus, SubagentMergeState,
};
use shacs_cron::{CronJob, CronScheduleKind};
use shacs_eval::evaluator::{AutomationExecutionMode, AutomationRecursionGuard, DeliverySeverity};
use std::path::Path;

pub(super) fn enqueue_result(
    adapter: &AgentLoopChatCompletionAdapter,
    message: &InboundMessage,
) -> Result<(), String> {
    let Some(source) = result_source(message)? else {
        return Ok(());
    };
    enqueue(
        adapter,
        message.session_key(),
        source,
        AutomationScheduleKind::OneShot,
        None,
        AutomationExecutionMode::SkillBackedAgent,
        &message.content,
    )
    .map(|_| ())
}

fn result_source(message: &InboundMessage) -> Result<Option<AutomationSourceEventKind>, String> {
    match message.owner_accepted_automation_result() {
        Some(OwnerAcceptedAutomationResult::SubagentTerminal { result_ref }) => {
            Ok(Some(AutomationSourceEventKind::SubagentResult {
                merge_state: SubagentMergeState::Terminal,
                result_ref: result_ref.clone(),
            }))
        }
        None if message.metadata.get("injected_event").is_none() => Ok(None),
        None => Err("automation result lacks an owner-accepted terminal boundary".to_owned()),
    }
}

pub(super) fn enqueue_heartbeat(
    adapter: &AgentLoopChatCompletionAdapter,
    tasks: &str,
) -> Result<(), String> {
    enqueue(
        adapter,
        "heartbeat".to_owned(),
        AutomationSourceEventKind::Heartbeat,
        AutomationScheduleKind::Recurring,
        Some(now_millis()),
        AutomationExecutionMode::NoAgentCheck,
        tasks,
    )
    .map(|_| ())
}

pub(super) fn enqueue_recording_heartbeat(
    adapter: &AgentLoopChatCompletionAdapter,
    tasks: &str,
) -> Result<String, String> {
    enqueue(
        adapter,
        "trajectory:local".to_owned(),
        AutomationSourceEventKind::Heartbeat,
        AutomationScheduleKind::Recurring,
        Some(0),
        AutomationExecutionMode::NoAgentCheck,
        tasks,
    )
}

pub(super) fn enqueue_cron(
    adapter: &AgentLoopChatCompletionAdapter,
    job: &CronJob,
) -> Result<(), String> {
    let schedule = match job.schedule.kind {
        CronScheduleKind::At => AutomationScheduleKind::OneShot,
        CronScheduleKind::Every | CronScheduleKind::Cron => AutomationScheduleKind::Recurring,
    };
    enqueue(
        adapter,
        job.payload
            .session_key
            .clone()
            .ok_or_else(|| "cron automation lacks session key".to_owned())?,
        AutomationSourceEventKind::Cron {
            approved_automation_rule_ref: Some(format!("cron-job:{}", job.id)),
        },
        schedule,
        job.state
            .next_run_at_ms
            .and_then(|wake| u64::try_from(wake).ok()),
        AutomationExecutionMode::SkillBackedAgent,
        &job.payload.message,
    )
    .map(|_| ())
}

fn enqueue(
    adapter: &AgentLoopChatCompletionAdapter,
    session_id: String,
    source: AutomationSourceEventKind,
    schedule: AutomationScheduleKind,
    wake: Option<u64>,
    execution_mode: AutomationExecutionMode,
    instruction: &str,
) -> Result<String, String> {
    let data_dir = adapter
        .config_path
        .parent()
        .ok_or_else(|| "automation data directory is unavailable".to_owned())?;
    let now = wake.unwrap_or_else(|| source_observed_at(&source));
    let event_id = source_event_id(&session_id, &source, wake);
    let snapshot = adapter.automation_snapshot(instruction)?;
    let mut dispatcher = open_dispatcher(data_dir)?;
    let work_id = format!("automation-{event_id}");
    let outcome_policy = outcome_policy(&source);
    enqueue_production_automation(
        &mut dispatcher,
        AutomationProductionJob {
            work_id: work_id.clone(),
            envelope: AutomationWorkEnvelope {
                event: AutomationSourceEvent {
                    runtime_service_event_id: event_id.clone(),
                    source_owner: "production-runtime".to_owned(),
                    received_at_ms: now,
                    job_id: event_id.clone(),
                    session_id: Some(session_id),
                    goal_id: None,
                    active_goal: true,
                    pending_automation: true,
                    execution_mode: execution_mode.clone(),
                    timeout_policy_ref: "runtime-default".to_owned(),
                    retry_policy_ref: "durable-work-default".to_owned(),
                    delivery_policy_ref: "owner-route".to_owned(),
                    recursion_guard: AutomationRecursionGuard {
                        token: format!("guard-{event_id}"),
                        source_run_id: None,
                        depth: 0,
                        max_depth: 3,
                        parent_refs: Vec::new(),
                        blocked_reason: None,
                    },
                    prd008_goal_gate_ref: None,
                    source,
                },
                schedule,
                existing_runs: Vec::new(),
                expected_current_facts_digest: snapshot
                    .semantic_compatibility_digest()
                    .map_err(|error| error.to_string())?,
                enqueue_provenance_snapshot: Some(snapshot),
                hook_evidence: None,
                requirements: AutomationExecutionRequirements {
                    execution_sensitive: execution_mode
                        == AutomationExecutionMode::SkillBackedAgent,
                    credential_required: false,
                    sandbox_required: false,
                    confirmation: AutomationConfirmationFact::NotRequired,
                },
                instruction: Some(instruction.to_owned()),
                outcome_policy,
            },
            next_wake_at_ms: wake,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(work_id)
}

fn outcome_policy(source: &AutomationSourceEventKind) -> AutomationOutcomePolicy {
    match source {
        AutomationSourceEventKind::Heartbeat => AutomationOutcomePolicy::Continue,
        AutomationSourceEventKind::Cron { .. } => AutomationOutcomePolicy::Notify,
        AutomationSourceEventKind::SubagentResult { .. } => AutomationOutcomePolicy::Verify,
        AutomationSourceEventKind::AppTaskResult {
            self_improvement_apply_requested: true,
            ..
        } => AutomationOutcomePolicy::RollbackCandidate,
        AutomationSourceEventKind::AppTaskResult { .. } => AutomationOutcomePolicy::Verify,
        AutomationSourceEventKind::ChannelEvent { severity, .. }
            if *severity == DeliverySeverity::Error =>
        {
            AutomationOutcomePolicy::Escalate
        }
        AutomationSourceEventKind::ChannelEvent { .. } => AutomationOutcomePolicy::Notify,
        AutomationSourceEventKind::LocalApiBackground { .. } => AutomationOutcomePolicy::Suppress,
        AutomationSourceEventKind::ManualResume { .. } => AutomationOutcomePolicy::Continue,
    }
}

pub(super) fn open_dispatcher(data_dir: &Path) -> Result<DurableWorkDispatcher, String> {
    DurableWorkDispatcher::open(
        runtime_durable_event_root(data_dir),
        runtime_durable_work_payload_root(data_dir),
        MessageBus::new(),
        "production-automation-producer",
        DURABLE_WORK_LEASE_DURATION_MS,
    )
    .map_err(|error| error.to_string())
}

fn source_name(source: &AutomationSourceEventKind) -> &'static str {
    match source {
        AutomationSourceEventKind::Heartbeat => "heartbeat",
        AutomationSourceEventKind::Cron { .. } => "cron",
        AutomationSourceEventKind::SubagentResult { .. } => "subagent",
        AutomationSourceEventKind::AppTaskResult { .. } => "app",
        AutomationSourceEventKind::ChannelEvent { .. } => "channel",
        AutomationSourceEventKind::LocalApiBackground { .. } => "local-api",
        AutomationSourceEventKind::ManualResume { .. } => "manual",
    }
}

fn source_observed_at(source: &AutomationSourceEventKind) -> u64 {
    match source {
        AutomationSourceEventKind::Heartbeat | AutomationSourceEventKind::Cron { .. } => {
            now_millis()
        }
        AutomationSourceEventKind::SubagentResult { .. }
        | AutomationSourceEventKind::AppTaskResult { .. }
        | AutomationSourceEventKind::ChannelEvent { .. }
        | AutomationSourceEventKind::LocalApiBackground { .. }
        | AutomationSourceEventKind::ManualResume { .. } => 0,
    }
}

fn source_event_id(
    session_id: &str,
    source: &AutomationSourceEventKind,
    wake: Option<u64>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update([0]);
    hasher.update(serde_json::to_vec(source).unwrap_or_default());
    hasher.update([0]);
    hasher.update(wake.unwrap_or_default().to_le_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("production-{}-{}", source_name(source), &digest[..16])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn result_message(injected_event: &str) -> InboundMessage {
        let mut message = InboundMessage::new("system", "owner", "result", "result");
        message.metadata.insert(
            "injected_event".to_owned(),
            Value::String(injected_event.to_owned()),
        );
        message
    }

    #[test]
    fn accepted_terminal_subagent_result_is_the_only_supported_result_source() {
        let mut message = result_message("subagent_result");
        message.metadata.insert(
            "subagent_task_id".to_owned(),
            Value::String("child-1".to_owned()),
        );
        message = message.with_owner_accepted_automation_result(
            OwnerAcceptedAutomationResult::SubagentTerminal {
                result_ref: "child-1".to_owned(),
            },
        );

        assert!(matches!(
            result_source(&message),
            Ok(Some(AutomationSourceEventKind::SubagentResult {
                merge_state: SubagentMergeState::Terminal,
                result_ref,
            })) if result_ref == "child-1"
        ));
    }

    #[test]
    fn subagent_result_without_owner_terminal_acceptance_fails_closed() {
        let mut message = result_message("subagent_result");
        message.metadata.insert(
            "subagent_task_id".to_owned(),
            Value::String("child-1".to_owned()),
        );

        assert!(result_source(&message).is_err());
    }

    #[test]
    fn serialized_inbound_cannot_claim_owner_terminal_acceptance() {
        let message: InboundMessage = serde_json::from_value(serde_json::json!({
            "channel": "system",
            "sender_id": "subagent",
            "chat_id": "result",
            "content": "result",
            "timestamp": "2026-08-14T00:00:00Z",
            "media": [],
            "metadata": {
                "injected_event": "subagent_result",
                "subagent_task_id": "child-1"
            },
            "owner_accepted_automation_result": {
                "subagent_terminal": { "result_ref": "child-1" }
            }
        }))
        .expect("inbound fixture should deserialize");

        assert!(message.owner_accepted_automation_result().is_none());
        assert!(result_source(&message).is_err());
    }

    #[test]
    fn unsupported_result_boundaries_fail_before_automation_enqueue() {
        for injected_event in [
            "app_task_result",
            "channel_result",
            "local_api_background_result",
        ] {
            assert_eq!(
                result_source(&result_message(injected_event)),
                Err("automation result lacks an owner-accepted terminal boundary".to_owned())
            );
        }
    }
}
