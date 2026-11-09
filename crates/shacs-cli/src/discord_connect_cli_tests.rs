//! Spec 052 PRD 010 acceptance 5 and 6: argument and token-field rules, and
//! each compare-and-write failure leaves the files as the other writer left
//! them and names the file that changed. PRD 050 acceptance 1–6: setting
//! options, what they write and what the runtime then reads, the stops for
//! values the runtime would not read, and the `--status` lines.

use super::{
    change_settings, classify_token_field, connect, discord_config, status, Channels, Connection,
    DiscordConnectAction, DiscordConnectOptions, DiscordSettings, ExistingValue, GroupPolicy,
    TokenField, Users, WriteStep, ANYONE_WARNING,
};
use crate::{
    channel_enabled_from_plugins, discord_runtime_config, parse_cli_args, ChannelsCommand,
    CliCommand, CliError, DiscordChannelFilter, DiscordGroupPolicy, DiscordTransportMode,
};
use serde_json::{json, Value};
use shacs_config::EnvSource;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn Error>>;

const NAME: &str = "SHACS_CHANNELS_DISCORD_TOKEN";
const TOKEN: &str = "spec052-unit-discord-canary";

struct NoEnvironment;

impl EnvSource for NoEnvironment {
    fn var(&self, _name: &str) -> Option<String> {
        None
    }
}

/// A process environment holding only these variables.
struct Environment(&'static [(&'static str, &'static str)]);

impl EnvSource for Environment {
    fn var(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    }
}

fn placeholder() -> String {
    format!("${{{NAME}}}")
}

fn parsed(args: &[&str]) -> Result<CliCommand, CliError> {
    let mut all = vec!["channels", "connect"];
    all.extend_from_slice(args);
    parse_cli_args(all)
}

fn action(args: &[&str]) -> Result<DiscordConnectAction, String> {
    match parsed(args) {
        Ok(CliCommand::Channels(ChannelsCommand::Connect(DiscordConnectOptions {
            action,
            ..
        }))) => Ok(action),
        Ok(other) => Err(format!("not a connect command: {other:?}")),
        Err(error) => Err(error.to_string()),
    }
}

#[test]
fn arguments_and_token_fields_follow_the_plan() {
    // Given
    let rejected = [
        &["discord"][..],
        &["discord", "--status", "--token-stdin"],
        &["discord", "--status", "--replace"],
        &["discord", "--token-stdin", "--allow-from", "  "],
        &["slack", "--token-stdin"],
    ];
    let connect_args = [
        "discord",
        "--token-stdin",
        "--allow-from",
        " 111 ",
        "--allow-from",
        "222",
        "--replace",
    ];
    let placeholder = placeholder();
    let token_fields = [
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

    // When
    let rejected_results: Vec<_> = rejected.iter().map(|args| parsed(args)).collect();
    let connect_options = action(&connect_args);
    let status_options = action(&["discord", "--status"]);
    let classified: Vec<_> = token_fields
        .iter()
        .map(|(value, _)| classify_token_field(value.as_ref(), NAME))
        .collect();

    // Then
    for (args, result) in rejected.iter().zip(&rejected_results) {
        assert!(result.is_err(), "{args:?}");
    }
    assert_eq!(
        connect_options,
        Ok(DiscordConnectAction::Connect {
            settings: DiscordSettings {
                users: Some(Users::Only(vec!["111".to_owned(), "222".to_owned()])),
                ..DiscordSettings::default()
            },
            existing: ExistingValue::Replace,
        })
    );
    assert_eq!(status_options, Ok(DiscordConnectAction::Status));
    for ((value, expected), field) in token_fields.iter().zip(classified) {
        assert_eq!(&field, expected, "{value:?}");
    }
}

#[test]
fn setting_options_parse_into_one_action() {
    // Given
    let settings = |settings: DiscordSettings| Ok(DiscordConnectAction::Settings(settings));
    let only = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
    let cases: Vec<(&[&str], Result<DiscordConnectAction, String>)> = vec![
        (
            &[
                "discord",
                "--allow-from",
                " 1 ",
                "--allow-from",
                "1",
                "--allow-from",
                "2",
            ],
            settings(DiscordSettings {
                users: Some(Users::Only(only(&["1", "2"]))),
                ..DiscordSettings::default()
            }),
        ),
        (
            &["discord", "--allow-anyone"],
            settings(DiscordSettings {
                users: Some(Users::Anyone),
                ..DiscordSettings::default()
            }),
        ),
        (
            &["discord", "--allow-channel", "3", "--allow-channel", " 4"],
            settings(DiscordSettings {
                channels: Some(Channels::Only(only(&["3", "4"]))),
                ..DiscordSettings::default()
            }),
        ),
        (
            &["discord", "--all-channels"],
            settings(DiscordSettings {
                channels: Some(Channels::All),
                ..DiscordSettings::default()
            }),
        ),
        (
            &[
                "discord",
                "--group-policy",
                "open",
                "--group-policy",
                "open",
            ],
            settings(DiscordSettings {
                group_policy: Some(GroupPolicy::Open),
                ..DiscordSettings::default()
            }),
        ),
        (
            &["discord", "--enable"],
            settings(DiscordSettings {
                enabled: Some(true),
                ..DiscordSettings::default()
            }),
        ),
        (
            &["discord", "--disable"],
            settings(DiscordSettings {
                enabled: Some(false),
                ..DiscordSettings::default()
            }),
        ),
        (
            &[
                "discord",
                "--token-stdin",
                "--enable",
                "--allow-channel",
                "9",
                "--group-policy",
                "mention",
            ],
            Ok(DiscordConnectAction::Connect {
                settings: DiscordSettings {
                    channels: Some(Channels::Only(only(&["9"]))),
                    group_policy: Some(GroupPolicy::Mention),
                    enabled: Some(true),
                    ..DiscordSettings::default()
                },
                existing: ExistingValue::Keep,
            }),
        ),
    ];
    let rejected = [
        &["discord", "--allow-from", "1", "--allow-anyone"][..],
        &["discord", "--allow-channel", "3", "--all-channels"],
        &["discord", "--allow-channel", " "],
        &["discord", "--enable", "--disable"],
        &["discord", "--token-stdin", "--disable"],
        &[
            "discord",
            "--group-policy",
            "open",
            "--group-policy",
            "mention",
        ],
        &["discord", "--group-policy", "sometimes"],
        &["discord", "--group-policy"],
        &["discord", "--replace"],
        &["discord", "--status", "--enable"],
        &["discord", "--status", "--all-channels"],
    ];

    for (args, expected) in cases {
        // When
        let result = action(args);

        // Then
        assert_eq!(result, expected, "{args:?}");
    }
    for args in rejected {
        // When
        let result = action(args);

        // Then
        assert!(result.is_err(), "{args:?}");
    }
}

fn plugins(config: &Value) -> BTreeMap<String, Value> {
    config["channels"]
        .as_object()
        .map(|channels| channels.clone().into_iter().collect())
        .unwrap_or_default()
}

#[test]
fn settings_apply_only_their_fields_and_the_runtime_reads_them() -> TestResult {
    // Given: fields the runtime reads only after the written ones, or not at
    // all, which must stay as they are.
    let kept = json!({
        "allowedSenders": ["9"],
        "allow_from": ["9"],
        "allowed_senders": ["9"],
        "channel_ids": ["8"],
        "allowed_channel_ids": ["8"],
        "allow_channels": ["8"],
        "group_policy": "open",
        "streaming": true,
        "replyInThread": false
    });
    let mut discord = json!({
        "enabled": true,
        "token": placeholder(),
        "allowFrom": ["1"],
        "allowChannels": [],
        "groupPolicy": "mention"
    });
    if let (Some(discord), Some(kept)) = (discord.as_object_mut(), kept.as_object()) {
        discord.extend(kept.clone());
    }
    let base = json!({"other": 1, "channels": {"discord": discord}});
    let back_to_base = |settings: &DiscordSettings| DiscordSettings {
        users: settings
            .users
            .as_ref()
            .map(|_| Users::Only(vec!["1".to_owned()])),
        channels: settings.channels.as_ref().map(|_| Channels::All),
        group_policy: settings.group_policy.map(|_| GroupPolicy::Mention),
        enabled: settings.enabled.map(|_| true),
    };
    let cases = [
        (
            DiscordSettings {
                users: Some(Users::Anyone),
                ..DiscordSettings::default()
            },
            vec!["channels.discord.allowFrom"],
        ),
        (
            DiscordSettings {
                channels: Some(Channels::Only(vec!["3".to_owned(), "4".to_owned()])),
                group_policy: Some(GroupPolicy::Open),
                ..DiscordSettings::default()
            },
            vec![
                "channels.discord.allowChannels",
                "channels.discord.groupPolicy",
            ],
        ),
        (
            DiscordSettings {
                enabled: Some(false),
                ..DiscordSettings::default()
            },
            vec!["channels.discord.enabled"],
        ),
        (
            DiscordSettings {
                users: Some(Users::Only(vec!["1".to_owned()])),
                channels: Some(Channels::All),
                group_policy: Some(GroupPolicy::Mention),
                enabled: Some(true),
            },
            Vec::new(),
        ),
    ];
    for (settings, expected) in cases {
        let token_names: &[Option<&str>] = if settings.enabled == Some(false) {
            &[None]
        } else {
            &[None, Some(NAME)]
        };
        for &token_name in token_names {
            // When
            let (updated, changed) = discord_config(&base, token_name, &settings)?;
            let (reverted, changed_back) =
                discord_config(&updated, token_name, &back_to_base(&settings))?;

            // Then
            let case = format!("{settings:?} with token name {token_name:?}");
            assert_eq!(changed, expected, "{case}");
            assert_eq!(changed_back, expected, "{case}");
            assert_eq!(reverted, base, "{case}");
            assert_eq!(updated["other"], json!(1), "{case}");
            for (field, value) in kept.as_object().into_iter().flatten() {
                assert_eq!(
                    &updated["channels"]["discord"][field], value,
                    "{case}: {field}"
                );
            }
            let runtime = discord_runtime_config(&plugins(&updated)).ok_or("runtime config")?;
            let enabled = channel_enabled_from_plugins(&plugins(&updated), "discord", false);
            assert_eq!(enabled, settings.enabled.unwrap_or(true), "{case}");
            match &settings.users {
                Some(Users::Anyone) => assert!(runtime.allowed_senders.is_empty(), "{case}"),
                _ => assert_eq!(runtime.allowed_senders, ["1"], "{case}"),
            }
            match &settings.channels {
                Some(Channels::Only(ids)) => assert_eq!(
                    runtime.channel_filter,
                    DiscordChannelFilter::Only(ids.clone()),
                    "{case}"
                ),
                _ => assert_eq!(
                    runtime.channel_filter,
                    DiscordChannelFilter::AllVisible,
                    "{case}"
                ),
            }
            let expected_policy = match settings.group_policy {
                Some(GroupPolicy::Open) => DiscordGroupPolicy::Open,
                _ => DiscordGroupPolicy::Mention,
            };
            assert_eq!(runtime.group_policy, expected_policy, "{case}");
            assert_eq!(runtime.transport, DiscordTransportMode::Gateway, "{case}");
        }
    }
    Ok(())
}

#[test]
fn connecting_without_a_config_adds_the_token_name_and_turns_discord_on() -> TestResult {
    // Given
    let settings = DiscordSettings {
        users: Some(Users::Only(vec!["5".to_owned()])),
        ..DiscordSettings::default()
    };

    // When
    let (connected, changed) = discord_config(&json!({}), Some(NAME), &settings)?;

    // Then
    assert_eq!(
        changed,
        [
            "channels.discord.enabled",
            "channels.discord.token",
            "channels.discord.allowFrom"
        ]
    );
    assert_eq!(
        connected,
        json!({"channels": {"discord": {
            "enabled": true,
            "token": placeholder(),
            "allowFrom": ["5"]
        }}})
    );
    Ok(())
}

/// A data folder with `discord` as `channels.discord` and the token saved.
fn data_with(discord: Value) -> Result<(tempfile::TempDir, PathBuf), Box<dyn Error>> {
    data_with_env_file(discord, json!({NAME: TOKEN}))
}

fn data_with_env_file(
    discord: Value,
    env_file: Value,
) -> Result<(tempfile::TempDir, PathBuf), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let config_path = root.path().join("config.json");
    write_json(&config_path, &json!({"channels": {"discord": discord}}))?;
    write_json(&root.path().join(".env.json"), &env_file)?;
    Ok((root, config_path))
}

fn file_bytes(config_path: &Path) -> Result<(Vec<u8>, Vec<u8>), Box<dyn Error>> {
    let env_path = config_path.with_file_name(".env.json");
    Ok((fs::read(config_path)?, fs::read(env_path)?))
}

fn written_discord(config_path: &Path) -> Result<Value, Box<dyn Error>> {
    let config: Value = serde_json::from_slice(&fs::read(config_path)?)?;
    Ok(config["channels"]["discord"].clone())
}

struct Outcome {
    result: Result<String, CliError>,
    files_kept: bool,
    token_read: bool,
    discord_after: Value,
}

/// Runs the settings both ways: without a token, and connecting with one.
fn both_actions(
    discord: &Value,
    settings: &DiscordSettings,
) -> Result<[Outcome; 2], Box<dyn Error>> {
    let (_settings_root, settings_config) = data_with(discord.clone())?;
    let before = file_bytes(&settings_config)?;
    let changed = change_settings(&settings_config, settings, &NoEnvironment, &mut |_| {});
    let settings_outcome = Outcome {
        result: changed,
        files_kept: file_bytes(&settings_config)? == before,
        token_read: false,
        discord_after: written_discord(&settings_config)?,
    };

    let (_connect_root, connect_config) = data_with(discord.clone())?;
    let before = file_bytes(&connect_config)?;
    let token_reads = Cell::new(0);
    let connected = connect(
        &connect_config,
        &Connection {
            settings,
            existing: ExistingValue::Keep,
        },
        &|| {
            token_reads.set(token_reads.get() + 1);
            Ok(TOKEN.to_owned())
        },
        &NoEnvironment,
        &mut |_| {},
    );
    let connect_outcome = Outcome {
        result: connected,
        files_kept: file_bytes(&connect_config)? == before,
        token_read: token_reads.get() > 0,
        discord_after: written_discord(&connect_config)?,
    };
    Ok([settings_outcome, connect_outcome])
}

#[test]
fn a_channel_field_the_runtime_reads_first_stops_both_actions() -> TestResult {
    let channels = DiscordSettings {
        channels: Some(Channels::Only(vec!["3".to_owned()])),
        ..DiscordSettings::default()
    };
    let cases = [
        ("channelIds", json!([]), true),
        ("allowedChannelIds", json!(["7"]), true),
        ("defaultChannelId", json!("7"), true),
        ("default_channel_id", json!("7"), true),
        ("channelId", json!("7"), true),
        ("channel_id", json!("7"), true),
        ("defaultChannelId", json!("  "), false),
        ("channel_ids", json!(["7"]), false),
        ("allowed_channel_ids", json!(["7"]), false),
        ("allow_channels", json!(["7"]), false),
    ];
    for (field, value, stops) in cases {
        // Given
        let mut discord = json!({"enabled": true, "token": placeholder()});
        discord[field] = value.clone();

        // When
        let results = both_actions(&discord, &channels)?;

        // Then
        for (index, outcome) in results.into_iter().enumerate() {
            if stops {
                let message = outcome
                    .result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                assert!(
                    message.contains("discord connect stopped")
                        && message.contains(&format!("channels.discord.{field}")),
                    "{field} #{index}: {message}"
                );
                assert!(outcome.files_kept, "{field} #{index}");
                assert!(!outcome.token_read, "{field} #{index}");
            } else {
                assert!(
                    outcome.result.is_ok(),
                    "{field} #{index}: {:?}",
                    outcome.result
                );
                assert_eq!(outcome.discord_after[field], value, "{field} #{index}");
                assert_eq!(
                    outcome.discord_after["allowChannels"],
                    json!(["3"]),
                    "{field} #{index}"
                );
            }
        }
    }

    // Given
    let (_root, config_path) = data_with(json!({
        "enabled": true,
        "token": placeholder(),
        "channel_ids": ["7"]
    }))?;

    // When
    change_settings(&config_path, &channels, &NoEnvironment, &mut |_| {})?;

    // Then: the runtime reads the written allowChannels before the alias.
    let written: Value = serde_json::from_slice(&fs::read(&config_path)?)?;
    let runtime = discord_runtime_config(&plugins(&written)).ok_or("runtime config")?;
    assert_eq!(
        runtime.channel_filter,
        DiscordChannelFilter::Only(vec!["3".to_owned()])
    );
    assert_eq!(runtime.transport, DiscordTransportMode::Gateway);

    // Given: a call that leaves the channels alone.
    let users = DiscordSettings {
        users: Some(Users::Only(vec!["1".to_owned()])),
        ..DiscordSettings::default()
    };

    // When
    let results = both_actions(
        &json!({"enabled": true, "token": placeholder(), "channelIds": []}),
        &users,
    )?;

    // Then
    for outcome in results {
        assert!(outcome.result.is_ok(), "{:?}", outcome.result);
        assert_eq!(outcome.discord_after["channelIds"], json!([]));
    }
    Ok(())
}

#[test]
fn a_group_policy_stops_where_the_runtime_polls_channels() -> TestResult {
    let open = |channels: Option<Channels>| DiscordSettings {
        channels,
        group_policy: Some(GroupPolicy::Open),
        ..DiscordSettings::default()
    };
    let cases = [
        ("channel_ids", json!(["7"]), open(None), true),
        (
            "channel_ids",
            json!(["7"]),
            open(Some(Channels::Only(vec!["3".to_owned()]))),
            false,
        ),
        (
            "channel_ids",
            json!(["7"]),
            DiscordSettings {
                users: Some(Users::Only(vec!["1".to_owned()])),
                ..DiscordSettings::default()
            },
            false,
        ),
        ("channelIds", json!(["7"]), open(None), true),
        ("defaultChannelId", json!("7"), open(None), true),
        ("default_channel_id", json!("7"), open(None), true),
    ];
    for (field, value, settings, stops) in cases {
        // Given
        let mut discord = json!({"enabled": true, "token": placeholder()});
        discord[field] = value.clone();

        // When
        let results = both_actions(&discord, &settings)?;

        // Then
        for (index, outcome) in results.into_iter().enumerate() {
            if stops {
                let message = outcome
                    .result
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_default();
                assert!(
                    message.contains("does not apply groupPolicy"),
                    "{field} #{index}: {message}"
                );
                assert!(outcome.files_kept, "{field} #{index}");
                assert!(!outcome.token_read, "{field} #{index}");
            } else {
                assert!(
                    outcome.result.is_ok(),
                    "{field} #{index}: {:?}",
                    outcome.result
                );
                assert_eq!(outcome.discord_after[field], value, "{field} #{index}");
            }
        }
    }
    Ok(())
}

#[test]
fn a_config_changed_before_the_settings_write_is_left_alone() -> TestResult {
    // Given
    let (root, config_path) = data_with(json!({"enabled": true, "token": placeholder()}))?;
    let env_before = fs::read(root.path().join(".env.json"))?;

    // When
    let error = change_settings(
        &config_path,
        &DiscordSettings {
            group_policy: Some(GroupPolicy::Open),
            ..DiscordSettings::default()
        },
        &NoEnvironment,
        &mut |step| {
            if step == WriteStep::BeforeConfig {
                fs::write(&config_path, "{\"changed\": true}").expect("writable");
            }
        },
    )
    .expect_err("the changed config stops the write");

    // Then
    assert!(error.to_string().contains(
        "config.json changed while this command ran; nothing was written. Run it again."
    ));
    assert_eq!(fs::read_to_string(&config_path)?, "{\"changed\": true}");
    assert_eq!(fs::read(root.path().join(".env.json"))?, env_before);
    Ok(())
}

#[test]
fn settings_change_only_where_a_token_is_saved() -> TestResult {
    // Given
    let enable = DiscordSettings {
        enabled: Some(true),
        ..DiscordSettings::default()
    };
    let saved = || json!({NAME: TOKEN});
    let cases = [
        (
            "a token in botToken",
            json!({"enabled": true, "botToken": placeholder()}),
            saved(),
            Environment(&[]),
            true,
        ),
        (
            "a token in bot_token",
            json!({"enabled": true, "bot_token": placeholder()}),
            saved(),
            Environment(&[]),
            true,
        ),
        (
            "an unresolved botToken before a good token",
            json!({"enabled": true, "botToken": "${SPEC052_UNIT_MISSING}", "token": placeholder()}),
            saved(),
            Environment(&[]),
            false,
        ),
        (
            "turned off with a token",
            json!({"enabled": false, "token": placeholder()}),
            saved(),
            Environment(&[]),
            true,
        ),
        (
            "no token",
            json!({"enabled": true, "token": ""}),
            saved(),
            Environment(&[]),
            false,
        ),
        (
            "a token only in the process environment",
            json!({"enabled": false, "token": placeholder()}),
            json!({}),
            Environment(&[(NAME, TOKEN)]),
            true,
        ),
        (
            "a token name that resolves nowhere",
            json!({"enabled": false, "token": placeholder()}),
            json!({}),
            Environment(&[]),
            false,
        ),
    ];
    for (name, discord, env_file, environment, applies) in cases {
        // Given
        let (_root, config_path) = data_with_env_file(discord, env_file)?;
        let before = file_bytes(&config_path)?;

        // When
        let result = change_settings(&config_path, &enable, &environment, &mut |_| {});

        // Then
        if applies {
            assert!(result.is_ok(), "{name}: {result:?}");
            assert_eq!(file_bytes(&config_path)?.1, before.1, "{name}");
        } else {
            let message = result
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(
                message.contains("discord is not connected yet"),
                "{name}: {message}"
            );
            assert_eq!(file_bytes(&config_path)?, before, "{name}");
        }
    }

    // Given: no config file, so no token either.
    let root = tempfile::tempdir()?;

    // When
    let missing = change_settings(
        &root.path().join("config.json"),
        &enable,
        &NoEnvironment,
        &mut |_| {},
    );

    // Then
    assert!(missing.is_err());
    assert!(!root.path().join("config.json").exists());
    assert!(!root.path().join(".env.json").exists());
    Ok(())
}

#[test]
fn the_warning_follows_the_sender_list_the_runtime_reads() -> TestResult {
    let cases = [
        (json!({"allowFrom": []}), true),
        (json!({"allowFrom": ["*"]}), true),
        (json!({"allowedSenders": ["*"]}), true),
        (json!({"allowFrom": ["1", "2"]}), false),
    ];
    for (senders, warns) in cases {
        // Given
        let mut discord = json!({"enabled": true, "token": placeholder()});
        if let (Some(discord), Some(senders)) = (discord.as_object_mut(), senders.as_object()) {
            discord.extend(senders.clone());
        }
        let (_root, config_path) = data_with(discord)?;

        // When
        let output = change_settings(
            &config_path,
            &DiscordSettings {
                group_policy: Some(GroupPolicy::Mention),
                ..DiscordSettings::default()
            },
            &NoEnvironment,
            &mut |_| {},
        )?;

        // Then
        assert_eq!(
            output.lines().any(|line| line == ANYONE_WARNING),
            warns,
            "{senders}: {output}"
        );
        assert_eq!(output.lines().next(), Some("discord: settings updated"));
    }
    Ok(())
}

#[test]
fn status_lines_follow_what_the_runtime_reads() -> TestResult {
    // Given
    let settings = |users: &str, channels: &str, replies: &str| {
        format!("users: {users}\nchannels: {channels}\nserver channels: {replies}")
    };
    let connected = |field: &str, rest: String| {
        format!("discord: connected\ntoken: channels.discord.{field}\n{rest}")
    };
    let cases = [
        (
            json!({"enabled": true, "token": placeholder(), "allowFrom": ["1", "2"], "allowChannels": []}),
            connected("token", settings("1, 2", "all", "when mentioned")),
        ),
        (
            json!({"enabled": true, "token": placeholder(), "allowFrom": [], "allowChannels": ["*"], "groupPolicy": "open"}),
            connected("token", settings("anyone", "all", "every message")),
        ),
        (
            json!({"enabled": true, "token": placeholder(), "allowFrom": ["*"], "allowChannels": ["3", "4"]}),
            connected("token", settings("anyone", "3, 4", "when mentioned")),
        ),
        (
            json!({"enabled": true, "botToken": placeholder(), "token": ""}),
            connected("botToken", settings("anyone", "all", "when mentioned")),
        ),
        (
            json!({"enabled": true, "bot_token": placeholder()}),
            connected("bot_token", settings("anyone", "all", "when mentioned")),
        ),
        (
            json!({"enabled": false, "token": placeholder()}),
            format!(
                "discord: not connected\nreason: channels.discord.enabled is not true\ntoken: channels.discord.token\n{}",
                settings("anyone", "all", "when mentioned")
            ),
        ),
        (
            json!({"enabled": false, "token": ""}),
            "discord: not connected\nreason: channels.discord.enabled is not true".to_owned(),
        ),
        (
            json!({"enabled": true, "token": ""}),
            "discord: not connected\nreason: channels.discord has no token in botToken, bot_token or token".to_owned(),
        ),
        (
            json!({"enabled": true, "token": placeholder(), "channel_ids": ["*", "7"]}),
            connected(
                "token",
                settings(
                    "anyone",
                    "*, 7",
                    "every message (channelIds polling does not apply groupPolicy)",
                ),
            ),
        ),
    ];
    let token_only_in_environment = [
        (
            json!({"enabled": true, "token": placeholder(), "allowFrom": ["1"], "allowChannels": ["3"]}),
            Environment(&[(NAME, TOKEN)]),
            connected("token", settings("1", "3", "when mentioned")),
        ),
        (
            json!({"enabled": true, "botToken": "${SPEC052_UNIT_ENV_TOKEN}", "groupPolicy": "open"}),
            Environment(&[("SPEC052_UNIT_ENV_TOKEN", TOKEN)]),
            connected("botToken", settings("anyone", "all", "every message")),
        ),
        (
            json!({"enabled": false, "token": placeholder(), "allowFrom": ["2"]}),
            Environment(&[(NAME, TOKEN)]),
            format!(
                "discord: not connected\nreason: channels.discord.enabled is not true\ntoken: channels.discord.token\n{}",
                settings("2", "all", "when mentioned")
            ),
        ),
    ];

    for (discord, expected) in cases {
        // Given
        let (_root, config_path) = data_with(discord.clone())?;

        // When
        let output = status(&config_path, &NoEnvironment)?;

        // Then
        assert_eq!(output, expected, "{discord}");
    }
    for (discord, environment, expected) in token_only_in_environment {
        // Given
        let (_root, config_path) = data_with_env_file(discord.clone(), json!({}))?;

        // When
        let output = status(&config_path, &environment)?;

        // Then
        assert_eq!(output, expected, "{discord}");
    }

    // Given: an earlier field that cannot be resolved.
    let (_root, config_path) = data_with(json!({
        "enabled": true,
        "botToken": "${SPEC052_UNIT_MISSING}",
        "token": placeholder()
    }))?;

    // When
    let output = status(&config_path, &NoEnvironment)?;

    // Then: it hides the settings lines.
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 2, "{output}");
    assert_eq!(lines[0], "discord: not connected");
    assert!(
        lines[1].starts_with("reason: channels.discord.botToken:"),
        "{output}"
    );
    Ok(())
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
            settings: &DiscordSettings::default(),
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
