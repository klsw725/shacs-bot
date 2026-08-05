use shacs_core::runtime::SurfaceAction;
use shacs_tui::{
    input::TuiInput,
    state::{ApprovalLineage, RuntimeSnapshot, SessionKey, TuiState},
    update::{apply_input, approval_by_lineage, UpdateEffect},
    view::render_lines,
};
use std::error::Error;

#[test]
fn state_reports_unavailable_actions_without_recording_local_success() -> Result<(), Box<dyn Error>>
{
    let snapshot = RuntimeSnapshot {
        sessions: vec![fixture_session("cli:one", "approval-live", 1, 0)],
    };
    let mut state = TuiState::from_snapshot(snapshot, None);

    let effect = approval_by_lineage(
        &mut state,
        &SessionKey::new("cli:one")?,
        &ApprovalLineage::new("stale")?,
        true,
    );
    assert_eq!(effect, UpdateEffect::None);
    assert!(render_lines(&state)
        .join("\n")
        .contains("stale approval lineage"));

    let effect = approval_by_lineage(
        &mut state,
        &SessionKey::new("cli:other")?,
        &ApprovalLineage::new("approval-live")?,
        true,
    );
    assert_eq!(effect, UpdateEffect::None);
    assert!(render_lines(&state)
        .join("\n")
        .contains("approval session mismatch"));

    assert_eq!(
        apply_input(&mut state, TuiInput::Approve),
        UpdateEffect::RunAction(SurfaceAction::Approve {
            session_key: "cli:one".to_owned(),
            lineage: "approval-live".to_owned()
        })
    );
    assert!(!render_lines(&state).join("\n").contains("requested:"));
    assert!(render_lines(&state)
        .join("\n")
        .contains("a approve / d deny"));

    assert_eq!(
        apply_input(&mut state, TuiInput::Cancel),
        UpdateEffect::None
    );
    assert!(render_lines(&state)
        .join("\n")
        .contains("lineage cancel is unavailable"));
    assert_eq!(
        apply_input(&mut state, TuiInput::Recover),
        UpdateEffect::RunAction(SurfaceAction::Recover)
    );
    assert_eq!(
        apply_input(&mut state, TuiInput::Stop),
        UpdateEffect::RunAction(SurfaceAction::Stop)
    );
    assert_eq!(
        apply_input(&mut state, TuiInput::Restart),
        UpdateEffect::RunAction(SurfaceAction::Restart)
    );

    assert_eq!(
        apply_input(&mut state, TuiInput::Refresh),
        UpdateEffect::RefreshRequested
    );
    assert_eq!(
        apply_input(
            &mut state,
            TuiInput::Resize {
                columns: 32,
                rows: 10
            }
        ),
        UpdateEffect::None
    );
    assert_eq!(state.terminal_size.columns, 32);
    assert_eq!(
        apply_input(&mut state, TuiInput::Invalid),
        UpdateEffect::None
    );
    assert!(render_lines(&state).join("\n").contains("invalid action"));
    assert_eq!(
        apply_input(&mut state, TuiInput::Exit),
        UpdateEffect::ExitRequested
    );
    Ok(())
}

#[test]
fn approval_key_help_tracks_live_action_capability() -> Result<(), Box<dyn Error>> {
    let actionable = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![fixture_session("cli:one", "approval-live", 1, 0)],
        },
        None,
    );
    assert!(render_lines(&actionable)
        .join("\n")
        .contains("a approve / d deny"));
    assert!(!render_lines(&actionable)
        .join("\n")
        .contains("owner-fixture"));

    let mut unavailable_session = fixture_session("cli:one", "approval-live", 1, 0);
    unavailable_session
        .pending_approval
        .as_mut()
        .ok_or("missing fixture approval")?
        .action =
        shacs_tui::state::ApprovalActionState::unavailable("no active runtime owner found");
    let unavailable = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![unavailable_session],
        },
        None,
    );
    assert!(render_lines(&unavailable)
        .join("\n")
        .contains("a/d unavailable (no active runtime owner found)"));

    let empty = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: Vec::new(),
        },
        None,
    );
    assert!(render_lines(&empty)
        .join("\n")
        .contains("a/d unavailable (no pending approval)"));
    Ok(())
}

#[test]
fn unavailable_approval_key_does_not_enqueue_action() -> Result<(), Box<dyn Error>> {
    let mut session = fixture_session("cli:one", "approval-live", 1, 0);
    session
        .pending_approval
        .as_mut()
        .ok_or("missing fixture approval")?
        .action = shacs_tui::state::ApprovalActionState::unavailable(
        "stale ownership marker exists; run `shacs-bot runtime recover`",
    );
    let mut state = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![session],
        },
        None,
    );

    assert_eq!(
        apply_input(&mut state, TuiInput::Approve),
        UpdateEffect::None
    );
    assert!(render_lines(&state)
        .join("\n")
        .contains("stale ownership marker exists"));
    Ok(())
}

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

fn fixture_session(
    key: &str,
    lineage: &str,
    progress: u64,
    outcomes: u64,
) -> shacs_tui::state::RuntimeSession {
    shacs_tui::state::RuntimeSession {
        key: SessionKey::new(key)
            .unwrap_or_else(|error| panic!("fixture session key failed: {error:?}")),
        updated_at: Some("2026-08-02T00:00:00Z".to_owned()),
        message_count: 2,
        recovery_markers: Vec::new(),
        checkpoint_phase: None,
        diagnostics_ref_count: 0,
        workflow: Some(shacs_session::SessionRuntimeWorkflowProjection {
            schema_label: Some("024WorkflowProjection".to_owned()),
            schema_version: Some("024WorkflowProjection.v1".to_owned()),
            workflow_id: Some("wf-1".to_owned()),
            pattern: Some("workflow_sequence".to_owned()),
            state: Some("running".to_owned()),
            progress_count: Some(progress),
            active_child_count: Some(1),
            pending_barrier_count: Some(0),
            verifier_status: Some("pending".to_owned()),
            budget_usage: None,
            worktree_ref_count: 0,
            evidence_ref_count: 0,
            blocked_reason: None,
            next_action: None,
            resume_available: false,
        }),
        execution: Some(shacs_session::SessionRuntimeExecutionProjection {
            pending_count: 1,
            outcome_count: outcomes,
            pending_by_domain: shacs_session::SessionRuntimeExecutionDomainCounts::default(),
            outcomes_by_domain: shacs_session::SessionRuntimeExecutionDomainCounts::default(),
            decisions: shacs_session::SessionRuntimeExecutionDecisionCounts::default(),
            artifact_ref_count: 0,
            safe_artifact_ref_count: 0,
            recent_outcomes: Vec::new(),
        }),
        pending_approval: Some(shacs_tui::state::PendingApproval {
            lineage: ApprovalLineage::new(lineage)
                .unwrap_or_else(|error| panic!("fixture lineage failed: {error:?}")),
            tool_name: "exec".to_owned(),
            status: shacs_tui::state::ApprovalStatus::Pending,
            expires_at_unix_ms: Some(9_999),
            action: shacs_tui::state::ApprovalActionState::Actionable {
                target_owner_id: "owner-fixture".to_owned(),
            },
        }),
    }
}
