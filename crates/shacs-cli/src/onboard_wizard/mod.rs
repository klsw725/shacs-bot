mod config_apply;
mod format;
mod io_loop;
mod readiness;
mod resume;

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

use crate::{CliError, OnboardOptions, OnboardOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardWizardReport {
    pub status: OnboardWizardStatus,
    pub resumed: bool,
    pub provider_secret_refs: Vec<OnboardWizardProviderRef>,
    pub external_owner_facts: Vec<OnboardWizardExternalOwnerFact>,
    pub readiness_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardWizardStatus {
    Complete,
    Partial,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardWizardProviderRef {
    pub provider: String,
    pub source_kind: String,
    pub locator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardWizardExternalOwnerFact {
    pub owner: String,
    pub capability: String,
    pub state: String,
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct OnboardWizardResumeState {
    pub provider_secret_refs: Vec<OnboardWizardProviderRef>,
}

pub fn run<R: BufRead, W: Write>(
    options: OnboardOptions,
    input: R,
    output: W,
) -> Result<OnboardOutcome, CliError> {
    io_loop::run(options, input, output)
}

pub(crate) fn partial_outcome(
    config_path: std::path::PathBuf,
    workspace: std::path::PathBuf,
    report: OnboardWizardReport,
) -> OnboardOutcome {
    OnboardOutcome {
        config_path,
        workspace,
        runtime_dirs: Vec::new(),
        template_files: Vec::new(),
        template_dirs: Vec::new(),
        migrations: Vec::new(),
        wizard_report: Some(report),
    }
}

#[cfg(test)]
mod tests;
