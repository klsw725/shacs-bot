//! Spec 052 PRD 010 acceptance 5 and 6: argument and token-field rules, and
//! each compare-and-write failure leaves the files as the other writer left
//! them and names the file that changed.

use super::{
    classify_token_field, connect, Connection, DiscordConnectAction, DiscordConnectOptions,
    ExistingValue, TokenField, WriteStep,
};
use crate::{parse_cli_args, ChannelsCommand, CliCommand};
use serde_json::{json, Value};
use shacs_config::EnvSource;
use std::error::Error;
use std::fs;
use std::path::Path;

type TestResult = Result<(), Box<dyn Error>>;

const NAME: &str = "SHACS_CHANNELS_DISCORD_TOKEN";
const TOKEN: &str = "spec052-unit-discord-canary";

struct NoEnvironment;

impl EnvSource for NoEnvironment {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }
}

#[test]
fn arguments_and_token_fields_follow_the_plan() {
    let parsed = |args: &[&str]| {
        let mut all = vec!["channels", "connect"];
        all.extend_from_slice(args);
        parse_cli_args(all)
    };
    for rejected in [
        &["discord"][..],
        &["discord", "--status", "--token-stdin"],
        &["discord", "--status", "--replace"],
        &["discord", "--token-stdin", "--allow-from", "  "],
        &["slack", "--token-stdin"],
    ] {
        assert!(parsed(rejected).is_err(), "{rejected:?}");
    }
    let connect_options = parsed(&[
        "discord",
        "--token-stdin",
        "--allow-from",
        " 111 ",
        "--allow-from",
        "222",
        "--replace",
    ]);
    assert!(matches!(
        connect_options,
        Ok(CliCommand::Channels(ChannelsCommand::Connect(DiscordConnectOptions {
            action: DiscordConnectAction::Connect { ref allow_from, existing: ExistingValue::Replace },
            ..
        }))) if allow_from == &["111", "222"]
    ));
    assert!(matches!(
        parsed(&["discord", "--status"]),
        Ok(CliCommand::Channels(ChannelsCommand::Connect(
            DiscordConnectOptions {
                action: DiscordConnectAction::Status,
                ..
            }
        )))
    ));

    let placeholder = format!("${{{NAME}}}");
    let cases = [
        (None, TokenField::Open),
        (Some(Value::Null), TokenField::Open),
        (Some(json!("  ")), TokenField::Open),
        (Some(json!(placeholder)), TokenField::Open),
        (Some(json!("plain-token")), TokenField::Literal),
        (
            Some(json!("${OTHER_NAME}")),
            TokenField::OtherName("OTHER_NAME".to_owned()),
        ),
        (Some(json!("Bot ${OTHER_NAME}")), TokenField::Unusual),
        (Some(json!(format!(" {placeholder} "))), TokenField::Unusual),
        (Some(json!(42)), TokenField::Unusual),
    ];
    for (value, expected) in cases {
        assert_eq!(
            classify_token_field(value.as_ref(), NAME),
            expected,
            "{value:?}"
        );
    }
}

/// Connects with a config and `.env.json` in `root`, changing files with
/// `change` at `step`.
fn connect_with_change(
    root: &Path,
    step: WriteStep,
    change: &dyn Fn(&Path),
) -> Result<String, crate::CliError> {
    let config_path = root.join("config.json");
    connect(
        &config_path,
        &Connection {
            allow_from: &[],
            existing: ExistingValue::Keep,
        },
        &|| Ok(TOKEN.to_owned()),
        &NoEnvironment,
        &mut |current| {
            if current == step {
                change(root);
            }
        },
    )
}

fn write_json(path: &Path, value: &Value) -> TestResult {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

#[test]
fn a_changed_env_file_before_its_write_leaves_both_files_alone() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    write_json(&root.path().join("config.json"), &json!({"channels": {}}))?;
    write_json(&root.path().join(".env.json"), &json!({"OTHER": "first"}))?;
    let config_before = fs::read(root.path().join("config.json"))?;

    // When
    let error = connect_with_change(root.path(), WriteStep::BeforeEnvFile, &|root| {
        fs::write(root.join(".env.json"), "{\"OTHER\": \"second\"}").expect("writable");
    })
    .expect_err("the changed .env.json stops the write");

    // Then
    let message = error.to_string();
    assert!(message.contains(".env.json changed while this command ran; nothing was written"));
    assert_eq!(
        fs::read_to_string(root.path().join(".env.json"))?,
        "{\"OTHER\": \"second\"}"
    );
    assert_eq!(fs::read(root.path().join("config.json"))?, config_before);
    Ok(())
}

#[test]
fn a_changed_config_before_its_write_keeps_the_token_and_the_other_config() -> TestResult {
    // Given
    let root = tempfile::tempdir()?;
    write_json(&root.path().join("config.json"), &json!({"channels": {}}))?;

    // When
    let error = connect_with_change(root.path(), WriteStep::BeforeConfig, &|root| {
        fs::write(root.join("config.json"), "{\"changed\": true}").expect("writable");
    })
    .expect_err("the changed config stops its write");

    // Then
    let message = error.to_string();
    assert!(
        message.contains("config.json changed while this command ran; .env.json holds the token")
    );
    let env_file: Value = serde_json::from_slice(&fs::read(root.path().join(".env.json"))?)?;
    assert_eq!(env_file, json!({NAME: TOKEN}));
    assert_eq!(
        fs::read_to_string(root.path().join("config.json"))?,
        "{\"changed\": true}"
    );
    Ok(())
}

#[test]
fn a_name_removed_from_an_unchanged_env_file_stops_the_config_write() -> TestResult {
    // Given: .env.json already holds the token, so only config would change.
    let root = tempfile::tempdir()?;
    write_json(&root.path().join("config.json"), &json!({"channels": {}}))?;
    write_json(&root.path().join(".env.json"), &json!({NAME: TOKEN}))?;
    let config_before = fs::read(root.path().join("config.json"))?;

    // When
    let error = connect_with_change(root.path(), WriteStep::BeforeConfig, &|root| {
        fs::write(root.join(".env.json"), "{}").expect("writable");
    })
    .expect_err("the removed name stops the config write");

    // Then
    let message = error.to_string();
    assert!(message.contains(".env.json changed while this command ran; nothing was written"));
    assert_eq!(fs::read(root.path().join("config.json"))?, config_before);
    assert_eq!(fs::read_to_string(root.path().join(".env.json"))?, "{}");
    Ok(())
}
