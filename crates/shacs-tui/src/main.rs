use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use shacs_tui::{
    action_runner::run_surface_action,
    input::{key_to_input, TuiInput},
    live_source::{RuntimeProjectionSource, SessionRuntimeSource},
    state::{SessionKey, TuiState, UiStatus},
    update::{apply_action_outcome, apply_input, apply_snapshot, UpdateEffect},
    view::{draw_tui, render_lines},
};
use std::{io, path::PathBuf, time::Duration};

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
    let source = SessionRuntimeSource::with_config(options.config_path.clone(), &options.workspace);
    let preferred = options
        .session
        .as_ref()
        .map(|value| SessionKey::new(value.clone()))
        .transpose()
        .map_err(|error| format!("invalid session key: {error}"))?;
    let snapshot = source.load().map_err(|error| error.to_string())?;
    if let Some(preferred) = &preferred {
        if !snapshot
            .sessions
            .iter()
            .any(|session| session.key == *preferred)
        {
            return Err(format!("session `{preferred}` was not found"));
        }
    }
    let mut state = TuiState::from_snapshot(snapshot, preferred.as_ref());
    state.terminal_size.columns = 120;
    Ok(render_lines(&state).join("\n"))
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
    enable_raw_mode().map_err(|error| format!("terminal raw mode failed: {error}"))?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|error| format!("terminal alternate screen failed: {error}"))?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))
        .map_err(|error| format!("terminal could not start: {error}"))?;
    let result = event_loop(&mut terminal, &source, &options.workspace, &mut state);
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    result
}

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    source: &SessionRuntimeSource,
    workspace: &std::path::Path,
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
                Ok(snapshot) => apply_snapshot(state, snapshot),
                Err(error) => state.status = UiStatus::SourceError(error.to_string()),
            },
            UpdateEffect::RunAction(action) => {
                let outcome = run_surface_action(source.config_path(), workspace, action);
                apply_action_outcome(state, outcome);
                if let Ok(snapshot) = source.load() {
                    let status = state.status.clone();
                    apply_snapshot(state, snapshot);
                    state.status = status;
                }
            }
            UpdateEffect::ExitRequested => return Ok(()),
            UpdateEffect::None => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TuiOptions {
    config_path: Option<PathBuf>,
    workspace: PathBuf,
    session: Option<String>,
    once: bool,
    help: bool,
}

impl TuiOptions {
    fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let mut config_path = None;
        let mut workspace = std::env::current_dir()
            .map_err(|error| format!("current directory could not be read: {error}"))?;
        let mut session = None;
        let mut once = false;
        let mut help = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--workspace" | "-w" => workspace = PathBuf::from(take_value(&mut args, &arg)?),
                "--config" | "-c" => {
                    config_path = Some(PathBuf::from(take_value(&mut args, &arg)?))
                }
                "--session" | "-s" => session = Some(take_value(&mut args, &arg)?),
                "--once" => once = true,
                "--help" | "-h" => help = true,
                other => return Err(format!("unknown shacs-tui argument `{other}`")),
            }
        }
        Ok(Self {
            config_path,
            workspace,
            session,
            once,
            help,
        })
    }
}

fn take_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn help_text() -> String {
    [
        "shacs-tui",
        "",
        "Usage:",
        "  shacs-tui --workspace <path> [--session <key>]",
        "  shacs-tui --workspace <path> --once [--session <key>]",
        "",
        "Options:",
        "  -c, --config <path>     Config path whose parent is the runtime data dir",
        "  -w, --workspace <path>  Workspace containing local sessions",
        "  -s, --session <key>     Prefer a session key",
        "      --once              Render once and exit",
        "  -h, --help              Show help",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_workspace_without_session_for_interactive_tui() -> Result<(), String> {
        let options = TuiOptions::parse(["--workspace", "/tmp/ws"])?;
        assert_eq!(options.config_path, None);
        assert_eq!(options.workspace, PathBuf::from("/tmp/ws"));
        assert_eq!(options.session, None);
        Ok(())
    }

    #[test]
    fn parser_accepts_config_for_runtime_data_dir() -> Result<(), String> {
        let options = TuiOptions::parse([
            "--config",
            "/tmp/data/config.json",
            "--workspace",
            "/tmp/ws",
        ])?;
        assert_eq!(
            options.config_path,
            Some(PathBuf::from("/tmp/data/config.json"))
        );
        assert_eq!(options.workspace, PathBuf::from("/tmp/ws"));
        Ok(())
    }

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
