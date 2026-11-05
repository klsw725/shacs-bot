use crate::CliError;
use serde_json::{json, Value};
use shacs_config::{
    apply_config_migration, apply_secrets_migration, config_migration_marker_path,
    dry_run_config_migration, dry_run_secrets_migration, recover_config_migration,
    recover_secrets_migration, secrets_migration_marker_path, secrets_migration_needs_recover,
    ConfigError, ConfigMigrationEvidence, ProcessEnv, SecretsMigrationEvidence,
};
use shacs_core::runtime::{ActivationStore, ExecutionSnapshot, WorkspaceTrustRef};
use std::fs;
use std::path::Path;

/// The Spec 031 schema migration and the Spec 048 PRD 010 secrets move. The
/// output keeps the schema migration's fields and adds `secrets`.
pub(crate) fn config_migration(
    path: &Path,
    action: super::ConfigMigrationMode,
) -> Result<String, CliError> {
    match action {
        super::ConfigMigrationMode::DryRun => {
            let schema = dry_run_config_migration(path)?;
            let secrets = dry_run_secrets_migration(path, &ProcessEnv)?;
            render_json(&with_secrets(&schema, &secrets)?)
        }
        super::ConfigMigrationMode::Apply => {
            let check = dry_run_secrets_migration(path, &ProcessEnv)?;
            if !check.conflicts.is_empty() {
                return Err(ConfigError::SecretsMigrationStopped(check.conflicts).into());
            }
            let schema = apply_config_migration(path)?;
            let secrets = apply_secrets_migration(path, &ProcessEnv)?;
            render_json(&with_secrets(&schema, &secrets)?)
        }
        super::ConfigMigrationMode::Recover => {
            // An open secrets move first, then an open schema migration, then
            // backups a committed secrets move or recover left behind.
            let secrets_first = secrets_migration_marker_path(path).exists()
                || (!config_migration_marker_path(path).exists()
                    && secrets_migration_needs_recover(path));
            if secrets_first {
                render_json(&json!({"secrets": recover_secrets_migration(path)?}))
            } else {
                render_json(&recover_config_migration(path)?)
            }
        }
    }
}

fn with_secrets(
    schema: &ConfigMigrationEvidence,
    secrets: &SecretsMigrationEvidence,
) -> Result<Value, CliError> {
    let mut value =
        serde_json::to_value(schema).map_err(|error| CliError::Runtime(error.to_string()))?;
    let secrets =
        serde_json::to_value(secrets).map_err(|error| CliError::Runtime(error.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("secrets".to_owned(), secrets);
    }
    Ok(value)
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
