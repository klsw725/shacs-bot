use serde_json::json;
use shacs_cli::{
    load_runtime_config_with_env, runtime_recover, RemovedRuntimePathKind, RuntimeConfigOptions,
    RuntimeRecoverOptions,
};
use shacs_config::{begin_config_migration_apply, save_config_to_path, Config};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

#[test]
fn interrupted_config_migration_blocks_runtime_start_mutation() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(&config_path, b"{\"agents\":{}}")?;
    let pending = begin_config_migration_apply(&config_path)?;

    // When
    let error = load_runtime_config_with_env(
        RuntimeConfigOptions {
            config_path: Some(config_path),
            workspace_override: Some(root.path().join("workspace")),
            resolve_env: false,
        },
        &BTreeMap::<String, String>::new(),
    )
    .expect_err("interrupted config migration must block runtime mutation");

    // Then
    drop(pending);
    assert!(error
        .to_string()
        .contains("config migration is interrupted"));
    Ok(())
}

#[test]
fn writable_start_consumes_029_admission_without_spec_status() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("workspace");
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().into_owned();
    save_config_to_path(&config, &config_path)?;

    // When
    let bundle = load_runtime_config_with_env(
        RuntimeConfigOptions {
            config_path: Some(config_path),
            workspace_override: None,
            resolve_env: false,
        },
        &BTreeMap::<String, String>::new(),
    )?;

    // Then
    assert_eq!(bundle.context.workspace, workspace);
    Ok(())
}

#[test]
fn recover_preserves_owned_data_and_receipts_every_removed_marker() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = fixture(&root)?;
    let runtime = root.path().join("runtime");
    fs::create_dir_all(&runtime)?;
    let ownership = runtime.join("ownership-marker.json");
    fs::write(&ownership, stale_owner_marker(&config_path, &workspace))?;
    let update = runtime.join("update-marker.json");
    fs::write(
        &update,
        serde_json::to_vec(&json!({
            "version": 1,
            "fromVersion": "0.1.0",
            "targetVersion": "0.1.0",
            "phase": "completed_cleanup",
            "migrationRequired": false
        }))?,
    )?;
    let session_data = workspace.join("sessions/keep.json");
    fs::create_dir_all(session_data.parent().ok_or("sessions parent")?)?;
    fs::write(&session_data, b"keep")?;

    // When
    let outcome = runtime_recover(RuntimeRecoverOptions {
        config_path: Some(config_path),
        workspace_override: Some(workspace),
    })?;

    // Then
    assert_eq!(
        outcome
            .cleanup
            .removed
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        [
            RemovedRuntimePathKind::UpdateMarker,
            RemovedRuntimePathKind::OwnershipMarker,
        ]
    );
    assert_eq!(
        outcome
            .cleanup
            .removed
            .iter()
            .map(|item| item.path.as_path())
            .collect::<Vec<_>>(),
        [&update, &ownership]
    );
    assert_eq!(fs::read(session_data)?, b"keep");
    Ok(())
}

#[test]
fn recover_blocks_active_owner_without_removing_data_or_marker() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let (config_path, workspace) = fixture(&root)?;
    let ownership = root.path().join("runtime/ownership-marker.json");
    fs::create_dir_all(ownership.parent().ok_or("runtime parent")?)?;
    fs::write(&ownership, active_owner_marker(&config_path, &workspace))?;
    let session_data = workspace.join("sessions/keep.json");
    fs::create_dir_all(session_data.parent().ok_or("sessions parent")?)?;
    fs::write(&session_data, b"keep")?;

    // When
    let error = runtime_recover(RuntimeRecoverOptions {
        config_path: Some(config_path),
        workspace_override: Some(workspace),
    })
    .expect_err("active owner must block cleanup");

    // Then
    assert!(error.to_string().contains("active runtime owner"));
    assert!(ownership.exists());
    assert_eq!(fs::read(session_data)?, b"keep");
    Ok(())
}

fn fixture(
    root: &tempfile::TempDir,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn Error>> {
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("workspace");
    let mut config = Config::default();
    config.agents.defaults.workspace = workspace.to_string_lossy().into_owned();
    save_config_to_path(&config, &config_path)?;
    Ok((config_path, workspace))
}

fn stale_owner_marker(config_path: &Path, workspace: &Path) -> Vec<u8> {
    owner_marker(config_path, workspace, u32::MAX, 1, 2)
}

fn active_owner_marker(config_path: &Path, workspace: &Path) -> Vec<u8> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u128::from(u64::MAX)) as u64
        });
    owner_marker(
        config_path,
        workspace,
        std::process::id(),
        now,
        now.saturating_add(60_000),
    )
}

fn owner_marker(
    config_path: &Path,
    workspace: &Path,
    pid: u32,
    updated_at_ms: u64,
    expires_at_ms: u64,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": 1,
        "owner_id": format!("owner-{pid}-{updated_at_ms}"),
        "pid": pid,
        "acquired_at_ms": updated_at_ms,
        "renewed_at_ms": updated_at_ms,
        "expires_at_ms": expires_at_ms,
        "lifecycle": "running",
        "binary_version": "0.1.0",
        "data_schema_version": 1,
        "mode": "runtime-start",
        "config_path": config_path,
        "workspace": workspace,
        "lock_protocol": "exclusive_file_v1",
        "process_evidence": {
            "pid": pid,
            "pid_alive": true,
            "process_started_after_marker": false
        }
    }))
    .unwrap_or_default()
}
