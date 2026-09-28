use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use shacs_tui::{
    action_runner::{run_surface_action, run_tasks_action, TasksActionRequest},
    input::{key_to_input, TuiInput},
    live_source::{RuntimeProjectionSource, SessionRuntimeSource},
    state::{SessionKey, TuiState, UiStatus},
    update::{
        apply_action_outcome, apply_input, apply_snapshot, apply_task_action_result, UpdateEffect,
    },
    view::draw_tui,
};
use std::{io, time::Duration};

mod cli_options;
mod once;

use cli_options::{help_text, TuiOptions};

fn main() {
    if let Err(error) = run(std::env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let options = TuiOptions::parse(args)?;
    if options.help {
        println!("{}", help_text());
        return Ok(());
    }
    if options.once {
        println!("{}", render_once(&options)?);
        return Ok(());
    }
    run_interactive(&options)
}

fn render_once(options: &TuiOptions) -> Result<String, String> {
    once::render(
        options.config_path.clone(),
        &options.workspace,
        options.session.as_deref(),
    )
}

fn run_interactive(options: &TuiOptions) -> Result<(), String> {
    let source = SessionRuntimeSource::with_config(options.config_path.clone(), &options.workspace);
    let preferred = options
        .session
        .as_ref()
        .map(|value| SessionKey::new(value.clone()))
        .transpose()
        .map_err(|error| format!("invalid session key: {error:?}"))?;
    let mut state = TuiState::from_snapshot(
        source.load().map_err(|error| error.to_string())?,
        preferred.as_ref(),
    );
    state.set_trusted_runtime(source.trusted_runtime_projection());
    enable_raw_mode().map_err(|error| format!("terminal raw mode failed: {error}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|error| format!("terminal alternate screen failed: {error}"))?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))
        .map_err(|error| format!("terminal could not start: {error}"))?;
    let result = event_loop(
        &mut terminal,
        &source,
        &options.workspace,
        &options.transport_hello,
        &mut state,
    );
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    result
}

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    source: &SessionRuntimeSource,
    workspace: &std::path::Path,
    transport_hello: &shacs_projection::Spec035TransportClientHello,
    state: &mut TuiState,
) -> Result<(), String> {
    loop {
        terminal
            .draw(|frame| draw_tui(frame, state))
            .map_err(|error| format!("terminal draw failed: {error}"))?;
        if matches!(state.status, UiStatus::Exiting) {
            return Ok(());
        }
        if !event::poll(Duration::from_millis(250))
            .map_err(|error| format!("terminal event poll failed: {error}"))?
        {
            continue;
        }
        let input =
            match event::read().map_err(|error| format!("terminal event read failed: {error}"))? {
                Event::Key(key) => key_to_input(key),
                Event::Resize(columns, rows) => TuiInput::Resize { columns, rows },
                Event::Mouse(_) | Event::Paste(_) | Event::FocusGained | Event::FocusLost => {
                    TuiInput::Invalid
                }
            };
        match apply_input(state, input) {
            UpdateEffect::RefreshRequested => match source.load() {
                Ok(snapshot) => {
                    apply_snapshot(state, snapshot);
                    state.set_trusted_runtime(source.trusted_runtime_projection());
                }
                Err(error) => state.status = UiStatus::SourceError(error.to_string()),
            },
            UpdateEffect::RunAction(action) => {
                let outcome = run_surface_action(source.config_path(), workspace, action);
                apply_action_outcome(state, outcome);
                if let Ok(snapshot) = source.load() {
                    let status = state.status.clone();
                    apply_snapshot(state, snapshot);
                    state.set_trusted_runtime(source.trusted_runtime_projection());
                    state.status = status;
                }
            }
            UpdateEffect::RunTaskAction(action) => {
                let session_id = state
                    .selected_session()
                    .map(|session| session.key.as_str().to_owned())
                    .ok_or_else(|| "tasks action requires a selected session".to_owned())?;
                let outcome = run_tasks_action(TasksActionRequest {
                    config_path: source.config_path(),
                    workspace,
                    session_id: &session_id,
                    action,
                    transport_hello: Some(transport_hello),
                });
                apply_task_action_result(state, outcome);
                if let Ok(snapshot) = source.load() {
                    let status = state.status.clone();
                    apply_snapshot(state, snapshot);
                    state.set_trusted_runtime(source.trusted_runtime_projection());
                    state.status = status;
                }
            }
            UpdateEffect::ExitRequested => return Ok(()),
            UpdateEffect::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_renders_preferred_session_without_workflow_projection() -> Result<(), String> {
        let workspace = tempfile::tempdir().map_err(|error| error.to_string())?;
        let mut manager = shacs_session::SessionManager::new(workspace.path())
            .map_err(|error| error.to_string())?;
        let session = shacs_session::Session::new("cli:direct");
        manager.save(&session).map_err(|error| error.to_string())?;
        let workspace_arg = workspace.path().display().to_string();
        let options = TuiOptions::parse([
            "--workspace",
            workspace_arg.as_str(),
            "--session",
            "cli:direct",
            "--once",
        ])?;

        let rendered = render_once(&options)?;

        assert!(rendered.contains("active session: cli:direct"));
        assert!(rendered.contains("workflow: none"));
        Ok(())
    }
}
