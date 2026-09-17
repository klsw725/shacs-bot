use super::*;
use shacs_core::runtime::{
    accept_spec035_surface_action_outcome, build_spec035_tasks_projection,
    serialize_spec035_tasks_projection, validate_spec035_task_action, Spec035TasksSemanticAction,
};
use shacs_projection::{
    negotiate_spec035_task_mutation, Spec035TransportClientHello, Spec035TransportMutationRejection,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TasksOptions {
    pub config_path: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub session_id: String,
    pub action: Option<Spec035TasksSemanticAction>,
    pub transport_hello: Option<Spec035TransportClientHello>,
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut workspace = None;
    let mut data_dir = None;
    let mut session_id = None;
    let mut owner = None;
    let mut action = None;
    let mut locator = None;
    let mut transport_hello = None;
    let mut json = false;
    while let Some(argument) = parser.next() {
        match argument.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &argument)?),
            "--workspace" | "-w" => workspace = Some(take_path(&mut parser, &argument)?),
            "--data-dir" => data_dir = Some(take_path(&mut parser, &argument)?),
            "--session" => session_id = Some(take_value(&mut parser, &argument)?),
            "--owner" => owner = Some(take_value(&mut parser, &argument)?),
            "--action" => action = Some(take_value(&mut parser, &argument)?),
            "--locator" => locator = Some(take_value(&mut parser, &argument)?),
            "--transport-hello" => {
                let value = take_value(&mut parser, &argument)?;
                transport_hello = Some(
                    Spec035TransportClientHello::parse_json(&value)
                        .map_err(|error| CliError::InvalidArguments(error.to_string()))?,
                );
            }
            "--json" => json = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown tasks argument `{other}`"
                )))
            }
        }
    }
    if !json {
        return Err(CliError::InvalidArguments(
            "tasks requires --json".to_owned(),
        ));
    }
    let session_id = match session_id {
        Some(value) if value.trim().is_empty() => {
            return Err(CliError::InvalidArguments(
                "tasks requires a non-empty --session".to_owned(),
            ))
        }
        Some(value) => value,
        None => "cli:direct".to_owned(),
    };
    let action = match (owner, action, locator) {
        (None, None, None) => None,
        (Some(owner), Some(action), Some(locator)) => Some(
            Spec035TasksSemanticAction::parse(&owner, &action, &locator)
                .map_err(|error| CliError::InvalidArguments(error.to_string()))?,
        ),
        _ => {
            return Err(CliError::InvalidArguments(
                "tasks action requires --owner, --action, and --locator".to_owned(),
            ))
        }
    };
    Ok(CliCommand::Tasks(TasksOptions {
        config_path,
        workspace,
        data_dir,
        session_id,
        action,
        transport_hello,
    }))
}

pub(super) fn run(options: TasksOptions) -> Result<String, CliError> {
    if let Some(action) = options.action.as_ref() {
        let hello = options.transport_hello.as_ref().ok_or_else(|| {
            CliError::TransportRejected(Spec035TransportMutationRejection::capability_unavailable())
        })?;
        negotiate_spec035_task_mutation(
            hello,
            action.transport_capability(),
            "generation:cli-local",
        )
        .map_err(CliError::TransportRejected)?;
    }
    let (workspace, data_dir) = roots(&options)?;
    if let Some(action) = options.action {
        let current = build_spec035_tasks_projection(&workspace, &data_dir, &options.session_id)
            .map_err(|error| CliError::Runtime(error.to_string()))?;
        validate_spec035_task_action(&current, &action)
            .map_err(|error| CliError::InvalidArguments(error.to_string()))?;
        dispatch(&workspace, &data_dir, &options.session_id, action)?;
    }
    let projection = build_spec035_tasks_projection(&workspace, &data_dir, &options.session_id)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    serialize_spec035_tasks_projection(&projection)
        .map_err(|error| CliError::Runtime(error.to_string()))
}

fn roots(options: &TasksOptions) -> Result<(PathBuf, PathBuf), CliError> {
    match (&options.workspace, &options.data_dir) {
        (Some(workspace), Some(data_dir)) => Ok((workspace.clone(), data_dir.clone())),
        (Some(workspace), None) => Ok((workspace.clone(), workspace.clone())),
        (None, Some(_)) => Err(CliError::InvalidArguments(
            "tasks --data-dir requires --workspace".to_owned(),
        )),
        (None, None) => load_session_context(options.config_path.clone(), None),
    }
}

fn dispatch(
    workspace: &Path,
    data_dir: &Path,
    session_id: &str,
    action: Spec035TasksSemanticAction,
) -> Result<(), CliError> {
    match action {
        Spec035TasksSemanticAction::GoalPause { .. } => {
            shacs_core::runtime::apply_goal_surface_action(
                workspace,
                session_id,
                shacs_core::runtime::GoalSurfaceAction::Pause,
                &now_millis().to_string(),
            )
            .map(|_| ())
            .map_err(|error| CliError::Runtime(error.to_string()))
        }
        Spec035TasksSemanticAction::GoalResume { .. } => {
            shacs_core::runtime::apply_goal_surface_action(
                workspace,
                session_id,
                shacs_core::runtime::GoalSurfaceAction::Resume,
                &now_millis().to_string(),
            )
            .map(|_| ())
            .map_err(|error| CliError::Runtime(error.to_string()))
        }
        Spec035TasksSemanticAction::AppStop { app_id } => {
            AppSupervisorJournal::new(data_dir.join("runtime/apps"))
                .request(&app_id, AppLifecycleAction::Stop)
                .map(|_| ())
                .map_err(Into::into)
        }
        Spec035TasksSemanticAction::AppRecover { app_id } => {
            let journal = AppSupervisorJournal::new(data_dir.join("runtime/apps"));
            AppSupervisor::new(&journal)
                .recover(&app_id, false)
                .map(|_| ())
                .map_err(|error| CliError::Runtime(error.to_string()))
        }
        Spec035TasksSemanticAction::RuntimeRecover => {
            shacs_core::runtime::recover_runtime_surface(data_dir, now_millis())
                .map_err(|error| CliError::Runtime(error.to_string()))
                .and_then(|outcome| {
                    accept_spec035_surface_action_outcome(outcome)
                        .map_err(|error| CliError::Runtime(error.to_string()))
                })
        }
    }
}
