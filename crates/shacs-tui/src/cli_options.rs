use shacs_tui::action_runner::tui_transport_hello;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TuiOptions {
    pub(super) config_path: Option<PathBuf>,
    pub(super) workspace: PathBuf,
    pub(super) session: Option<String>,
    pub(super) transport_hello: shacs_projection::Spec035TransportClientHello,
    pub(super) once: bool,
    pub(super) help: bool,
}

impl TuiOptions {
    pub(super) fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let mut config_path = None;
        let mut workspace = std::env::current_dir()
            .map_err(|error| format!("current directory could not be read: {error}"))?;
        let mut session = None;
        let mut transport_hello = tui_transport_hello()
            .map_err(|error| format!("TUI transport hello failed: {error}"))?;
        let mut once = false;
        let mut help = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--workspace" | "-w" => workspace = PathBuf::from(take_value(&mut args, &arg)?),
                "--config" | "-c" => {
                    config_path = Some(PathBuf::from(take_value(&mut args, &arg)?))
                }
                "--session" | "-s" => session = Some(take_value(&mut args, &arg)?),
                "--transport-hello" => {
                    transport_hello = shacs_projection::Spec035TransportClientHello::parse_json(
                        &take_value(&mut args, &arg)?,
                    )
                    .map_err(|error| format!("invalid TUI transport hello: {error}"))?;
                }
                "--once" => once = true,
                "--help" | "-h" => help = true,
                other => return Err(format!("unknown shacs-tui argument `{other}`")),
            }
        }
        Ok(Self {
            config_path,
            workspace,
            session,
            transport_hello,
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

pub(super) fn help_text() -> String {
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
        "      --transport-hello <json>  Override the typed task mutation capability hello",
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
}
