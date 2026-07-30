mod remembered_permissions;

pub use remembered_permissions::{remembered_permissions_view, RememberedPermissionsView};

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

#[cfg(test)]
mod tests {
    use super::*;
    use shacs_workflow::{
        WorkflowBudgetUsage, WorkflowPattern, WorkflowProjection, WorkflowRunState,
    };

    #[test]
    fn workflow_progress_view_renders_shared_projection_without_raw_evidence() {
        let projection = WorkflowProjection {
            schema_label: "024WorkflowProjection".to_owned(),
            schema_version: "024WorkflowProjection.v1".to_owned(),
            workflow_id: "workflow-1".to_owned(),
            objective_summary: "verify the release gate".to_owned(),
            pattern: WorkflowPattern::FanOutAndSynthesize,
            state: WorkflowRunState::Blocked,
            progress_count: 2,
            active_child_count: 0,
            pending_barrier_count: 1,
            verifier_status: "blocked".to_owned(),
            budget_usage: WorkflowBudgetUsage {
                known_tokens: 0,
                estimated_tokens: 300,
                child_runs: 2,
                verifier_runs: 1,
                heavy_commands: 0,
            },
            worktree_refs: vec!["worktree://workflow-1/child-1".to_owned()],
            blocked_reason: Some("workflow is blocked; inspect checkpoint".to_owned()),
            next_action: Some("inspect_blocker".to_owned()),
            resume_available: true,
            evidence_refs: Vec::new(),
        };

        let rendered = workflow_progress_view(&projection).render_plain_text();

        assert!(rendered.contains("workflow workflow-1"));
        assert!(rendered.contains("schema: 024WorkflowProjection.v1"));
        assert!(rendered.contains("state: blocked"));
        assert!(rendered.contains("pattern: fan_out_and_synthesize"));
        assert!(rendered.contains("resume: available"));
        assert!(rendered.contains("evidence: 0 refs"));
        assert!(!rendered.contains("verify the release gate"));
        assert!(!rendered.contains("harness_plan"));
        assert!(!rendered.contains("raw"));
    }

    #[test]
    fn session_workflow_progress_view_renders_bounded_runtime_projection() {
        let projection = SessionRuntimeWorkflowProjection {
            schema_label: Some("024WorkflowProjection".to_owned()),
            schema_version: Some("024WorkflowProjection.v1".to_owned()),
            workflow_id: Some("workflow-2".to_owned()),
            pattern: Some("workflow_sequence".to_owned()),
            state: Some("running".to_owned()),
            progress_count: Some(3),
            active_child_count: Some(1),
            pending_barrier_count: Some(2),
            verifier_status: Some("pending".to_owned()),
            budget_usage: Some(shacs_session::SessionWorkflowBudgetUsage {
                known_tokens: Some(10),
                estimated_tokens: Some(20),
                child_runs: Some(3),
                verifier_runs: Some(1),
                heavy_commands: Some(0),
            }),
            worktree_ref_count: 1,
            evidence_ref_count: 2,
            blocked_reason: None,
            next_action: Some("continue".to_owned()),
            resume_available: true,
        };

        let rendered = session_workflow_progress_view(&projection).render_plain_text();

        assert!(rendered.contains("workflow workflow-2"));
        assert!(rendered.contains("state: running"));
        assert!(rendered.contains("pattern: workflow_sequence"));
        assert!(rendered.contains("budget: child_runs=3 verifier_runs=1 heavy_commands=0"));
        assert!(rendered.contains("worktrees: 1 refs"));
        assert!(rendered.contains("evidence: 2 refs"));
    }
}
