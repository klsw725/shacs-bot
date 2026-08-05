use std::io::{BufRead, Write};

use shacs_config::default_workspace_path;

use super::{config_apply, format, partial_outcome, readiness, resume};
use crate::{default_config_path, onboard, CliError, OnboardOptions, OnboardOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    ProviderEnv { provider: String, env_var: String },
    Finish,
    Cancel,
    Restart,
    Help,
}

pub(crate) fn run<R: BufRead, W: Write>(
    options: OnboardOptions,
    mut input: R,
    mut output: W,
) -> Result<OnboardOutcome, CliError> {
    let config_path = options.config_path_or_default();
    let workspace = options.workspace_or_default();
    let resume_path = resume::path(&config_path);
    let mut state = resume::read(&resume_path)?;
    let resumed = !state.provider_secret_refs.is_empty();
    let mut config = crate::read_config_value_for_patch(&config_path)?;
    writeln!(output, "{}", format::prompt(&state, resumed))?;

    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            resume::persist(&resume_path, &state)?;
            let report = format::report(
                super::OnboardWizardStatus::Partial,
                resumed,
                state,
                Vec::new(),
                Vec::new(),
            );
            return Ok(partial_outcome(config_path, workspace, report));
        }
        match parse_command(&line)? {
            Command::ProviderEnv { provider, env_var } => {
                config_apply::add_provider_ref(&config, &mut state, provider, env_var)?;
                writeln!(output, "{}", format::prompt(&state, resumed))?;
            }
            Command::Finish => {
                config_apply::apply_refs(&mut config, &state)?;
                serde_json::from_value::<shacs_config::Config>(config.clone()).map_err(
                    |error| {
                        CliError::InvalidArguments(format!(
                            "wizard produced invalid config: {error}"
                        ))
                    },
                )?;
                crate::write_config_value_for_patch(&config_path, &config)?;
                resume::remove(&resume_path)?;
                let mut outcome = onboard(OnboardOptions {
                    config_path: Some(config_path.clone()),
                    workspace: Some(workspace.clone()),
                    wizard: false,
                })?;
                outcome.wizard_report = Some(format::report(
                    super::OnboardWizardStatus::Complete,
                    resumed,
                    state,
                    readiness::external_owner_facts(),
                    readiness::lines(&config_path, &workspace),
                ));
                return Ok(outcome);
            }
            Command::Cancel => {
                resume::remove(&resume_path)?;
                let report = format::report(
                    super::OnboardWizardStatus::Cancelled,
                    resumed,
                    state,
                    Vec::new(),
                    Vec::new(),
                );
                return Ok(partial_outcome(config_path, workspace, report));
            }
            Command::Restart => {
                state = super::OnboardWizardResumeState::default();
                config = crate::read_config_value_for_patch(&config_path)?;
                resume::remove(&resume_path)?;
                writeln!(output, "{}", format::prompt(&state, false))?;
            }
            Command::Help => writeln!(output, "{}", format::prompt(&state, resumed))?,
        }
    }
}

pub(crate) fn parse_command(line: &str) -> Result<Command, CliError> {
    match line.trim() {
        "finish" => return Ok(Command::Finish),
        "cancel" => return Ok(Command::Cancel),
        "restart" => return Ok(Command::Restart),
        "help" | "?" => return Ok(Command::Help),
        _ => {}
    }
    match line.split_whitespace().collect::<Vec<_>>().as_slice() {
        ["provider", provider, "env", env_var] => {
            config_apply::parse_provider_id(provider)?;
            config_apply::parse_env_ref(env_var)?;
            Ok(Command::ProviderEnv { provider: (*provider).to_owned(), env_var: (*env_var).to_owned() })
        }
        _ => Err(CliError::InvalidArguments(
            "wizard command must be `provider <provider-id> env <ENV_VAR>`, `finish`, `cancel`, `restart`, or `help`".to_owned(),
        )),
    }
}

pub(crate) trait OnboardOptionsExt {
    fn config_path_or_default(&self) -> std::path::PathBuf;
    fn workspace_or_default(&self) -> std::path::PathBuf;
}

impl OnboardOptionsExt for OnboardOptions {
    fn config_path_or_default(&self) -> std::path::PathBuf {
        self.config_path.clone().unwrap_or_else(default_config_path)
    }

    fn workspace_or_default(&self) -> std::path::PathBuf {
        self.workspace
            .clone()
            .unwrap_or_else(default_workspace_path)
    }
}
