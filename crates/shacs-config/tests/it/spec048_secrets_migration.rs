//! Spec 048 PRD 010 acceptance 3 and 4: the secrets move plans without
//! values, stops without changes, and keeps the config resolving to the same
//! values at every interruption of apply and recover.

use serde_json::{json, Value};
use shacs_config::{
    apply_config_migration, apply_secrets_migration, apply_secrets_migration_observed,
    config_migration_marker_path, dry_run_secrets_migration, load_config_with_env,
    recover_secrets_migration, recover_secrets_migration_observed, secrets_migration_backup_paths,
    secrets_migration_marker_path, ConfigError, ConfigMigrationFileState, ConfigMigrationOperation,
    LoadOptions, SecretsApplyStep, SecretsMigrationAction, SecretsMigrationFile,
    SecretsRecoverStep, StepControl,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn Error>>;

const OPENROUTER: &str = "canary-openrouter-key-1234";
const DISCORD: &str = "canary-discord-token-5678";
const GITHUB: &str = "canary-github-token-9012";
const OTHER: &str = "canary-kept-entry-3456";

/// A data folder with a config holding three secrets and, optionally, an
/// existing `.env.json` with an unrelated entry.
struct Fixture {
    _root: tempfile::TempDir,
    config: PathBuf,
    env_file: PathBuf,
}

/// The bytes of the config and of `.env.json` (if any).
type Snapshot = (Vec<u8>, Option<Vec<u8>>);

impl Fixture {
    fn new(env_file_existed: bool) -> Result<Self, Box<dyn Error>> {
        Self::with_config(
            json!({
                "providers": {"openrouter": {"apiKey": OPENROUTER}},
                "channels": {"discord": {"enabled": true, "token": DISCORD}},
                "tools": {"exec": {"env": {"GITHUB_TOKEN": GITHUB}}}
            }),
            env_file_existed,
        )
    }

    fn with_config(config: Value, env_file_existed: bool) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;
        let env_file = root.path().join(".env.json");
        if env_file_existed {
            fs::write(&env_file, format!("{{\"OTHER_ENTRY\": \"{OTHER}\"}}"))?;
        }
        Ok(Self {
            _root: root,
            config: config_path,
            env_file,
        })
    }

    fn snapshot(&self) -> Result<Snapshot, Box<dyn Error>> {
        let env_file = match fs::read(&self.env_file) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok((fs::read(&self.config)?, env_file))
    }

    /// The three secrets as a resolving config read sees them.
    fn resolved(&self) -> Result<[String; 3], Box<dyn Error>> {
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(self.config.clone()),
                workspace_override: None,
                resolve_env: true,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let config = bundle.config;
        Ok([
            config.providers["openrouter"]
                .api_key
                .clone()
                .unwrap_or_default(),
            config.channels.plugins["discord"]["token"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            config.tools.exec.env["GITHUB_TOKEN"].clone(),
        ])
    }

    fn marker(&self) -> PathBuf {
        secrets_migration_marker_path(&self.config)
    }

    fn backups(&self) -> [PathBuf; 2] {
        secrets_migration_backup_paths(&self.config)
    }

    fn leftovers(&self) -> Vec<PathBuf> {
        let [config_backup, env_file_backup] = self.backups();
        [self.marker(), config_backup, env_file_backup]
            .into_iter()
            .filter(|path| path.exists())
            .collect()
    }
}

fn no_env() -> BTreeMap<String, String> {
    BTreeMap::new()
}

fn stop_at<S: PartialEq>(stop: S) -> impl FnMut(S) -> StepControl {
    move |step| {
        if step == stop {
            StepControl::Stop
        } else {
            StepControl::Continue
        }
    }
}

fn assert_no_values(text: &str) {
    for value in [OPENROUTER, DISCORD, GITHUB, OTHER] {
        assert!(!text.contains(value), "{text}");
    }
}

#[test]
fn apply_moves_values_and_the_config_resolves_to_the_same_values() -> TestResult {
    for env_file_existed in [false, true] {
        // Given
        let fixture = Fixture::new(env_file_existed)?;
        let expected = fixture.resolved()?;

        // When
        let dry = dry_run_secrets_migration(&fixture.config, &no_env())?;
        let applied = apply_secrets_migration(&fixture.config, &no_env())?;
        let again = apply_secrets_migration(&fixture.config, &no_env())?;

        // Then
        assert_eq!(dry.action, SecretsMigrationAction::DryRun);
        assert_eq!(applied.action, SecretsMigrationAction::Applied);
        assert_eq!(again.action, SecretsMigrationAction::NoOp);
        assert_eq!(applied.moved, dry.moved);
        assert_eq!(applied.moved.len(), 3);
        assert_no_values(&serde_json::to_string(&dry)?);
        assert_no_values(&serde_json::to_string(&applied)?);
        let config = fs::read_to_string(&fixture.config)?;
        assert_no_values(&config);
        assert!(
            config.contains("${SHACS_CHANNELS_DISCORD_TOKEN}"),
            "{config}"
        );
        assert!(config.contains("\"enabled\": true"), "{config}");
        let env_file = fs::read_to_string(&fixture.env_file)?;
        assert!(env_file.contains(DISCORD), "{env_file}");
        assert_eq!(env_file.contains(OTHER), env_file_existed, "{env_file}");
        assert_eq!(fixture.resolved()?, expected);
        assert!(fixture.leftovers().is_empty(), "{:?}", fixture.leftovers());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&fixture.env_file)?.permissions().mode() & 0o777,
                0o600
            );
        }
    }
    Ok(())
}

#[test]
fn a_conflict_stops_the_apply_before_any_file_changes() -> TestResult {
    // Given
    let fixture = Fixture::new(false)?;
    fs::write(
        &fixture.env_file,
        "{\"SHACS_CHANNELS_DISCORD_TOKEN\": \"a-different-token\"}",
    )?;
    let before = fixture.snapshot()?;

    // When
    let dry = dry_run_secrets_migration(&fixture.config, &no_env())?;
    let error = apply_secrets_migration(&fixture.config, &no_env())
        .expect_err("a different value in .env.json stops the move");

    // Then
    assert_eq!(dry.conflicts.len(), 1);
    assert!(
        matches!(&error, ConfigError::SecretsMigrationStopped(conflicts) if conflicts.len() == 1),
        "{error:?}"
    );
    let shown = error.to_string();
    assert!(shown.contains("channels.discord.token"), "{shown}");
    assert_no_values(&shown);
    assert!(!shown.contains("a-different-token"), "{shown}");
    assert_eq!(fixture.snapshot()?, before);
    assert!(fixture.leftovers().is_empty());
    Ok(())
}

#[test]
fn each_apply_interruption_resolves_and_recovers_to_the_bytes_before() -> TestResult {
    for env_file_existed in [false, true] {
        let mut steps = vec![
            SecretsApplyStep::MarkerWritten,
            SecretsApplyStep::ConfigBackupWritten,
            SecretsApplyStep::EnvFileWritten,
            SecretsApplyStep::ConfigWritten,
            SecretsApplyStep::MarkerRemoved,
        ];
        if env_file_existed {
            steps.push(SecretsApplyStep::EnvFileBackupWritten);
        }
        for step in steps {
            // Given
            let fixture = Fixture::new(env_file_existed)?;
            let before = fixture.snapshot()?;
            let expected = fixture.resolved()?;

            // When
            let error =
                apply_secrets_migration_observed(&fixture.config, &no_env(), &mut stop_at(step))
                    .expect_err("the observer stops the apply");

            // Then
            assert!(
                matches!(error, ConfigError::MigrationInterrupted { .. }),
                "{step:?} {error:?}"
            );
            assert_eq!(fixture.resolved()?, expected, "{step:?}");
            if step == SecretsApplyStep::MarkerRemoved {
                // Committed: a later apply or recover only removes the
                // backups left behind, and the move stays.
                assert!(!fixture.marker().exists());
                assert!(!fixture.leftovers().is_empty(), "{step:?}");
                let moved = fixture.snapshot()?;
                let cleaned = if env_file_existed {
                    recover_secrets_migration(&fixture.config)?
                } else {
                    apply_secrets_migration(&fixture.config, &no_env())?
                };
                assert!(!cleaned.removed_backups.is_empty(), "{cleaned:?}");
                assert!(cleaned.restored.is_empty(), "{cleaned:?}");
                assert_eq!(fixture.snapshot()?, moved);
                assert!(fixture.leftovers().is_empty(), "{step:?}");
                continue;
            }
            let blocked = apply_secrets_migration(&fixture.config, &no_env())
                .expect_err("the marker blocks a new apply");
            assert!(matches!(blocked, ConfigError::MigrationInterrupted { .. }));
            let blocked = apply_config_migration(&fixture.config)
                .expect_err("the marker blocks the schema migration too");
            assert!(matches!(blocked, ConfigError::MigrationInterrupted { .. }));
            let recovered = recover_secrets_migration(&fixture.config)?;
            assert_eq!(recovered.action, SecretsMigrationAction::Recovered);
            assert_eq!(fixture.snapshot()?, before, "{step:?}");
            assert!(fixture.leftovers().is_empty(), "{step:?}");
            let applied = apply_secrets_migration(&fixture.config, &no_env())?;
            assert_eq!(applied.action, SecretsMigrationAction::Applied, "{step:?}");
            assert_eq!(fixture.resolved()?, expected, "{step:?}");
        }
    }
    Ok(())
}

#[test]
fn an_interrupted_recover_finishes_when_run_again() -> TestResult {
    for env_file_existed in [false, true] {
        for step in [
            SecretsRecoverStep::ConfigRestored,
            SecretsRecoverStep::EnvFileRestored,
            SecretsRecoverStep::MarkerRemoved,
        ] {
            // Given
            let fixture = Fixture::new(env_file_existed)?;
            let before = fixture.snapshot()?;
            let expected = fixture.resolved()?;
            apply_secrets_migration_observed(
                &fixture.config,
                &no_env(),
                &mut stop_at(SecretsApplyStep::ConfigWritten),
            )
            .expect_err("stopped after the config write");

            // When
            let error = recover_secrets_migration_observed(&fixture.config, &mut stop_at(step))
                .expect_err("the observer stops the recover");

            // Then
            assert!(matches!(error, ConfigError::MigrationInterrupted { .. }));
            assert_eq!(fixture.resolved()?, expected, "{step:?}");
            let finished = recover_secrets_migration(&fixture.config)?;
            assert_eq!(finished.action, SecretsMigrationAction::Recovered);
            if step == SecretsRecoverStep::MarkerRemoved {
                assert!(finished.restored.is_empty(), "{finished:?}");
                assert!(!finished.removed_backups.is_empty(), "{finished:?}");
            }
            assert_eq!(fixture.snapshot()?, before, "{step:?}");
            assert!(fixture.leftovers().is_empty(), "{step:?}");
            let applied = apply_secrets_migration(&fixture.config, &no_env())?;
            assert_eq!(applied.action, SecretsMigrationAction::Applied, "{step:?}");
            assert!(fixture.leftovers().is_empty(), "{step:?}");
        }
    }
    Ok(())
}

#[test]
fn recover_refuses_edited_files_and_a_tampered_backup_without_changes() -> TestResult {
    // Given
    let edited = Fixture::new(true)?;
    apply_secrets_migration_observed(
        &edited.config,
        &no_env(),
        &mut stop_at(SecretsApplyStep::EnvFileWritten),
    )
    .expect_err("stopped after the .env.json write");
    fs::write(&edited.config, "{\"edited\": true}")?;
    let edited_before = edited.snapshot()?;
    let tampered = Fixture::new(false)?;
    apply_secrets_migration_observed(
        &tampered.config,
        &no_env(),
        &mut stop_at(SecretsApplyStep::ConfigWritten),
    )
    .expect_err("stopped after the config write");
    let [config_backup, _] = tampered.backups();
    fs::write(&config_backup, "{\"tampered\": true}")?;
    let tampered_before = tampered.snapshot()?;

    // When
    let edited_error = recover_secrets_migration(&edited.config).expect_err("edited config");
    let tampered_error = recover_secrets_migration(&tampered.config).expect_err("bad backup");

    // Then
    assert!(
        matches!(
            edited_error,
            ConfigError::SecretsMigrationConflict {
                file: SecretsMigrationFile::Config,
                operation: ConfigMigrationOperation::Recover,
                state: ConfigMigrationFileState::Unknown,
            }
        ),
        "{edited_error:?}"
    );
    assert!(matches!(
        tampered_error,
        ConfigError::MigrationBackupMismatch
    ));
    assert_eq!(edited.snapshot()?, edited_before);
    assert_eq!(tampered.snapshot()?, tampered_before);
    assert!(edited.marker().exists() && tampered.marker().exists());
    Ok(())
}

#[test]
fn a_file_changed_during_the_apply_fails_it_and_leaves_no_marker() -> TestResult {
    for env_file_existed in [false, true] {
        // Given: .env.json changes after the plan, before it is written.
        let env_changed = Fixture::new(env_file_existed)?;
        let config_before = fs::read(&env_changed.config)?;
        let env_file = env_changed.env_file.clone();
        let mut change_env_file = |step| {
            if step == SecretsApplyStep::ConfigBackupWritten {
                let _ = fs::write(&env_file, "{\"SOMEONE_ELSE\": \"wrote this\"}");
            }
            StepControl::Continue
        };
        // And: the config changes after .env.json is written.
        let config_changed = Fixture::new(env_file_existed)?;
        let env_before = config_changed.snapshot()?.1;
        let config = config_changed.config.clone();
        let mut change_config = |step| {
            if step == SecretsApplyStep::EnvFileWritten {
                let _ = fs::write(&config, "{\"someone\": \"else\"}");
            }
            StepControl::Continue
        };

        // When
        let env_error =
            apply_secrets_migration_observed(&env_changed.config, &no_env(), &mut change_env_file)
                .expect_err(".env.json changed");
        let config_error =
            apply_secrets_migration_observed(&config_changed.config, &no_env(), &mut change_config)
                .expect_err("config changed");

        // Then
        assert!(matches!(
            env_error,
            ConfigError::SecretsMigrationConflict {
                file: SecretsMigrationFile::EnvFile,
                ..
            }
        ));
        assert_eq!(fs::read(&env_changed.config)?, config_before);
        assert_eq!(
            fs::read_to_string(&env_changed.env_file)?,
            "{\"SOMEONE_ELSE\": \"wrote this\"}"
        );
        assert!(env_changed.leftovers().is_empty());
        assert!(matches!(
            config_error,
            ConfigError::SecretsMigrationConflict {
                file: SecretsMigrationFile::Config,
                ..
            }
        ));
        assert_eq!(config_changed.snapshot()?.1, env_before);
        assert_eq!(
            fs::read_to_string(&config_changed.config)?,
            "{\"someone\": \"else\"}"
        );
        assert!(config_changed.leftovers().is_empty());
    }
    Ok(())
}

#[test]
fn the_marker_and_backups_are_owner_only_while_the_move_is_open() -> TestResult {
    // Given
    let fixture = Fixture::new(true)?;

    // When
    apply_secrets_migration_observed(
        &fixture.config,
        &no_env(),
        &mut stop_at(SecretsApplyStep::ConfigWritten),
    )
    .expect_err("stopped after the config write");

    // Then
    let marker = fs::read_to_string(fixture.marker())?;
    assert_no_values(&marker);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let [config_backup, env_file_backup] = fixture.backups();
        for path in [
            fixture.marker(),
            config_backup,
            env_file_backup,
            fixture.env_file.clone(),
        ] {
            let mode = fs::metadata(&path)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", path.display());
        }
    }
    Ok(())
}

#[test]
fn the_schema_migration_runs_first_and_its_result_is_what_moves() -> TestResult {
    // Given
    let fixture = Fixture::with_config(
        json!({
            "agents": {"defaults": {"sessionTtlMinutes": 5}},
            "channels": {"telegram": {"token": DISCORD}}
        }),
        false,
    )?;

    // When
    let dry = dry_run_secrets_migration(&fixture.config, &no_env())?;
    apply_config_migration(&fixture.config)?;
    let applied = apply_secrets_migration(&fixture.config, &no_env())?;

    // Then
    assert_eq!(dry.moved, applied.moved);
    let config = fs::read_to_string(&fixture.config)?;
    assert!(config.contains("idleCompactAfterMinutes"), "{config}");
    assert!(
        config.contains("${SHACS_CHANNELS_TELEGRAM_TOKEN}"),
        "{config}"
    );
    assert!(!config_migration_marker_path(&fixture.config).exists());
    assert_no_values(&config);
    Ok(())
}
