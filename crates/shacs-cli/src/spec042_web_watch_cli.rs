use super::spec036_work_cli::{WorkAction, WorkOptions};
use super::*;
use shacs_core::runtime::{
    ConfigureWebNotifyRequest, ConfirmWebWatchRequest, EntrustedWorkReadRequest, WebCondition,
    WebFetchLimits, WebPreviewRequest, WebPreviewTarget, WebTextArea,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebWatchAction {
    Preview {
        work_id: String,
        url: String,
        target: WebPreviewTarget,
    },
    Confirm {
        work_id: String,
        url: String,
        candidate: String,
        condition: WebCondition,
        cadence_seconds: u64,
        expected_revision: u64,
    },
    Status {
        work_id: String,
        data_dir: PathBuf,
    },
    Notify {
        work_id: String,
        target: Option<String>,
        failure_threshold_seconds: Option<u64>,
        config_path: Option<PathBuf>,
    },
}

/// Whether the configured Email channel can send, by the same rule the runtimes use to start
/// the Email transport: enabled, consented and with SMTP settings.
pub(super) fn email_ready(config_path: Option<PathBuf>) -> Result<bool, CliError> {
    let bundle = shacs_config::load_config_or_default(LoadOptions {
        config_path,
        workspace_override: None,
        resolve_env: true,
        write_back_migrations: false,
    })
    .map_err(|error| CliError::Runtime(error.to_string()))?;
    let plugins = &bundle.config.channels.plugins;
    Ok(channel_enabled_from_plugins(plugins, EMAIL_CHANNEL, false)
        && email_runtime_config(plugins).is_some_and(|email| email.smtp.is_some()))
}

pub(super) fn run(options: WorkOptions, action: WebWatchAction) -> Result<String, CliError> {
    let request = |work_id| EntrustedWorkReadRequest {
        owner_session_id: options.owner_session_id.clone(),
        requester_target: options.requester_session_id.clone(),
        work_id,
    };
    let guard = super::web_watch_profile::web_watch_guard();
    let limits = WebFetchLimits::PRODUCTION;
    let runtime = |error: shacs_core::runtime::WebWatchError| CliError::Runtime(error.to_string());
    let output = match action {
        WebWatchAction::Preview {
            work_id,
            url,
            target,
        } => serde_json::to_value(
            shacs_core::runtime::preview_web_watch(
                &options.workspace,
                &WebPreviewRequest {
                    work: request(work_id),
                    url,
                    target,
                },
                &guard,
                &limits,
            )
            .map_err(runtime)?,
        ),
        WebWatchAction::Confirm {
            work_id,
            url,
            candidate,
            condition,
            cadence_seconds,
            expected_revision,
        } => serde_json::to_value(
            shacs_core::runtime::confirm_web_watch(
                &options.workspace,
                ConfirmWebWatchRequest {
                    work: request(work_id),
                    expected_revision,
                    url,
                    candidate_token: candidate,
                    condition,
                    cadence_seconds,
                    now_ms: now_millis(),
                },
                &guard,
                &limits,
            )
            .map_err(runtime)?,
        ),
        WebWatchAction::Status { work_id, data_dir } => serde_json::to_value(
            shacs_core::runtime::read_web_watch(&options.workspace, &data_dir, &request(work_id))
                .map_err(runtime)?,
        ),
        WebWatchAction::Notify {
            work_id,
            target,
            failure_threshold_seconds,
            config_path,
        } => {
            let email_ready = target.is_some() && email_ready(config_path)?;
            serde_json::to_value(
                shacs_core::runtime::configure_web_watch_notifications(
                    &options.workspace,
                    ConfigureWebNotifyRequest {
                        work: request(work_id),
                        target,
                        failure_threshold_seconds,
                        email_ready,
                        now_ms: now_millis(),
                    },
                )
                .map_err(runtime)?,
            )
        }
    };
    output
        .and_then(|value| serde_json::to_string(&value))
        .map_err(|error| CliError::Runtime(error.to_string()))
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let action_name = parser.next().ok_or_else(|| missing("an action"))?;
    let work_id = parser.next().ok_or_else(|| missing("a work id"))?;
    let mut workspace = None;
    let mut session = None;
    let mut owner = None;
    let mut data_dir = None;
    let mut url = None;
    let mut label = None;
    let mut phrase = None;
    let mut until = None;
    let mut decimal_comma = false;
    let mut candidate = None;
    let mut conditions = Vec::new();
    let mut cadence = None;
    let mut revision = None;
    let mut target = None;
    let mut off = false;
    let mut threshold = None;
    let mut config_path = global_config;
    while let Some(argument) = parser.next() {
        match argument.as_str() {
            "--workspace" | "-w" => workspace = Some(take_path(&mut parser, &argument)?),
            "--session" => session = Some(take_value(&mut parser, &argument)?),
            "--owner-session" => owner = Some(take_value(&mut parser, &argument)?),
            "--data-dir" => data_dir = Some(take_path(&mut parser, &argument)?),
            "--url" => url = Some(take_value(&mut parser, &argument)?),
            "--label" => label = Some(take_value(&mut parser, &argument)?),
            "--phrase" => phrase = Some(take_value(&mut parser, &argument)?),
            "--until" => until = Some(take_value(&mut parser, &argument)?),
            "--decimal-comma" => decimal_comma = true,
            "--candidate" => candidate = Some(take_value(&mut parser, &argument)?),
            "--at-most" => conditions.push(WebCondition::AtMost {
                value: take_value(&mut parser, &argument)?,
            }),
            "--at-least" => conditions.push(WebCondition::AtLeast {
                value: take_value(&mut parser, &argument)?,
            }),
            "--text-present" => conditions.push(WebCondition::TextPresent),
            "--text-absent" => conditions.push(WebCondition::TextAbsent),
            "--cadence-seconds" => cadence = Some(number(&mut parser, &argument)?),
            "--expected-revision" => revision = Some(number(&mut parser, &argument)?),
            "--target" => target = Some(take_value(&mut parser, &argument)?),
            "--off" => off = true,
            "--failure-threshold-seconds" => threshold = Some(number(&mut parser, &argument)?),
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &argument)?),
            "--help" | "-h" => return Ok(CliCommand::Help),
            _ => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown web watch argument `{argument}`"
                )))
            }
        }
    }
    let workspace = workspace.ok_or_else(|| missing("--workspace"))?;
    let session = session.ok_or_else(|| missing("--session"))?;
    let action = match action_name.as_str() {
        "web-preview" => WebWatchAction::Preview {
            work_id,
            url: url.ok_or_else(|| missing("--url"))?,
            target: match (label, phrase, until) {
                (Some(label), None, None) => WebPreviewTarget::Number {
                    label,
                    decimal_comma,
                },
                (None, Some(phrase), None) if !decimal_comma => {
                    WebPreviewTarget::Text { phrase, area: None }
                }
                (Some(label), Some(phrase), Some(until)) if !decimal_comma => {
                    WebPreviewTarget::Text {
                        phrase,
                        area: Some(WebTextArea { label, until }),
                    }
                }
                _ => {
                    return Err(missing(
                        "--label for a number, or --phrase with optional --label and --until",
                    ))
                }
            },
        },
        "web-watch" => WebWatchAction::Confirm {
            work_id,
            url: url.ok_or_else(|| missing("--url"))?,
            candidate: candidate.ok_or_else(|| missing("--candidate"))?,
            condition: match <[WebCondition; 1]>::try_from(conditions) {
                Ok([condition]) => condition,
                Err(_) => {
                    return Err(missing(
                        "one of --at-most, --at-least, --text-present, --text-absent",
                    ))
                }
            },
            cadence_seconds: cadence.ok_or_else(|| missing("--cadence-seconds"))?,
            expected_revision: revision.ok_or_else(|| missing("--expected-revision"))?,
        },
        "web-status" => WebWatchAction::Status {
            work_id,
            data_dir: data_dir.ok_or_else(|| missing("--data-dir"))?,
        },
        "web-notify" => WebWatchAction::Notify {
            work_id,
            target: match (target, off) {
                (Some(target), false) => Some(target),
                (None, true) => None,
                _ => return Err(missing("--target email:<address> or --off")),
            },
            failure_threshold_seconds: threshold,
            config_path,
        },
        _ => return Err(missing("web-preview, web-watch, web-status or web-notify")),
    };
    Ok(CliCommand::Work(WorkOptions {
        workspace,
        owner_session_id: owner.unwrap_or_else(|| session.clone()),
        requester_session_id: session,
        action: WorkAction::WebWatch(action),
    }))
}

fn missing(field: &str) -> CliError {
    CliError::InvalidArguments(format!("work web watch requires {field}"))
}

fn number(parser: &mut ArgParser, flag: &str) -> Result<u64, CliError> {
    take_value(parser, flag)?
        .parse()
        .map_err(|_| CliError::InvalidArguments(format!("invalid {flag}")))
}
