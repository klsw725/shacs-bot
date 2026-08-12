use serde_json::json;
use shacs_config::{
    begin_config_migration_apply, recover_config_migration, ConfigError, ConfigMigrationAction,
    ConfigMigrationFileState, ConfigMigrationOperation,
};
use std::fs;
use std::path::{Path, PathBuf};

fn transaction_paths(path: &Path) -> (PathBuf, PathBuf) {
    (
        path.with_extension("json.migration-in-progress"),
        path.with_extension("json.migration-backup"),
    )
}

#[test]
fn pending_apply_rejects_user_edit_and_preserves_transaction_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let user_edit = br#"{"userEdit":"USER_EDIT_CANARY"}"#;
    fs::write(
        &path,
        br#"{"agents":{"defaults":{"sessionTtlMinutes":11}}}"#,
    )?;
    let pending = begin_config_migration_apply(&path)?;
    let (marker, backup) = transaction_paths(&path);
    fs::write(&path, user_edit)?;

    let error = pending.apply().expect_err("CAS rejects changed config");

    assert!(matches!(
        error,
        ConfigError::MigrationConflict {
            operation: ConfigMigrationOperation::Apply,
            state: ConfigMigrationFileState::Unknown,
        }
    ));
    assert_eq!(fs::read(&path)?, user_edit);
    assert!(marker.exists());
    assert!(backup.exists());
    assert!(!error.to_string().contains("USER_EDIT_CANARY"));
    Ok(())
}

#[test]
fn recover_rejects_user_edit_and_preserves_transaction_files(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let user_edit = br#"{"userEdit":"RECOVERY_EDIT_CANARY"}"#;
    fs::write(&path, br#"{"agents":{"defaults":{"sessionTtlMinutes":7}}}"#)?;
    drop(begin_config_migration_apply(&path)?);
    let (marker, backup) = transaction_paths(&path);
    fs::write(&path, user_edit)?;

    let error = recover_config_migration(&path).expect_err("unknown config conflicts");

    assert!(matches!(
        error,
        ConfigError::MigrationConflict {
            operation: ConfigMigrationOperation::Recover,
            state: ConfigMigrationFileState::Unknown,
        }
    ));
    assert_eq!(fs::read(&path)?, user_edit);
    assert!(marker.exists());
    assert!(backup.exists());
    assert!(!error.to_string().contains("RECOVERY_EDIT_CANARY"));
    Ok(())
}

#[test]
fn recover_finalizes_result_left_after_apply_before_cleanup(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    fs::write(&path, br#"{"agents":{"defaults":{"sessionTtlMinutes":9}}}"#)?;
    drop(begin_config_migration_apply(&path)?);
    let (marker, backup) = transaction_paths(&path);
    let mut result = serde_json::to_vec_pretty(&json!({
        "schemaVersion": 1,
        "agents": {"defaults": {"idleCompactAfterMinutes": 9}}
    }))?;
    result.push(b'\n');
    fs::write(&path, &result)?;

    let evidence = recover_config_migration(&path)?;

    assert_eq!(evidence.action, ConfigMigrationAction::Recovered);
    assert!(!evidence.rollback_performed);
    assert_eq!(evidence.file_state, ConfigMigrationFileState::Result);
    assert_eq!(fs::read(&path)?, result);
    assert!(!marker.exists());
    assert!(!backup.exists());
    Ok(())
}

#[test]
fn recover_restores_missing_config_from_valid_backup() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let original = br#"{"agents":{"defaults":{"sessionTtlMinutes":13}}}"#;
    fs::write(&path, original)?;
    drop(begin_config_migration_apply(&path)?);
    let (marker, backup) = transaction_paths(&path);
    fs::remove_file(&path)?;

    let evidence = recover_config_migration(&path)?;

    assert!(evidence.rollback_performed);
    assert_eq!(evidence.file_state, ConfigMigrationFileState::Missing);
    assert_eq!(fs::read(&path)?, original);
    assert!(!marker.exists());
    assert!(!backup.exists());
    Ok(())
}

#[test]
fn recover_rejects_tampered_backup_without_touching_any_file(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("config.json");
    let original = br#"{"agents":{"defaults":{"sessionTtlMinutes":17}}}"#;
    fs::write(&path, original)?;
    drop(begin_config_migration_apply(&path)?);
    let (marker, backup) = transaction_paths(&path);
    let marker_before = fs::read(&marker)?;
    fs::write(&backup, br#"{"tampered":"BACKUP_CANARY"}"#)?;
    let backup_before = fs::read(&backup)?;

    let error = recover_config_migration(&path).expect_err("tampered backup is rejected");

    assert!(matches!(error, ConfigError::MigrationBackupMismatch));
    assert_eq!(fs::read(&path)?, original);
    assert_eq!(fs::read(&marker)?, marker_before);
    assert_eq!(fs::read(&backup)?, backup_before);
    assert!(!error.to_string().contains("BACKUP_CANARY"));
    Ok(())
}
