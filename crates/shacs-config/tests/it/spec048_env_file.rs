//! Spec 048 PRD 010 acceptance 1 and 2: `${NAME}` resolves from a non-empty
//! process value, then `.env.json`, and a malformed `.env.json` fails the
//! resolving read without showing its text.

use serde_json::json;
use shacs_config::{
    load_config_with_env, load_config_with_env_or_default, ConfigError, LoadOptions,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

const CANARY: &str = "prd010-env-canary-7311";

fn options(config_path: &Path, resolve_env: bool) -> LoadOptions {
    LoadOptions {
        config_path: Some(config_path.to_path_buf()),
        workspace_override: None,
        resolve_env,
        write_back_migrations: false,
    }
}

fn write_config(root: &Path, value: serde_json::Value) -> Result<PathBuf, Box<dyn Error>> {
    let config_path = root.join("config.json");
    fs::write(&config_path, serde_json::to_vec_pretty(&value)?)?;
    Ok(config_path)
}

#[test]
fn names_resolve_from_a_non_empty_process_value_then_the_env_file() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = write_config(
        root.path(),
        json!({"providers": {
            "a": {"apiKey": "${SHACS_A}"},
            "b": {"apiKey": "${SHACS_B}"},
            "c": {"apiKey": "${SHACS_C}"}
        }}),
    )?;
    fs::write(
        root.path().join(".env.json"),
        r#"{"SHACS_A": "file-a", "SHACS_B": "file-b"}"#,
    )?;
    let process = BTreeMap::from([
        ("SHACS_A".to_owned(), "process-a".to_owned()),
        ("SHACS_B".to_owned(), String::new()),
        ("SHACS_C".to_owned(), String::new()),
    ]);

    // When
    let bundle = load_config_with_env(options(&config_path, true), &process)?;

    // Then
    let key = |name: &str| bundle.config.providers[name].api_key.clone();
    assert_eq!(key("a").as_deref(), Some("process-a"));
    assert_eq!(key("b").as_deref(), Some("file-b"));
    assert_eq!(key("c").as_deref(), Some(""));
    Ok(())
}

#[test]
fn a_name_in_neither_place_fails_with_only_its_name() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = write_config(
        root.path(),
        json!({"providers": {"a": {"apiKey": "${SHACS_MISSING}"}}}),
    )?;
    fs::write(
        root.path().join(".env.json"),
        format!(r#"{{"SHACS_OTHER": "{CANARY}"}}"#),
    )?;

    // When
    let error = load_config_with_env(options(&config_path, true), &BTreeMap::new())
        .expect_err("a missing name fails the read");

    // Then
    assert!(matches!(error, ConfigError::Env(_)), "{error:?}");
    let shown = error.to_string();
    assert!(shown.contains("SHACS_MISSING"), "{shown}");
    assert!(!shown.contains(CANARY), "{shown}");
    Ok(())
}

#[test]
fn a_malformed_env_file_fails_resolving_reads_only_and_never_falls_back() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = write_config(root.path(), json!({"permissions": {"deny": ["exec"]}}))?;
    fs::write(
        root.path().join(".env.json"),
        format!(r#"{{"lower_case": "{CANARY}"}}"#),
    )?;

    // When
    let unresolved = load_config_with_env(options(&config_path, false), &BTreeMap::new())?;
    let resolved = load_config_with_env(options(&config_path, true), &BTreeMap::new())
        .expect_err("a malformed .env.json fails the resolving read");
    let fallback = load_config_with_env_or_default(options(&config_path, true), &BTreeMap::new())
        .expect_err("no default config stands in for a malformed .env.json");

    // Then
    assert_eq!(unresolved.config.permissions.user_rules.deny, vec!["exec"]);
    for error in [&resolved, &fallback] {
        assert!(matches!(error, ConfigError::EnvFile(_)), "{error:?}");
        let shown = error.to_string();
        assert!(
            !shown.contains(CANARY) && !shown.contains("lower_case"),
            "{shown}"
        );
    }
    Ok(())
}

#[test]
fn env_file_values_reach_only_the_env_blocks_that_name_them() -> TestResult {
    // Given
    let only_in_file = "SHACS_PRD010_ONLY_IN_FILE_4417";
    let root = tempfile::tempdir()?;
    let config_path = write_config(
        root.path(),
        json!({"env": {"GITHUB_TOKEN": "${SHACS_ENV_GITHUB_TOKEN}"}}),
    )?;
    fs::write(
        root.path().join(".env.json"),
        format!(r#"{{"SHACS_ENV_GITHUB_TOKEN": "{CANARY}", "{only_in_file}": "unused"}}"#),
    )?;

    // When
    let bundle = load_config_with_env(options(&config_path, true), &BTreeMap::new())?;

    // Then
    assert_eq!(
        bundle.config.env,
        BTreeMap::from([("GITHUB_TOKEN".to_owned(), CANARY.to_owned())])
    );
    assert!(bundle.config.tools.exec.env.is_empty());
    assert!(std::env::var(only_in_file).is_err());
    assert!(std::env::var("SHACS_ENV_GITHUB_TOKEN").is_err());
    Ok(())
}

#[test]
fn an_unreadable_env_file_fails_the_resolving_read() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    let config_path = write_config(root.path(), json!({}))?;
    fs::create_dir(root.path().join(".env.json"))?;

    // When
    let error = load_config_with_env(options(&config_path, true), &BTreeMap::new())
        .expect_err("a folder named .env.json cannot be read");

    // Then
    assert!(
        matches!(
            error,
            ConfigError::EnvFile(shacs_config::EnvFileProblem::Unreadable(_))
        ),
        "{error:?}"
    );
    Ok(())
}
