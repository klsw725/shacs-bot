//! `shacs-bot browser forget` on the command line (Spec 050 PRD 020 scope 5).
//! The user runs it, so no permission rule applies, and it works with
//! `tools.browser.enable` and `tools.browser.persistLogin` off.

use super::browser::{
    forget, ForgetConfirmation, ForgetRequest, LinuxBrowserProcessRunner, SystemBrowserClock,
};
use super::consent_prompt::StdPrompt;
use super::{take_path, ArgParser, CliCommand, CliError};
use shacs_config::{load_config, LoadOptions};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserOptions {
    Forget {
        config_path: Option<PathBuf>,
        workspace_override: Option<PathBuf>,
        confirmation: ForgetConfirmation,
    },
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    match parser.next().as_deref() {
        Some("forget") => parse_forget(parser, global_config),
        Some("--help" | "-h") => Ok(CliCommand::Help),
        Some(other) => Err(CliError::InvalidArguments(format!(
            "unknown browser action `{other}`"
        ))),
        None => Err(CliError::InvalidArguments(
            "browser requires `forget`".to_owned(),
        )),
    }
}

fn parse_forget(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut workspace_override = None;
    let mut confirmation = ForgetConfirmation::Ask;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => workspace_override = Some(take_path(&mut parser, &arg)?),
            "--yes" | "-y" => confirmation = ForgetConfirmation::Skip,
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown browser forget argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Browser(BrowserOptions::Forget {
        config_path,
        workspace_override,
        confirmation,
    }))
}

pub(super) fn run(options: BrowserOptions) -> Result<String, CliError> {
    let BrowserOptions::Forget {
        config_path,
        workspace_override,
        confirmation,
    } = options;
    let bundle = load_config(LoadOptions {
        config_path,
        workspace_override,
        ..LoadOptions::default()
    })?;
    let report = forget(
        &ForgetRequest {
            data_dir: &bundle.context.data_dir,
            confirmation,
            runner: &LinuxBrowserProcessRunner,
            clock: &SystemBrowserClock,
        },
        &mut StdPrompt,
    );
    let said = report.lines.join("\n");
    if report.succeeded {
        Ok(said)
    } else {
        Err(CliError::BrowserForget(said))
    }
}
