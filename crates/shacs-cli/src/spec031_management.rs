use crate::CliError;
use shacs_config::{apply_config_migration, dry_run_config_migration, recover_config_migration};
use shacs_core::runtime::{ActivationStore, ExecutionSnapshot, WorkspaceTrustRef};
use std::fs;
use std::path::Path;

pub(crate) fn config_migration(
    path: &Path,
    action: super::ConfigMigrationMode,
) -> Result<String, CliError> {
    let evidence = match action {
        super::ConfigMigrationMode::DryRun => dry_run_config_migration(path)?,
        super::ConfigMigrationMode::Apply => apply_config_migration(path)?,
        super::ConfigMigrationMode::Recover => recover_config_migration(path)?,
    };
    render_json(&evidence)
}

pub(crate) fn snapshot(path: &Path) -> Result<String, CliError> {
    let text = fs::read_to_string(path)?;
    let snapshot = ExecutionSnapshot::parse_json(&text)
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    render_json(&snapshot)
}

pub(crate) fn activation(
    store: &Path,
    activation_ref: &str,
    owner: &str,
) -> Result<String, CliError> {
    let record = ActivationStore::new(store)
        .inspect(activation_ref, &WorkspaceTrustRef::new(owner))
        .map_err(|error| CliError::Runtime(error.to_string()))?;
    render_json(&record)
}

fn render_json(value: &impl serde::Serialize) -> Result<String, CliError> {
    serde_json::to_string_pretty(value).map_err(|error| CliError::Runtime(error.to_string()))
}
