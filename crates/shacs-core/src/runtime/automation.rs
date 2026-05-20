use serde::{Deserialize, Serialize};
use shacs_utils::evaluator::{
    automation_run_idempotency_key, automation_run_state_projection_status,
    AutomationDeliveryRecord, AutomationExecutionMode, AutomationRecursionGuard,
    AutomationRunRequest, AutomationRunState, AutomationRunStateRecord, AutomationRunTriggerKind,
    AutomationTriggerRef, DeliverySeverity, ProjectionSurface,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationSourceEvent {
    pub runtime_service_event_id: String,
    pub source_owner: String,
    pub received_at_ms: u64,
    pub job_id: String,
    pub session_id: Option<String>,
    pub goal_id: Option<String>,
    pub active_goal: bool,
    pub pending_automation: bool,
    pub execution_mode: AutomationExecutionMode,
    pub timeout_policy_ref: String,
    pub retry_policy_ref: String,
    pub delivery_policy_ref: String,
    pub recursion_guard: AutomationRecursionGuard,
    pub prd008_goal_gate_ref: Option<String>,
    pub source: AutomationSourceEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationSourceEventKind {
    Heartbeat,
    Cron {
        approved_automation_rule_ref: Option<String>,
    },
    SubagentResult {
        merge_state: SubagentMergeState,
        result_ref: String,
    },
    AppTaskResult {
        app_task_id: Option<String>,
        manifest_ref: Option<String>,
        capability_scope: Option<String>,
        evidence_ref: String,
        self_improvement_apply_requested: bool,
    },
    ChannelEvent {
        channel_event_ref: String,
        user_visible: bool,
        redacted_message: String,
        target_surface: ProjectionSurface,
        severity: DeliverySeverity,
    },
    LocalApiBackground {
        caller_auth_ref: Option<String>,
        redaction_profile_ref: Option<String>,
        redacted_evidence_ref: String,
    },
    ManualResume {
        resume_ref: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentMergeState {
    Pending,
    Running,
    Terminal,
    Reviewable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationTaskOutcomeEligibility {
    pub evaluator_should_run: bool,
    pub continue_requires_prd008_goal_gate: bool,
    pub direct_execution_allowed: bool,
    pub app_authority_can_apply_self_improvement: bool,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationPrd008LinkageMetadata {
    pub goal_id: Option<String>,
    pub goal_gate_ref: Option<String>,
    pub source_ledger_ref: String,
    pub can_build_evaluator_decision_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationCoordinationOutcome {
    pub request: Option<AutomationRunRequest>,
    pub run_state_record: Option<AutomationRunStateRecord>,
    pub delivery_record: Option<AutomationDeliveryRecord>,
    pub suppress_reason: Option<String>,
    pub task_outcome_eligibility: AutomationTaskOutcomeEligibility,
    pub prd008_linkage: AutomationPrd008LinkageMetadata,
}

pub fn coordinate_automation_run(
    event: &AutomationSourceEvent,
    existing_run_state_records: &[AutomationRunStateRecord],
) -> AutomationCoordinationOutcome {
    let trigger_kind = trigger_kind(&event.source);
    let trigger_ref = trigger_ref(event, &trigger_kind);
    let idempotency_key = automation_run_idempotency_key(&event.job_id, &trigger_ref);
    let run_id = format!("run-{idempotency_key}");
    let prd008_linkage = prd008_linkage(event, &run_id);

    if let Some(suppress_reason) =
        suppression_reason(event, existing_run_state_records, &run_id, &idempotency_key)
    {
        return suppressed_outcome(
            event,
            trigger_kind,
            trigger_ref,
            idempotency_key,
            suppress_reason,
            prd008_linkage,
        );
    }

    let request = AutomationRunRequest {
        run_id: run_id.clone(),
        job_id: event.job_id.clone(),
        trigger_kind: trigger_kind.clone(),
        trigger_ref: trigger_ref.clone(),
        session_id: event.session_id.clone(),
        goal_id: event.goal_id.clone(),
        execution_mode: event.execution_mode.clone(),
        timeout_policy_ref: event.timeout_policy_ref.clone(),
        retry_policy_ref: event.retry_policy_ref.clone(),
        delivery_policy_ref: event.delivery_policy_ref.clone(),
        recursion_guard_token: event.recursion_guard.token.clone(),
    };
    let run_state_record =
        run_state_record(&request, AutomationRunState::Queued, idempotency_key, None);

    AutomationCoordinationOutcome {
        request: Some(request),
        run_state_record: Some(run_state_record),
        delivery_record: delivery_record(event, &run_id, None),
        suppress_reason: None,
        task_outcome_eligibility: task_outcome_eligibility(event, true),
        prd008_linkage,
    }
}

fn suppression_reason(
    event: &AutomationSourceEvent,
    existing_run_state_records: &[AutomationRunStateRecord],
    run_id: &str,
    idempotency_key: &str,
) -> Option<String> {
    if matches!(event.source, AutomationSourceEventKind::Heartbeat)
        && !event.active_goal
        && !event.pending_automation
    {
        return Some("heartbeat has no active goal or pending automation".to_owned());
    }

    match &event.source {
        AutomationSourceEventKind::Cron {
            approved_automation_rule_ref,
        } if approved_automation_rule_ref.is_none() => {
            return Some("cron wake lacks approved automation rule ref".to_owned());
        }
        AutomationSourceEventKind::SubagentResult { merge_state, .. }
            if !matches!(
                merge_state,
                SubagentMergeState::Terminal | SubagentMergeState::Reviewable
            ) =>
        {
            return Some("subagent result merge state is not terminal or reviewable".to_owned());
        }
        AutomationSourceEventKind::AppTaskResult {
            app_task_id,
            manifest_ref,
            capability_scope,
            ..
        } if app_task_id.is_none() || manifest_ref.is_none() || capability_scope.is_none() => {
            return Some("app task result lacks required evidence fields".to_owned());
        }
        AutomationSourceEventKind::ChannelEvent { user_visible, .. } if !user_visible => {
            return Some("channel event is not user-visible".to_owned());
        }
        AutomationSourceEventKind::LocalApiBackground {
            caller_auth_ref,
            redaction_profile_ref,
            ..
        } if caller_auth_ref.is_none() || redaction_profile_ref.is_none() => {
            return Some(
                "local API background request lacks auth or redaction profile ref".to_owned(),
            );
        }
        _ => {}
    }

    if existing_run_state_records
        .iter()
        .any(|record| record.idempotency_key == idempotency_key)
    {
        return Some("duplicate automation wake idempotency key".to_owned());
    }

    let guard_decision = event.recursion_guard.evaluate_next_run(run_id);
    if !guard_decision.allowed {
        return guard_decision.blocked_reason;
    }

    None
}

fn suppressed_outcome(
    event: &AutomationSourceEvent,
    trigger_kind: AutomationRunTriggerKind,
    trigger_ref: AutomationTriggerRef,
    idempotency_key: String,
    suppress_reason: String,
    prd008_linkage: AutomationPrd008LinkageMetadata,
) -> AutomationCoordinationOutcome {
    let request = AutomationRunRequest {
        run_id: format!("run-{idempotency_key}"),
        job_id: event.job_id.clone(),
        trigger_kind,
        trigger_ref,
        session_id: event.session_id.clone(),
        goal_id: event.goal_id.clone(),
        execution_mode: event.execution_mode.clone(),
        timeout_policy_ref: event.timeout_policy_ref.clone(),
        retry_policy_ref: event.retry_policy_ref.clone(),
        delivery_policy_ref: event.delivery_policy_ref.clone(),
        recursion_guard_token: event.recursion_guard.token.clone(),
    };
    let run_state_record =
        if suppress_reason == "heartbeat has no active goal or pending automation" {
            None
        } else {
            Some(run_state_record(
                &request,
                AutomationRunState::Suppressed,
                idempotency_key,
                Some(suppress_reason.clone()),
            ))
        };

    AutomationCoordinationOutcome {
        request: None,
        run_state_record,
        delivery_record: delivery_record(event, &request.run_id, Some(suppress_reason.clone())),
        suppress_reason: Some(suppress_reason),
        task_outcome_eligibility: task_outcome_eligibility(event, false),
        prd008_linkage,
    }
}

fn run_state_record(
    request: &AutomationRunRequest,
    state: AutomationRunState,
    idempotency_key: String,
    suppress_reason: Option<String>,
) -> AutomationRunStateRecord {
    AutomationRunStateRecord {
        run_id: request.run_id.clone(),
        job_id: request.job_id.clone(),
        trigger_kind: request.trigger_kind.clone(),
        trigger_ref: request.trigger_ref.clone(),
        projection_status: automation_run_state_projection_status(&state),
        state,
        idempotency_key,
        suppress_reason,
    }
}

fn delivery_record(
    event: &AutomationSourceEvent,
    run_id: &str,
    suppress_reason: Option<String>,
) -> Option<AutomationDeliveryRecord> {
    let AutomationSourceEventKind::ChannelEvent {
        user_visible,
        redacted_message,
        target_surface,
        severity,
        ..
    } = &event.source
    else {
        return None;
    };

    if !user_visible {
        return None;
    }

    Some(AutomationDeliveryRecord {
        delivery_id: format!("delivery-{run_id}"),
        run_id: run_id.to_owned(),
        target_surface: target_surface.clone(),
        severity: severity.clone(),
        redacted_message: redacted_message.clone(),
        suppress_reason,
        acknowledged_at_ms: None,
    })
}

fn task_outcome_eligibility(
    event: &AutomationSourceEvent,
    evaluator_should_run: bool,
) -> AutomationTaskOutcomeEligibility {
    let mut evidence_refs = Vec::new();
    let app_authority_can_apply_self_improvement = false;

    match &event.source {
        AutomationSourceEventKind::SubagentResult { result_ref, .. } => {
            evidence_refs.push(result_ref.clone());
        }
        AutomationSourceEventKind::AppTaskResult {
            app_task_id,
            manifest_ref,
            capability_scope,
            evidence_ref,
            ..
        } => {
            evidence_refs.extend(app_task_id.iter().cloned());
            evidence_refs.extend(manifest_ref.iter().cloned());
            evidence_refs.extend(capability_scope.iter().cloned());
            evidence_refs.push(evidence_ref.clone());
        }
        AutomationSourceEventKind::ChannelEvent {
            channel_event_ref, ..
        } => evidence_refs.push(channel_event_ref.clone()),
        AutomationSourceEventKind::LocalApiBackground {
            caller_auth_ref,
            redaction_profile_ref,
            redacted_evidence_ref,
        } => {
            evidence_refs.extend(caller_auth_ref.iter().cloned());
            evidence_refs.extend(redaction_profile_ref.iter().cloned());
            evidence_refs.push(redacted_evidence_ref.clone());
        }
        AutomationSourceEventKind::Cron {
            approved_automation_rule_ref,
        } => evidence_refs.extend(approved_automation_rule_ref.iter().cloned()),
        AutomationSourceEventKind::ManualResume { resume_ref } => {
            evidence_refs.push(resume_ref.clone())
        }
        AutomationSourceEventKind::Heartbeat => {}
    }

    AutomationTaskOutcomeEligibility {
        evaluator_should_run,
        continue_requires_prd008_goal_gate: true,
        direct_execution_allowed: false,
        app_authority_can_apply_self_improvement,
        evidence_refs,
    }
}

fn prd008_linkage(event: &AutomationSourceEvent, run_id: &str) -> AutomationPrd008LinkageMetadata {
    AutomationPrd008LinkageMetadata {
        goal_id: event.goal_id.clone(),
        goal_gate_ref: event.prd008_goal_gate_ref.clone(),
        source_ledger_ref: format!("automation-run:{run_id}"),
        can_build_evaluator_decision_input: event.goal_id.is_some()
            && event.prd008_goal_gate_ref.is_some(),
    }
}

fn trigger_kind(source: &AutomationSourceEventKind) -> AutomationRunTriggerKind {
    match source {
        AutomationSourceEventKind::Heartbeat => AutomationRunTriggerKind::Heartbeat,
        AutomationSourceEventKind::Cron { .. } => AutomationRunTriggerKind::Cron,
        AutomationSourceEventKind::SubagentResult { .. } => {
            AutomationRunTriggerKind::SubagentResult
        }
        AutomationSourceEventKind::AppTaskResult { .. } => AutomationRunTriggerKind::AppTaskResult,
        AutomationSourceEventKind::ChannelEvent { .. } => AutomationRunTriggerKind::ChannelEvent,
        AutomationSourceEventKind::LocalApiBackground { .. } => {
            AutomationRunTriggerKind::LocalApiBackground
        }
        AutomationSourceEventKind::ManualResume { .. } => AutomationRunTriggerKind::ManualResume,
    }
}

fn trigger_ref(
    event: &AutomationSourceEvent,
    trigger_kind: &AutomationRunTriggerKind,
) -> AutomationTriggerRef {
    AutomationTriggerRef {
        runtime_service_event_id: event.runtime_service_event_id.clone(),
        source_type: source_type(trigger_kind).to_owned(),
        source_owner: event.source_owner.clone(),
        received_at_ms: event.received_at_ms,
        idempotency_key: source_idempotency_key(event),
    }
}

fn source_type(trigger_kind: &AutomationRunTriggerKind) -> &'static str {
    match trigger_kind {
        AutomationRunTriggerKind::Heartbeat => "heartbeat",
        AutomationRunTriggerKind::Cron => "cron",
        AutomationRunTriggerKind::SubagentResult => "subagent_result",
        AutomationRunTriggerKind::AppTaskResult => "app_task_result",
        AutomationRunTriggerKind::ChannelEvent => "channel_event",
        AutomationRunTriggerKind::LocalApiBackground => "local_api_background",
        AutomationRunTriggerKind::ManualResume => "manual_resume",
    }
}

fn source_idempotency_key(event: &AutomationSourceEvent) -> String {
    match &event.source {
        AutomationSourceEventKind::Cron {
            approved_automation_rule_ref: Some(rule_ref),
        } => format!("cron:{}:{rule_ref}", event.runtime_service_event_id),
        AutomationSourceEventKind::SubagentResult { result_ref, .. } => {
            format!("subagent-result:{result_ref}")
        }
        AutomationSourceEventKind::AppTaskResult {
            app_task_id: Some(app_task_id),
            ..
        } => format!("app-task-result:{app_task_id}"),
        AutomationSourceEventKind::ChannelEvent {
            channel_event_ref, ..
        } => format!("channel-event:{channel_event_ref}"),
        AutomationSourceEventKind::LocalApiBackground {
            redacted_evidence_ref,
            ..
        } => format!("local-api-background:{redacted_evidence_ref}"),
        AutomationSourceEventKind::ManualResume { resume_ref } => {
            format!("manual-resume:{resume_ref}")
        }
        _ => event.runtime_service_event_id.clone(),
    }
}
