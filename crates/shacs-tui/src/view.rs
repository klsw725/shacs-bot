// allow: SIZE_OK — preexisting TUI renderer; Spec034 diff is one focused media-view projection hook
use crate::state::{
    action_outcome_label, ApprovalActionState, ApprovalStatus, RuntimeSession, TuiState, UiStatus,
};
use crate::trusted_runtime_view::trusted_runtime_lines;
use crate::workflow_view::session_workflow_progress_view;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use shacs_projection::{
    Spec033AutomationJobStatus, Spec033DeliveryStatus, Spec033EvaluatorRoute, Spec033GoalStatus,
    Spec033HookConfirmationFact, Spec033ReplayStatus,
};
use unicode_width::UnicodeWidthStr;

pub fn draw_tui(frame: &mut Frame<'_>, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(frame.size());
    let sessions = state.sessions.iter().enumerate().map(|(index, session)| {
        let marker = if index == state.selected { ">" } else { " " };
        ListItem::new(format!("{marker} {}", session.key))
    });
    frame.render_widget(
        List::new(sessions).block(Block::default().title("sessions").borders(Borders::ALL)),
        chunks[0],
    );
    let runtime_block = Block::default()
        .title("runtime projection")
        .borders(Borders::ALL);
    let runtime_inner = runtime_block.inner(chunks[1]);
    let runtime_lines = visible_lines(
        render_lines_for_width(state, runtime_inner.width),
        usize::from(runtime_inner.height),
    )
    .join("\n");
    frame.render_widget(
        Paragraph::new(runtime_lines).block(runtime_block),
        chunks[1],
    );
}

pub fn render_lines(state: &TuiState) -> Vec<String> {
    let width = state.terminal_size.columns.saturating_sub(4);
    render_lines_for_width(state, width)
}

pub fn render_lines_for_width(state: &TuiState, width: u16) -> Vec<String> {
    let width = usize::from(width);
    let mut lines = vec!["shacs-tui Spec031 live runtime".to_owned()];
    match &state.status {
        UiStatus::Ready => lines.push("status: ready".to_owned()),
        UiStatus::Empty => lines.push("status: no sessions".to_owned()),
        UiStatus::InvalidAction(reason) => lines.push(format!("invalid action: {reason}")),
        UiStatus::ActionUnavailable(reason) => lines.push(format!("action unavailable: {reason}")),
        UiStatus::ActionOutcome(outcome) => lines.push(format!(
            "action {}: {}",
            action_outcome_label(outcome.kind),
            outcome.detail
        )),
        UiStatus::SourceError(reason) => lines.push(format!("source error: {reason}")),
        UiStatus::Exiting => lines.push("status: exiting".to_owned()),
    }
    if let Some(session) = state.selected_session() {
        lines.extend(session_lines(session));
    } else {
        lines.push("runtime projection: no selected session".to_owned());
    }
    lines.extend(trusted_runtime_lines(&state.trusted_runtime));
    lines.extend(key_help_lines(state));
    lines.into_iter().map(|line| clip(&line, width)).collect()
}

fn key_help_lines(state: &TuiState) -> Vec<String> {
    let approval_help = state
        .selected_session()
        .and_then(|session| session.pending_approval.as_ref())
        .map(|approval| match &approval.action {
            ApprovalActionState::Actionable { .. } => "[a] approve [d] deny".to_owned(),
            ApprovalActionState::Unavailable { reason } => {
                format!("approval unavailable: {reason}")
            }
        })
        .unwrap_or_else(|| "approval unavailable: no pending approval".to_owned());
    vec![
        "keys: [up/down] select [r] refresh [s] stop [R] restart".to_owned(),
        format!("keys: [e] recover [x] cancel [q] quit; {approval_help}"),
    ]
}

fn session_lines(session: &RuntimeSession) -> Vec<String> {
    let mut lines = vec![
        format!("active session: {}", session.key),
        format!(
            "updated: {}",
            session.updated_at.as_deref().unwrap_or("unknown")
        ),
        format!("messages: {}", session.message_count),
        readiness_line(session),
        recovery_line(session),
    ];
    if let Some(workflow) = &session.workflow {
        lines.extend(session_workflow_progress_view(workflow).lines);
    } else {
        lines.push("workflow: none".to_owned());
    }
    if let Some(execution) = &session.execution {
        lines.push(format!(
            "runtime pending: {} final outcomes: {}",
            execution.pending_count, execution.outcome_count
        ));
        lines.extend(recent_outcome_lines(session));
    }
    if let Some(approval) = &session.pending_approval {
        lines.push(format!(
            "approval: status={} tool={} lineage={} expires={}",
            approval_status(approval.status),
            approval.tool_name,
            approval.lineage.as_str(),
            approval
                .expires_at_unix_ms
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        ));
    } else {
        lines.push("approval: none".to_owned());
    }
    lines.extend(session.media.lines().iter().cloned());
    lines.extend(spec033_lines(session));
    lines
}

fn spec033_lines(session: &RuntimeSession) -> Vec<String> {
    let projection = &session.spec033;
    let mut lines = Vec::new();
    if let Some(goal) = projection.goal.fact.as_ref() {
        lines.push(format!(
            "task goal: status={} stop={} budget={}/{} remaining={}",
            goal_status_label(goal.status),
            goal.stop_reason.as_deref().unwrap_or("none"),
            goal.budget.turns_used,
            goal.budget.turn_budget,
            goal.budget.remaining_turns
        ));
    } else {
        lines.push("task goal: unavailable".to_owned());
    }
    if let Some(evaluator) = projection.evaluator.fact.as_ref() {
        lines.push(format!(
            "evaluator: verdict={} route={}",
            evaluator.verdict,
            evaluator_route_label(evaluator.route)
        ));
    } else {
        lines.push("evaluator: unavailable".to_owned());
    }
    if let Some(automation) = projection.automation.fact.as_ref() {
        lines.push(format!(
            "automation: job={} delivery={}",
            automation_job_label(automation.job_status),
            delivery_label(automation.delivery_status)
        ));
    } else {
        lines.push("automation: unavailable".to_owned());
    }
    if let Some(confirmation) = projection.hook_confirmation.fact {
        lines.push(format!(
            "hook confirmation: {}",
            confirmation_label(confirmation)
        ));
    } else {
        lines.push("hook confirmation: unavailable".to_owned());
    }
    if let Some(improvement) = projection.self_improvement.fact.as_ref() {
        lines.push(format!(
            "workspace improvement: proposal={} applied={} rolled_back={}",
            improvement.proposal_id, improvement.applied, improvement.rolled_back
        ));
    } else {
        lines.push("workspace improvement: unavailable".to_owned());
    }
    if let Some(verify) = projection.verify.fact.as_ref() {
        lines.push(format!("workspace verify: passed={}", verify.passed));
    } else {
        lines.push("workspace verify: unavailable".to_owned());
    }
    if let Some(candidate) = projection.rollback_candidate.fact.as_ref() {
        lines.push(format!(
            "workspace rollback candidate: {}",
            candidate.verify_failure_ref
        ));
    } else {
        lines.push("workspace rollback candidate: unavailable".to_owned());
    }
    if let Some(replay) = projection.replay.fact.as_ref() {
        lines.push(format!(
            "workspace replay: result={}",
            replay_label(replay.status)
        ));
    } else {
        lines.push("workspace replay: unavailable".to_owned());
    }
    lines
}

fn evaluator_route_label(value: Spec033EvaluatorRoute) -> &'static str {
    match value {
        Spec033EvaluatorRoute::Notify => "notify",
        Spec033EvaluatorRoute::Suppress => "suppress",
        Spec033EvaluatorRoute::Continue => "continue",
        Spec033EvaluatorRoute::Escalate => "escalate",
        Spec033EvaluatorRoute::Verify => "verify",
        Spec033EvaluatorRoute::RollbackCandidate => "rollback_candidate",
    }
}

fn goal_status_label(value: Spec033GoalStatus) -> &'static str {
    match value {
        Spec033GoalStatus::Unavailable => "unavailable",
        Spec033GoalStatus::Active => "active",
        Spec033GoalStatus::Paused => "paused",
        Spec033GoalStatus::Blocked => "blocked",
        Spec033GoalStatus::Done => "done",
        Spec033GoalStatus::Cleared => "cleared",
    }
}

fn automation_job_label(value: Spec033AutomationJobStatus) -> &'static str {
    match value {
        Spec033AutomationJobStatus::Pending => "pending",
        Spec033AutomationJobStatus::Succeeded => "succeeded",
        Spec033AutomationJobStatus::Failed => "failed",
        Spec033AutomationJobStatus::TimedOut => "timed_out",
        Spec033AutomationJobStatus::Cancelled => "cancelled",
        Spec033AutomationJobStatus::Suppressed => "suppressed",
    }
}

fn delivery_label(value: Spec033DeliveryStatus) -> &'static str {
    match value {
        Spec033DeliveryStatus::NotRequested => "not_requested",
        Spec033DeliveryStatus::Pending => "pending",
        Spec033DeliveryStatus::Succeeded => "succeeded",
        Spec033DeliveryStatus::Failed => "failed",
    }
}

fn confirmation_label(value: Spec033HookConfirmationFact) -> &'static str {
    match value {
        Spec033HookConfirmationFact::NotRequired => "not_required",
        Spec033HookConfirmationFact::Confirmed => "confirmed",
        Spec033HookConfirmationFact::Denied => "denied",
        Spec033HookConfirmationFact::HeadlessDenied => "headless_denied",
        Spec033HookConfirmationFact::Vetoed => "vetoed",
        Spec033HookConfirmationFact::Failed => "failed",
    }
}

fn replay_label(value: Spec033ReplayStatus) -> &'static str {
    match value {
        Spec033ReplayStatus::Passed => "passed",
        Spec033ReplayStatus::Failed => "failed",
        Spec033ReplayStatus::Blocked => "blocked",
    }
}

fn readiness_line(session: &RuntimeSession) -> String {
    if let Some(workflow) = &session.workflow {
        if let Some(reason) = &workflow.blocked_reason {
            return format!("readiness: blocked reason={reason}");
        }
    }
    if session.recovery_markers.is_empty() {
        "readiness: ready".to_owned()
    } else {
        format!("readiness: degraded {}", session.recovery_markers.join(","))
    }
}

fn recovery_line(session: &RuntimeSession) -> String {
    format!(
        "recovery: markers={} checkpoint={} diagnostics={}",
        session.recovery_markers.len(),
        session.checkpoint_phase.as_deref().unwrap_or("none"),
        session.diagnostics_ref_count
    )
}

fn recent_outcome_lines(session: &RuntimeSession) -> Vec<String> {
    let Some(execution) = &session.execution else {
        return Vec::new();
    };
    execution
        .recent_outcomes
        .iter()
        .rev()
        .take(3)
        .map(|outcome| {
            format!(
                "outcome: {} {} {}",
                outcome.domain, outcome.outcome, outcome.decision
            )
        })
        .collect()
}

fn approval_status(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Executing => "executing",
        ApprovalStatus::Unknown => "unknown",
    }
}

fn clip(line: &str, width: usize) -> String {
    if UnicodeWidthStr::width(line) <= width {
        return line.to_owned();
    }
    let mut output = String::new();
    for character in line.chars() {
        let next = format!("{output}{character}");
        if UnicodeWidthStr::width(next.as_str()) > width.saturating_sub(1) {
            break;
        }
        output.push(character);
    }
    if width > 0 {
        output.push('…');
    }
    output
}

fn visible_lines(lines: Vec<String>, height: usize) -> Vec<String> {
    if lines.len() <= height {
        return lines;
    }
    if height < 3 {
        return lines.into_iter().take(height).collect();
    }
    let footer = lines[lines.len() - 2..].to_vec();
    let content_height = height.saturating_sub(3);
    let hidden = lines.len().saturating_sub(content_height + footer.len());
    let mut visible = lines.into_iter().take(content_height).collect::<Vec<_>>();
    visible.push(format!("… {hidden} more lines hidden"));
    visible.extend(footer);
    visible
}
