//! Spec 048 PRD 010 acceptance 1, 5 and 8 through the CLI: `runtime
//! config-migrate` shows the secrets move without values, runtime start
//! resolves `${NAME}` from `.env.json` and stops while a move is open, and
//! `onboard` writes no secret values.

use serde_json::{json, Value};
use shacs_cli::{
    load_runtime_config_with_env, parse_cli_args, run_command, CliError, RuntimeConfigOptions,
};
use shacs_config::{
    apply_secrets_migration_observed, dry_run_secrets_migration,
    recover_secrets_migration_observed, SecretsApplyStep, SecretsRecoverStep, StepControl,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

const DISCORD: &str = "cli-canary-discord-token-2207";
const OPENROUTER: &str = "cli-canary-openrouter-9911";

fn write_config(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let config_path = root.join("config.json");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&json!({
            "agents": {"defaults": {"workspace": root.join("ws").display().to_string()}},
            "providers": {"openrouter": {"apiKey": OPENROUTER}},
            "channels": {"discord": {"enabled": false, "token": DISCORD}}
        }))?,
    )?;
    Ok(config_path)
}

fn migrate(config_path: &Path, flag: &str) -> Result<String, CliError> {
    let path = config_path.display().to_string();
    run_command(parse_cli_args([
        "runtime",
        "config-migrate",
        flag,
        "--config",
        path.as_str(),
    ])?)
}

fn assert_no_values(text: &str) {
    assert!(
        !text.contains(DISCORD) && !text.contains(OPENROUTER),
        "{text}"
    );
}

#[test]
fn dry_run_apply_and_recover_show_fields_and_names_but_no_values() -> TestResult {
    // Given
    let applied_root = tempfile::tempdir()?;
    let applied_config = write_config(applied_root.path())?;
    let open_root = tempfile::tempdir()?;
    let open_config = write_config(open_root.path())?;
    let open_before = fs::read(&open_config)?;

    // When
    let dry = migrate(&applied_config, "--dry-run")?;
    let applied = migrate(&applied_config, "--apply")?;
    apply_secrets_migration_observed(
        &open_config,
        &BTreeMap::<String, String>::new(),
        &mut |step| {
            if step == SecretsApplyStep::EnvFileWritten {
                StepControl::Stop
            } else {
                StepControl::Continue
            }
        },
    )
    .expect_err("stopped after writing .env.json");
    let blocked = load_runtime_config_with_env(
        RuntimeConfigOptions {
            config_path: Some(open_config.clone()),
            workspace_override: Some(open_root.path().join("ws")),
            resolve_env: true,
        },
        &BTreeMap::<String, String>::new(),
    )
    .expect_err("an open secrets move blocks runtime start");
    let recovered = migrate(&open_config, "--recover")?;

    // Then
    let dry: Value = serde_json::from_str(&dry)?;
    assert_eq!(dry["action"], "dryRun");
    assert_eq!(dry["secrets"]["action"], "dryRun");
    assert_eq!(
        dry["secrets"]["moved"],
        json!([
            {"field": "channels.discord.token", "name": "SHACS_CHANNELS_DISCORD_TOKEN"},
            {"field": "providers.openrouter.apiKey", "name": "SHACS_PROVIDERS_OPENROUTER_API_KEY"}
        ])
    );
    assert_eq!(dry["secrets"]["conflicts"], json!([]));
    let applied_json: Value = serde_json::from_str(&applied)?;
    assert_eq!(applied_json["secrets"]["action"], "applied");
    for text in [
        dry.to_string(),
        applied.clone(),
        recovered.clone(),
        blocked.to_string(),
    ] {
        assert_no_values(&text);
    }
    let config = fs::read_to_string(&applied_config)?;
    assert_no_values(&config);
    assert!(
        config.contains("${SHACS_CHANNELS_DISCORD_TOKEN}"),
        "{config}"
    );
    assert!(
        blocked
            .to_string()
            .contains("config migration is interrupted"),
        "{blocked}"
    );
    let recovered: Value = serde_json::from_str(&recovered)?;
    assert_eq!(recovered["secrets"]["action"], "recovered");
    assert_eq!(recovered["secrets"]["restored"], json!([".env.json"]));
    assert_eq!(fs::read(&open_config)?, open_before);
    assert!(!open_root.path().join(".env.json").exists());
    Ok(())
}

#[test]
fn a_conflicting_apply_fails_without_changes_or_values() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = write_config(root.path())?;
    let env_file = root.path().join(".env.json");
    fs::write(
        &env_file,
        "{\"SHACS_CHANNELS_DISCORD_TOKEN\": \"cli-other-token-value\"}",
    )?;
    let config_before = fs::read(&config_path)?;
    let env_before = fs::read(&env_file)?;

    // When
    let dry: Value = serde_json::from_str(&migrate(&config_path, "--dry-run")?)?;
    let error = migrate(&config_path, "--apply").expect_err("a conflict stops the apply");

    // Then
    assert_eq!(
        dry["secrets"]["conflicts"],
        json!([{
            "field": "channels.discord.token",
            "name": "SHACS_CHANNELS_DISCORD_TOKEN",
            "reason": "envFileDiffers"
        }])
    );
    let shown = error.to_string();
    assert!(shown.contains("channels.discord.token"), "{shown}");
    assert_no_values(&shown);
    assert!(!shown.contains("cli-other-token-value"), "{shown}");
    assert_eq!(fs::read(&config_path)?, config_before);
    assert_eq!(fs::read(&env_file)?, env_before);
    Ok(())
}

#[test]
fn runtime_start_resolves_names_from_env_json() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&json!({
            "channels": {"discord": {"enabled": false, "token": "${SHACS_CHANNELS_DISCORD_TOKEN}"}}
        }))?,
    )?;
    fs::write(
        root.path().join(".env.json"),
        format!("{{\"SHACS_CHANNELS_DISCORD_TOKEN\": \"{DISCORD}\"}}"),
    )?;

    // When
    let bundle = load_runtime_config_with_env(
        RuntimeConfigOptions {
            config_path: Some(config_path),
            workspace_override: Some(root.path().join("ws")),
            resolve_env: true,
        },
        &BTreeMap::from([("SHACS_CHANNELS_DISCORD_TOKEN".to_owned(), String::new())]),
    )?;

    // Then
    assert_eq!(bundle.config.channels.plugins["discord"]["token"], DISCORD);
    Ok(())
}

#[test]
fn onboard_writes_no_secret_values() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    let workspace = root.path().join("ws");
    let config = config_path.display().to_string();
    let workspace_text = workspace.display().to_string();

    // When
    run_command(parse_cli_args([
        "--config",
        config.as_str(),
        "onboard",
        "--workspace",
        workspace_text.as_str(),
    ])?)?;
    let plan = dry_run_secrets_migration(&config_path, &BTreeMap::<String, String>::new())?;

    // Then
    assert!(plan.moved.is_empty(), "{plan:?}");
    assert!(plan.conflicts.is_empty(), "{plan:?}");
    assert!(!root.path().join(".env.json").exists());
    Ok(())
}

#[test]
fn recover_runs_again_after_stopping_once_the_marker_is_gone() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = write_config(root.path())?;
    let before = fs::read(&config_path)?;
    apply_secrets_migration_observed(
        &config_path,
        &BTreeMap::<String, String>::new(),
        &mut |step| {
            if step == SecretsApplyStep::ConfigWritten {
                StepControl::Stop
            } else {
                StepControl::Continue
            }
        },
    )
    .expect_err("stopped after writing the config");
    recover_secrets_migration_observed(&config_path, &mut |step| {
        if step == SecretsRecoverStep::MarkerRemoved {
            StepControl::Stop
        } else {
            StepControl::Continue
        }
    })
    .expect_err("stopped after removing the marker");

    // When
    let again: Value = serde_json::from_str(&migrate(&config_path, "--recover")?)?;
    let after = migrate(&config_path, "--recover");

    // Then
    assert_eq!(again["secrets"]["action"], "recovered");
    assert_eq!(again["secrets"]["restored"], json!([]));
    assert_eq!(
        again["secrets"]["removedBackups"],
        json!(["config.json.secrets-migration-backup"])
    );
    assert_eq!(fs::read(&config_path)?, before);
    assert!(!root.path().join(".env.json").exists());
    assert!(after.is_err(), "nothing is left to recover: {after:?}");
    Ok(())
}
