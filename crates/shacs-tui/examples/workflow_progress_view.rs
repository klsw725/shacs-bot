use shacs_tui::workflow_progress_view;
use shacs_workflow::{WorkflowBudgetUsage, WorkflowPattern, WorkflowProjection, WorkflowRunState};

fn main() {
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

    println!(
        "{}",
        workflow_progress_view(&projection).render_plain_text()
    );
}
