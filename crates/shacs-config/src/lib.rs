use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
}

impl Config {
    pub fn workspace_path(&self) -> PathBuf {
        expand_home(&self.agents.defaults.workspace)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default)]
    pub defaults: AgentDefaults,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefaults {
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_context_window_tokens")]
    pub context_window_tokens: u32,
    #[serde(default)]
    pub context_block_limit: Option<u32>,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tool_iterations")]
    pub max_tool_iterations: u32,
    #[serde(default = "default_max_tool_result_chars")]
    pub max_tool_result_chars: usize,
    #[serde(default = "default_retry_mode")]
    pub provider_retry_mode: String,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default)]
    pub unified_session: bool,
    #[serde(default)]
    pub disabled_skills: Vec<String>,
    #[serde(default, alias = "sessionTtlMinutes")]
    pub idle_compact_after_minutes: u32,
    #[serde(default = "default_max_messages")]
    pub max_messages: u32,
    #[serde(default = "default_consolidation_ratio")]
    pub consolidation_ratio: f64,
    #[serde(default)]
    pub dream: DreamConfig,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
            model: default_model(),
            provider: default_provider(),
            max_tokens: default_max_tokens(),
            context_window_tokens: default_context_window_tokens(),
            context_block_limit: None,
            temperature: default_temperature(),
            max_tool_iterations: default_max_tool_iterations(),
            max_tool_result_chars: default_max_tool_result_chars(),
            provider_retry_mode: default_retry_mode(),
            reasoning_effort: None,
            timezone: default_timezone(),
            unified_session: false,
            disabled_skills: Vec::new(),
            idle_compact_after_minutes: 0,
            max_messages: default_max_messages(),
            consolidation_ratio: default_consolidation_ratio(),
            dream: DreamConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DreamConfig {
    #[serde(default = "default_dream_interval_h")]
    pub interval_h: u32,
    #[serde(default)]
    pub model_override: Option<String>,
    #[serde(default = "default_dream_max_batch_size")]
    pub max_batch_size: u32,
    #[serde(default = "default_dream_max_iterations")]
    pub max_iterations: u32,
    #[serde(default = "default_true")]
    pub annotate_line_ages: bool,
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            interval_h: default_dream_interval_h(),
            model_override: None,
            max_batch_size: default_dream_max_batch_size(),
            max_iterations: default_dream_max_iterations(),
            annotate_line_ages: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    #[serde(default, alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(default, alias = "api_base")]
    pub api_base: Option<String>,
    #[serde(default, alias = "extra_headers")]
    pub extra_headers: Option<BTreeMap<String, String>>,
    #[serde(default, alias = "extra_body")]
    pub extra_body: Option<Map<String, Value>>,
}

impl ProviderConfig {
    pub fn api_base_or<'a>(&'a self, default: Option<&'a str>) -> Option<&'a str> {
        self.api_base.as_deref().or(default)
    }
}

pub type ProvidersConfig = BTreeMap<String, ProviderConfig>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AuthStore {
    #[serde(default, flatten)]
    pub providers: BTreeMap<String, ProviderAuth>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAuth {
    #[serde(rename = "type")]
    pub kind: String,
    pub access: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

impl ProviderAuth {
    pub fn oauth_access(access: impl Into<String>, account_id: Option<String>) -> Self {
        Self {
            kind: "oauth".to_owned(),
            access: access.into(),
            refresh: None,
            expires: None,
            account_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelsConfig {
    #[serde(default = "default_true")]
    pub send_progress: bool,
    #[serde(default)]
    pub send_memory_hints: bool,
    #[serde(default)]
    pub send_tool_hints: bool,
    #[serde(default = "default_send_max_retries")]
    pub send_max_retries: u32,
    #[serde(default = "default_transcription_provider")]
    pub transcription_provider: String,
    #[serde(default)]
    pub transcription_language: Option<String>,
    #[serde(flatten)]
    pub plugins: BTreeMap<String, Value>,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            send_progress: true,
            send_memory_hints: false,
            send_tool_hints: false,
            send_max_retries: default_send_max_retries(),
            transcription_provider: default_transcription_provider(),
            transcription_language: None,
            plugins: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiConfig {
    #[serde(default = "default_bind_host")]
    pub host: String,
    #[serde(default = "default_api_port")]
    pub port: u16,
    #[serde(default = "default_api_timeout")]
    pub timeout: f64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: default_bind_host(),
            port: default_api_port(),
            timeout: default_api_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_bind_host")]
    pub host: String,
    #[serde(default = "default_gateway_port")]
    pub port: u16,
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_bind_host(),
            port: default_gateway_port(),
            heartbeat: HeartbeatConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_heartbeat_interval_s")]
    pub interval_s: u32,
    #[serde(default = "default_heartbeat_keep_recent_messages")]
    pub keep_recent_messages: u32,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_s: default_heartbeat_interval_s(),
            keep_recent_messages: default_heartbeat_keep_recent_messages(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsConfig {
    #[serde(default)]
    pub web: WebToolsConfig,
    #[serde(default)]
    pub exec: ExecToolConfig,
    #[serde(default)]
    pub my: MyToolConfig,
    #[serde(default)]
    pub restrict_to_workspace: bool,
    #[serde(default)]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    #[serde(default)]
    pub ssrf_whitelist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebToolsConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub search: WebSearchConfig,
    #[serde(default)]
    pub fetch: WebFetchConfig,
}

impl Default for WebToolsConfig {
    fn default() -> Self {
        Self {
            enable: true,
            proxy: None,
            user_agent: None,
            search: WebSearchConfig::default(),
            fetch: WebFetchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchConfig {
    #[serde(default = "default_web_search_provider")]
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_web_max_results")]
    pub max_results: u32,
    #[serde(default = "default_web_timeout")]
    pub timeout: u32,
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            provider: default_web_search_provider(),
            api_key: String::new(),
            base_url: String::new(),
            max_results: default_web_max_results(),
            timeout: default_web_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebFetchConfig {
    #[serde(default = "default_true")]
    pub use_jina_reader: bool,
}

impl Default for WebFetchConfig {
    fn default() -> Self {
        Self {
            use_jina_reader: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecToolConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
    #[serde(default = "default_exec_timeout")]
    pub timeout: u32,
    #[serde(default)]
    pub path_append: String,
    #[serde(default)]
    pub sandbox: String,
    #[serde(default)]
    pub allowed_env_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl Default for ExecToolConfig {
    fn default() -> Self {
        Self {
            enable: true,
            timeout: default_exec_timeout(),
            path_append: String::new(),
            sandbox: String::new(),
            allowed_env_keys: Vec::new(),
            env: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyToolConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
    #[serde(default)]
    pub allow_set: bool,
}

impl Default for MyToolConfig {
    fn default() -> Self {
        Self {
            enable: true,
            allow_set: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_mcp_tool_timeout")]
    pub tool_timeout: u32,
    #[serde(default = "default_mcp_enabled_tools")]
    pub enabled_tools: Vec<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            r#type: None,
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            url: String::new(),
            headers: BTreeMap::new(),
            tool_timeout: default_mcp_tool_timeout(),
            enabled_tools: default_mcp_enabled_tools(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigContext {
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub workspace: PathBuf,
}

impl ConfigContext {
    pub fn auth_path(&self) -> PathBuf {
        self.data_dir.join("auth.json")
    }

    pub fn runtime_subdir(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    pub fn media_dir(&self, channel: Option<&str>) -> PathBuf {
        match channel.filter(|value| !value.is_empty()) {
            Some(channel) => self.data_dir.join("media").join(channel),
            None => self.data_dir.join("media"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub resolve_env: bool,
    pub write_back_migrations: bool,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            workspace_override: None,
            resolve_env: true,
            write_back_migrations: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigBundle {
    pub config: Config,
    pub context: ConfigContext,
    pub migrations: Vec<Migration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub key: String,
    pub note: String,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Env(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "config I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "config JSON failed: {error}"),
            Self::Env(error) => write!(formatter, "config env resolution failed: {error}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub trait EnvSource {
    fn var(&self, name: &str) -> Option<String>;
}

static CURRENT_CONFIG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

impl EnvSource for BTreeMap<String, String> {
    fn var(&self, name: &str) -> Option<String> {
        self.get(name).cloned()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }
}

pub fn default_config_path() -> PathBuf {
    shacs_home_dir().join("config.json")
}

pub fn set_config_path(path: impl Into<PathBuf>) {
    if let Ok(mut current) = CURRENT_CONFIG_PATH.lock() {
        *current = Some(path.into());
    }
}

pub fn clear_config_path() {
    if let Ok(mut current) = CURRENT_CONFIG_PATH.lock() {
        *current = None;
    }
}

pub fn get_config_path() -> PathBuf {
    CURRENT_CONFIG_PATH
        .lock()
        .ok()
        .and_then(|current| current.clone())
        .unwrap_or_else(default_config_path)
}

pub fn get_data_dir() -> PathBuf {
    let data_dir = get_config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(shacs_home_dir);
    ensure_dir(&data_dir)
}

pub fn get_runtime_subdir(name: &str) -> PathBuf {
    ensure_dir(&get_data_dir().join(name))
}

pub fn get_media_dir(channel: Option<&str>) -> PathBuf {
    let base = get_runtime_subdir("media");
    match channel.filter(|value| !value.is_empty()) {
        Some(channel) => ensure_dir(&base.join(channel)),
        None => base,
    }
}

pub fn get_cron_dir() -> PathBuf {
    get_runtime_subdir("cron")
}

pub fn get_logs_dir() -> PathBuf {
    get_runtime_subdir("logs")
}

pub fn get_workspace_path(workspace: Option<&Path>) -> PathBuf {
    let path = workspace
        .map(expand_home_path)
        .unwrap_or_else(default_workspace_path);
    ensure_dir(&path)
}

pub fn is_default_workspace(workspace: Option<&Path>) -> bool {
    let current = workspace
        .map(expand_home_path)
        .unwrap_or_else(default_workspace_path);
    path_normalized(&current) == path_normalized(&default_workspace_path())
}

pub fn default_auth_path() -> PathBuf {
    shacs_home_dir().join("auth.json")
}

pub fn default_workspace_path() -> PathBuf {
    shacs_home_dir().join("workspace")
}

pub fn cli_history_path() -> PathBuf {
    shacs_home_dir().join("history").join("cli_history")
}

pub fn get_cli_history_path() -> PathBuf {
    cli_history_path()
}

pub fn bridge_install_dir() -> PathBuf {
    shacs_home_dir().join("bridge")
}

pub fn get_bridge_install_dir() -> PathBuf {
    bridge_install_dir()
}

pub fn legacy_sessions_dir() -> PathBuf {
    shacs_home_dir().join("sessions")
}

pub fn get_legacy_sessions_dir() -> PathBuf {
    legacy_sessions_dir()
}

pub fn load_config(options: LoadOptions) -> Result<ConfigBundle, ConfigError> {
    load_config_with_env(options, &ProcessEnv)
}

pub fn load_config_with_env(
    options: LoadOptions,
    env: &impl EnvSource,
) -> Result<ConfigBundle, ConfigError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let (mut config, migrations, migrated_value) = if config_path.exists() {
        let raw = fs::read_to_string(&config_path)?;
        let mut value = serde_json::from_str::<Value>(&raw)?;
        let migrations = migrate_config_value(&mut value);
        let config = serde_json::from_value(value.clone())?;
        (config, migrations, Some(value))
    } else {
        (Config::default(), Vec::new(), None)
    };

    if options.write_back_migrations && !migrations.is_empty() {
        if let Some(value) = migrated_value.as_ref() {
            save_config_value_to_path(value, &config_path)?;
        }
    }
    if let Some(workspace) = &options.workspace_override {
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    }
    if options.resolve_env {
        resolve_config_env_refs(&mut config, env)?;
    }
    let context = config_context(Some(config_path), Some(config.workspace_path()));
    Ok(ConfigBundle {
        config,
        context,
        migrations,
    })
}

pub fn load_config_or_default(options: LoadOptions) -> Result<ConfigBundle, ConfigError> {
    load_config_with_env_or_default(options, &ProcessEnv)
}

pub fn load_config_with_env_or_default(
    options: LoadOptions,
    env: &impl EnvSource,
) -> Result<ConfigBundle, ConfigError> {
    let config_path = options
        .config_path
        .clone()
        .unwrap_or_else(default_config_path);
    let workspace_override = options.workspace_override.clone();
    match load_config_with_env(options, env) {
        Ok(bundle) => Ok(bundle),
        Err(ConfigError::Json(_)) => {
            let mut config = Config::default();
            if let Some(workspace) = &workspace_override {
                config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
            }
            let context = config_context(Some(config_path), Some(config.workspace_path()));
            Ok(ConfigBundle {
                config,
                context,
                migrations: Vec::new(),
            })
        }
        Err(error) => Err(error),
    }
}

pub fn save_config(bundle: &ConfigBundle) -> Result<(), ConfigError> {
    save_config_to_path(&bundle.config, &bundle.context.config_path)
}

pub fn save_config_to_path(config: &Config, path: &Path) -> Result<(), ConfigError> {
    save_config_value_to_path(&serde_json::to_value(config)?, path)
}

pub fn load_auth_store(path: &Path) -> Result<AuthStore, ConfigError> {
    if !path.exists() {
        return Ok(AuthStore::default());
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_auth_store_to_path(store: &AuthStore, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(store)?;
    write_secret_file(path, format!("{text}\n").as_bytes())?;
    Ok(())
}

fn write_secret_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp_path = secret_temp_path(path);
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let write_result = (|| {
        let mut file = options.open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok::<_, io::Error>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    #[cfg(unix)]
    {
        let permissions = std::fs::Permissions::from_mode(0o600);
        fs::set_permissions(&temp_path, permissions)?;
    }
    fs::rename(&temp_path, path)?;
    Ok(())
}

fn secret_temp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("auth.json");
    path.with_file_name(format!(".{name}.tmp-{}-{nanos}", process::id()))
}

fn save_config_value_to_path(value: &Value, path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{text}\n"))?;
    Ok(())
}

pub fn refresh_config(path: &Path) -> Result<ConfigBundle, ConfigError> {
    let options = LoadOptions {
        config_path: Some(path.to_path_buf()),
        resolve_env: false,
        write_back_migrations: true,
        workspace_override: None,
    };
    let bundle = load_config_with_env(options, &BTreeMap::<String, String>::new())?;
    save_config(&bundle)?;
    Ok(bundle)
}

pub fn config_context(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> ConfigContext {
    let config_path = config_path.unwrap_or_else(default_config_path);
    let data_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(shacs_home_dir);
    let workspace = workspace_override.unwrap_or_else(default_workspace_path);
    ConfigContext {
        config_path,
        data_dir,
        workspace,
    }
}

pub fn ensure_runtime_dirs(context: &ConfigContext) -> std::io::Result<Vec<PathBuf>> {
    let dirs = [
        context.data_dir.clone(),
        context.workspace.clone(),
        context.data_dir.join("media"),
        context.data_dir.join("cron"),
        context.data_dir.join("logs"),
        context.data_dir.join("channels"),
        context.data_dir.join("channels").join("worker-metadata"),
        context.data_dir.join("skills"),
    ];
    for dir in &dirs {
        fs::create_dir_all(dir)?;
    }
    Ok(dirs.into_iter().collect())
}

pub fn resolve_config_env_refs(
    config: &mut Config,
    env: &impl EnvSource,
) -> Result<(), ConfigError> {
    let mut value = serde_json::to_value(&*config)?;
    resolve_env_value(&mut value, env).map_err(ConfigError::Env)?;
    *config = serde_json::from_value(value)?;
    Ok(())
}

pub fn interpolate_env(input: &str, env: &BTreeMap<String, String>) -> Result<String, String> {
    interpolate_env_with_source(input, env)
}

pub fn interpolate_env_with_source(input: &str, env: &impl EnvSource) -> Result<String, String> {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&rest[start..]);
            return Ok(output);
        };
        let key = &after_start[..end];
        let Some(value) = env.var(key) else {
            return Err(format!("missing environment variable: {key}"));
        };
        output.push_str(&value);
        rest = &after_start[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

pub fn migrate_config_value(value: &mut Value) -> Vec<Migration> {
    let mut migrations = Vec::new();
    migrate_tools(value, &mut migrations);
    migrate_agent_defaults(value, &mut migrations);
    migrations
}

fn migrate_tools(value: &mut Value, migrations: &mut Vec<Migration>) {
    let Some(tools) = value.get_mut("tools").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(exec) = tools.get_mut("exec").and_then(Value::as_object_mut) {
        if let Some(restrict) = exec.remove("restrictToWorkspace") {
            tools.entry("restrictToWorkspace").or_insert(restrict);
            migrations.push(Migration {
                key: "tools.exec.restrictToWorkspace".to_owned(),
                note: "moved to tools.restrictToWorkspace".to_owned(),
            });
        }
    }
    if tools.contains_key("myEnabled") || tools.contains_key("mySet") {
        let enable = tools.remove("myEnabled");
        let allow_set = tools.remove("mySet");
        let my = tools
            .entry("my")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(my) = my.as_object_mut() {
            if let Some(enable) = enable {
                my.entry("enable").or_insert(enable);
            }
            if let Some(allow_set) = allow_set {
                my.entry("allowSet").or_insert(allow_set);
            }
        }
        migrations.push(Migration {
            key: "tools.myEnabled/tools.mySet".to_owned(),
            note: "moved to tools.my".to_owned(),
        });
    }
}

fn migrate_agent_defaults(value: &mut Value, migrations: &mut Vec<Migration>) {
    let Some(defaults) = value
        .get_mut("agents")
        .and_then(Value::as_object_mut)
        .and_then(|agents| agents.get_mut("defaults"))
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if defaults.contains_key("memoryWindow") {
        defaults.remove("memoryWindow");
        migrations.push(Migration {
            key: "agents.defaults.memoryWindow".to_owned(),
            note: "removed legacy memory window".to_owned(),
        });
    }
    if let Some(session_ttl) = defaults.remove("sessionTtlMinutes") {
        defaults
            .entry("idleCompactAfterMinutes")
            .or_insert(session_ttl);
        migrations.push(Migration {
            key: "agents.defaults.sessionTtlMinutes".to_owned(),
            note: "renamed to idleCompactAfterMinutes".to_owned(),
        });
    }
    if let Some(dream) = defaults.get_mut("dream").and_then(Value::as_object_mut) {
        if let Some(model) = dream.remove("model") {
            dream.entry("modelOverride").or_insert(model);
            migrations.push(Migration {
                key: "agents.defaults.dream.model".to_owned(),
                note: "renamed to modelOverride".to_owned(),
            });
        }
        if dream.remove("cron").is_some() {
            migrations.push(Migration {
                key: "agents.defaults.dream.cron".to_owned(),
                note: "removed legacy cron override from persisted config".to_owned(),
            });
        }
    }
}

fn resolve_env_value(value: &mut Value, env: &impl EnvSource) -> Result<(), String> {
    match value {
        Value::String(text) => {
            let resolved = interpolate_env_with_source(text, env)?;
            *text = resolved;
        }
        Value::Array(items) => {
            for item in items {
                resolve_env_value(item, env)?;
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                resolve_env_value(item, env)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn shacs_home_dir() -> PathBuf {
    home_dir().join(".shacs-bot")
}

fn ensure_dir(path: &Path) -> PathBuf {
    let _ = fs::create_dir_all(path);
    path.to_path_buf()
}

fn path_normalized(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn expand_home_path(path: &Path) -> PathBuf {
    path.to_str()
        .map(expand_home)
        .unwrap_or_else(|| path.to_path_buf())
}

fn default_workspace() -> String {
    "~/.shacs-bot/workspace".to_owned()
}

fn default_provider() -> String {
    "auto".to_owned()
}

fn default_model() -> String {
    "anthropic/claude-opus-4-5".to_owned()
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_temperature() -> f64 {
    0.1
}

fn default_context_window_tokens() -> u32 {
    65_536
}

fn default_max_tool_iterations() -> u32 {
    200
}

fn default_max_tool_result_chars() -> usize {
    16_000
}

fn default_retry_mode() -> String {
    "standard".to_owned()
}

fn default_timezone() -> String {
    "UTC".to_owned()
}

fn default_max_messages() -> u32 {
    120
}

fn default_consolidation_ratio() -> f64 {
    0.5
}

fn default_dream_interval_h() -> u32 {
    2
}

fn default_dream_max_batch_size() -> u32 {
    20
}

fn default_dream_max_iterations() -> u32 {
    15
}

fn default_true() -> bool {
    true
}

fn default_send_max_retries() -> u32 {
    3
}

fn default_transcription_provider() -> String {
    "groq".to_owned()
}

fn default_bind_host() -> String {
    "127.0.0.1".to_owned()
}

fn default_api_port() -> u16 {
    8900
}

fn default_api_timeout() -> f64 {
    120.0
}

fn default_gateway_port() -> u16 {
    18_790
}

fn default_heartbeat_interval_s() -> u32 {
    30 * 60
}

fn default_heartbeat_keep_recent_messages() -> u32 {
    8
}

fn default_web_search_provider() -> String {
    "duckduckgo".to_owned()
}

fn default_web_max_results() -> u32 {
    5
}

fn default_web_timeout() -> u32 {
    30
}

fn default_exec_timeout() -> u32 {
    60
}

fn default_mcp_tool_timeout() -> u32 {
    30
}

fn default_mcp_enabled_tools() -> Vec<String> {
    vec!["*".to_owned()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_use_shacs_paths_and_nanobot_provider_values() {
        let config = Config::default();
        assert_eq!(config.agents.defaults.workspace, "~/.shacs-bot/workspace");
        assert_eq!(config.agents.defaults.model, "anthropic/claude-opus-4-5");
        assert_eq!(config.agents.defaults.max_tokens, 8192);
        assert!(config.channels.send_progress);
        assert_eq!(
            default_config_path(),
            home_dir().join(".shacs-bot/config.json")
        );
    }

    #[test]
    fn config_deserializes_top_level_and_exec_env() -> Result<(), Box<dyn std::error::Error>> {
        let config: Config = serde_json::from_value(json!({
            "env": {
                "NOTION_TOKEN_KEY": "configured",
                "EMPTY_OK": ""
            },
            "tools": {
                "exec": {
                    "env": {
                        "SHACS_TOKEN": "configured"
                    }
                }
            }
        }))?;
        assert_eq!(
            config.env.get("NOTION_TOKEN_KEY").map(String::as_str),
            Some("configured")
        );
        assert_eq!(config.env.get("EMPTY_OK").map(String::as_str), Some(""));
        assert_eq!(
            config.tools.exec.env.get("SHACS_TOKEN").map(String::as_str),
            Some("configured")
        );
        assert!(Config::default().env.is_empty());
        assert!(Config::default().tools.exec.env.is_empty());
        Ok(())
    }

    #[test]
    fn provider_config_accepts_camel_and_snake_case_fields(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let camel: ProviderConfig = serde_json::from_value(json!({
            "apiKey": "key",
            "apiBase": "https://api.example.test",
            "extraHeaders": {"X-Test": "yes"},
            "extraBody": {"metadata": true}
        }))?;
        let snake: ProviderConfig = serde_json::from_value(json!({
            "api_key": "key",
            "api_base": "https://api.example.test",
            "extra_headers": {"X-Test": "yes"},
            "extra_body": {"metadata": true}
        }))?;
        assert_eq!(camel, snake);
        assert_eq!(
            camel.api_base_or(Some("fallback")),
            Some("https://api.example.test")
        );
        Ok(())
    }

    #[test]
    fn auth_store_roundtrips_open_code_style_oauth_entries(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let auth_path = root.path().join("auth.json");
        let mut store = AuthStore::default();
        store.providers.insert(
            "openai_codex".to_owned(),
            ProviderAuth {
                kind: "oauth".to_owned(),
                access: "access-token".to_owned(),
                refresh: Some("refresh-token".to_owned()),
                expires: Some(123),
                account_id: Some("acct_123".to_owned()),
            },
        );

        save_auth_store_to_path(&store, &auth_path)?;
        let saved = fs::read_to_string(&auth_path)?;
        assert!(saved.contains("openai_codex"));
        assert!(saved.contains("accountId"));
        assert!(!saved.contains("providers"));
        assert_eq!(load_auth_store(&auth_path)?, store);
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&auth_path)?.permissions().mode() & 0o777,
            0o600
        );
        Ok(())
    }

    #[test]
    fn resolves_env_refs_recursively_and_reports_missing() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut config: Config = serde_json::from_value(json!({
            "providers": {
                "openrouter": {
                    "apiKey": "${OPENROUTER_API_KEY}",
                    "extraHeaders": {"X-Trace": "prefix-${TRACE_ID}"},
                    "extraBody": {"labels": ["${LABEL}", 7]}
                }
            }
        }))?;
        let env = BTreeMap::from([
            ("OPENROUTER_API_KEY".to_owned(), "sk-or-test".to_owned()),
            ("TRACE_ID".to_owned(), "abc".to_owned()),
            ("LABEL".to_owned(), "nightly".to_owned()),
        ]);
        resolve_config_env_refs(&mut config, &env)?;
        let provider = config
            .providers
            .get("openrouter")
            .ok_or("missing provider")?;
        assert_eq!(provider.api_key.as_deref(), Some("sk-or-test"));
        assert_eq!(
            provider
                .extra_headers
                .as_ref()
                .and_then(|headers| headers.get("X-Trace"))
                .map(String::as_str),
            Some("prefix-abc")
        );
        assert!(resolve_config_env_refs(
            &mut serde_json::from_value::<Config>(
                json!({"agents": {"defaults": {"model": "${MISSING}"}}})
            )?,
            &BTreeMap::<String, String>::new()
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn migrates_legacy_keys_without_overriding_new_values() {
        let mut value = json!({
            "agents": {"defaults": {"sessionTtlMinutes": 30, "idleCompactAfterMinutes": 5, "memoryWindow": 99, "dream": {"model": "fast", "cron": "0 * * * *"}}},
            "tools": {"exec": {"restrictToWorkspace": true}, "myEnabled": false, "mySet": true, "my": {"enable": true}}
        });
        let migrations = migrate_config_value(&mut value);
        assert!(migrations.len() >= 5);
        assert_eq!(value["agents"]["defaults"]["idleCompactAfterMinutes"], 5);
        assert!(value["agents"]["defaults"]
            .get("sessionTtlMinutes")
            .is_none());
        assert!(value["agents"]["defaults"].get("memoryWindow").is_none());
        assert_eq!(
            value["agents"]["defaults"]["dream"]["modelOverride"],
            "fast"
        );
        assert!(value["agents"]["defaults"]["dream"].get("cron").is_none());
        assert_eq!(value["tools"]["restrictToWorkspace"], true);
        assert_eq!(value["tools"]["my"]["enable"], true);
        assert_eq!(value["tools"]["my"]["allowSet"], true);
    }

    #[test]
    fn load_save_refresh_and_runtime_context_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("instance").join("config.json");
        let raw = json!({
            "agents": {"defaults": {"workspace": "~/custom-workspace", "model": "openrouter/anthropic/claude-opus"}},
            "providers": {"openrouter": {"apiKey": "${OPENROUTER_API_KEY}"}}
        });
        fs::create_dir_all(config_path.parent().ok_or("missing parent")?)?;
        fs::write(&config_path, serde_json::to_string_pretty(&raw)?)?;
        let env = BTreeMap::from([("OPENROUTER_API_KEY".to_owned(), "secret".to_owned())]);
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(root.path().join("workspace-override")),
                resolve_env: true,
                write_back_migrations: false,
            },
            &env,
        )?;
        assert_eq!(bundle.context.config_path, config_path);
        assert_eq!(bundle.context.data_dir, root.path().join("instance"));
        assert_eq!(
            bundle.context.workspace,
            root.path().join("workspace-override")
        );
        assert_eq!(
            bundle.config.providers["openrouter"].api_key.as_deref(),
            Some("secret")
        );
        let dirs = ensure_runtime_dirs(&bundle.context)?;
        assert!(dirs.iter().all(|dir| dir.is_dir()));
        save_config(&bundle)?;
        assert!(fs::read_to_string(&bundle.context.config_path)?.contains("workspace-override"));
        Ok(())
    }

    #[test]
    fn ensure_runtime_dirs_creates_current_layout_contract(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("instance").join("config.json");
        let workspace_path = root.path().join("workspace-override");
        fs::create_dir_all(config_path.parent().ok_or("missing parent")?)?;

        let context = config_context(Some(config_path), Some(workspace_path.clone()));
        let dirs = ensure_runtime_dirs(&context)?;
        let expected_dirs = vec![
            root.path().join("instance"),
            workspace_path,
            root.path().join("instance").join("media"),
            root.path().join("instance").join("cron"),
            root.path().join("instance").join("logs"),
            root.path().join("instance").join("channels"),
            root.path()
                .join("instance")
                .join("channels")
                .join("worker-metadata"),
            root.path().join("instance").join("skills"),
        ];

        assert_eq!(dirs, expected_dirs);
        assert!(dirs.iter().all(|dir| dir.is_dir()));
        Ok(())
    }

    #[test]
    fn public_path_helpers_follow_active_config_context() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("instance").join("config.json");
        set_config_path(config_path.clone());

        assert_eq!(get_config_path(), config_path);
        assert_eq!(get_data_dir(), root.path().join("instance"));
        assert!(get_runtime_subdir("runtime").is_dir());
        assert!(get_media_dir(None).is_dir());
        assert!(get_media_dir(Some("api")).is_dir());
        assert!(get_cron_dir().is_dir());
        assert!(get_logs_dir().is_dir());

        let workspace = root.path().join("workspace");
        assert_eq!(get_workspace_path(Some(&workspace)), workspace);
        assert!(get_workspace_path(Some(&workspace)).is_dir());
        assert!(is_default_workspace(None));
        assert_eq!(get_cli_history_path(), cli_history_path());
        assert_eq!(get_bridge_install_dir(), bridge_install_dir());
        assert_eq!(get_legacy_sessions_dir(), legacy_sessions_dir());

        clear_config_path();
        Ok(())
    }

    #[test]
    fn load_config_or_default_matches_nanobot_invalid_config_fallback(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        fs::write(&config_path, "{not json")?;

        let bundle = load_config_with_env_or_default(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(root.path().join("fallback-workspace")),
                resolve_env: true,
                write_back_migrations: true,
            },
            &BTreeMap::<String, String>::new(),
        )?;

        assert_eq!(bundle.context.config_path, config_path);
        assert_eq!(bundle.context.data_dir, root.path());
        assert_eq!(
            bundle.context.workspace,
            root.path().join("fallback-workspace")
        );
        assert_eq!(bundle.config.agents.defaults.model, default_model());
        assert!(bundle.migrations.is_empty());
        Ok(())
    }

    #[test]
    fn load_config_or_default_does_not_hide_io_or_env_errors(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory_path = root.path().join("config-dir");
        fs::create_dir_all(&directory_path)?;
        let io_error = load_config_with_env_or_default(
            LoadOptions {
                config_path: Some(directory_path),
                workspace_override: None,
                resolve_env: true,
                write_back_migrations: true,
            },
            &BTreeMap::<String, String>::new(),
        )
        .expect_err("directory config path must remain an I/O error");
        assert!(matches!(io_error, ConfigError::Io(_)));

        let config_path = root.path().join("config.json");
        fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "providers": {"openrouter": {"apiKey": "${MISSING_KEY}"}}
            }))?,
        )?;
        let env_error = load_config_with_env_or_default(
            LoadOptions {
                config_path: Some(config_path),
                workspace_override: None,
                resolve_env: true,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )
        .expect_err("missing env refs must not fall back to defaults");
        assert!(matches!(env_error, ConfigError::Env(_)));
        Ok(())
    }

    #[test]
    fn migration_writeback_preserves_env_templates_and_does_not_persist_workspace_override(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let raw = json!({
            "agents": {"defaults": {"workspace": "~/original", "sessionTtlMinutes": 10}},
            "providers": {"openrouter": {"apiKey": "${OPENROUTER_API_KEY}"}}
        });
        fs::write(&config_path, serde_json::to_string_pretty(&raw)?)?;
        let env = BTreeMap::from([("OPENROUTER_API_KEY".to_owned(), "secret".to_owned())]);

        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(root.path().join("temporary-workspace")),
                resolve_env: true,
                write_back_migrations: true,
            },
            &env,
        )?;

        assert_eq!(
            bundle.config.providers["openrouter"].api_key.as_deref(),
            Some("secret")
        );
        assert_eq!(
            bundle.context.workspace,
            root.path().join("temporary-workspace")
        );
        let saved = fs::read_to_string(config_path)?;
        assert!(saved.contains("${OPENROUTER_API_KEY}"));
        assert!(!saved.contains("secret"));
        assert!(saved.contains("~/original"));
        assert!(!saved.contains("temporary-workspace"));
        assert!(saved.contains("idleCompactAfterMinutes"));
        assert!(!saved.contains("sessionTtlMinutes"));
        Ok(())
    }
}
