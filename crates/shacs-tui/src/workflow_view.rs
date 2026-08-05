use shacs_session::SessionRuntimeWorkflowProjection;
use shacs_workflow::{WorkflowPattern, WorkflowProjection, WorkflowRunState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowProgressView {
    pub title: String,
    pub lines: Vec<String>,
}

impl WorkflowProgressView {
    pub fn render_plain_text(&self) -> String {
        std::iter::once(self.title.clone())
            .chain(self.lines.clone())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn workflow_progress_view(projection: &WorkflowProjection) -> WorkflowProgressView {
    let mut lines = vec![
        format!("schema: {}", projection.schema_version),
        format!("state: {}", run_state_label(projection.state)),
        format!("pattern: {}", pattern_label(projection.pattern)),
        format!("progress: {} completed steps", projection.progress_count),
        format!("active children: {}", projection.active_child_count),
        format!("pending barriers: {}", projection.pending_barrier_count),
        format!("verifier: {}", projection.verifier_status),
        format!(
            "budget: child_runs={} verifier_runs={} heavy_commands={}",
            projection.budget_usage.child_runs,
            projection.budget_usage.verifier_runs,
            projection.budget_usage.heavy_commands
        ),
        format!("worktrees: {} refs", projection.worktree_refs.len()),
        format!("resume: {}", resume_label(projection.resume_available)),
    ];
    if let Some(blocked_reason) = projection.blocked_reason.as_ref() {
        lines.push(format!("blocked: {blocked_reason}"));
    }
    if let Some(next_action) = projection.next_action.as_ref() {
        lines.push(format!("next: {next_action}"));
    }
    lines.push(format!("evidence: {} refs", projection.evidence_refs.len()));

    WorkflowProgressView {
        title: format!("workflow {}", projection.workflow_id),
        lines,
    }
}

pub fn session_workflow_progress_view(
    projection: &SessionRuntimeWorkflowProjection,
) -> WorkflowProgressView {
    let workflow_id = projection.workflow_id.as_deref().unwrap_or("unknown");
    let mut lines = vec![
        format!(
            "schema: {}",
            projection.schema_version.as_deref().unwrap_or("unknown")
        ),
        format!(
            "state: {}",
            projection.state.as_deref().unwrap_or("unknown")
        ),
        format!(
            "pattern: {}",
            projection.pattern.as_deref().unwrap_or("unknown")
        ),
        format!(
            "progress: {} completed steps",
            projection.progress_count.unwrap_or_default()
        ),
        format!(
            "active children: {}",
            projection.active_child_count.unwrap_or_default()
        ),
        format!(
            "pending barriers: {}",
            projection.pending_barrier_count.unwrap_or_default()
        ),
        format!(
            "verifier: {}",
            projection.verifier_status.as_deref().unwrap_or("unknown")
        ),
        format!("budget: {}", session_budget_label(projection)),
        format!("worktrees: {} refs", projection.worktree_ref_count),
        format!("resume: {}", resume_label(projection.resume_available)),
    ];
    if let Some(blocked_reason) = projection.blocked_reason.as_ref() {
        lines.push(format!("blocked: {blocked_reason}"));
    }
    if let Some(next_action) = projection.next_action.as_ref() {
        lines.push(format!("next: {next_action}"));
    }
    lines.push(format!("evidence: {} refs", projection.evidence_ref_count));

    WorkflowProgressView {
        title: format!("workflow {workflow_id}"),
        lines,
    }
}

fn session_budget_label(projection: &SessionRuntimeWorkflowProjection) -> String {
    let Some(budget) = projection.budget_usage.as_ref() else {
        return "unknown".to_owned();
    };
    format!(
        "child_runs={} verifier_runs={} heavy_commands={}",
        budget.child_runs.unwrap_or_default(),
        budget.verifier_runs.unwrap_or_default(),
        budget.heavy_commands.unwrap_or_default()
    )
}

fn resume_label(resume_available: bool) -> &'static str {
    if resume_available {
        "available"
    } else {
        "not_available"
    }
}

fn run_state_label(state: WorkflowRunState) -> &'static str {
    match state {
        WorkflowRunState::Planned => "planned",
        WorkflowRunState::Admitted => "admitted",
        WorkflowRunState::Running => "running",
        WorkflowRunState::WaitingForChildren => "waiting_for_children",
        WorkflowRunState::Verifying => "verifying",
        WorkflowRunState::Synthesizing => "synthesizing",
        WorkflowRunState::WaitingForUser => "waiting_for_user",
        WorkflowRunState::Blocked => "blocked",
        WorkflowRunState::Completed => "completed",
        WorkflowRunState::Failed => "failed",
        WorkflowRunState::Cancelled => "cancelled",
        WorkflowRunState::Stale => "stale",
    }
}

fn pattern_label(pattern: WorkflowPattern) -> &'static str {
    match pattern {
        WorkflowPattern::ClassifyAndAct => "classify_and_act",
        WorkflowPattern::FanOutAndSynthesize => "fan_out_and_synthesize",
        WorkflowPattern::AdversarialVerification => "adversarial_verification",
        WorkflowPattern::GenerateAndFilter => "generate_and_filter",
        WorkflowPattern::Tournament => "tournament",
        WorkflowPattern::LoopUntilDone => "loop_until_done",
        WorkflowPattern::WorkflowSequence => "workflow_sequence",
        WorkflowPattern::Hybrid => "hybrid",
    }
}
