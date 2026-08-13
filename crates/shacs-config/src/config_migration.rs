use crate::{
    classify_config_schema, migrate_config_value, ConfigError, ConfigMigrationFileState,
    ConfigMigrationOperation, ConfigSchemaError, ConfigSchemaState, Migration,
    CURRENT_CONFIG_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigMigrationAction {
    NoOp,
    DryRun,
    Applied,
    Recovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMigrationEvidence {
    pub action: ConfigMigrationAction,
    pub changed: bool,
    pub rollback_performed: bool,
    pub from_schema_version: Option<u32>,
    pub to_schema_version: u32,
    pub original_digest: String,
    pub result_digest: String,
    pub migration_keys: Vec<String>,
    pub file_state: ConfigMigrationFileState,
}

#[derive(Debug)]
pub struct PendingConfigMigration {
    config_path: PathBuf,
    marker_path: PathBuf,
    backup_path: PathBuf,
    plan: MigrationPlan,
}

#[derive(Debug, Clone)]
struct MigrationPlan {
    original: Vec<u8>,
    result: Vec<u8>,
    from_schema_version: Option<u32>,
    migrations: Vec<Migration>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationMarker {
    schema_version: u32,
    original_digest: String,
    result_digest: String,
}

pub fn dry_run_config_migration(path: &Path) -> Result<ConfigMigrationEvidence, ConfigError> {
    let plan = plan(path)?;
    Ok(plan.evidence(
        ConfigMigrationAction::DryRun,
        false,
        ConfigMigrationFileState::Original,
    ))
}

pub fn config_migration_marker_path(path: &Path) -> PathBuf {
    marker_path(path)
}

pub fn begin_config_migration_apply(path: &Path) -> Result<PendingConfigMigration, ConfigError> {
    ensure_uninterrupted(path)?;
    let plan = plan(path)?;
    let marker_path = marker_path(path);
    let backup_path = backup_path(path);
    crate::write_atomic_file(&backup_path, &plan.original)?;
    let marker = MigrationMarker {
        schema_version: CURRENT_CONFIG_SCHEMA_VERSION,
        original_digest: digest(&plan.original),
        result_digest: digest(&plan.result),
    };
    if let Err(error) = crate::write_atomic_file(&marker_path, &serde_json::to_vec_pretty(&marker)?)
    {
        let _ = fs::remove_file(&backup_path);
        return Err(ConfigError::Io(error));
    }
    Ok(PendingConfigMigration {
        config_path: path.to_path_buf(),
        marker_path,
        backup_path,
        plan,
    })
}

impl PendingConfigMigration {
    pub fn apply(self) -> Result<ConfigMigrationEvidence, ConfigError> {
        let state = crate::config_migration_state::classify_file(
            &self.config_path,
            &digest(&self.plan.original),
            &digest(&self.plan.result),
        )?;
        if state != ConfigMigrationFileState::Original {
            return Err(ConfigError::MigrationConflict {
                operation: ConfigMigrationOperation::Apply,
                state,
            });
        }
        crate::write_atomic_file(&self.config_path, &self.plan.result)?;
        fs::remove_file(&self.marker_path)?;
        fs::remove_file(&self.backup_path)?;
        Ok(self.plan.evidence(
            ConfigMigrationAction::Applied,
            false,
            ConfigMigrationFileState::Original,
        ))
    }
}

pub fn apply_config_migration(path: &Path) -> Result<ConfigMigrationEvidence, ConfigError> {
    ensure_uninterrupted(path)?;
    let plan = plan(path)?;
    if !plan.changed() {
        return Ok(plan.evidence(
            ConfigMigrationAction::NoOp,
            false,
            ConfigMigrationFileState::Original,
        ));
    }
    begin_config_migration_apply(path)?.apply()
}

pub fn recover_config_migration(path: &Path) -> Result<ConfigMigrationEvidence, ConfigError> {
    let marker_path = marker_path(path);
    if !marker_path.exists() {
        return Err(ConfigError::MigrationRecovery(
            "migration marker is missing".to_owned(),
        ));
    }
    let marker: MigrationMarker = serde_json::from_slice(&fs::read(&marker_path)?)?;
    let backup_path = backup_path(path);
    let original = fs::read(&backup_path)?;
    if digest(&original) != marker.original_digest {
        return Err(ConfigError::MigrationBackupMismatch);
    }
    let state = crate::config_migration_state::classify_file(
        path,
        &marker.original_digest,
        &marker.result_digest,
    )?;
    let rollback_performed = match state {
        ConfigMigrationFileState::Original | ConfigMigrationFileState::Result => false,
        ConfigMigrationFileState::Missing => {
            crate::write_atomic_file(path, &original)?;
            true
        }
        ConfigMigrationFileState::Unknown => {
            return Err(ConfigError::MigrationConflict {
                operation: ConfigMigrationOperation::Recover,
                state,
            });
        }
    };
    fs::remove_file(marker_path)?;
    fs::remove_file(backup_path)?;
    Ok(ConfigMigrationEvidence {
        action: ConfigMigrationAction::Recovered,
        changed: true,
        rollback_performed,
        from_schema_version: None,
        to_schema_version: CURRENT_CONFIG_SCHEMA_VERSION,
        original_digest: marker.original_digest.clone(),
        result_digest: marker.result_digest,
        migration_keys: Vec::new(),
        file_state: state,
    })
}

impl MigrationPlan {
    fn changed(&self) -> bool {
        !self.migrations.is_empty()
    }

    fn evidence(
        &self,
        action: ConfigMigrationAction,
        rollback_performed: bool,
        file_state: ConfigMigrationFileState,
    ) -> ConfigMigrationEvidence {
        ConfigMigrationEvidence {
            action,
            changed: self.changed(),
            rollback_performed,
            from_schema_version: self.from_schema_version,
            to_schema_version: CURRENT_CONFIG_SCHEMA_VERSION,
            original_digest: digest(&self.original),
            result_digest: digest(&self.result),
            migration_keys: self
                .migrations
                .iter()
                .map(|migration| migration.key.clone())
                .collect(),
            file_state,
        }
    }
}

fn plan(path: &Path) -> Result<MigrationPlan, ConfigError> {
    let original = fs::read(path)?;
    let mut value: Value = serde_json::from_slice(&original)?;
    let state = classify_config_schema(&value).map_err(schema_error)?;
    let from_schema_version = match state {
        ConfigSchemaState::Legacy => None,
        ConfigSchemaState::Current => Some(CURRENT_CONFIG_SCHEMA_VERSION),
        ConfigSchemaState::FutureUnsupported { found } => {
            return Err(ConfigError::UnsupportedSchema {
                found,
                current: CURRENT_CONFIG_SCHEMA_VERSION,
            });
        }
    };
    let migrations = migrate_config_value(&mut value);
    let result = if migrations.is_empty() {
        original.clone()
    } else {
        let mut bytes = serde_json::to_vec_pretty(&value)?;
        bytes.push(b'\n');
        bytes
    };
    Ok(MigrationPlan {
        original,
        result,
        from_schema_version,
        migrations,
    })
}

fn ensure_uninterrupted(path: &Path) -> Result<(), ConfigError> {
    let marker = marker_path(path);
    if marker.exists() {
        return Err(ConfigError::MigrationInterrupted { marker });
    }
    Ok(())
}

fn marker_path(path: &Path) -> PathBuf {
    path.with_extension("json.migration-in-progress")
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.migration-backup")
}

fn digest(bytes: &[u8]) -> String {
    crate::config_migration_state::digest(bytes)
}

fn schema_error(error: ConfigSchemaError) -> ConfigError {
    match error {
        ConfigSchemaError::Invalid => ConfigError::InvalidSchemaVersion,
    }
}
