use crate::action_runner::TasksActionResult;
use crate::input::TuiInput;
use crate::state::{
    ApprovalActionState, ApprovalLineage, RuntimeSnapshot, SessionKey, TuiState, UiStatus,
};
use shacs_core::runtime::{Spec035TasksSemanticAction, SurfaceAction, SurfaceActionOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateEffect {
    None,
    RefreshRequested,
    RunAction(SurfaceAction),
    RunTaskAction(Spec035TasksSemanticAction),
    ExitRequested,
}

pub fn apply_input(state: &mut TuiState, input: TuiInput) -> UpdateEffect {
    match input {
        TuiInput::SelectPrevious => {
            state.selected = state.selected.saturating_sub(1);
            state.status = UiStatus::Ready;
            UpdateEffect::None
        }
        TuiInput::SelectNext => {
            if !state.sessions.is_empty() {
                state.selected = (state.selected + 1).min(state.sessions.len() - 1);
            }
            state.status = UiStatus::Ready;
            UpdateEffect::None
        }
        TuiInput::Approve => approval_action(state, true),
        TuiInput::Deny => approval_action(state, false),
        TuiInput::Stop => UpdateEffect::RunAction(SurfaceAction::Stop),
        TuiInput::Restart => UpdateEffect::RunAction(SurfaceAction::Restart),
        TuiInput::Recover => UpdateEffect::RunAction(SurfaceAction::Recover),
        TuiInput::TaskAction => task_action(state),
        TuiInput::Cancel => unavailable(
            state,
            "lineage cancel is unavailable; send /stop in the original session channel",
        ),
        TuiInput::Refresh => UpdateEffect::RefreshRequested,
        TuiInput::Resize { columns, rows } => {
            state.terminal_size = crate::state::TerminalSize { columns, rows };
            UpdateEffect::None
        }
        TuiInput::Exit => {
            state.status = UiStatus::Exiting;
            UpdateEffect::ExitRequested
        }
        TuiInput::Invalid => {
            state.status = UiStatus::InvalidAction("unknown key".to_owned());
            UpdateEffect::None
        }
    }
}

fn task_action(state: &mut TuiState) -> UpdateEffect {
    let Some(session) = state.selected_session() else {
        return unavailable(state, "no active session");
    };
    let Some(tasks) = session.tasks.as_ref() else {
        return unavailable(state, "tasks projection is unavailable");
    };
    let selected = tasks
        .rows()
        .iter()
        .filter_map(|row| row.action().map(|action| (row, action)))
        .find(|(_, action)| action.status == shacs_projection::Spec035TaskActionStatus::Available)
        .or_else(|| {
            tasks
                .rows()
                .iter()
                .find_map(|row| row.action().map(|action| (row, action)))
        });
    let Some((row, action)) = selected else {
        return unavailable(state, "tasks action is not advertised");
    };
    match Spec035TasksSemanticAction::parse(
        crate::tasks_view::owner_kind_label(row.owner().kind()),
        crate::tasks_view::action_kind_label(action.kind),
        row.owner().locator().as_str(),
    ) {
        Ok(action) => UpdateEffect::RunTaskAction(action),
        Err(error) => unavailable(state, &error.to_string()),
    }
}

pub fn apply_action_outcome(state: &mut TuiState, outcome: SurfaceActionOutcome) {
    state.status = UiStatus::ActionOutcome(outcome);
}

pub fn apply_task_action_result(state: &mut TuiState, result: TasksActionResult) {
    state.status = match result {
        TasksActionResult::Owner(outcome) => UiStatus::ActionOutcome(outcome),
        TasksActionResult::Rejected(rejection) => UiStatus::TransportRejected(rejection),
    };
}

pub fn apply_snapshot(state: &mut TuiState, snapshot: RuntimeSnapshot) {
    let preferred = state.selected_session().map(|session| session.key.clone());
    let mut next = TuiState::from_snapshot(snapshot, preferred.as_ref());
    next.terminal_size = state.terminal_size;
    next.trusted_runtime = state.trusted_runtime.clone();
    *state = next;
}

pub fn approval_by_lineage(
    state: &mut TuiState,
    session: &SessionKey,
    lineage: &ApprovalLineage,
    approve: bool,
) -> UpdateEffect {
    let Some(current) = state.selected_session() else {
        state.status = UiStatus::InvalidAction("no active session".to_owned());
        return UpdateEffect::None;
    };
    if &current.key != session {
        state.status = UiStatus::InvalidAction("approval session mismatch".to_owned());
        return UpdateEffect::None;
    }
    let Some(pending) = current.pending_approval.as_ref() else {
        state.status = UiStatus::InvalidAction("no pending approval".to_owned());
        return UpdateEffect::None;
    };
    if &pending.lineage != lineage {
        state.status = UiStatus::InvalidAction("stale approval lineage".to_owned());
        return UpdateEffect::None;
    }
    if let ApprovalActionState::Unavailable { reason } = &pending.action {
        state.status = UiStatus::ActionUnavailable(reason.clone());
        return UpdateEffect::None;
    }
    if approve {
        UpdateEffect::RunAction(SurfaceAction::Approve {
            session_key: session.as_str().to_owned(),
            lineage: lineage.as_str().to_owned(),
        })
    } else {
        UpdateEffect::RunAction(SurfaceAction::Deny {
            session_key: session.as_str().to_owned(),
            lineage: lineage.as_str().to_owned(),
        })
    }
}

fn approval_action(state: &mut TuiState, approve: bool) -> UpdateEffect {
    let Some(session) = state.selected_session() else {
        state.status = UiStatus::InvalidAction("no active session".to_owned());
        return UpdateEffect::None;
    };
    let Some(pending) = session.pending_approval.as_ref() else {
        state.status = UiStatus::InvalidAction("no pending approval".to_owned());
        return UpdateEffect::None;
    };
    approval_by_lineage(
        state,
        &session.key.clone(),
        &pending.lineage.clone(),
        approve,
    )
}

fn unavailable(state: &mut TuiState, reason: &str) -> UpdateEffect {
    state.status = UiStatus::ActionUnavailable(reason.to_owned());
    UpdateEffect::None
}
