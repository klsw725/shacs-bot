use shacs_session::SessionManager;
use shacs_tui::session_workflow_progress_view;
use std::path::PathBuf;

fn main() {
    match run(std::env::args().skip(1)) {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn run<I, S>(args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let options = TuiOptions::parse(args)?;
    if options.help {
        return Ok(help_text());
    }
    let manager = SessionManager::open_existing(&options.workspace)
        .map_err(|error| format!("session store could not be opened: {error}"))?
        .ok_or_else(|| "session store was not found".to_owned())?;
    let detail = manager
        .session_ux_detail(&options.session)
        .ok_or_else(|| format!("session `{}` was not found", options.session))?;
    let projection = detail.runtime_workflow.ok_or_else(|| {
        format!(
            "session `{}` has no runtime workflow projection",
            options.session
        )
    })?;
    Ok(session_workflow_progress_view(&projection).render_plain_text())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TuiOptions {
    workspace: PathBuf,
    session: String,
    help: bool,
}

impl TuiOptions {
    fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let mut workspace = std::env::current_dir()
            .map_err(|error| format!("current directory could not be read: {error}"))?;
        let mut session = None;
        let mut help = false;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--workspace" | "-w" => {
                    workspace = PathBuf::from(take_value(&mut args, &arg)?);
                }
                "--session" | "-s" => {
                    session = Some(take_value(&mut args, &arg)?);
                }
                "--help" | "-h" => help = true,
                other => return Err(format!("unknown shacs-tui argument `{other}`")),
            }
        }
        if help {
            return Ok(Self {
                workspace,
                session: session.unwrap_or_else(|| "cli:direct".to_owned()),
                help,
            });
        }
        let session = session.ok_or_else(|| "shacs-tui requires --session <key>".to_owned())?;
        Ok(Self {
            workspace,
            session,
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
        "  shacs-tui --workspace <path> --session <key>",
        "",
        "Options:",
        "  -w, --workspace <path>  Workspace containing local sessions",
        "  -s, --session <key>     Session key to render",
        "  -h, --help              Show help",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_requires_session_for_runtime_entrypoint() {
        let error = TuiOptions::parse(["--workspace", "/tmp/ws"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--session"));
    }

    #[test]
    fn parser_accepts_workspace_and_session() -> Result<(), String> {
        let options = TuiOptions::parse(["--workspace", "/tmp/ws", "--session", "cli:direct"])?;
        assert_eq!(options.workspace, PathBuf::from("/tmp/ws"));
        assert_eq!(options.session, "cli:direct");
        Ok(())
    }
}
