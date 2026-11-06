//! `permissions auto-mode reset` (Spec 048 PRD 050 scope 5): removes the
//! user's `permissions.autoMode` after showing it, and nothing else.

use crate::consent_prompt::{Consent, Prompt};
use crate::{AutoModeWriteOptions, CliError};
use serde_json::Value;
use shacs_config::{default_config_path, EnvFile};
use shacs_core::runtime::SecretValues;
use shacs_redaction::redact_value;
use std::{fs, io};

const QUESTION: &str = "Reset auto mode configuration to defaults? [y/N]";
const NOTHING_TO_RESET: &str = "No autoMode settings to reset.";

pub(crate) fn reset(
    options: &AutoModeWriteOptions,
    prompt: &mut dyn Prompt,
) -> Result<String, CliError> {
    let path = options
        .config_path
        .clone()
        .unwrap_or_else(default_config_path);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(NOTHING_TO_RESET.to_owned())
        }
        Err(error) => return Err(error.into()),
    };
    let mut value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| CliError::InvalidArguments(format!("invalid config JSON: {error}")))?;
    let Some(permissions) = value.get_mut("permissions").and_then(Value::as_object_mut) else {
        return Ok(NOTHING_TO_RESET.to_owned());
    };
    let Some(auto_mode) = permissions.remove("autoMode") else {
        return Ok(NOTHING_TO_RESET.to_owned());
    };
    let secrets = SecretValues::new(&EnvFile::load(&path)?);
    let shown = serde_json::to_string_pretty(&redact_value(&secrets.mask_value(&auto_mode)))
        .map_err(|error| CliError::Runtime(format!("could not render JSON: {error}")))?;
    prompt.show(&format!(
        "permissions.autoMode in {}:\n{shown}",
        path.display()
    ));
    let consent = if options.yes {
        Consent::Given
    } else {
        Consent::asked(prompt, QUESTION)
    };
    if let Some(unwritten) = consent.unwritten() {
        return Ok(unwritten.to_owned());
    }
    shacs_config::write_config_value_if_unchanged(&path, Some(&bytes), &value)?;
    Ok("Reset auto mode configuration to defaults.".to_owned())
}

#[cfg(test)]
#[path = "auto_mode_reset_tests.rs"]
mod tests;
