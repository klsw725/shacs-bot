use shacs_tui::{
    live_source::{RuntimeProjectionSource, SessionRuntimeSource},
    state::{SessionKey, TuiState},
    view::render_lines,
};
use std::path::{Path, PathBuf};

pub(super) fn render(
    config_path: Option<PathBuf>,
    workspace: &Path,
    session: Option<&str>,
) -> Result<String, String> {
    let source = SessionRuntimeSource::with_config(config_path.clone(), workspace);
    let preferred = session
        .map(|value| SessionKey::new(value.to_owned()))
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
    state.set_trusted_runtime(source.trusted_runtime_projection());
    state.terminal_size.columns = 120;
    Ok(render_lines(&state).join("\n"))
}
