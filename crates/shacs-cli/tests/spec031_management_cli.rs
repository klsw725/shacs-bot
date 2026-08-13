use serde_json::json;
use shacs_cli::{parse_cli_args, run_command};
use shacs_config::begin_config_migration_apply;
use shacs_core::runtime::{
    ActivationReason, ActivationRecord, ActivationRecordInput, ActivationSource, ActivationStatus,
    ActivationStore, WorkspaceTrustRef,
};
use std::error::Error;
use std::fs;
use std::process::Command;

#[test]
fn spec031_management_help_paths_are_discoverable() -> Result<(), Box<dyn Error>> {
    // Given
    let binary = env!("CARGO_BIN_EXE_shacs-bot");

    // When / Then
    for (args, expected) in [
        (
            &["runtime", "config-migrate", "--help"][..],
            &[
                "runtime config-migrate",
                "--dry-run",
                "--apply",
                "--recover",
            ][..],
        ),
        (
            &["runtime", "snapshot", "--help"][..],
            &["runtime snapshot", "inspect"][..],
        ),
        (
            &["runtime", "snapshot", "inspect", "--help"][..],
            &["runtime snapshot inspect", "<path>"][..],
        ),
        (
            &["runtime", "activation", "--help"][..],
            &["runtime activation", "inspect"][..],
        ),
        (
            &["runtime", "activation", "inspect", "--help"][..],
            &["runtime activation inspect", "--store", "--owner"][..],
        ),
    ] {
        let output = Command::new(binary).args(args).output()?;
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout)?;
        for token in expected {
            assert!(stdout.contains(token), "{args:?} missing {token}: {stdout}");
        }
    }

    Ok(())
}

#[test]
fn spec031_management_invalid_arguments_fail() -> Result<(), Box<dyn Error>> {
    // Given / When
    let invalid = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
        .args(["runtime", "config-migrate", "--invalid"])
        .output()?;

    // Then
    assert!(!invalid.status.success());
    assert!(String::from_utf8(invalid.stderr)?.contains("unknown runtime config-migrate argument"));
    Ok(())
}

#[test]
fn config_migration_dry_run_apply_and_recover_are_user_observable() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.json");
    fs::write(
        &config,
        b"{\"agents\":{\"defaults\":{\"sessionTtlMinutes\":5}}}",
    )?;

    // When
    let dry = run_command(parse_cli_args([
        "runtime",
        "config-migrate",
        "--dry-run",
        "--config",
        config.to_str().ok_or("config path")?,
    ])?)?;
    let applied = run_command(parse_cli_args([
        "runtime",
        "config-migrate",
        "--apply",
        "--config",
        config.to_str().ok_or("config path")?,
    ])?)?;

    // Then
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&dry)?["action"],
        "dryRun"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&applied)?["action"],
        "applied"
    );
    Ok(())
}

#[test]
fn config_migration_recover_reports_conflict_without_raw_user_content() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.json");
    fs::write(
        &config,
        b"{\"agents\":{\"defaults\":{\"sessionTtlMinutes\":5}}}",
    )?;
    drop(begin_config_migration_apply(&config)?);
    fs::write(&config, b"{\"userEdit\":\"CLI_SECRET_CANARY\"}")?;

    // When
    let error = run_command(parse_cli_args([
        "runtime",
        "config-migrate",
        "--recover",
        "--config",
        config.to_str().ok_or("config path")?,
    ])?)
    .expect_err("unknown config state blocks CLI recovery");
    let rendered = error.to_string();

    // Then
    assert!(rendered.contains("migration Recover rejected current file state Unknown"));
    assert!(!rendered.contains("CLI_SECRET_CANARY"));
    assert_eq!(fs::read(&config)?, b"{\"userEdit\":\"CLI_SECRET_CANARY\"}");
    assert!(config.with_extension("json.migration-in-progress").exists());
    assert!(config.with_extension("json.migration-backup").exists());
    Ok(())
}

#[test]
fn snapshot_and_activation_inspection_are_diagnostic_json() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let activation_path = root.path().join("activations.json");
    ActivationStore::new(&activation_path).put(ActivationRecord::new(ActivationRecordInput {
        activation_ref: "activation:skill:formatter:v1".to_owned(),
        source: ActivationSource::TrustedWorkspace,
        workspace_trust_ref: WorkspaceTrustRef::new("workspace:sha256:owner-a"),
        resource_ref: "resource:skill:formatter".to_owned(),
        source_identity: "source:project:formatter".to_owned(),
        content_digest: "sha256:content".to_owned(),
        dependency_manifest_digest: "sha256:deps".to_owned(),
        status: ActivationStatus::Active,
        reason: ActivationReason::Activated,
        recorded_at_unix_ms: 31_005,
    }))?;

    // When
    let output = run_command(parse_cli_args([
        "runtime",
        "activation",
        "inspect",
        "activation:skill:formatter:v1",
        "--store",
        activation_path.to_str().ok_or("activation path")?,
        "--owner",
        "workspace:sha256:owner-a",
    ])?)?;

    // Then
    let value: serde_json::Value = serde_json::from_str(&output)?;
    assert_eq!(
        value["activationRef"],
        json!("activation:skill:formatter:v1")
    );
    assert_eq!(value["status"], json!("active"));
    assert_eq!(value["authorization"], serde_json::Value::Null);
    Ok(())
}
