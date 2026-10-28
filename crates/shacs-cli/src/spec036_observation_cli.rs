use super::spec036_work_cli::{WorkAction, WorkOptions};
use super::{now_millis, take_path, take_value, ArgParser, CliCommand, CliError};
use shacs_core::runtime::{
    ConfigureObservationRequest, EntrustedObservationPlan, EntrustedWorkReadRequest,
    ObservationComparisonPolicy, ObservationCriterion, ObservationSource,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObservationAction {
    Configure {
        work_id: String,
        expected_revision: u64,
        source: String,
        cadence_seconds: u64,
        comparison: Option<ObservationComparisonPolicy>,
    },
    Read {
        work_id: String,
        data_dir: PathBuf,
    },
}

pub(super) fn run(options: WorkOptions, action: ObservationAction) -> Result<String, CliError> {
    let request = |work_id| EntrustedWorkReadRequest {
        owner_session_id: options.owner_session_id.clone(),
        requester_target: options.requester_session_id.clone(),
        work_id,
    };
    match action {
        ObservationAction::Configure {
            work_id,
            expected_revision,
            source,
            cadence_seconds,
            comparison,
        } => {
            let now = now_millis();
            let first_wake_at_ms = cadence_seconds
                .checked_mul(1000)
                .and_then(|delay| now.checked_add(delay))
                .ok_or_else(|| {
                    CliError::InvalidArguments("invalid observation cadence".to_owned())
                })?;
            let snapshot = shacs_core::runtime::configure_entrusted_observation(
                &options.workspace,
                ConfigureObservationRequest {
                    work: request(work_id),
                    expected_revision,
                    plan: EntrustedObservationPlan {
                        source: ObservationSource::LocalFile {
                            relative_path: source,
                        },
                        cadence_seconds,
                        first_wake_at_ms,
                        comparison,
                    },
                    changed_at: now.to_string(),
                },
            )
            .map_err(|error| CliError::Runtime(error.to_string()))?;
            serde_json::to_string(&snapshot).map_err(|error| CliError::Runtime(error.to_string()))
        }
        ObservationAction::Read { work_id, data_dir } => {
            let snapshot = shacs_core::runtime::read_entrusted_observation(
                &options.workspace,
                &data_dir,
                &request(work_id),
            )
            .map_err(|error| CliError::Runtime(error.to_string()))?;
            serde_json::to_string(&snapshot).map_err(|error| CliError::Runtime(error.to_string()))
        }
    }
}

pub(super) fn parse(mut parser: ArgParser) -> Result<CliCommand, CliError> {
    let action_name = required(&mut parser, "action")?;
    let work_id = required(&mut parser, "work id")?;
    let mut workspace = None;
    let mut session = None;
    let mut owner = None;
    let mut data_dir = None;
    let mut source = None;
    let mut cadence = None;
    let mut revision = None;
    let mut at_most = None;
    let mut max_age = None;
    let mut regular_summary = None;
    while let Some(argument) = parser.next() {
        match argument.as_str() {
            "--workspace" | "-w" => workspace = Some(take_path(&mut parser, &argument)?),
            "--session" => session = Some(take_value(&mut parser, &argument)?),
            "--owner-session" => owner = Some(take_value(&mut parser, &argument)?),
            "--data-dir" => data_dir = Some(take_path(&mut parser, &argument)?),
            "--source" => source = Some(take_value(&mut parser, &argument)?),
            "--cadence-seconds" => cadence = Some(number(&mut parser, &argument)?),
            "--expected-revision" => revision = Some(number(&mut parser, &argument)?),
            "--at-most" => {
                at_most = Some(take_value(&mut parser, &argument)?.parse::<i64>().map_err(
                    |_| CliError::InvalidArguments("invalid --at-most integer".to_owned()),
                )?)
            }
            "--max-age-seconds" => max_age = Some(number(&mut parser, &argument)?),
            "--regular-summary" => {
                regular_summary = Some(
                    take_value(&mut parser, &argument)?
                        .parse::<bool>()
                        .map_err(|_| {
                            CliError::InvalidArguments(
                                "--regular-summary requires true or false".to_owned(),
                            )
                        })?,
                )
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            _ => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown observation argument `{argument}`"
                )))
            }
        }
    }
    let workspace = workspace.ok_or_else(|| missing("--workspace"))?;
    let session = session.ok_or_else(|| missing("--session"))?;
    let comparison = match (at_most, max_age, regular_summary) {
        (None, None, None) => None,
        (Some(value), Some(max_age_seconds), regular_summary) => {
            Some(ObservationComparisonPolicy {
                criterion: ObservationCriterion::IntegerAtMost { value },
                max_age_seconds,
                regular_summary: regular_summary.unwrap_or(false),
            })
        }
        _ => {
            return Err(missing(
                "both --at-most and --max-age-seconds for comparison",
            ))
        }
    };
    if action_name == "observations" && comparison.is_some() {
        return Err(CliError::InvalidArguments(
            "comparison flags require work observe".to_owned(),
        ));
    }
    let action = match action_name.as_str() {
        "observe" => ObservationAction::Configure {
            work_id,
            expected_revision: revision.ok_or_else(|| missing("--expected-revision"))?,
            source: source.ok_or_else(|| missing("--source"))?,
            cadence_seconds: cadence.ok_or_else(|| missing("--cadence-seconds"))?,
            comparison,
        },
        "observations" => ObservationAction::Read {
            work_id,
            data_dir: data_dir.ok_or_else(|| missing("--data-dir"))?,
        },
        _ => return Err(missing("observe or observations action")),
    };
    Ok(CliCommand::Work(WorkOptions {
        workspace,
        owner_session_id: owner.unwrap_or_else(|| session.clone()),
        requester_session_id: session,
        action: WorkAction::Observation(action),
    }))
}

fn missing(field: &str) -> CliError {
    CliError::InvalidArguments(format!("work observation requires {field}"))
}
fn required(parser: &mut ArgParser, field: &str) -> Result<String, CliError> {
    parser.next().ok_or_else(|| missing(field))
}
fn number(parser: &mut ArgParser, flag: &str) -> Result<u64, CliError> {
    take_value(parser, flag)?
        .parse()
        .map_err(|_| CliError::InvalidArguments(format!("invalid {flag}")))
}
