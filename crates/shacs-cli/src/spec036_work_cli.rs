use super::{now_millis, take_path, take_value, ArgParser, CliCommand, CliError};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkOptions {
    pub workspace: PathBuf,
    pub requester_session_id: String,
    pub owner_session_id: String,
    pub action: WorkAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkAction {
    Observation(super::spec036_observation_cli::ObservationAction),
    WebWatch(super::spec042_web_watch_cli::WebWatchAction),
    Create(CreateOptions),
    Read {
        work_id: String,
    },
    Revise(ReviseOptions),
    Transition {
        work_id: String,
        action: shacs_core::runtime::EntrustedWorkLifecycleAction,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOptions {
    goal_text: String,
    turn_budget: u32,
    consent_kind: shacs_core::runtime::EntrustedWorkConsentKind,
    evidence_ref: String,
    conditions: shacs_core::runtime::EntrustedWorkConditions,
    linked_targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviseOptions {
    work_id: String,
    expected_revision: u64,
    conditions: shacs_core::runtime::EntrustedWorkConditions,
    linked_targets: Option<Vec<String>>,
}

pub(super) fn run(options: WorkOptions) -> Result<String, CliError> {
    let observed_at = current_timestamp();
    let snapshot = match options.action.clone() {
        WorkAction::Observation(action) => {
            return super::spec036_observation_cli::run(options, action)
        }
        WorkAction::WebWatch(action) => return super::spec042_web_watch_cli::run(options, action),
        WorkAction::Create(create) => shacs_core::runtime::create_entrusted_work(
            &options.workspace,
            shacs_core::runtime::EntrustedWorkCreateRequest {
                owner_session_id: options.requester_session_id,
                goal_text: create.goal_text,
                turn_budget: create.turn_budget,
                consent: shacs_core::runtime::EntrustedWorkConsentProof {
                    kind: create.consent_kind,
                    evidence_ref: create.evidence_ref,
                    recorded_at: observed_at.clone(),
                },
                conditions: create.conditions,
                linked_targets: create.linked_targets,
                observed_at,
            },
        ),
        WorkAction::Read { work_id } => shacs_core::runtime::read_entrusted_work(
            &options.workspace,
            &shacs_core::runtime::EntrustedWorkReadRequest {
                owner_session_id: options.owner_session_id,
                requester_target: options.requester_session_id,
                work_id,
            },
        ),
        WorkAction::Revise(revise) => shacs_core::runtime::revise_entrusted_work(
            &options.workspace,
            shacs_core::runtime::EntrustedWorkReviseRequest {
                owner_session_id: options.owner_session_id,
                requester_target: options.requester_session_id,
                work_id: revise.work_id,
                expected_revision: revise.expected_revision,
                conditions: revise.conditions,
                linked_targets: revise.linked_targets,
                changed_at: observed_at,
            },
        ),
        WorkAction::Transition { work_id, action } => {
            shacs_core::runtime::transition_entrusted_work(
                &options.workspace,
                shacs_core::runtime::EntrustedWorkLifecycleRequest {
                    owner_session_id: options.owner_session_id,
                    requester_target: options.requester_session_id,
                    work_id,
                    action,
                    observed_at,
                },
            )
        }
    }
    .map_err(|error| CliError::Runtime(error.to_string()))?;
    serde_json::to_string(&snapshot).map_err(|error| CliError::Runtime(error.to_string()))
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    if matches!(parser.peek(), Some("observe" | "observations")) {
        return super::spec036_observation_cli::parse(parser);
    }
    if matches!(
        parser.peek(),
        Some("web-preview" | "web-watch" | "web-status" | "web-notify" | "web-deadline")
    ) {
        return super::spec042_web_watch_cli::parse(parser, global_config);
    }
    if matches!(parser.peek(), Some("--help" | "-h")) {
        return Ok(CliCommand::Help);
    }
    let action_name = parser
        .next()
        .ok_or_else(|| CliError::InvalidArguments("work requires an action".to_owned()))?;
    let primary = match action_name.as_str() {
        "create" => take_required(&mut parser, "work create requires goal text")?,
        "read" | "revise" | "pause" | "resume" | "done" | "clear" => {
            take_required(&mut parser, "work action requires a work id")?
        }
        other => {
            return Err(CliError::InvalidArguments(format!(
                "unknown work action `{other}`"
            )))
        }
    };
    let mut workspace = None;
    let mut requester_session_id = None;
    let mut owner_session_id = None;
    let mut turn_budget = shacs_core::runtime::DEFAULT_GOAL_TURN_BUDGET;
    let mut consent_kind = None;
    let mut evidence_ref = None;
    let mut instructions = None;
    let mut budget = None;
    let mut prior_decisions = Vec::new();
    let mut remaining_questions = Vec::new();
    let mut linked_targets = Vec::new();
    let mut clear_linked_targets = false;
    let mut expected_revision = None;
    while let Some(argument) = parser.next() {
        match argument.as_str() {
            "--workspace" | "-w" => workspace = Some(take_path(&mut parser, &argument)?),
            "--session" => requester_session_id = Some(take_value(&mut parser, &argument)?),
            "--owner-session" => owner_session_id = Some(take_value(&mut parser, &argument)?),
            "--turn-budget" => {
                turn_budget = parse_u32(take_value(&mut parser, &argument)?, &argument)?
            }
            "--consent" => {
                consent_kind = Some(parse_consent(&take_value(&mut parser, &argument)?)?)
            }
            "--evidence-ref" => evidence_ref = Some(take_value(&mut parser, &argument)?),
            "--instructions" => instructions = Some(take_value(&mut parser, &argument)?),
            "--budget" => budget = Some(take_value(&mut parser, &argument)?),
            "--prior-decision" => prior_decisions.push(take_value(&mut parser, &argument)?),
            "--remaining-question" => remaining_questions.push(take_value(&mut parser, &argument)?),
            "--linked-target" => linked_targets.push(take_value(&mut parser, &argument)?),
            "--clear-linked-targets" => clear_linked_targets = true,
            "--expected-revision" => {
                expected_revision = Some(parse_u64(take_value(&mut parser, &argument)?, &argument)?)
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown work argument `{other}`"
                )))
            }
        }
    }
    let workspace = workspace
        .ok_or_else(|| CliError::InvalidArguments("work requires --workspace".to_owned()))?;
    let requester_session_id = requester_session_id
        .ok_or_else(|| CliError::InvalidArguments("work requires --session".to_owned()))?;
    let owner_session_id = owner_session_id.unwrap_or_else(|| requester_session_id.clone());
    let conditions = || -> Result<shacs_core::runtime::EntrustedWorkConditions, CliError> {
        Ok(shacs_core::runtime::EntrustedWorkConditions {
            instructions: instructions.clone().ok_or_else(|| {
                CliError::InvalidArguments("work action requires --instructions".to_owned())
            })?,
            budget: budget.clone(),
            prior_decisions: prior_decisions.clone(),
            remaining_questions: remaining_questions.clone(),
        })
    };
    let action = match action_name.as_str() {
        "create" => WorkAction::Create(CreateOptions {
            goal_text: primary,
            turn_budget,
            consent_kind: consent_kind.ok_or_else(|| {
                CliError::InvalidArguments("work create requires --consent".to_owned())
            })?,
            evidence_ref: evidence_ref.ok_or_else(|| {
                CliError::InvalidArguments("work create requires --evidence-ref".to_owned())
            })?,
            conditions: conditions()?,
            linked_targets,
        }),
        "read" => WorkAction::Read { work_id: primary },
        "revise" => WorkAction::Revise(ReviseOptions {
            work_id: primary,
            expected_revision: expected_revision.ok_or_else(|| {
                CliError::InvalidArguments("work revise requires --expected-revision".to_owned())
            })?,
            conditions: conditions()?,
            linked_targets: if clear_linked_targets {
                if !linked_targets.is_empty() {
                    return Err(CliError::InvalidArguments(
                        "--clear-linked-targets conflicts with --linked-target".to_owned(),
                    ));
                }
                Some(Vec::new())
            } else if linked_targets.is_empty() {
                None
            } else {
                Some(linked_targets)
            },
        }),
        "pause" => transition(
            primary,
            shacs_core::runtime::EntrustedWorkLifecycleAction::Pause,
        ),
        "resume" => transition(
            primary,
            shacs_core::runtime::EntrustedWorkLifecycleAction::Resume,
        ),
        "done" => transition(
            primary,
            shacs_core::runtime::EntrustedWorkLifecycleAction::Done,
        ),
        "clear" => transition(
            primary,
            shacs_core::runtime::EntrustedWorkLifecycleAction::Clear,
        ),
        other => {
            return Err(CliError::InvalidArguments(format!(
                "unknown work action `{other}`"
            )))
        }
    };
    Ok(CliCommand::Work(WorkOptions {
        workspace,
        requester_session_id,
        owner_session_id,
        action,
    }))
}

fn transition(
    work_id: String,
    action: shacs_core::runtime::EntrustedWorkLifecycleAction,
) -> WorkAction {
    WorkAction::Transition { work_id, action }
}

fn parse_consent(value: &str) -> Result<shacs_core::runtime::EntrustedWorkConsentKind, CliError> {
    match value {
        "explicit-request" | "explicit_request" => {
            Ok(shacs_core::runtime::EntrustedWorkConsentKind::ExplicitRequest)
        }
        "confirmed-proposal" | "confirmed_proposal" => {
            Ok(shacs_core::runtime::EntrustedWorkConsentKind::ConfirmedProposal)
        }
        _ => Err(CliError::InvalidArguments(
            "--consent must be explicit-request or confirmed-proposal".to_owned(),
        )),
    }
}

fn parse_u32(value: String, flag: &str) -> Result<u32, CliError> {
    value
        .parse()
        .map_err(|error| CliError::InvalidArguments(format!("invalid value for {flag}: {error}")))
}

fn parse_u64(value: String, flag: &str) -> Result<u64, CliError> {
    value
        .parse()
        .map_err(|error| CliError::InvalidArguments(format!("invalid value for {flag}: {error}")))
}

fn take_required(parser: &mut ArgParser, message: &str) -> Result<String, CliError> {
    parser
        .next()
        .ok_or_else(|| CliError::InvalidArguments(message.to_owned()))
}

fn current_timestamp() -> String {
    now_millis().to_string()
}
