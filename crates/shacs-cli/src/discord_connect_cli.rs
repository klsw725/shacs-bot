//! `shacs-bot channels connect discord`: puts the Discord bot token in
//! `.env.json` and leaves only `${NAME}` in config, and `--status`, which
//! reads whether the Discord fields meet the runtime's worker condition
//! (Spec 052 PRD 010 scopes 3 and 5).

use super::{
    allowed_sender_config, channel_enabled_from_plugins, default_config_path,
    discord_runtime_config, guard_config_migration_admission, hidden_input, take_path, take_value,
    ArgParser, ChannelsCommand, CliCommand, CliError, DISCORD_CHANNEL,
};
use serde_json::{Map, Value};
use shacs_config::{
    config_env, env_file_path, interpolate_env_with_source, load_config_with_env, secret_env_name,
    write_config_value_if_unchanged, ConfigError, ConfigWriteError, EnvFile, EnvSource,
    LoadOptions, ProcessEnv,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// The fields the runtime takes the Discord token from, in its order.
const TOKEN_FIELDS: [&str; 3] = ["botToken", "bot_token", "token"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordConnectOptions {
    pub config_path: Option<PathBuf>,
    pub action: DiscordConnectAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscordConnectAction {
    Connect {
        allow_from: Vec<String>,
        existing: ExistingValue,
    },
    Status,
}

/// What to do with a different `.env.json` value for the token's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingValue {
    Keep,
    Replace,
}

pub(super) fn parse(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    match parser.next().as_deref() {
        Some("discord") => {}
        Some("--help" | "-h") => return Ok(CliCommand::Help),
        Some(other) => {
            return Err(CliError::InvalidArguments(format!(
                "channels connect supports only `discord`, not `{other}`"
            )))
        }
        None => {
            return Err(CliError::InvalidArguments(
                "channels connect requires `discord`".to_owned(),
            ))
        }
    }
    let mut config_path = global_config;
    let mut token_stdin = false;
    let mut allow_from = Vec::new();
    let mut existing = ExistingValue::Keep;
    let mut status = false;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--token-stdin" => token_stdin = true,
            "--allow-from" => {
                let id = take_value(&mut parser, &arg)?.trim().to_owned();
                if id.is_empty() {
                    return Err(CliError::InvalidArguments(
                        "--allow-from requires a non-empty user id".to_owned(),
                    ));
                }
                allow_from.push(id);
            }
            "--replace" => existing = ExistingValue::Replace,
            "--status" => status = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown channels connect discord argument `{other}`"
                )))
            }
        }
    }
    let action = if status {
        if token_stdin || existing == ExistingValue::Replace || !allow_from.is_empty() {
            return Err(CliError::InvalidArguments(
                "channels connect discord --status takes no --token-stdin, --allow-from or --replace"
                    .to_owned(),
            ));
        }
        DiscordConnectAction::Status
    } else {
        if !token_stdin {
            return Err(CliError::InvalidArguments(
                "channels connect discord requires --token-stdin (or --status)".to_owned(),
            ));
        }
        DiscordConnectAction::Connect {
            allow_from,
            existing,
        }
    };
    Ok(CliCommand::Channels(ChannelsCommand::Connect(
        DiscordConnectOptions {
            config_path,
            action,
        },
    )))
}

pub(super) fn run(options: DiscordConnectOptions) -> Result<String, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    match options.action {
        DiscordConnectAction::Status => status(&config_path, &ProcessEnv),
        DiscordConnectAction::Connect {
            allow_from,
            existing,
        } => connect(
            &config_path,
            &Connection {
                allow_from: &allow_from,
                existing,
            },
            &|| hidden_input::read_secret("Discord bot token: "),
            &ProcessEnv,
            &mut |_| {},
        ),
    }
}

struct Connection<'a> {
    allow_from: &'a [String],
    existing: ExistingValue,
}

/// The two-file write calls its observer just before each comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteStep {
    BeforeEnvFile,
    BeforeConfig,
}

/// What `channels.discord.token` holds before connecting.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenField {
    /// Absent, `null`, empty, or already `${NAME}`: connecting may go on.
    Open,
    /// A token typed into config.
    Literal,
    /// Exactly another `${NAME}`.
    OtherName(String),
    /// `${` mixed with text, or not text at all.
    Unusual,
}

fn classify_token_field(value: Option<&Value>, name: &str) -> TokenField {
    let text = match value {
        None | Some(Value::Null) => return TokenField::Open,
        Some(Value::String(text)) => text.as_str(),
        Some(_) => return TokenField::Unusual,
    };
    if text.trim().is_empty() || text == placeholder(name) {
        return TokenField::Open;
    }
    let single_name = text
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .filter(|inner| !inner.contains("${") && !inner.contains('}'));
    match single_name {
        Some(other) => TokenField::OtherName(other.to_owned()),
        None if text.contains("${") => TokenField::Unusual,
        None => TokenField::Literal,
    }
}

fn placeholder(name: &str) -> String {
    format!("${{{name}}}")
}

fn connect(
    config_path: &Path,
    connection: &Connection<'_>,
    read_token: &dyn Fn() -> io::Result<String>,
    environment: &dyn EnvSource,
    observe: &mut dyn FnMut(WriteStep),
) -> Result<String, CliError> {
    guard_config_migration_admission(config_path)?;
    let name = secret_env_name(&["channels", "discord", "token"]);
    let config_bytes = read_if_present(config_path)?;
    let config = match &config_bytes {
        Some(bytes) => serde_json::from_slice::<Value>(bytes).map_err(ConfigError::Json)?,
        None => Value::Object(Map::new()),
    };
    let discord = discord_object(&config)?;
    check_token_fields(discord, &name)?;

    let token = read_token()?.trim().to_owned();
    if token.is_empty() {
        return Err(stopped("the Discord bot token is empty"));
    }
    if let Some(value) = environment.var(&name) {
        if !value.is_empty() && value != token {
            return Err(stopped(&format!(
                "the environment has {name} with another value, and the runtime uses it before .env.json; unset it or run where the runtime runs"
            )));
        }
    }

    let env_path = env_file_path(config_path);
    let env_bytes = read_if_present(&env_path)?;
    let env_file = match &env_bytes {
        Some(bytes) => EnvFile::parse(bytes).map_err(ConfigError::EnvFile)?,
        None => EnvFile::default(),
    };
    let env_changed = match env_file.get(&name) {
        None => true,
        Some(saved) if saved == token => false,
        Some(_) if connection.existing == ExistingValue::Replace => true,
        Some(_) => {
            return Err(stopped(&format!(
                ".env.json has {name} with another value; add --replace to replace it"
            )))
        }
    };
    let mut entries: BTreeMap<String, String> = env_file
        .entries()
        .map(|(entry_name, value)| (entry_name.to_owned(), value.to_owned()))
        .collect();
    entries.insert(name.clone(), token);
    let new_env = Value::Object(
        entries
            .into_iter()
            .map(|(entry_name, value)| (entry_name, Value::String(value)))
            .collect(),
    );

    let (new_config, changed_fields) = connected_config(&config, &name, connection.allow_from)?;
    let senders_empty = discord_object(&new_config)?
        .map(allowed_sender_config)
        .unwrap_or_default()
        .is_empty();

    let wrote_env = write_files(
        &Files {
            config_path,
            config_bytes: config_bytes.as_deref(),
            new_config: (!changed_fields.is_empty()).then_some(&new_config),
            env_path: &env_path,
            env_bytes: env_bytes.as_deref(),
            new_env: env_changed.then_some(&new_env),
        },
        observe,
    )?;

    let mut lines = vec!["discord: connected".to_owned()];
    if wrote_env {
        lines.push(format!("updated: .env.json {name}"));
    }
    if !changed_fields.is_empty() {
        lines.push(format!("updated: config {}", changed_fields.join(", ")));
    }
    if !wrote_env && changed_fields.is_empty() {
        lines.push("unchanged".to_owned());
    }
    if senders_empty {
        lines.push(
            "warning: channels.discord has no allowFrom list; anyone the bot can see can use it"
                .to_owned(),
        );
    }
    Ok(lines.join("\n"))
}

fn stopped(reason: &str) -> CliError {
    CliError::Runtime(format!("discord connect stopped: {reason}"))
}

fn read_if_present(path: &Path) -> Result<Option<Vec<u8>>, CliError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// `channels.discord` of `config`, `None` when absent; a value that is not
/// an object where one belongs is an error.
fn discord_object(config: &Value) -> Result<Option<&Map<String, Value>>, CliError> {
    let Some(root) = config.as_object() else {
        return Err(not_object("config"));
    };
    let Some(channels) = root.get("channels") else {
        return Ok(None);
    };
    let Some(channels) = channels.as_object() else {
        return Err(not_object("channels"));
    };
    match channels.get(DISCORD_CHANNEL) {
        None => Ok(None),
        Some(Value::Object(discord)) => Ok(Some(discord)),
        Some(_) => Err(not_object("channels.discord")),
    }
}

fn not_object(field: &str) -> CliError {
    CliError::Config(ConfigError::Invalid(format!(
        "`{field}` must be a JSON object"
    )))
}

fn check_token_fields(discord: Option<&Map<String, Value>>, name: &str) -> Result<(), CliError> {
    let Some(discord) = discord else {
        return Ok(());
    };
    match classify_token_field(discord.get("token"), name) {
        TokenField::Open => {}
        TokenField::Literal => {
            return Err(stopped(
                "channels.discord.token holds a token; move it to .env.json first with `runtime config-migrate --apply`",
            ))
        }
        TokenField::OtherName(other) => {
            return Err(stopped(&format!(
                "channels.discord.token uses ${{{other}}}; change it by hand to {} or remove it",
                placeholder(name)
            )))
        }
        TokenField::Unusual => {
            return Err(stopped(
                "channels.discord.token holds a value this command does not replace; fix it by hand",
            ))
        }
    }
    for field in ["botToken", "bot_token"] {
        let has_value = discord
            .get(field)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if has_value {
            return Err(stopped(&format!(
                "channels.discord.{field} has a value, and the runtime reads it before token; remove it by hand"
            )));
        }
    }
    Ok(())
}

/// `config` with `channels.discord` connected through `${name}`, and the
/// fields that changed.
fn connected_config(
    config: &Value,
    name: &str,
    allow_from: &[String],
) -> Result<(Value, Vec<String>), CliError> {
    let mut updated = config.clone();
    let root = updated
        .as_object_mut()
        .ok_or_else(|| not_object("config"))?;
    let channels = root
        .entry("channels")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| not_object("channels"))?;
    let discord = channels
        .entry(DISCORD_CHANNEL)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| not_object("channels.discord"))?;
    let mut wanted = vec![
        ("enabled", Value::Bool(true)),
        ("token", Value::String(placeholder(name))),
    ];
    if !allow_from.is_empty() {
        let ids: Vec<Value> = allow_from.iter().cloned().map(Value::String).collect();
        wanted.push(("allowFrom", Value::Array(ids)));
    }
    let mut changed = Vec::new();
    for (field, value) in wanted {
        if discord.get(field) != Some(&value) {
            discord.insert(field.to_owned(), value);
            changed.push(format!("channels.discord.{field}"));
        }
    }
    Ok((updated, changed))
}

struct Files<'a> {
    config_path: &'a Path,
    config_bytes: Option<&'a [u8]>,
    new_config: Option<&'a Value>,
    env_path: &'a Path,
    env_bytes: Option<&'a [u8]>,
    new_env: Option<&'a Value>,
}

/// Writes `.env.json`, then config, each only if it still holds the bytes
/// first read, and config only if `.env.json` still holds what it should.
/// Returns whether `.env.json` was written.
fn write_files(files: &Files<'_>, observe: &mut dyn FnMut(WriteStep)) -> Result<bool, CliError> {
    observe(WriteStep::BeforeEnvFile);
    let mut expected_env = files.env_bytes.map(<[u8]>::to_vec);
    if let Some(new_env) = files.new_env {
        write_config_value_if_unchanged(files.env_path, files.env_bytes, new_env).map_err(
            |error| {
                changed_or(
                    error,
                    ".env.json changed while this command ran; nothing was written",
                )
            },
        )?;
        expected_env = Some(written_bytes(new_env)?);
    }
    let wrote_env = files.new_env.is_some();
    observe(WriteStep::BeforeConfig);
    let Some(new_config) = files.new_config else {
        return Ok(wrote_env);
    };
    if read_if_present(files.env_path)? != expected_env {
        return Err(run_again(if wrote_env {
            ".env.json changed after the token was written to it; config was not changed"
        } else {
            ".env.json changed while this command ran; nothing was written"
        }));
    }
    write_config_value_if_unchanged(files.config_path, files.config_bytes, new_config).map_err(
        |error| {
            changed_or(
                error,
                if wrote_env {
                    "config.json changed while this command ran; .env.json holds the token but config was not changed"
                } else {
                    "config.json changed while this command ran; nothing was written"
                },
            )
        },
    )?;
    Ok(wrote_env)
}

/// The bytes `write_config_value_if_unchanged` writes for `value`.
fn written_bytes(value: &Value) -> Result<Vec<u8>, CliError> {
    let text = serde_json::to_string_pretty(value).map_err(ConfigError::Json)?;
    Ok(format!("{text}\n").into_bytes())
}

fn changed_or(error: ConfigWriteError, changed: &str) -> CliError {
    match error {
        ConfigWriteError::Changed => run_again(changed),
        other => other.into(),
    }
}

fn run_again(reason: &str) -> CliError {
    CliError::Runtime(format!("{reason}. Run it again."))
}

/// Reads config like the runtime but resolves only the Discord token fields;
/// writes nothing and does not look at migration markers.
fn status(config_path: &Path, environment: &dyn EnvSource) -> Result<String, CliError> {
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.to_path_buf()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let resolver = config_env(config_path, environment)?;
    let plugins = &bundle.config.channels.plugins;
    let mut resolved = plugins.clone();
    let mut unresolved = None;
    if let Some(Value::Object(discord)) = resolved.get_mut(DISCORD_CHANNEL) {
        for field in TOKEN_FIELDS {
            if let Some(Value::String(text)) = discord.get_mut(field) {
                match interpolate_env_with_source(text, &resolver) {
                    Ok(value) => *text = value,
                    Err(problem) => {
                        unresolved.get_or_insert(format!("channels.discord.{field}: {problem}"));
                    }
                }
            }
        }
    }
    let reason = if let Some(problem) = unresolved {
        Some(format!(
            "{problem}, so the runtime cannot read config until it is set"
        ))
    } else if !channel_enabled_from_plugins(plugins, DISCORD_CHANNEL, false) {
        Some("channels.discord.enabled is not true".to_owned())
    } else if discord_runtime_config(&resolved).is_none() {
        Some("channels.discord has no token in botToken, bot_token or token".to_owned())
    } else {
        None
    };
    Ok(match reason {
        Some(reason) => format!("discord: not connected\nreason: {reason}"),
        None => {
            let field = resolved
                .get(DISCORD_CHANNEL)
                .and_then(Value::as_object)
                .and_then(|discord| {
                    TOKEN_FIELDS.into_iter().find(|field| {
                        discord
                            .get(*field)
                            .and_then(Value::as_str)
                            .is_some_and(|value| !value.trim().is_empty())
                    })
                })
                .unwrap_or("token");
            format!("discord: connected\ntoken: channels.discord.{field}")
        }
    })
}

#[cfg(test)]
#[path = "discord_connect_cli_tests.rs"]
mod tests;
