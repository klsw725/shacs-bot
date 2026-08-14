use super::*;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectoryOptions {
    config_path: Option<PathBuf>,
    workspace: PathBuf,
    store: PathBuf,
    trajectory_id: String,
    instruction: String,
}

#[derive(Serialize)]
struct TrajectoryReceipt<'a> {
    trajectory_id: &'a str,
    record_digest: &'a str,
    record_path: String,
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    if matches!(parser.peek(), Some("--help" | "-h")) {
        return Ok(CliCommand::Help);
    }
    match parser.next().as_deref() {
        Some("record") => {}
        Some(other) => {
            return Err(CliError::InvalidArguments(format!(
                "unknown trajectory action `{other}`"
            )))
        }
        None => {
            return Err(CliError::InvalidArguments(
                "trajectory requires record".to_owned(),
            ))
        }
    }
    let mut options = TrajectoryOptions {
        config_path: global_config,
        workspace: PathBuf::new(),
        store: PathBuf::new(),
        trajectory_id: String::new(),
        instruction: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => options.workspace = take_path(&mut parser, &arg)?,
            "--store" => options.store = take_path(&mut parser, &arg)?,
            "--trajectory-id" => options.trajectory_id = take_value(&mut parser, &arg)?,
            "--instruction" => options.instruction = take_value(&mut parser, &arg)?,
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown trajectory argument `{other}`"
                )))
            }
        }
    }
    if options.workspace.as_os_str().is_empty()
        || options.store.as_os_str().is_empty()
        || options.trajectory_id.is_empty()
        || options.instruction.is_empty()
    {
        return Err(CliError::InvalidArguments(
            "trajectory record requires --workspace, --store, --trajectory-id, and --instruction"
                .to_owned(),
        ));
    }
    Ok(CliCommand::Trajectory(options))
}

pub(super) fn run(options: TrajectoryOptions) -> Result<String, CliError> {
    let bundle = load_config(LoadOptions {
        config_path: options.config_path,
        workspace_override: Some(options.workspace),
        resolve_env: false,
        write_back_migrations: false,
    })?;
    fs::create_dir_all(&bundle.context.workspace)?;
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(bundle, false)?;
    let record = automation_worker::record_no_provider_trajectory(
        &adapter,
        &options.store,
        &options.trajectory_id,
        &options.instruction,
    )
    .map_err(CliError::Runtime)?;
    serde_json::to_string(&TrajectoryReceipt {
        trajectory_id: &record.trajectory_id,
        record_digest: &record.record_digest,
        record_path: options
            .store
            .join("trajectories")
            .join(&record.trajectory_id)
            .join("record.json")
            .display()
            .to_string(),
    })
    .map_err(|error| CliError::Runtime(error.to_string()))
}
