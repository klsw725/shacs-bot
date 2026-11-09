//! `shacs-bot channels connect discord`: puts the Discord bot token in
//! `.env.json` and leaves only `${NAME}` in config, changes who the bot
//! answers, which channels it reads, when it answers in server channels and
//! whether it is on, and `--status`, which reads whether the Discord fields
//! meet the runtime's worker condition and what the runtime reads from them
//! (Spec 052 PRD 010 scopes 3 and 5, PRD 050).

use super::{
    allowed_sender_config, channel_enabled_from_plugins, default_config_path,
    discord_channel_config, discord_runtime_config, guard_config_migration_admission, hidden_input,
    take_path, take_value, ArgParser, ChannelsCommand, CliCommand, CliError, DiscordChannelFilter,
    DiscordGroupPolicy, DiscordRuntimeConfig, DiscordTransportMode, DISCORD_CHANNEL,
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

const NOT_CONNECTED: &str = "discord is not connected yet; connect it with --token-stdin";
const ANYONE_WARNING: &str =
    "warning: the Discord sender list is empty or has *; anyone the bot can see can use it";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordConnectOptions {
    pub config_path: Option<PathBuf>,
    pub action: DiscordConnectAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscordConnectAction {
    /// Connects with a token read from standard input.
    Connect {
        settings: DiscordSettings,
        existing: ExistingValue,
    },
    /// Changes only the settings given and keeps the saved token.
    Settings(DiscordSettings),
    Status,
}

/// The settings a call changes; `None` leaves that field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscordSettings {
    pub users: Option<Users>,
    pub channels: Option<Channels>,
    pub group_policy: Option<GroupPolicy>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Users {
    Only(Vec<String>),
    Anyone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channels {
    Only(Vec<String>),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPolicy {
    Mention,
    Open,
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
    let mut existing = ExistingValue::Keep;
    let mut status = false;
    let mut users = Vec::new();
    let mut anyone = false;
    let mut channels = Vec::new();
    let mut all_channels = false;
    let mut group_policy = None;
    let (mut enable, mut disable) = (false, false);
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--token-stdin" => token_stdin = true,
            "--allow-from" => push_id(
                &mut users,
                &take_value(&mut parser, &arg)?,
                "--allow-from requires a non-empty user id",
            )?,
            "--allow-anyone" => anyone = true,
            "--allow-channel" => push_id(
                &mut channels,
                &take_value(&mut parser, &arg)?,
                "--allow-channel requires a non-empty channel id",
            )?,
            "--all-channels" => all_channels = true,
            "--group-policy" => {
                let policy = parse_group_policy(&take_value(&mut parser, &arg)?)?;
                if group_policy.is_some_and(|given| given != policy) {
                    return Err(invalid(
                        "--group-policy is given twice with different values",
                    ));
                }
                group_policy = Some(policy);
            }
            "--enable" => enable = true,
            "--disable" => disable = true,
            "--replace" => existing = ExistingValue::Replace,
            "--status" => status = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(invalid(&format!(
                    "unknown channels connect discord argument `{other}`"
                )))
            }
        }
    }
    for (conflict, message) in [
        (
            !users.is_empty() && anyone,
            "use either --allow-from or --allow-anyone",
        ),
        (
            !channels.is_empty() && all_channels,
            "use either --allow-channel or --all-channels",
        ),
        (enable && disable, "use either --enable or --disable"),
        (
            disable && token_stdin,
            "--disable cannot be used with --token-stdin",
        ),
        (
            existing == ExistingValue::Replace && !token_stdin,
            "--replace needs --token-stdin",
        ),
    ] {
        if conflict {
            return Err(invalid(message));
        }
    }
    let settings = DiscordSettings {
        users: if anyone {
            Some(Users::Anyone)
        } else {
            (!users.is_empty()).then_some(Users::Only(users))
        },
        channels: if all_channels {
            Some(Channels::All)
        } else {
            (!channels.is_empty()).then_some(Channels::Only(channels))
        },
        group_policy,
        enabled: (enable || disable).then_some(enable),
    };
    let action = if status {
        if token_stdin || settings != DiscordSettings::default() {
            return Err(invalid(
                "channels connect discord --status takes no other option",
            ));
        }
        DiscordConnectAction::Status
    } else if token_stdin {
        DiscordConnectAction::Connect { settings, existing }
    } else if settings != DiscordSettings::default() {
        DiscordConnectAction::Settings(settings)
    } else {
        return Err(invalid(
            "channels connect discord requires --token-stdin, a setting option or --status",
        ));
    };
    Ok(CliCommand::Channels(ChannelsCommand::Connect(
        DiscordConnectOptions {
            config_path,
            action,
        },
    )))
}

fn invalid(message: &str) -> CliError {
    CliError::InvalidArguments(message.to_owned())
}

fn push_id(ids: &mut Vec<String>, value: &str, empty: &str) -> Result<(), CliError> {
    let id = value.trim();
    if id.is_empty() {
        return Err(invalid(empty));
    }
    if !ids.iter().any(|known| known == id) {
        ids.push(id.to_owned());
    }
    Ok(())
}

fn parse_group_policy(value: &str) -> Result<GroupPolicy, CliError> {
    match value.trim() {
        "mention" => Ok(GroupPolicy::Mention),
        "open" => Ok(GroupPolicy::Open),
        other => Err(invalid(&format!(
            "--group-policy must be `mention` or `open`, not `{other}`"
        ))),
    }
}

pub(super) fn run(options: DiscordConnectOptions) -> Result<String, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    match options.action {
        DiscordConnectAction::Status => status(&config_path, &ProcessEnv),
        DiscordConnectAction::Connect { settings, existing } => connect(
            &config_path,
            &Connection {
                settings: &settings,
                existing,
            },
            &|| hidden_input::read_secret("Discord bot token: "),
            &ProcessEnv,
            &mut |_| {},
        ),
        DiscordConnectAction::Settings(settings) => {
            change_settings(&config_path, &settings, &ProcessEnv, &mut |_| {})
        }
    }
}

struct Connection<'a> {
    settings: &'a DiscordSettings,
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
    let (new_config, changed_fields) = discord_config(&config, Some(&name), connection.settings)?;
    check_settings(discord, discord_object(&new_config)?, connection.settings)?;

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
    if anyone_allowed(discord_object(&new_config)?) {
        lines.push(ANYONE_WARNING.to_owned());
    }
    Ok(lines.join("\n"))
}

/// Changes only the settings given, keeping the saved token: the token is
/// neither asked for nor read, and `.env.json` is only read.
fn change_settings(
    config_path: &Path,
    settings: &DiscordSettings,
    environment: &dyn EnvSource,
    observe: &mut dyn FnMut(WriteStep),
) -> Result<String, CliError> {
    guard_config_migration_admission(config_path)?;
    let Some(config_bytes) = read_if_present(config_path)? else {
        return Err(stopped(NOT_CONNECTED));
    };
    let config = serde_json::from_slice::<Value>(&config_bytes).map_err(ConfigError::Json)?;
    let discord = discord_object(&config)?;
    if !discord_reading(config_path, environment)?.has_token() {
        return Err(stopped(NOT_CONNECTED));
    }
    let (new_config, changed_fields) = discord_config(&config, None, settings)?;
    check_settings(discord, discord_object(&new_config)?, settings)?;

    let mut lines = vec!["discord: settings updated".to_owned()];
    if changed_fields.is_empty() {
        lines.push("unchanged".to_owned());
    } else {
        observe(WriteStep::BeforeConfig);
        write_config_value_if_unchanged(config_path, Some(&config_bytes), &new_config).map_err(
            |error| {
                changed_or(
                    error,
                    "config.json changed while this command ran; nothing was written",
                )
            },
        )?;
        lines.push(format!("updated: config {}", changed_fields.join(", ")));
    }
    if anyone_allowed(discord_object(&new_config)?) {
        lines.push(ANYONE_WARNING.to_owned());
    }
    Ok(lines.join("\n"))
}

/// Stops a change the runtime would not read: a channel list behind a field
/// the runtime reads first or adds to the list, or a `groupPolicy` for
/// channels it polls over REST, where it does not apply.
fn check_settings(
    before: Option<&Map<String, Value>>,
    after: Option<&Map<String, Value>>,
    settings: &DiscordSettings,
) -> Result<(), CliError> {
    if let (Some(_), Some(before)) = (&settings.channels, before) {
        for field in ["channelIds", "allowedChannelIds"] {
            if before.contains_key(field) {
                return Err(stopped(&format!(
                    "channels.discord.{field} is read before allowChannels; remove it by hand to set the channels here"
                )));
            }
        }
        for field in [
            "defaultChannelId",
            "default_channel_id",
            "channelId",
            "channel_id",
        ] {
            let has_value = before
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            if has_value {
                return Err(stopped(&format!(
                    "channels.discord.{field} adds a channel to the list; remove it by hand to set the channels here"
                )));
            }
        }
    }
    if let (Some(_), Some(after)) = (&settings.group_policy, after) {
        let (channel_ids, gateway_style) = discord_channel_config(after);
        if !gateway_style && !channel_ids.is_empty() {
            return Err(stopped(
                "channels.discord lists channels without allowChannels, so the runtime polls them and does not apply groupPolicy; set the channels with --allow-channel or move them to allowChannels by hand",
            ));
        }
    }
    Ok(())
}

fn anyone_allowed(discord: Option<&Map<String, Value>>) -> bool {
    discord
        .map(allowed_sender_config)
        .is_none_or(|senders| anyone(&senders))
}

fn anyone(senders: &[String]) -> bool {
    senders.is_empty() || senders.iter().any(|sender| sender == "*")
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

/// `config` with the settings applied to `channels.discord`, connected
/// through `${token_name}` when one is given, and the fields that changed.
fn discord_config(
    config: &Value,
    token_name: Option<&str>,
    settings: &DiscordSettings,
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
    let mut wanted = Vec::new();
    match token_name {
        Some(name) => {
            wanted.push(("enabled", Value::Bool(true)));
            wanted.push(("token", Value::String(placeholder(name))));
        }
        None => {
            if let Some(enabled) = settings.enabled {
                wanted.push(("enabled", Value::Bool(enabled)));
            }
        }
    }
    if let Some(users) = &settings.users {
        let ids = match users {
            Users::Only(ids) => ids.as_slice(),
            Users::Anyone => &[],
        };
        wanted.push(("allowFrom", id_list(ids)));
    }
    if let Some(channels) = &settings.channels {
        let ids = match channels {
            Channels::Only(ids) => ids.as_slice(),
            Channels::All => &[],
        };
        wanted.push(("allowChannels", id_list(ids)));
    }
    if let Some(policy) = settings.group_policy {
        let value = match policy {
            GroupPolicy::Mention => "mention",
            GroupPolicy::Open => "open",
        };
        wanted.push(("groupPolicy", Value::String(value.to_owned())));
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

fn id_list(ids: &[String]) -> Value {
    Value::Array(ids.iter().cloned().map(Value::String).collect())
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

/// What the runtime reads from `channels.discord`, with only the token
/// fields resolved.
struct DiscordReading {
    /// A token field whose `${NAME}` resolves nowhere.
    unresolved: Option<String>,
    enabled: bool,
    runtime: Option<DiscordRuntimeConfig>,
    token_field: &'static str,
}

impl DiscordReading {
    /// Everything the worker needs except `enabled`.
    fn has_token(&self) -> bool {
        self.unresolved.is_none() && self.runtime.is_some()
    }
}

/// Reads config like the runtime but resolves only the Discord token fields;
/// writes nothing and does not look at migration markers.
fn discord_reading(
    config_path: &Path,
    environment: &dyn EnvSource,
) -> Result<DiscordReading, CliError> {
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
    let token_field = resolved
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
    Ok(DiscordReading {
        unresolved,
        enabled: channel_enabled_from_plugins(plugins, DISCORD_CHANNEL, false),
        runtime: discord_runtime_config(&resolved),
        token_field,
    })
}

fn status(config_path: &Path, environment: &dyn EnvSource) -> Result<String, CliError> {
    let reading = discord_reading(config_path, environment)?;
    let reason = if let Some(problem) = &reading.unresolved {
        Some(format!(
            "{problem}, so the runtime cannot read config until it is set"
        ))
    } else if !reading.enabled {
        Some("channels.discord.enabled is not true".to_owned())
    } else if reading.runtime.is_none() {
        Some("channels.discord has no token in botToken, bot_token or token".to_owned())
    } else {
        None
    };
    let mut lines = vec![match reason {
        Some(reason) => format!("discord: not connected\nreason: {reason}"),
        None => "discord: connected".to_owned(),
    }];
    if let (None, Some(runtime)) = (&reading.unresolved, &reading.runtime) {
        lines.push(format!("token: channels.discord.{}", reading.token_field));
        lines.extend(settings_lines(runtime));
    }
    Ok(lines.join("\n"))
}

fn settings_lines(runtime: &DiscordRuntimeConfig) -> [String; 3] {
    let senders = &runtime.allowed_senders;
    let users = if anyone(senders) {
        "anyone".to_owned()
    } else {
        senders.join(", ")
    };
    // REST polling asks for each listed channel as it is, so `*` there is
    // one channel ID rather than every channel.
    let polling = runtime.transport == DiscordTransportMode::RestPolling;
    let channels = match &runtime.channel_filter {
        DiscordChannelFilter::AllVisible => "all".to_owned(),
        DiscordChannelFilter::Only(ids) if !polling && ids.iter().any(|id| id == "*") => {
            "all".to_owned()
        }
        DiscordChannelFilter::Only(ids) => ids.join(", "),
    };
    let replies = match (polling, runtime.group_policy) {
        (true, _) => "every message (channelIds polling does not apply groupPolicy)",
        (false, DiscordGroupPolicy::Mention) => "when mentioned",
        (false, DiscordGroupPolicy::Open) => "every message",
    };
    [
        format!("users: {users}"),
        format!("channels: {channels}"),
        format!("server channels: {replies}"),
    ]
}

#[cfg(test)]
#[path = "discord_connect_cli_tests.rs"]
mod tests;
