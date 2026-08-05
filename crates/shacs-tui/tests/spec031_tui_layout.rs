use ratatui::{
    backend::TestBackend,
    prelude::*,
    widgets::{Block, Borders},
    Terminal,
};
use shacs_tui::{
    state::{ApprovalLineage, RuntimeSnapshot, SessionKey, TuiState},
    view::{draw_tui, render_lines_for_width},
};
use std::error::Error;

#[test]
fn cjk_rendering_fits_actual_right_pane_width_when_terminal_is_small() -> Result<(), Box<dyn Error>>
{
    let state = cjk_state(60, 16)?;
    let right_inner_width = right_pane_inner(60, 16).width;

    let rendered = render_lines_for_width(&state, right_inner_width);

    assert!(rendered.iter().all(|line| {
        unicode_width::UnicodeWidthStr::width(line.as_str()) <= usize::from(right_inner_width)
    }));
    Ok(())
}

#[test]
fn cjk_rendering_keeps_right_border_intact_at_small_sizes() -> Result<(), Box<dyn Error>> {
    for (columns, rows) in [(60, 16), (48, 12)] {
        let state = cjk_state(columns, rows)?;
        let backend = TestBackend::new(columns, rows);
        let mut terminal = Terminal::new(backend)?;

        terminal.draw(|frame| draw_tui(frame, &state))?;

        let buffer = terminal.backend().buffer();
        for y in 1..rows.saturating_sub(1) {
            assert_eq!(buffer.get(columns - 1, y).symbol(), "│");
        }
    }
    Ok(())
}

fn right_pane_inner(columns: u16, rows: u16) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(Rect::new(0, 0, columns, rows));
    Block::default().borders(Borders::ALL).inner(chunks[1])
}

fn cjk_state(columns: u16, rows: u16) -> Result<TuiState, Box<dyn Error>> {
    let mut state = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![cjk_session()?],
        },
        None,
    );
    state.terminal_size.columns = columns;
    state.terminal_size.rows = rows;
    Ok(state)
}

fn cjk_session() -> Result<shacs_tui::state::RuntimeSession, Box<dyn Error>> {
    Ok(shacs_tui::state::RuntimeSession {
        key: SessionKey::new("cli:cjk")?,
        updated_at: Some("2026-08-02T00:00:00Z".to_owned()),
        message_count: 2,
        recovery_markers: vec!["복구요청".to_owned(), "런타임진행".to_owned()],
        checkpoint_phase: None,
        diagnostics_ref_count: 0,
        workflow: Some(shacs_session::SessionRuntimeWorkflowProjection {
            schema_label: Some("024WorkflowProjection".to_owned()),
            schema_version: Some("024WorkflowProjection.v1".to_owned()),
            workflow_id: Some("wf-1".to_owned()),
            pattern: Some("workflow_sequence".to_owned()),
            state: Some("running".to_owned()),
            progress_count: Some(2),
            active_child_count: Some(1),
            pending_barrier_count: Some(0),
            verifier_status: Some("pending".to_owned()),
            budget_usage: None,
            worktree_ref_count: 0,
            evidence_ref_count: 0,
            blocked_reason: Some("복구요청 대기".to_owned()),
            next_action: Some("recover_after_audit".to_owned()),
            resume_available: false,
        }),
        execution: None,
        pending_approval: Some(shacs_tui::state::PendingApproval {
            lineage: ApprovalLineage::new("approval-cjk")?,
            tool_name: "exec".to_owned(),
            status: shacs_tui::state::ApprovalStatus::Pending,
            expires_at_unix_ms: Some(9_999),
            action: shacs_tui::state::ApprovalActionState::Actionable {
                target_owner_id: "owner-fixture".to_owned(),
            },
        }),
    })
}
