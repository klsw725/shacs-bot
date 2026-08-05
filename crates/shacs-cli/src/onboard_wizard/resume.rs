use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::OnboardWizardResumeState;
use crate::CliError;

pub(crate) fn path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("onboard-wizard.partial.json")
}

pub(crate) fn read(path: &Path) -> Result<OnboardWizardResumeState, CliError> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| {
            CliError::InvalidArguments(format!("invalid onboard wizard resume marker: {error}"))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(OnboardWizardResumeState::default())
        }
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn persist(path: &Path, state: &OnboardWizardResumeState) -> Result<(), CliError> {
    if state.provider_secret_refs.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(state).map_err(|error| {
        CliError::InvalidArguments(format!("invalid onboard wizard resume state: {error}"))
    })?;
    fs::write(path, format!("{text}\n"))?;
    Ok(())
}

pub(crate) fn remove(path: &Path) -> Result<(), CliError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
