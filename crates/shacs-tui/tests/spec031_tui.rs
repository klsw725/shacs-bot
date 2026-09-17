#[path = "support/tui_session.rs"]
mod tui_session;

use shacs_core::runtime::SurfaceAction;
use shacs_projection::{Spec030RuntimeProjection, Spec030UnavailableReason};
use shacs_tui::{
    input::TuiInput,
    state::{ApprovalLineage, RuntimeSnapshot, SessionKey, TuiState},
    update::{apply_input, apply_snapshot, approval_by_lineage, UpdateEffect},
    view::render_lines,
};
use std::error::Error;
use tui_session::fixture_session;

#[test]
fn task_action_snapshot_refresh_does_not_reset_owner_projection() {
    // Given: the TUI has an owner-derived runtime projection before a source snapshot refresh.
    let mut state = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: vec![fixture_session("cli:one", "approval-live", 1, 0)],
        },
        None,
    );
    let owner_projection =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerUnavailable);
    state.set_trusted_runtime(owner_projection.clone());

    // When: the task action path applies its refreshed session snapshot.
    apply_snapshot(
        &mut state,
        RuntimeSnapshot {
            sessions: vec![fixture_session("cli:one", "approval-live", 2, 0)],
        },
    );

    // Then: the owner projection remains intact until the source replaces it.
    assert_eq!(state.trusted_runtime, owner_projection);
}

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
        .contains("[a] approve [d] deny"));

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
fn tui_runtime_view_contains_revised_projection_from_trusted_owner_facts() {
    // Given: the TUI state has the actual trusted-runtime owner projection.
    let mut state = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: Vec::new(),
        },
        None,
    );
    state.set_trusted_runtime(Spec030RuntimeProjection::unavailable(
        Spec030UnavailableReason::OwnerFactsMissing,
    ));
    state.terminal_size.columns = 1_000;

    // When: the existing TUI view is rendered.
    let rendered = render_lines(&state).join("\n");

    // Then: the revised projection is present without a new panel or action.
    assert!(rendered.contains("Spec035 revised projection:"));
    assert!(rendered.contains("\"freshness\":\"unavailable\""));
}

#[test]
fn tui_runtime_view_projects_selected_session_durable_approval() {
    // Given: the selected session owns a pending durable approval.
    let snapshot = RuntimeSnapshot {
        sessions: vec![fixture_session("cli:one", "approval-live", 1, 0)],
    };
    let mut state = TuiState::from_snapshot(snapshot, None);
    state.terminal_size.columns = 1_000;

    // When: the existing TUI view renders the selected session.
    let rendered = render_lines(&state).join("\n");

    // Then: the shared projection preserves its durable lineage and pending state.
    assert!(rendered.contains("\"kind\":\"durable_approval\""));
    assert!(rendered.contains("\"approval_ref\":\"approval-live\""));
    assert!(rendered.contains("\"state\":\"pending\""));
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
        .contains("[a] approve [d] deny"));
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
        .contains("approval unavailable: no active runtime owner found"));

    let empty = TuiState::from_snapshot(
        RuntimeSnapshot {
            sessions: Vec::new(),
        },
        None,
    );
    assert!(render_lines(&empty)
        .join("\n")
        .contains("approval unavailable: no pending approval"));
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
