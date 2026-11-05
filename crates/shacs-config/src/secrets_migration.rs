//! Moving config secrets into `.env.json` as one transaction over two files,
//! with its own marker and backups (Spec 048 PRD 010 scopes 6 and 7).
//!
//! The marker is written first and removed last: removing it commits the
//! move. `.env.json` is written before the config, so the config resolves to
//! the same values at every step.

use crate::config_migration_state::{classify_file, digest};
use crate::env_file::{env_file_path, read_env_file, EnvFile, ENV_FILE_NAME};
use crate::secret_places::{self, KeptSecret, MovedSecret, SecretConflict, SecretsPlan};
use crate::{
    classify_config_schema, config_migration_marker_path, migrate_config_value, ConfigError,
    ConfigMigrationFileState, ConfigMigrationOperation, ConfigSchemaState, EnvSource,
    CURRENT_CONFIG_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

const MARKER_SCHEMA_VERSION: u32 = 1;

pub fn secrets_migration_marker_path(config_path: &Path) -> PathBuf {
    config_path.with_extension("json.secrets-migration-in-progress")
}

/// The backups of the original config and of the original `.env.json`.
pub fn secrets_migration_backup_paths(config_path: &Path) -> [PathBuf; 2] {
    [
        config_path.with_extension("json.secrets-migration-backup"),
        config_path.with_extension("json.secrets-migration-env-backup"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretsMigrationAction {
    DryRun,
    NoOp,
    Applied,
    Recovered,
}

/// What a dry run, apply or recover did. It names fields and `.env.json`
/// names, never values or digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretsMigrationEvidence {
    pub action: SecretsMigrationAction,
    pub moved: Vec<MovedSecret>,
    pub kept: Vec<KeptSecret>,
    pub conflicts: Vec<SecretConflict>,
    pub restored: Vec<String>,
    /// Backups a committed apply or recover left behind and this run removed.
    pub removed_backups: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsMigrationFile {
    Config,
    EnvFile,
}

impl fmt::Display for SecretsMigrationFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Config => "config",
            Self::EnvFile => ENV_FILE_NAME,
        })
    }
}

/// The points between the writes and removals of an apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsApplyStep {
    MarkerWritten,
    ConfigBackupWritten,
    EnvFileBackupWritten,
    EnvFileWritten,
    ConfigWritten,
    MarkerRemoved,
}

/// The points between the writes and removals of a recover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsRecoverStep {
    ConfigRestored,
    EnvFileRestored,
    MarkerRemoved,
}

/// What an observer wants after a step: go on, or stop there as if the
/// process had been killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepControl {
    Continue,
    Stop,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretsMarker {
    schema_version: u32,
    config: FileDigests,
    env_file: EnvFileDigests,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileDigests {
    original: String,
    result: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvFileDigests {
    original: Option<String>,
    result: String,
}

struct Paths {
    config: PathBuf,
    env_file: PathBuf,
    marker: PathBuf,
    config_backup: PathBuf,
    env_file_backup: PathBuf,
}

impl Paths {
    fn new(config_path: &Path) -> Self {
        let [config_backup, env_file_backup] = secrets_migration_backup_paths(config_path);
        Self {
            config: config_path.to_path_buf(),
            env_file: env_file_path(config_path),
            marker: secrets_migration_marker_path(config_path),
            config_backup,
            env_file_backup,
        }
    }
}

struct Prepared {
    config_original: Vec<u8>,
    env_file_original: Option<Vec<u8>>,
    plan: SecretsPlan,
}

/// The plan for the config as Spec 031's migration would leave it.
pub fn dry_run_secrets_migration(
    config_path: &Path,
    env: &(impl EnvSource + ?Sized),
) -> Result<SecretsMigrationEvidence, ConfigError> {
    let prepared = prepare(config_path, env, true)?;
    Ok(evidence(
        SecretsMigrationAction::DryRun,
        &prepared.plan,
        Vec::new(),
    ))
}

pub fn apply_secrets_migration(
    config_path: &Path,
    env: &(impl EnvSource + ?Sized),
) -> Result<SecretsMigrationEvidence, ConfigError> {
    apply_secrets_migration_observed(config_path, env, &mut |_| StepControl::Continue)
}

/// Applies the move, calling `observer` after each write or removal. A
/// `Stop` ends the apply there with `MigrationInterrupted`, leaving the
/// files as a killed process would.
pub fn apply_secrets_migration_observed(
    config_path: &Path,
    env: &(impl EnvSource + ?Sized),
    observer: &mut dyn FnMut(SecretsApplyStep) -> StepControl,
) -> Result<SecretsMigrationEvidence, ConfigError> {
    let paths = Paths::new(config_path);
    ensure_no_marker(config_path)?;
    let prepared = prepare(config_path, env, false)?;
    if !prepared.plan.conflicts.is_empty() {
        return Err(ConfigError::SecretsMigrationStopped(
            prepared.plan.conflicts,
        ));
    }
    let leftovers = remove_backups(&paths)?;
    if prepared.plan.moved.is_empty() {
        let mut evidence = evidence(SecretsMigrationAction::NoOp, &prepared.plan, Vec::new());
        evidence.removed_backups = leftovers;
        return Ok(evidence);
    }
    let mut config_result = serde_json::to_vec_pretty(&prepared.plan.config)?;
    config_result.push(b'\n');
    let env_file_result = prepared.plan.env_file.to_bytes()?;
    let marker = SecretsMarker {
        schema_version: MARKER_SCHEMA_VERSION,
        config: FileDigests {
            original: digest(&prepared.config_original),
            result: digest(&config_result),
        },
        env_file: EnvFileDigests {
            original: prepared.env_file_original.as_deref().map(digest),
            result: digest(&env_file_result),
        },
    };
    let stopped = || ConfigError::MigrationInterrupted {
        marker: paths.marker.clone(),
    };

    write_synced(&paths.marker, &serde_json::to_vec_pretty(&marker)?)?;
    if observer(SecretsApplyStep::MarkerWritten) == StepControl::Stop {
        return Err(stopped());
    }
    if let Err(error) = write_synced(&paths.config_backup, &prepared.config_original) {
        abandon(&paths);
        return Err(error);
    }
    if observer(SecretsApplyStep::ConfigBackupWritten) == StepControl::Stop {
        return Err(stopped());
    }
    if let Some(original) = &prepared.env_file_original {
        if let Err(error) = write_synced(&paths.env_file_backup, original) {
            abandon(&paths);
            return Err(error);
        }
        if observer(SecretsApplyStep::EnvFileBackupWritten) == StepControl::Stop {
            return Err(stopped());
        }
    }

    let env_file_state = classify_env_file(
        &paths.env_file,
        marker.env_file.original.as_deref(),
        &marker.env_file.result,
    )?;
    let env_file_written = if env_file_state == ConfigMigrationFileState::Original {
        write_synced(&paths.env_file, &env_file_result)
    } else {
        Err(apply_conflict(
            SecretsMigrationFile::EnvFile,
            env_file_state,
        ))
    };
    if let Err(error) = env_file_written {
        abandon(&paths);
        return Err(error);
    }
    if observer(SecretsApplyStep::EnvFileWritten) == StepControl::Stop {
        return Err(stopped());
    }

    let config_state = classify_file(
        &paths.config,
        &marker.config.original,
        &marker.config.result,
    )?;
    let config_written = if config_state == ConfigMigrationFileState::Original {
        write_synced(&paths.config, &config_result)
    } else {
        Err(apply_conflict(SecretsMigrationFile::Config, config_state))
    };
    if let Err(error) = config_written {
        if restore_env_file(&paths, prepared.env_file_original.as_deref()).is_ok() {
            abandon(&paths);
        }
        return Err(error);
    }
    if observer(SecretsApplyStep::ConfigWritten) == StepControl::Stop {
        return Err(stopped());
    }

    remove_synced(&paths.marker)?;
    if observer(SecretsApplyStep::MarkerRemoved) == StepControl::Stop {
        return Err(stopped());
    }
    remove_backups(&paths)?;
    let mut evidence = evidence(SecretsMigrationAction::Applied, &prepared.plan, Vec::new());
    evidence.removed_backups = leftovers;
    Ok(evidence)
}

pub fn recover_secrets_migration(
    config_path: &Path,
) -> Result<SecretsMigrationEvidence, ConfigError> {
    recover_secrets_migration_observed(config_path, &mut |_| StepControl::Continue)
}

/// Puts both files back as they were before the move. A `Stop` from
/// `observer` ends the recover there; running it again finishes it. With no
/// marker, an apply or recover has committed, so only the backups it left
/// are removed and both files stay as they are.
pub fn recover_secrets_migration_observed(
    config_path: &Path,
    observer: &mut dyn FnMut(SecretsRecoverStep) -> StepControl,
) -> Result<SecretsMigrationEvidence, ConfigError> {
    let paths = Paths::new(config_path);
    if !paths.marker.exists() {
        let removed_backups = remove_backups(&paths)?;
        if removed_backups.is_empty() {
            return Err(ConfigError::MigrationRecovery(
                "secrets migration marker is missing".to_owned(),
            ));
        }
        return Ok(recovered(Vec::new(), removed_backups));
    }
    let marker = read_marker(&paths.marker)?;
    let config_state = classify_file(
        &paths.config,
        &marker.config.original,
        &marker.config.result,
    )?;
    let env_file_state = classify_env_file(
        &paths.env_file,
        marker.env_file.original.as_deref(),
        &marker.env_file.result,
    )?;
    for (file, state) in [
        (SecretsMigrationFile::Config, config_state),
        (SecretsMigrationFile::EnvFile, env_file_state),
    ] {
        if state == ConfigMigrationFileState::Unknown {
            return Err(ConfigError::SecretsMigrationConflict {
                file,
                operation: ConfigMigrationOperation::Recover,
                state,
            });
        }
    }
    let config_backup = (config_state != ConfigMigrationFileState::Original)
        .then(|| read_backup(&paths.config_backup, &marker.config.original))
        .transpose()?;
    let env_file_backup = (env_file_state != ConfigMigrationFileState::Original)
        .then(|| {
            marker
                .env_file
                .original
                .as_deref()
                .map(|original| read_backup(&paths.env_file_backup, original))
                .transpose()
        })
        .transpose()?;
    let stopped = || ConfigError::MigrationInterrupted {
        marker: paths.marker.clone(),
    };

    let mut restored = Vec::new();
    if let Some(original) = config_backup {
        write_synced(&paths.config, &original)?;
        restored.push(file_label(&paths.config));
        if observer(SecretsRecoverStep::ConfigRestored) == StepControl::Stop {
            return Err(stopped());
        }
    }
    if let Some(original) = env_file_backup {
        restore_env_file(&paths, original.as_deref())?;
        restored.push(ENV_FILE_NAME.to_owned());
        if observer(SecretsRecoverStep::EnvFileRestored) == StepControl::Stop {
            return Err(stopped());
        }
    }
    remove_synced(&paths.marker)?;
    if observer(SecretsRecoverStep::MarkerRemoved) == StepControl::Stop {
        return Err(stopped());
    }
    remove_backups(&paths)?;
    Ok(recovered(restored, Vec::new()))
}

fn recovered(restored: Vec<String>, removed_backups: Vec<String>) -> SecretsMigrationEvidence {
    SecretsMigrationEvidence {
        action: SecretsMigrationAction::Recovered,
        moved: Vec::new(),
        kept: Vec::new(),
        conflicts: Vec::new(),
        restored,
        removed_backups,
    }
}

/// Whether a secrets migration left a marker or backups for `--recover`.
pub fn secrets_migration_needs_recover(config_path: &Path) -> bool {
    let paths = Paths::new(config_path);
    [paths.marker, paths.config_backup, paths.env_file_backup]
        .iter()
        .any(|path| path.exists())
}

/// Either marker blocks a new apply until it is recovered.
pub(crate) fn ensure_no_marker(config_path: &Path) -> Result<(), ConfigError> {
    for marker in [
        config_migration_marker_path(config_path),
        secrets_migration_marker_path(config_path),
    ] {
        if marker.exists() {
            return Err(ConfigError::MigrationInterrupted { marker });
        }
    }
    Ok(())
}

fn prepare(
    config_path: &Path,
    env: &(impl EnvSource + ?Sized),
    migrated_shape: bool,
) -> Result<Prepared, ConfigError> {
    let config_original = fs::read(config_path)?;
    let mut value = serde_json::from_slice::<Value>(&config_original)?;
    match classify_config_schema(&value).map_err(|_| ConfigError::InvalidSchemaVersion)? {
        ConfigSchemaState::Legacy | ConfigSchemaState::Current => {}
        ConfigSchemaState::FutureUnsupported { found } => {
            return Err(ConfigError::UnsupportedSchema {
                found,
                current: CURRENT_CONFIG_SCHEMA_VERSION,
            })
        }
    }
    if migrated_shape {
        migrate_config_value(&mut value);
    }
    let env_file = read_env_file(&env_file_path(config_path))?;
    let plan = secret_places::plan(
        &value,
        env_file
            .as_ref()
            .map_or(&EnvFile::default(), |(file, _)| file),
        env,
    );
    Ok(Prepared {
        config_original,
        env_file_original: env_file.map(|(_, bytes)| bytes),
        plan,
    })
}

fn evidence(
    action: SecretsMigrationAction,
    plan: &SecretsPlan,
    restored: Vec<String>,
) -> SecretsMigrationEvidence {
    SecretsMigrationEvidence {
        action,
        moved: plan.moved.clone(),
        kept: plan.kept.clone(),
        conflicts: plan.conflicts.clone(),
        restored,
        removed_backups: Vec::new(),
    }
}

fn apply_conflict(file: SecretsMigrationFile, state: ConfigMigrationFileState) -> ConfigError {
    ConfigError::SecretsMigrationConflict {
        file,
        operation: ConfigMigrationOperation::Apply,
        state,
    }
}

/// Writes through a 0600 temporary file and syncs the folder so the order of
/// the transaction's writes survives.
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    crate::write_secret_file(path, bytes)?;
    sync_folder(path)?;
    Ok(())
}

fn remove_synced(path: &Path) -> Result<(), ConfigError> {
    match fs::remove_file(path) {
        Ok(()) => sync_folder(path).map_err(ConfigError::Io),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ConfigError::Io(error)),
    }
}

/// Removes both backups and names the ones that were there.
fn remove_backups(paths: &Paths) -> Result<Vec<String>, ConfigError> {
    let mut removed = Vec::new();
    for backup in [&paths.config_backup, &paths.env_file_backup] {
        if backup.exists() {
            remove_synced(backup)?;
            removed.push(file_label(backup));
        }
    }
    Ok(removed)
}

/// Undoes a start that wrote nothing but the marker and backups.
fn abandon(paths: &Paths) {
    if remove_backups(paths).is_ok() {
        let _ = remove_synced(&paths.marker);
    }
}

fn restore_env_file(paths: &Paths, original: Option<&[u8]>) -> Result<(), ConfigError> {
    match original {
        Some(bytes) => write_synced(&paths.env_file, bytes),
        None => remove_synced(&paths.env_file),
    }
}

#[cfg(unix)]
fn sync_folder(path: &Path) -> io::Result<()> {
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    fs::File::open(folder)?.sync_all()
}

#[cfg(not(unix))]
fn sync_folder(_: &Path) -> io::Result<()> {
    Ok(())
}

fn read_marker(path: &Path) -> Result<SecretsMarker, ConfigError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ConfigError::MigrationRecovery(
                "secrets migration marker is missing".to_owned(),
            ))
        }
        Err(error) => return Err(ConfigError::Io(error)),
    };
    serde_json::from_slice::<SecretsMarker>(&bytes)
        .ok()
        .filter(|marker| marker.schema_version == MARKER_SCHEMA_VERSION)
        .ok_or_else(|| {
            ConfigError::MigrationRecovery("secrets migration marker is malformed".to_owned())
        })
}

fn read_backup(path: &Path, original_digest: &str) -> Result<Vec<u8>, ConfigError> {
    match fs::read(path) {
        Ok(bytes) if digest(&bytes) == original_digest => Ok(bytes),
        Ok(_) => Err(ConfigError::MigrationBackupMismatch),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(ConfigError::MigrationBackupMismatch)
        }
        Err(error) => Err(ConfigError::Io(error)),
    }
}

/// Like `classify_file`, except that a file that did not exist before the
/// move is in its original state while it is missing.
fn classify_env_file(
    path: &Path,
    original_digest: Option<&str>,
    result_digest: &str,
) -> Result<ConfigMigrationFileState, ConfigError> {
    match original_digest {
        Some(original) => Ok(classify_file(path, original, result_digest)?),
        None => match fs::read(path) {
            Ok(bytes) if digest(&bytes) == result_digest => Ok(ConfigMigrationFileState::Result),
            Ok(_) => Ok(ConfigMigrationFileState::Unknown),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(ConfigMigrationFileState::Original)
            }
            Err(error) => Err(ConfigError::Io(error)),
        },
    }
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
