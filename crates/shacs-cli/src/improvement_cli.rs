use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImprovementAction {
    Propose {
        candidate: PathBuf,
        snapshot: PathBuf,
        expected_digest: String,
        confirmation_required: bool,
    },
    Inspect,
    Apply,
    Verify,
    Candidate,
    Rollback,
}

pub(super) fn parse(
    mut parser: ArgParser,
    config_path: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    if matches!(parser.peek(), Some("--help" | "-h")) {
        return Ok(CliCommand::Help);
    }
    let action_name = parser
        .next()
        .ok_or_else(|| CliError::InvalidArguments("improve requires an action".to_owned()))?;
    let mut root = None;
    let mut proposal_id = None;
    let mut target_ref = None;
    let mut candidate = None;
    let mut snapshot = None;
    let mut expected_digest = None;
    let mut confirmation_required = false;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--root" => root = Some(take_path(&mut parser, &arg)?),
            "--proposal" => proposal_id = Some(take_value(&mut parser, &arg)?),
            "--target" => target_ref = Some(take_value(&mut parser, &arg)?),
            "--candidate" => candidate = Some(take_path(&mut parser, &arg)?),
            "--snapshot" => snapshot = Some(take_path(&mut parser, &arg)?),
            "--expected-digest" => expected_digest = Some(take_value(&mut parser, &arg)?),
            "--require-confirmation" => confirmation_required = true,
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown improve argument `{other}`"
                )))
            }
        }
    }
    let missing = |name: &str| CliError::InvalidArguments(format!("improve requires {name}"));
    let action = match action_name.as_str() {
        "propose" => ImprovementAction::Propose {
            candidate: candidate.ok_or_else(|| missing("--candidate"))?,
            snapshot: snapshot.ok_or_else(|| missing("--snapshot"))?,
            expected_digest: expected_digest.ok_or_else(|| missing("--expected-digest"))?,
            confirmation_required,
        },
        "inspect" => ImprovementAction::Inspect,
        "apply" => ImprovementAction::Apply,
        "verify" => ImprovementAction::Verify,
        "candidate" => ImprovementAction::Candidate,
        "rollback" => ImprovementAction::Rollback,
        other => {
            return Err(CliError::InvalidArguments(format!(
                "unknown improve action `{other}`"
            )))
        }
    };
    Ok(CliCommand::Improve(ImprovementOptions {
        config_path,
        root: root.ok_or_else(|| missing("--root"))?,
        proposal_id: proposal_id.ok_or_else(|| missing("--proposal"))?,
        target_ref,
        action,
    }))
}

pub(super) fn run(options: ImprovementOptions) -> Result<String, CliError> {
    let service = production_improvement_service(options.config_path, &options.root, "cli")?;
    let value = match options.action {
        ImprovementAction::Propose {
            candidate,
            snapshot,
            expected_digest,
            confirmation_required,
        } => serde_json::to_value(
            service
                .propose(
                    &options.proposal_id,
                    options.target_ref.as_deref().ok_or_else(|| {
                        CliError::InvalidArguments("improve propose requires --target".to_owned())
                    })?,
                    &expected_digest,
                    &fs::read_to_string(candidate)?,
                    &fs::read_to_string(snapshot)?,
                    confirmation_required,
                )
                .map_err(runtime_error)?,
        ),
        ImprovementAction::Inspect => serde_json::to_value(
            service
                .inspect(&options.proposal_id)
                .map_err(runtime_error)?,
        ),
        ImprovementAction::Apply => {
            serde_json::to_value(service.apply(&options.proposal_id).map_err(runtime_error)?)
        }
        ImprovementAction::Verify => serde_json::to_value(
            service
                .verify(&options.proposal_id)
                .map_err(runtime_error)?,
        ),
        ImprovementAction::Candidate => {
            serde_json::to_value(service.rollback_candidate(&options.proposal_id))
        }
        ImprovementAction::Rollback => serde_json::to_value(
            service
                .rollback(&options.proposal_id)
                .map_err(runtime_error)?,
        ),
    }
    .map_err(|error| CliError::Runtime(error.to_string()))?;
    serde_json::to_string(&value).map_err(|error| CliError::Runtime(error.to_string()))
}

fn runtime_error(error: shacs_core::runtime::LocalImprovementBlock) -> CliError {
    CliError::Runtime(error.to_string())
}
