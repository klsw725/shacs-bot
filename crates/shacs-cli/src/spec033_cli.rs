use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Inspect,
    Set(String),
    Pause,
    Resume,
    Clear,
    Done,
    Blocked(String),
}

pub(super) fn run(options: &GoalOptions) -> Result<String, CliError> {
    let snapshot = match &options.action {
        Action::Inspect => {
            shacs_core::runtime::build_spec033_snapshot(&options.workspace, &options.session_key)
        }
        action => shacs_core::runtime::apply_goal_surface_action(
            &options.workspace,
            &options.session_key,
            owner_action(action),
            &current_timestamp(),
        ),
    }
    .map_err(|error| CliError::Runtime(error.to_string()))?;
    serde_json::to_value(snapshot)
        .and_then(|value| serde_json::to_string(&value))
        .map_err(|error| CliError::Runtime(error.to_string()))
}

pub(super) fn parse(mut parser: ArgParser) -> Result<CliCommand, CliError> {
    if matches!(parser.peek(), Some("--help" | "-h")) {
        return Ok(CliCommand::Help);
    }
    let action = match parser.next().as_deref() {
        Some("inspect" | "status") => Action::Inspect,
        Some("set") => Action::Set(take_required(&mut parser, "goal set requires text")?),
        Some("pause") => Action::Pause,
        Some("resume") => Action::Resume,
        Some("clear") => Action::Clear,
        Some("done") => Action::Done,
        Some("blocked") => Action::Blocked(take_required(
            &mut parser,
            "goal blocked requires a reason",
        )?),
        Some(other) => {
            return Err(CliError::InvalidArguments(format!(
                "unknown goal action `{other}`"
            )))
        }
        None => {
            return Err(CliError::InvalidArguments(
                "goal requires an action".to_owned(),
            ))
        }
    };
    let mut workspace = None;
    let mut session_key = None;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--workspace" | "-w" => workspace = Some(take_path(&mut parser, &arg)?),
            "--session" => session_key = Some(take_value(&mut parser, &arg)?),
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown goal argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Goal(GoalOptions {
        workspace: workspace
            .ok_or_else(|| CliError::InvalidArguments("goal requires --workspace".to_owned()))?,
        session_key: session_key
            .ok_or_else(|| CliError::InvalidArguments("goal requires --session".to_owned()))?,
        action,
    }))
}

fn take_required(parser: &mut ArgParser, message: &str) -> Result<String, CliError> {
    parser
        .next()
        .ok_or_else(|| CliError::InvalidArguments(message.to_owned()))
}

fn owner_action(action: &Action) -> shacs_core::runtime::GoalSurfaceAction {
    match action {
        Action::Inspect => shacs_core::runtime::GoalSurfaceAction::Pause,
        Action::Set(text) => shacs_core::runtime::GoalSurfaceAction::Set {
            text: text.clone(),
            turn_budget: shacs_core::runtime::DEFAULT_GOAL_TURN_BUDGET,
        },
        Action::Pause => shacs_core::runtime::GoalSurfaceAction::Pause,
        Action::Resume => shacs_core::runtime::GoalSurfaceAction::Resume,
        Action::Clear => shacs_core::runtime::GoalSurfaceAction::Clear,
        Action::Done => shacs_core::runtime::GoalSurfaceAction::Done,
        Action::Blocked(reason) => shacs_core::runtime::GoalSurfaceAction::Blocked {
            reason: reason.clone(),
        },
    }
}

fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_default()
}
