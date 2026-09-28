#[path = "support/tui_session.rs"]
mod tui_session;

use shacs_projection::{
    Spec033AutomationFact, Spec033AutomationJobStatus, Spec033Availability, Spec033DeliveryStatus,
    Spec033EvidenceSource, Spec033GoalBudgetFact, Spec033GoalFact, Spec033GoalOwner,
    Spec033GoalStatus, Spec033Owner, Spec033OwnerFact, Spec033Snapshot,
};
use shacs_tui::{
    state::{RuntimeSnapshot, TuiState},
    view::render_lines,
};
use std::error::Error;
use tui_session::fixture_session;

#[test]
fn workflow_view_keeps_blocked_next_and_cjk_safe_clipping() -> Result<(), Box<dyn Error>> {
    let mut session = fixture_session("cli:cjk", "approval-cjk", 2, 0);
    if let Some(workflow) = session.workflow.as_mut() {
        workflow.blocked_reason = Some("복구요청 대기".to_owned());
        workflow.next_action = Some("recover_after_audit".to_owned());
    }
    session.recovery_markers = vec!["복구요청".to_owned(), "런타임진행".to_owned()];
    let mut state = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![session],
        },
        None,
    );
    state.terminal_size.columns = 28;

    let rendered = render_lines(&state);

    assert!(rendered
        .iter()
        .all(|line| unicode_width::UnicodeWidthStr::width(line.as_str()) <= 24));
    let joined = rendered.join("\n");
    assert!(joined.contains("복구"));
    assert!(joined.contains("blocked:"));
    assert!(joined.contains("next:"));
    Ok(())
}

#[test]
fn tasks_view_renders_spec033_goal_and_automation_owner_facts() -> Result<(), Box<dyn Error>> {
    let mut session = fixture_session("cli:one", "approval-live", 1, 0);
    let mut projection = Spec033Snapshot::unavailable("cli:one");
    projection.goal = Spec033GoalOwner {
        availability: Spec033Availability::Available,
        fact: Some(Spec033GoalFact {
            goal_id: "goal-1".to_owned(),
            session_id: "cli:one".to_owned(),
            status: Spec033GoalStatus::Blocked,
            turn_budget: 8,
            turns_used: 3,
            last_verdict: None,
            blocked: true,
            stop_reason: Some("evaluator_blocked".to_owned()),
            budget: Spec033GoalBudgetFact {
                turn_budget: 8,
                turns_used: 3,
                remaining_turns: 5,
            },
            usage: shacs_projection::Spec033GoalUsageSummary {
                turn_limit: 8,
                turns_used: 3,
                remaining_turns: 5,
                exhausted: false,
            },
            user_interrupted: false,
            latest_transition: None,
        }),
        lineage: shacs_projection::Spec033EvidenceLineage::new(
            Spec033Owner::Goal,
            Spec033EvidenceSource::SessionMetadata,
            vec!["session_metadata:persistent_goal".to_owned()],
        ),
    };
    projection.automation = Spec033OwnerFact::available(
        Spec033Owner::Automation,
        Spec033EvidenceSource::DurableStore,
        Spec033AutomationFact {
            work_id: "work-1".to_owned(),
            job_id: "job-1".to_owned(),
            run_id: "run-1".to_owned(),
            turn_id: None,
            snapshot_id: None,
            snapshot_digest: None,
            checkpoint_id: None,
            artifact_refs: Vec::new(),
            job_status: Spec033AutomationJobStatus::Succeeded,
            delivery_status: Spec033DeliveryStatus::Failed,
        },
        vec!["durable_work:work-1:terminal:7".to_owned()],
    );
    session.spec033 = projection;
    let state = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![session],
        },
        None,
    );

    let rendered = render_lines(&state).join("\n");

    assert!(rendered
        .contains("task goal: status=blocked stop=evaluator_blocked budget=3/8 remaining=5"));
    assert!(rendered.contains("automation: job=succeeded delivery=failed"));
    assert!(rendered.contains("workspace improvement: unavailable"));
    assert!(rendered.contains("workspace verify: unavailable"));
    assert!(rendered.contains("workspace replay: unavailable"));
    Ok(())
}
