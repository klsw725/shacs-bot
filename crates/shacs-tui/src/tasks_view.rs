use shacs_projection::{
    Spec031Freshness, Spec033AutomationJobStatus, Spec033DeliveryStatus, Spec035AppProcessState,
    Spec035ChildTerminal, Spec035RecoveryStatus, Spec035TaskActionKind, Spec035TaskActionStatus,
    Spec035TaskCount, Spec035TaskDetail, Spec035TaskOwnerKind, Spec035TaskState,
    Spec035TasksProjection, Spec035WorkflowPhase,
};

pub fn tasks_projection_lines(projection: &Spec035TasksProjection) -> Vec<String> {
    let coverage = projection.coverage();
    let mut lines = vec![format!(
        "tasks coverage: goal={} child={} workflow={} automation={} app={} recovery={}",
        count_label(coverage.goal),
        count_label(coverage.child),
        count_label(coverage.workflow),
        count_label(coverage.automation),
        count_label(coverage.app),
        count_label(coverage.recovery),
    )];
    if projection.rows().is_empty() {
        lines.push("tasks: no owner rows".to_owned());
        return lines;
    }
    for row in projection.rows() {
        lines.push(format!(
            "task: owner={} locator={} observed={} freshness={} state={}",
            owner_kind_label(row.owner().kind()),
            row.owner().locator().as_str(),
            row.observed_at_unix_ms().as_u64(),
            freshness_label(row.freshness()),
            state_label(row.state()),
        ));
        lines.push(detail_line(row.detail()));
        lines.push(row.action().map_or_else(
            || "task action: unavailable (unadvertised)".to_owned(),
            |action| {
                format!(
                    "action: {} status={}",
                    action_kind_label(action.kind),
                    action_status_label(action.status)
                )
            },
        ));
    }
    lines
}

pub const fn owner_kind_label(kind: Spec035TaskOwnerKind) -> &'static str {
    match kind {
        Spec035TaskOwnerKind::Goal => "goal",
        Spec035TaskOwnerKind::Child => "child",
        Spec035TaskOwnerKind::Workflow => "workflow",
        Spec035TaskOwnerKind::Automation => "automation",
        Spec035TaskOwnerKind::App => "app",
        Spec035TaskOwnerKind::Recovery => "recovery",
    }
}

pub const fn action_kind_label(kind: Spec035TaskActionKind) -> &'static str {
    match kind {
        Spec035TaskActionKind::Pause => "pause",
        Spec035TaskActionKind::Resume => "resume",
        Spec035TaskActionKind::Stop => "stop",
        Spec035TaskActionKind::Retry => "retry",
        Spec035TaskActionKind::Recover => "recover",
        Spec035TaskActionKind::Inspect => "inspect",
    }
}

fn count_label(count: Spec035TaskCount) -> String {
    match count {
        Spec035TaskCount::Available { count } => count.as_u64().to_string(),
        Spec035TaskCount::Unavailable => "unavailable".to_owned(),
    }
}

fn detail_line(detail: &Spec035TaskDetail) -> String {
    match detail {
        Spec035TaskDetail::Goal { fact } => format!(
            "goal: id={} status={} stop={} budget={}/{} remaining={}",
            fact.goal_id,
            goal_status_label(fact.status),
            fact.stop_reason.as_deref().unwrap_or("none"),
            fact.budget.turns_used,
            fact.budget.turn_budget,
            fact.budget.remaining_turns,
        ),
        Spec035TaskDetail::Child { attempt, terminal } => format!(
            "child: attempt={} terminal={}",
            attempt.as_u64(),
            terminal.map_or("none", child_terminal_label)
        ),
        Spec035TaskDetail::Workflow {
            phase,
            completed_steps,
            active_children,
        } => format!(
            "workflow: phase={} completed_steps={} active_children={}",
            workflow_phase_label(*phase),
            completed_steps.as_u64(),
            active_children.as_u64()
        ),
        Spec035TaskDetail::Automation {
            job_status,
            delivery_status,
        } => format!(
            "automation: job={} delivery={}",
            automation_job_label(*job_status),
            delivery_label(*delivery_status)
        ),
        Spec035TaskDetail::App { process_state } => {
            format!("app: process={}", app_process_label(*process_state))
        }
        Spec035TaskDetail::Recovery {
            status,
            issue_count,
        } => format!(
            "recovery task: status={} issues={}",
            recovery_status_label(*status),
            issue_count.as_u64()
        ),
    }
}

const fn freshness_label(value: Spec031Freshness) -> &'static str {
    match value {
        Spec031Freshness::Current => "current",
        Spec031Freshness::Stale => "stale",
        Spec031Freshness::Unavailable => "unavailable",
        Spec031Freshness::Unknown => "unknown",
    }
}

const fn child_terminal_label(value: Spec035ChildTerminal) -> &'static str {
    match value {
        Spec035ChildTerminal::Completed => "completed",
        Spec035ChildTerminal::Failed => "failed",
        Spec035ChildTerminal::TimedOut => "timed_out",
        Spec035ChildTerminal::Cancelled => "cancelled",
    }
}

const fn workflow_phase_label(value: Spec035WorkflowPhase) -> &'static str {
    match value {
        Spec035WorkflowPhase::Planned => "planned",
        Spec035WorkflowPhase::Admitted => "admitted",
        Spec035WorkflowPhase::Running => "running",
        Spec035WorkflowPhase::WaitingForChildren => "waiting_for_children",
        Spec035WorkflowPhase::Verifying => "verifying",
        Spec035WorkflowPhase::Synthesizing => "synthesizing",
        Spec035WorkflowPhase::WaitingForUser => "waiting_for_user",
        Spec035WorkflowPhase::Blocked => "blocked",
        Spec035WorkflowPhase::Completed => "completed",
        Spec035WorkflowPhase::Failed => "failed",
        Spec035WorkflowPhase::Cancelled => "cancelled",
        Spec035WorkflowPhase::Stale => "stale",
    }
}

const fn automation_job_label(value: Spec033AutomationJobStatus) -> &'static str {
    match value {
        Spec033AutomationJobStatus::Pending => "pending",
        Spec033AutomationJobStatus::Succeeded => "succeeded",
        Spec033AutomationJobStatus::Failed => "failed",
        Spec033AutomationJobStatus::TimedOut => "timed_out",
        Spec033AutomationJobStatus::Cancelled => "cancelled",
        Spec033AutomationJobStatus::Suppressed => "suppressed",
    }
}

const fn delivery_label(value: Spec033DeliveryStatus) -> &'static str {
    match value {
        Spec033DeliveryStatus::NotRequested => "not_requested",
        Spec033DeliveryStatus::Pending => "pending",
        Spec033DeliveryStatus::Succeeded => "succeeded",
        Spec033DeliveryStatus::Failed => "failed",
    }
}

const fn app_process_label(value: Spec035AppProcessState) -> &'static str {
    match value {
        Spec035AppProcessState::Stopped => "stopped",
        Spec035AppProcessState::Starting => "starting",
        Spec035AppProcessState::Running => "running",
        Spec035AppProcessState::Stopping => "stopping",
        Spec035AppProcessState::Failed => "failed",
        Spec035AppProcessState::RecoveryNeeded => "recovery_needed",
    }
}

const fn recovery_status_label(value: Spec035RecoveryStatus) -> &'static str {
    match value {
        Spec035RecoveryStatus::Healthy => "healthy",
        Spec035RecoveryStatus::Recoverable => "recoverable",
        Spec035RecoveryStatus::InspectOnly => "inspect_only",
        Spec035RecoveryStatus::Blocked => "blocked",
        Spec035RecoveryStatus::Recovered => "recovered",
        Spec035RecoveryStatus::Unavailable => "unavailable",
    }
}

const fn state_label(value: Spec035TaskState) -> &'static str {
    match value {
        Spec035TaskState::Unavailable => "unavailable",
        Spec035TaskState::Unknown => "unknown",
        Spec035TaskState::Pending => "pending",
        Spec035TaskState::Requested => "requested",
        Spec035TaskState::Running => "running",
        Spec035TaskState::Paused => "paused",
        Spec035TaskState::Blocked => "blocked",
        Spec035TaskState::Done => "done",
        Spec035TaskState::Failed => "failed",
        Spec035TaskState::TimedOut => "timed_out",
        Spec035TaskState::Cancelled => "cancelled",
        Spec035TaskState::Suppressed => "suppressed",
        Spec035TaskState::Recovered => "recovered",
    }
}

const fn action_status_label(value: Spec035TaskActionStatus) -> &'static str {
    match value {
        Spec035TaskActionStatus::Available => "available",
        Spec035TaskActionStatus::Requested => "requested",
        Spec035TaskActionStatus::Completed => "completed",
    }
}

const fn goal_status_label(value: shacs_projection::Spec033GoalStatus) -> &'static str {
    match value {
        shacs_projection::Spec033GoalStatus::Unavailable => "unavailable",
        shacs_projection::Spec033GoalStatus::Active => "active",
        shacs_projection::Spec033GoalStatus::Paused => "paused",
        shacs_projection::Spec033GoalStatus::Blocked => "blocked",
        shacs_projection::Spec033GoalStatus::Done => "done",
        shacs_projection::Spec033GoalStatus::Cleared => "cleared",
    }
}
