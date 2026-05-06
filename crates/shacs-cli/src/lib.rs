use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials as SmtpCredentials;
use lettre::{Message as EmailMessage, SmtpTransport, Transport};
use mailparse::MailHeaderMap;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use shacs_api::{
    chat_completion_invocation, ApiChatMessage, ApiError, ApiMessageContent, ApiModel,
    ChatCompletionAdapter, ChatCompletionInvocation, ChatCompletionRequest,
};
use shacs_channels::{
    builtin_live_worker_descriptors, normalize_websocket_frame, normalize_whatsapp_bridge_message,
    websocket_event_from_outbound, whatsapp_outbound_frames, ChannelCapabilities,
    ChannelDescriptor, ChannelRegistry, DiscordInbound, EmailInbound, LiveChannelWorkerDescriptor,
    LiveChannelWorkerKind, OutboundMessage, RecentMessageIds, SlackInbound, TelegramInbound,
    WebSocketInboundAction, WebSocketServerEvent, WhatsAppBridgeMessage, WhatsAppChannelConfig,
    WhatsAppGroupPolicy, WhatsAppOutboundFrame, DISCORD_CHANNEL, EMAIL_CHANNEL, SLACK_CHANNEL,
    TELEGRAM_CHANNEL, WEBSOCKET_CHANNEL, WHATSAPP_CHANNEL,
};
use shacs_config::{
    config_context, default_config_path, ensure_runtime_dirs, load_auth_store,
    load_config_with_env, save_auth_store_to_path, save_config_to_path, ApiConfig, ConfigBundle,
    ConfigError, EnvSource, LoadOptions, ProcessEnv, ProviderAuth, ProviderConfig,
};
use shacs_core::runtime::{
    find_legal_message_start, AgentLoop, AgentLoopConfig, AgentLoopTurnResult, ContextBuilder,
    DreamLifecycle, InboundMessage, McpLifecycle, MessageBus, RuntimeCapabilityReport,
    RuntimeCapabilityStatus, Session, SessionHistoryOptions, SessionManager,
    SubagentExecutionConfig, SubagentRuntime,
};
use shacs_core::tools::{
    AskUserTool, EditFileTool, ExecConfig, ExecTool, FileState, GlobTool, GrepTool, ListDirTool,
    McpRuntime, McpServerConnectionReport, McpServerSpec, NetworkGuard, PathContext, ReadFileTool,
    SelfRuntimeState, SelfTool, SpawnTool, StdioMcpConnector, ToolRegistry, WebFetchConfig,
    WebFetchTool, WebSearchConfig, WebSearchTool, WriteFileTool,
};
use shacs_providers::{
    chat_with_retry, prepare_provider_request, resolve_provider_client, AgentDefaults, LlmResponse,
    ProviderClient, ProviderError, ProviderRegistry, ProviderRetryMode, ResolvedProviderClient,
};
use shacs_skills::{
    discover_skill_registry, sync_builtin_skills, SkillRegistryEntry, SkillRegistryOptions,
    SkillRegistryStatus,
};
use shacs_templates::sync_workspace_templates;
use shacs_utils::media_decode::{save_base64_data_url, MediaDecodeError, DEFAULT_MAX_BYTES};
use shacs_utils::text::safe_filename;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_ISSUER: &str = "https://auth.openai.com";
const CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CODEX_DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const CODEX_DEVICE_URL: &str = "https://auth.openai.com/codex/device";
const CODEX_PROVIDER_ID: &str = "openai_codex";
const CODEX_DEFAULT_MODEL: &str = "gpt-5.4";
const CODEX_BROWSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CODEX_DEVICE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
type ApiProviderEventCallback = Arc<dyn Fn(&shacs_providers::ProviderEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub enum CliCommand {
    Help,
    Version,
    Onboard(OnboardOptions),
    Status(StatusOptions),
    RuntimeInspect(RuntimeInspectOptions),
    Session(SessionCommand),
    Skills(SkillsCommand),
    Channels(ChannelsCommand),
    Ask(AskOptions),
    Run(RunOptions),
    Serve(ServeOptions),
    Gateway(GatewayOptions),
    Web(WebOptions),
    Provider(ProviderCommand),
    Unsupported(UnsupportedCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OnboardOptions {
    pub config_path: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub wizard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusOptions {
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeInspectOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand {
    List(SessionListOptions),
    Inspect(SessionInspectOptions),
    Create(SessionCreateOptions),
    History(SessionHistoryCliOptions),
    Export(SessionExportOptions),
    Clear(SessionClearOptions),
    Diagnostics(SessionDiagnosticsOptions),
    Compact(SessionCompactOptions),
    Delete(SessionDeleteOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillsCommand {
    List(SkillsListOptions),
    Show(SkillsShowOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelsCommand {
    List(ChannelsListOptions),
    Status(ChannelsStatusOptions),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelsListOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelsStatusOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillsListOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub all: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillsShowOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionListOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionInspectOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionCreateOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHistoryCliOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
    pub max_messages: usize,
    pub max_tokens: usize,
    pub timestamps: bool,
    pub json: bool,
}

impl Default for SessionHistoryCliOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            workspace_override: None,
            session: String::new(),
            max_messages: 10,
            max_tokens: 0,
            timestamps: false,
            json: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionExportFormat {
    Json,
    Jsonl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExportOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
    pub format: SessionExportFormat,
    pub yes: bool,
}

impl Default for SessionExportOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            workspace_override: None,
            session: String::new(),
            format: SessionExportFormat::Json,
            yes: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionClearOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
    pub yes: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionDiagnosticsOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCompactOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
    pub keep_messages: usize,
    pub yes: bool,
}

impl Default for SessionCompactOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            workspace_override: None,
            session: String::new(),
            keep_messages: 8,
            yes: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionDeleteOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub session: String,
    pub yes: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AskOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub message: String,
    pub session: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub allow_side_effects: bool,
    pub markdown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShacsBotOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub allow_side_effects: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShacsBotRunOptions {
    pub message: String,
    pub session_key: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

impl Default for ShacsBotRunOptions {
    fn default() -> Self {
        Self {
            message: String::new(),
            session_key: "sdk:default".to_owned(),
            temperature: None,
            max_tokens: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub content: String,
    pub tools_used: Vec<String>,
    pub messages: Vec<Value>,
}

pub struct ShacsBot {
    adapter: AgentLoopChatCompletionAdapter,
}

pub type Nanobot = ShacsBot;

impl Default for AskOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            workspace_override: None,
            message: String::new(),
            session: None,
            temperature: None,
            max_tokens: None,
            allow_side_effects: false,
            markdown: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServeOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub bind: Option<SocketAddr>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub timeout: Option<f64>,
    pub verbose: bool,
    pub allow_remote: bool,
    pub allow_api_side_effects: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GatewayOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub port: Option<u16>,
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub gateway_port: Option<u16>,
    pub websocket_host: Option<String>,
    pub websocket_port: Option<u16>,
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub websocket_host: Option<String>,
    pub websocket_port: Option<u16>,
    pub timeout: Option<f64>,
    pub verbose: bool,
    pub allow_remote: bool,
    pub allow_side_effects: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderCommand {
    CodexImportToken(CodexImportTokenOptions),
    CodexLogin(CodexLoginOptions),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexImportTokenOptions {
    pub config_path: Option<PathBuf>,
    pub token_source: TokenSource,
    pub account_id: Option<String>,
    pub select: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    Stdin,
    Env(String),
    Literal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexLoginOptions {
    pub config_path: Option<PathBuf>,
    pub headless: bool,
    pub no_browser: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexImportOutcome {
    pub config_path: PathBuf,
    pub auth_path: PathBuf,
    pub provider: String,
    pub selected_model: Option<String>,
    pub selected: bool,
    pub has_account_id: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexLoginOutcome {
    pub config_path: PathBuf,
    pub auth_path: PathBuf,
    pub provider: String,
    pub selected_model: String,
    pub account_id: Option<String>,
    pub expires: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct CodexTokenResponse {
    access: String,
    refresh: Option<String>,
    expires: Option<u64>,
    account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct CodexAuthHttpResponse {
    status: u16,
    body: Value,
}

trait CodexAuthTransport {
    fn post_form(
        &self,
        url: &str,
        fields: &[(String, String)],
    ) -> Result<CodexAuthHttpResponse, CliError>;

    fn post_json(&self, url: &str, body: Value) -> Result<CodexAuthHttpResponse, CliError>;
}

struct UreqCodexAuthTransport {
    agent: ureq::Agent,
}

impl Default for UreqCodexAuthTransport {
    fn default() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(60)))
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }
}

impl CodexAuthTransport for UreqCodexAuthTransport {
    fn post_form(
        &self,
        url: &str,
        fields: &[(String, String)],
    ) -> Result<CodexAuthHttpResponse, CliError> {
        let body = fields
            .iter()
            .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
            .collect::<Vec<_>>()
            .join("&");
        let response = self
            .agent
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(body)
            .map_err(codex_auth_transport_error)?;
        codex_auth_response(response)
    }

    fn post_json(&self, url: &str, body: Value) -> Result<CodexAuthHttpResponse, CliError> {
        let body = serde_json::to_string(&body).map_err(|error| {
            CliError::InvalidArguments(format!("Codex auth JSON could not be serialized: {error}"))
        })?;
        let response = self
            .agent
            .post(url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "shacs-bot (rust)")
            .send(body)
            .map_err(codex_auth_transport_error)?;
        codex_auth_response(response)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfigOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub resolve_env: bool,
}

impl Default for RuntimeConfigOptions {
    fn default() -> Self {
        Self {
            config_path: None,
            workspace_override: None,
            resolve_env: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCommand {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardOutcome {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub runtime_dirs: Vec<PathBuf>,
    pub template_files: Vec<String>,
    pub template_dirs: Vec<String>,
    pub migrations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    pub config_path: PathBuf,
    pub config_exists: bool,
    pub workspace: PathBuf,
    pub workspace_exists: bool,
    pub model: String,
    pub provider: String,
    pub providers: Vec<ProviderStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub name: String,
    pub has_api_key: bool,
    pub has_api_base: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInspectReport {
    pub config_path: PathBuf,
    pub config_exists: bool,
    pub workspace: PathBuf,
    pub workspace_exists: bool,
    pub data_dir: PathBuf,
    pub model: String,
    pub provider: String,
    pub providers: Vec<ProviderStatus>,
    pub capabilities: Vec<RuntimeCapabilityReport>,
    pub sessions: RuntimeSessionInspect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSessionInspect {
    pub count: usize,
    pub latest_key: Option<String>,
    pub latest_updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsListReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub entries: Vec<SkillRegistryEntry>,
    pub all: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsShowReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub entry: SkillRegistryEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelsReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub channels: Vec<ChannelReportItem>,
    pub unknown_plugins: Vec<String>,
    pub worker_count: usize,
    pub send_progress: bool,
    pub send_tool_hints: bool,
    pub send_max_retries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelReportItem {
    pub descriptor: ChannelDescriptor,
    pub configured: bool,
    pub enabled: bool,
    pub workers: Vec<LiveChannelWorkerDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayPresetReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub addr: SocketAddr,
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebPresetReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub gateway_addr: SocketAddr,
    pub websocket: WebsocketPreset,
    pub assets_dir: PathBuf,
    pub assets_populated: bool,
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsocketPreset {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRuntimeReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub websocket: WebsocketPreset,
    pub websocket_addr: SocketAddr,
    pub workers: Vec<ChannelRuntimeWorkerReport>,
    pub verbose: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRuntimeWorkerReport {
    pub descriptor: LiveChannelWorkerDescriptor,
    pub enabled: bool,
    pub state: ChannelRuntimeWorkerState,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRuntimeWorkerState {
    Started,
    SkippedDisabled,
    SkippedMissingCredentials,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelegramRuntimeConfig {
    token: String,
    poll_timeout_seconds: u64,
    poll_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscordRuntimeConfig {
    token: String,
    channel_ids: Vec<String>,
    poll_interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlackRuntimeConfig {
    bot_token: String,
    channel_ids: Vec<String>,
    poll_interval_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmailRuntimeConfig {
    smtp: Option<EmailSmtpRuntimeConfig>,
    imap: Option<EmailImapRuntimeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmailSmtpRuntimeConfig {
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    from: String,
    security: EmailSecurity,
    timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmailImapRuntimeConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    mailbox: String,
    security: EmailSecurity,
    poll_interval_seconds: u64,
    timeout_seconds: u64,
    mark_seen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmailSecurity {
    Tls,
    StartTls,
    Plain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WhatsAppRuntimeConfig {
    bridge_url: String,
    bridge_token: Option<String>,
    poll_path: String,
    send_path: String,
    poll_interval_seconds: u64,
    allowlist: shacs_channels::ChannelAllowlist,
    group_policy: WhatsAppGroupPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExternalTransportSpec {
    Telegram(TelegramRuntimeConfig),
    Discord(DiscordRuntimeConfig),
    Slack(SlackRuntimeConfig),
    Email(EmailRuntimeConfig),
    WhatsApp(WhatsAppRuntimeConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListReport {
    pub workspace: PathBuf,
    pub sessions: Vec<SessionListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionListItem {
    pub key: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInspectReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub message_count: usize,
    pub metadata_keys: Vec<String>,
    pub last_consolidated: usize,
    pub recovery_markers: Vec<String>,
    pub checkpoint_phase: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCreateReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionHistoryCliReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub history: Vec<Value>,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionExportReport {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionClearReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub cleared: bool,
    pub message_count_before: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDiagnosticsReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub exists: bool,
    pub message_count: usize,
    pub last_consolidated: usize,
    pub metadata_keys: Vec<String>,
    pub recovery_markers: Vec<String>,
    pub checkpoint_phase: Option<String>,
    pub legal_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCompactReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub compacted: bool,
    pub kept_messages: usize,
    pub archived_messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDeleteReport {
    pub workspace: PathBuf,
    pub key: String,
    pub path: PathBuf,
    pub deleted: bool,
}

#[derive(Debug)]
pub enum CliError {
    Api(ApiError),
    Config(ConfigError),
    Io(std::io::Error),
    InvalidArguments(String),
    Provider(ProviderError),
    Unsupported(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(error) => write!(formatter, "{error}"),
            Self::Config(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "CLI I/O failed: {error}"),
            Self::InvalidArguments(error) => write!(formatter, "invalid CLI arguments: {error}"),
            Self::Provider(error) => write!(formatter, "{error}"),
            Self::Unsupported(error) => write!(formatter, "unsupported command: {error}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<ConfigError> for CliError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ApiError> for CliError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl From<ProviderError> for CliError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

pub fn version() -> &'static str {
    VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliRenderMode {
    InteractiveTerminal,
    PlainText,
}

pub fn cli_render_mode(stdout_is_tty: bool) -> CliRenderMode {
    if stdout_is_tty {
        CliRenderMode::InteractiveTerminal
    } else {
        CliRenderMode::PlainText
    }
}

pub fn render_agent_response(
    content: impl AsRef<str>,
    _render_markdown: bool,
    metadata: Option<&Value>,
) -> String {
    let content = content.as_ref();
    if metadata
        .and_then(|value| value.get("render_as"))
        .and_then(Value::as_str)
        == Some("text")
    {
        return content.to_owned();
    }
    content.to_owned()
}

pub fn get_all_models() -> Vec<String> {
    Vec::new()
}

pub fn find_model_info(_model_name: &str) -> Option<Value> {
    None
}

pub fn get_model_context_limit(_model: &str, _provider: &str) -> Option<usize> {
    None
}

pub fn get_model_suggestions(_partial: &str, _provider: &str, _limit: usize) -> Vec<String> {
    Vec::new()
}

pub fn format_token_count(tokens: usize) -> String {
    let raw = tokens.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    let first_group = raw.len() % 3;
    for (index, ch) in raw.chars().enumerate() {
        if index > 0
            && (index == first_group || (index > first_group && (index - first_group) % 3 == 0))
        {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl ShacsBot {
    pub fn from_config(
        config_path: Option<PathBuf>,
        workspace_override: Option<PathBuf>,
    ) -> Result<Self, CliError> {
        Self::from_options(ShacsBotOptions {
            config_path,
            workspace_override,
            allow_side_effects: false,
        })
    }

    pub fn from_options(options: ShacsBotOptions) -> Result<Self, CliError> {
        let bundle = load_runtime_config(RuntimeConfigOptions {
            config_path: options.config_path,
            workspace_override: options.workspace_override,
            resolve_env: true,
        })?;
        let adapter =
            AgentLoopChatCompletionAdapter::from_bundle(bundle, options.allow_side_effects)?;
        Ok(Self { adapter })
    }

    pub fn run(&self, message: impl Into<String>) -> Result<RunResult, CliError> {
        self.run_with_options(ShacsBotRunOptions {
            message: message.into(),
            ..ShacsBotRunOptions::default()
        })
    }

    pub fn run_with_options(&self, options: ShacsBotRunOptions) -> Result<RunResult, CliError> {
        if options.message.trim().is_empty() {
            return Err(CliError::InvalidArguments(
                "ShacsBot::run requires a non-empty message".to_owned(),
            ));
        }
        if options.session_key.trim().is_empty() {
            return Err(CliError::InvalidArguments(
                "ShacsBot::run requires a non-empty session_key".to_owned(),
            ));
        }
        let mut invocation = sdk_message_invocation(self.adapter.configured_model(), &options)?;
        invocation.session_key = options.session_key.trim().to_owned();
        self.adapter
            .complete_sdk_run(invocation)
            .map_err(Into::into)
    }
}

pub fn run_from_env() -> Result<String, CliError> {
    let command = parse_cli_args(std::env::args().skip(1))?;
    run_command(command)
}

pub fn run_command(command: CliCommand) -> Result<String, CliError> {
    match command {
        CliCommand::Help => Ok(help_text()),
        CliCommand::Version => Ok(format!("shacs-bot {VERSION}")),
        CliCommand::Onboard(options) => onboard(options).map(format_onboard_outcome),
        CliCommand::Status(options) => status(options).map(format_status_report),
        CliCommand::RuntimeInspect(options) => runtime_inspect(options).map(format_runtime_inspect),
        CliCommand::Session(command) => run_session_command(command),
        CliCommand::Skills(command) => run_skills_command(command),
        CliCommand::Channels(command) => run_channels_command(command),
        CliCommand::Ask(options) => ask(options),
        CliCommand::Run(options) => run_runtime(options),
        CliCommand::Serve(options) => serve(options),
        CliCommand::Gateway(options) => gateway_preset(options).map(format_gateway_preset_report),
        CliCommand::Web(options) => web_preset(options).map(format_web_preset_report),
        CliCommand::Provider(command) => run_provider_command(command),
        CliCommand::Unsupported(command) => Err(CliError::Unsupported(format!(
            "{} ({})",
            command.name, command.reason
        ))),
    }
}

pub fn parse_cli_args<I, S>(args: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    if args.is_empty() {
        return Ok(CliCommand::Help);
    }

    let mut parser = ArgParser::new(args);
    let global_config = parse_global_config(&mut parser)?;
    let Some(command) = parser.next() else {
        return Ok(CliCommand::Help);
    };
    match command.as_str() {
        "--help" | "-h" => Ok(CliCommand::Help),
        "--version" | "-v" => Ok(CliCommand::Version),
        "onboard" => parse_onboard(parser, global_config),
        "status" => parse_status(parser, global_config),
        "runtime" => parse_runtime(parser, global_config),
        "session" | "sessions" => parse_session(parser, global_config),
        "skills" | "skill" => parse_skills(parser, global_config),
        "channels" | "channel" => parse_channels(parser, global_config),
        "ask" => parse_ask(parser, global_config, false),
        "agent" => parse_ask(parser, global_config, true),
        "run" => parse_run(parser, global_config),
        "serve" => parse_serve(parser, global_config),
        "api" => parse_api(parser, global_config),
        "gateway" => parse_gateway(parser, global_config),
        "web" => parse_web(parser, global_config),
        "provider" => parse_provider(parser, global_config),
        "plugins" => Ok(CliCommand::Unsupported(UnsupportedCommand {
            name: command,
            reason: "command surface is reserved for a later runtime/channel slice".to_owned(),
        })),
        other => Err(CliError::InvalidArguments(format!(
            "unknown command `{other}`"
        ))),
    }
}

fn parse_api(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "api requires `serve`".to_owned(),
        ));
    };
    match action.as_str() {
        "serve" => parse_serve(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown api action `{other}`"
        ))),
    }
}

fn parse_runtime(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "runtime requires `inspect`".to_owned(),
        ));
    };
    match action.as_str() {
        "inspect" => parse_runtime_inspect(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown runtime action `{other}`"
        ))),
    }
}

pub fn load_runtime_config(options: RuntimeConfigOptions) -> Result<ConfigBundle, CliError> {
    load_runtime_config_with_env(options, &ProcessEnv)
}

pub fn load_runtime_config_with_env(
    options: RuntimeConfigOptions,
    env: &impl EnvSource,
) -> Result<ConfigBundle, CliError> {
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: options.config_path,
            workspace_override: options.workspace_override,
            resolve_env: options.resolve_env,
            write_back_migrations: true,
        },
        env,
    )?;
    apply_codex_auth_overlay(&mut bundle)?;
    Ok(bundle)
}

fn apply_codex_auth_overlay(bundle: &mut ConfigBundle) -> Result<(), CliError> {
    apply_codex_auth_overlay_with_transport(bundle, &UreqCodexAuthTransport::default())
}

fn apply_codex_auth_overlay_with_transport(
    bundle: &mut ConfigBundle,
    transport: &impl CodexAuthTransport,
) -> Result<(), CliError> {
    let auth_path = bundle.context.auth_path();
    let mut auth = load_auth_store(&auth_path)?;
    let Some(mut codex_auth) = auth.providers.get(CODEX_PROVIDER_ID).cloned() else {
        return Ok(());
    };
    if codex_auth_is_expired(&codex_auth) {
        let Some(refresh) = codex_auth
            .refresh
            .as_deref()
            .filter(|value| !value.is_empty())
        else {
            return Err(CliError::Provider(ProviderError::AuthRequired {
                provider_id: CODEX_PROVIDER_ID.to_owned(),
            }));
        };
        let refreshed = refresh_codex_token(transport, refresh)?;
        codex_auth.access = refreshed.access;
        codex_auth.refresh = refreshed.refresh.or(codex_auth.refresh);
        codex_auth.expires = refreshed.expires.or(codex_auth.expires);
        codex_auth.account_id = refreshed.account_id.or(codex_auth.account_id);
        auth.providers
            .insert(CODEX_PROVIDER_ID.to_owned(), codex_auth.clone());
        save_auth_store_to_path(&auth, &auth_path)?;
    }
    let provider = bundle
        .config
        .providers
        .entry(CODEX_PROVIDER_ID.to_owned())
        .or_insert_with(codex_provider_config);
    provider.api_key = Some(codex_auth.access.clone());
    if let Some(account_id) = codex_auth
        .account_id
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        let headers = provider.extra_headers.get_or_insert_with(BTreeMap::new);
        headers.insert("ChatGPT-Account-Id".to_owned(), account_id.clone());
    }
    Ok(())
}

fn codex_auth_is_expired(auth: &ProviderAuth) -> bool {
    auth.expires
        .is_some_and(|expires| expires <= now_millis().saturating_add(60_000))
}

pub fn onboard(options: OnboardOptions) -> Result<OnboardOutcome, CliError> {
    if options.wizard {
        return Err(CliError::Unsupported(
            "onboard --wizard is deferred until interactive CLI support is migrated".to_owned(),
        ));
    }

    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: true,
        },
        &ProcessEnv,
    )?;

    if let Some(workspace) = options.workspace {
        bundle.config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    }

    save_config_to_path(&bundle.config, &config_path)?;
    let context = config_context(
        Some(config_path.clone()),
        Some(bundle.config.workspace_path()),
    );
    let runtime_dirs = ensure_runtime_dirs(&context)?;
    let mut templates = sync_workspace_templates(&context.workspace)?;
    let builtin_skills = sync_builtin_skills(&context.workspace)?;
    templates.created_files.extend(builtin_skills.created_files);
    templates.created_dirs.extend(builtin_skills.created_dirs);

    Ok(OnboardOutcome {
        config_path,
        workspace: context.workspace,
        runtime_dirs,
        template_files: templates.created_files,
        template_dirs: templates.created_dirs,
        migrations: bundle
            .migrations
            .into_iter()
            .map(|migration| migration.key)
            .collect(),
    })
}

pub fn status(options: StatusOptions) -> Result<StatusReport, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let config_exists = config_path.exists();
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let workspace = bundle.context.workspace;
    let workspace_exists = workspace.exists();
    let mut providers = bundle
        .config
        .providers
        .iter()
        .map(|(name, config)| ProviderStatus {
            name: name.clone(),
            has_api_key: non_empty(config.api_key.as_deref()),
            has_api_base: non_empty(config.api_base.as_deref()),
        })
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(StatusReport {
        config_path,
        config_exists,
        workspace,
        workspace_exists,
        model: bundle.config.agents.defaults.model,
        provider: bundle.config.agents.defaults.provider,
        providers,
    })
}

pub fn runtime_inspect(options: RuntimeInspectOptions) -> Result<RuntimeInspectReport, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let config_exists = config_path.exists();
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let workspace = bundle.context.workspace.clone();
    let workspace_exists = workspace.exists();
    let mut providers = bundle
        .config
        .providers
        .iter()
        .map(|(name, config)| ProviderStatus {
            name: name.clone(),
            has_api_key: non_empty(config.api_key.as_deref()),
            has_api_base: non_empty(config.api_base.as_deref()),
        })
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    let capabilities = runtime_capabilities(&bundle);
    let sessions = inspect_runtime_sessions(&workspace)?;

    Ok(RuntimeInspectReport {
        config_path,
        config_exists,
        workspace,
        workspace_exists,
        data_dir: bundle.context.data_dir,
        model: bundle.config.agents.defaults.model,
        provider: bundle.config.agents.defaults.provider,
        providers,
        capabilities,
        sessions,
    })
}

fn runtime_capabilities(bundle: &ConfigBundle) -> Vec<RuntimeCapabilityReport> {
    let provider = bundle.config.agents.defaults.provider.as_str();
    let dream = bundle
        .config
        .providers
        .get(provider)
        .filter(|config| {
            non_empty(config.api_key.as_deref()) || non_empty(config.api_base.as_deref())
        })
        .map(|_| DreamLifecycle::configured())
        .unwrap_or_default();
    vec![
        McpLifecycle::configured(bundle.config.tools.mcp_servers.len()).status(),
        dream.status(),
        SubagentRuntime::new().status(),
    ]
}

fn inspect_runtime_sessions(workspace: &Path) -> Result<RuntimeSessionInspect, CliError> {
    if !workspace.join("sessions").exists() {
        return Ok(RuntimeSessionInspect {
            count: 0,
            latest_key: None,
            latest_updated_at: None,
        });
    }
    let sessions = SessionManager::new(workspace)?.list_sessions()?;
    let latest = sessions.first();
    Ok(RuntimeSessionInspect {
        count: sessions.len(),
        latest_key: latest.map(|session| session.key.clone()),
        latest_updated_at: latest.and_then(|session| session.updated_at.clone()),
    })
}

fn run_session_command(command: SessionCommand) -> Result<String, CliError> {
    match command {
        SessionCommand::List(options) => session_list(options).map(format_session_list),
        SessionCommand::Inspect(options) => session_inspect(options).map(format_session_inspect),
        SessionCommand::Create(options) => session_create(options).map(format_session_create),
        SessionCommand::History(options) => session_history(options).map(format_session_history),
        SessionCommand::Export(options) => session_export(options).map(|report| report.content),
        SessionCommand::Clear(options) => session_clear(options).map(format_session_clear),
        SessionCommand::Diagnostics(options) => {
            session_diagnostics(options).map(format_session_diagnostics)
        }
        SessionCommand::Compact(options) => session_compact(options).map(format_session_compact),
        SessionCommand::Delete(options) => session_delete(options).map(format_session_delete),
    }
}

fn run_skills_command(command: SkillsCommand) -> Result<String, CliError> {
    match command {
        SkillsCommand::List(options) => skills_list(options).map(format_skills_list),
        SkillsCommand::Show(options) => skills_show(options).map(format_skills_show),
    }
}

fn run_channels_command(command: ChannelsCommand) -> Result<String, CliError> {
    match command {
        ChannelsCommand::List(options) => channels_list(options).map(format_channels_list),
        ChannelsCommand::Status(options) => channels_status(options).map(format_channels_status),
    }
}

pub fn channels_list(options: ChannelsListOptions) -> Result<ChannelsReport, CliError> {
    load_channels_report(options.config_path, options.workspace_override)
}

pub fn channels_status(options: ChannelsStatusOptions) -> Result<ChannelsReport, CliError> {
    load_channels_report(options.config_path, options.workspace_override)
}

pub fn skills_list(options: SkillsListOptions) -> Result<SkillsListReport, CliError> {
    let (config_path, workspace, entries) = load_skill_registry(
        options.config_path.clone(),
        options.workspace_override.clone(),
    )?;
    let mut entries = entries;
    if !options.all {
        entries.retain(|entry| entry.status == SkillRegistryStatus::Active);
    }
    entries.sort_by(|left, right| left.descriptor.name.cmp(&right.descriptor.name));
    Ok(SkillsListReport {
        config_path,
        workspace,
        entries,
        all: options.all,
    })
}

pub fn skills_show(options: SkillsShowOptions) -> Result<SkillsShowReport, CliError> {
    if options.name.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "skills show requires a skill name".to_owned(),
        ));
    }
    let (config_path, workspace, entries) = load_skill_registry(
        options.config_path.clone(),
        options.workspace_override.clone(),
    )?;
    let entry = entries
        .iter()
        .find(|entry| {
            entry.status == SkillRegistryStatus::Active && entry.descriptor.name == options.name
        })
        .or_else(|| {
            entries
                .iter()
                .find(|entry| entry.descriptor.name == options.name)
        })
        .cloned()
        .ok_or_else(|| {
            CliError::InvalidArguments(format!("unknown skill `{}`", options.name.trim()))
        })?;
    Ok(SkillsShowReport {
        config_path,
        workspace,
        entry,
    })
}

fn load_skill_registry(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf, Vec<SkillRegistryEntry>), CliError> {
    let config_path = config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let mut options = SkillRegistryOptions::new(bundle.context.workspace.clone());
    options.user_skills_dir = Some(bundle.context.data_dir.join("skills"));
    let registry = discover_skill_registry(options)?;
    Ok((config_path, bundle.context.workspace, registry.entries))
}

fn load_channels_report(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Result<ChannelsReport, CliError> {
    let config_path = config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let registry = ChannelRegistry::with_builtin_channels();
    let workers = builtin_live_worker_descriptors();
    let known = registry
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let mut channels = registry
        .names()
        .into_iter()
        .filter_map(|name| registry.get(name).cloned())
        .map(|descriptor| {
            let configured = bundle
                .config
                .channels
                .plugins
                .contains_key(&descriptor.name);
            let enabled = channel_enabled_from_plugins(
                &bundle.config.channels.plugins,
                &descriptor.name,
                descriptor.enabled_by_default,
            );
            let workers = workers
                .iter()
                .filter(|worker| worker.channel == descriptor.name)
                .cloned()
                .collect::<Vec<_>>();
            ChannelReportItem {
                descriptor,
                configured,
                enabled,
                workers,
            }
        })
        .collect::<Vec<_>>();
    channels.sort_by(|left, right| left.descriptor.name.cmp(&right.descriptor.name));
    let mut unknown_plugins = bundle
        .config
        .channels
        .plugins
        .keys()
        .filter(|name| !known.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    unknown_plugins.sort();
    Ok(ChannelsReport {
        config_path,
        workspace: bundle.context.workspace,
        channels,
        unknown_plugins,
        worker_count: workers.len(),
        send_progress: bundle.config.channels.send_progress,
        send_tool_hints: bundle.config.channels.send_tool_hints,
        send_max_retries: bundle.config.channels.send_max_retries,
    })
}

fn channel_enabled_from_plugins(
    plugins: &BTreeMap<String, Value>,
    name: &str,
    default_enabled: bool,
) -> bool {
    plugins
        .get(name)
        .and_then(Value::as_object)
        .and_then(|object| object.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(default_enabled)
}

pub fn channel_runtime_plan(options: RunOptions) -> Result<ChannelRuntimeReport, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    channel_runtime_report_from_bundle(&bundle, &options)
}

fn channel_runtime_report_from_bundle(
    bundle: &ConfigBundle,
    options: &RunOptions,
) -> Result<ChannelRuntimeReport, CliError> {
    let web_options = WebOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        gateway_port: None,
        websocket_host: options.websocket_host.clone(),
        websocket_port: options.websocket_port,
        verbose: options.verbose,
    };
    let websocket = websocket_preset(&bundle.config.channels.plugins, &web_options)?;
    let websocket_addr = resolve_websocket_addr(&websocket)?;
    if websocket.enabled {
        validate_run_security(options, websocket_addr)?;
    }
    let registry = ChannelRegistry::with_builtin_channels();
    let workers = builtin_live_worker_descriptors()
        .into_iter()
        .map(|descriptor| {
            let default_enabled = registry
                .get(&descriptor.channel)
                .map(|channel| channel.enabled_by_default)
                .unwrap_or(false);
            let enabled = channel_enabled_from_plugins(
                &bundle.config.channels.plugins,
                &descriptor.channel,
                default_enabled,
            );
            let (state, detail) = runtime_worker_state(
                &bundle.config.channels.plugins,
                &descriptor,
                enabled,
                &websocket,
            );
            ChannelRuntimeWorkerReport {
                descriptor,
                enabled,
                state,
                detail,
            }
        })
        .collect::<Vec<_>>();
    Ok(ChannelRuntimeReport {
        config_path: bundle.context.config_path.clone(),
        workspace: bundle.context.workspace.clone(),
        websocket,
        websocket_addr,
        workers,
        verbose: options.verbose,
    })
}

fn runtime_worker_state(
    plugins: &BTreeMap<String, Value>,
    descriptor: &LiveChannelWorkerDescriptor,
    enabled: bool,
    websocket: &WebsocketPreset,
) -> (ChannelRuntimeWorkerState, String) {
    if !enabled {
        return (
            ChannelRuntimeWorkerState::SkippedDisabled,
            "channel disabled by config".to_owned(),
        );
    }
    if descriptor.channel == WEBSOCKET_CHANNEL {
        if websocket.enabled && descriptor.ready_for_runtime {
            return (
                ChannelRuntimeWorkerState::Started,
                format!(
                    "listening on ws://{}:{}{}",
                    websocket.host, websocket.port, websocket.path
                ),
            );
        }
        return (
            ChannelRuntimeWorkerState::SkippedDisabled,
            "websocket channel disabled by config".to_owned(),
        );
    }
    if descriptor.requires_external_credentials && !worker_has_credentials(plugins, descriptor) {
        return (
            ChannelRuntimeWorkerState::SkippedMissingCredentials,
            "missing channel credentials/config".to_owned(),
        );
    }
    (
        ChannelRuntimeWorkerState::Started,
        "worker eligible for startup".to_owned(),
    )
}

fn worker_has_credentials(
    plugins: &BTreeMap<String, Value>,
    descriptor: &LiveChannelWorkerDescriptor,
) -> bool {
    match descriptor.kind {
        LiveChannelWorkerKind::TelegramLongPolling => telegram_runtime_config(plugins).is_some(),
        LiveChannelWorkerKind::DiscordGateway => discord_runtime_config(plugins).is_some(),
        LiveChannelWorkerKind::SlackSocketMode => slack_runtime_config(plugins).is_some(),
        LiveChannelWorkerKind::EmailSmtp => email_runtime_config(plugins)
            .and_then(|config| config.smtp)
            .is_some(),
        LiveChannelWorkerKind::EmailImap => email_runtime_config(plugins)
            .and_then(|config| config.imap)
            .is_some(),
        LiveChannelWorkerKind::WhatsAppBridge => whatsapp_runtime_config(plugins).is_some(),
        LiveChannelWorkerKind::WebSocketServer => true,
    }
}

fn external_transport_specs(plugins: &BTreeMap<String, Value>) -> Vec<ExternalTransportSpec> {
    let mut specs = Vec::new();
    if channel_enabled_from_plugins(plugins, TELEGRAM_CHANNEL, false) {
        if let Some(config) = telegram_runtime_config(plugins) {
            specs.push(ExternalTransportSpec::Telegram(config));
        }
    }
    if channel_enabled_from_plugins(plugins, DISCORD_CHANNEL, false) {
        if let Some(config) = discord_runtime_config(plugins) {
            specs.push(ExternalTransportSpec::Discord(config));
        }
    }
    if channel_enabled_from_plugins(plugins, SLACK_CHANNEL, false) {
        if let Some(config) = slack_runtime_config(plugins) {
            specs.push(ExternalTransportSpec::Slack(config));
        }
    }
    if channel_enabled_from_plugins(plugins, EMAIL_CHANNEL, false) {
        if let Some(config) = email_runtime_config(plugins) {
            if config.smtp.is_some() || config.imap.is_some() {
                specs.push(ExternalTransportSpec::Email(config));
            }
        }
    }
    if channel_enabled_from_plugins(plugins, WHATSAPP_CHANNEL, false) {
        if let Some(config) = whatsapp_runtime_config(plugins) {
            specs.push(ExternalTransportSpec::WhatsApp(config));
        }
    }
    specs
}

fn runtime_needs_process(report: &ChannelRuntimeReport, specs: &[ExternalTransportSpec]) -> bool {
    report.websocket.enabled || !specs.is_empty()
}

fn telegram_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<TelegramRuntimeConfig> {
    let object = plugin_object(plugins, TELEGRAM_CHANNEL)?;
    let token = plugin_string_alias(object, &["botToken", "bot_token", "token"])?;
    Some(TelegramRuntimeConfig {
        token,
        poll_timeout_seconds: plugin_u64_alias(
            object,
            &["pollTimeoutSeconds", "poll_timeout_seconds"],
        )
        .unwrap_or(30)
        .max(1),
        poll_limit: plugin_u64_alias(object, &["pollLimit", "poll_limit"])
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(20)
            .max(1),
    })
}

fn discord_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<DiscordRuntimeConfig> {
    let object = plugin_object(plugins, DISCORD_CHANNEL)?;
    let token = plugin_string_alias(object, &["botToken", "bot_token", "token"])?;
    let mut channel_ids = plugin_string_array_alias(
        object,
        &[
            "channelIds",
            "allowedChannelIds",
            "channel_ids",
            "allowed_channel_ids",
        ],
    );
    if let Some(default_channel) = plugin_string_alias(
        object,
        &[
            "defaultChannelId",
            "default_channel_id",
            "channelId",
            "channel_id",
        ],
    ) {
        if !channel_ids.contains(&default_channel) {
            channel_ids.push(default_channel);
        }
    }
    if channel_ids.is_empty() {
        return None;
    }
    Some(DiscordRuntimeConfig {
        token,
        channel_ids,
        poll_interval_seconds: plugin_u64_alias(
            object,
            &["pollIntervalSeconds", "poll_interval_seconds"],
        )
        .unwrap_or(5)
        .max(1),
    })
}

fn slack_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<SlackRuntimeConfig> {
    let object = plugin_object(plugins, SLACK_CHANNEL)?;
    let bot_token = plugin_string_alias(object, &["botToken", "bot_token", "token"])?;
    let mut channel_ids = plugin_string_array_alias(
        object,
        &[
            "channelIds",
            "allowedChannelIds",
            "channel_ids",
            "allowed_channel_ids",
        ],
    );
    if let Some(default_channel) = plugin_string_alias(
        object,
        &[
            "defaultChannelId",
            "default_channel_id",
            "channelId",
            "channel_id",
        ],
    ) {
        if !channel_ids.contains(&default_channel) {
            channel_ids.push(default_channel);
        }
    }
    if channel_ids.is_empty() {
        return None;
    }
    Some(SlackRuntimeConfig {
        bot_token,
        channel_ids,
        poll_interval_seconds: plugin_u64_alias(
            object,
            &["pollIntervalSeconds", "poll_interval_seconds"],
        )
        .unwrap_or(5)
        .max(1),
    })
}

fn email_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<EmailRuntimeConfig> {
    let object = plugin_object(plugins, EMAIL_CHANNEL)?;
    Some(EmailRuntimeConfig {
        smtp: email_smtp_runtime_config(object),
        imap: email_imap_runtime_config(object),
    })
}

fn email_smtp_runtime_config(object: &Map<String, Value>) -> Option<EmailSmtpRuntimeConfig> {
    let smtp = nested_object(object, "smtp").unwrap_or(object);
    let host = plugin_string_alias(smtp, &["host", "smtpHost", "smtp_host"])?;
    let from = plugin_string_alias(smtp, &["from", "fromEmail", "from_email"])?;
    Some(EmailSmtpRuntimeConfig {
        host,
        port: plugin_u64_alias(smtp, &["port", "smtpPort", "smtp_port"])
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(587),
        username: plugin_string_alias(smtp, &["username", "user"]),
        password: plugin_string_alias(smtp, &["password"]),
        from,
        security: email_security(smtp),
        timeout_seconds: plugin_u64_alias(smtp, &["timeoutSeconds", "timeout_seconds"])
            .unwrap_or(30)
            .max(1),
    })
}

fn email_imap_runtime_config(object: &Map<String, Value>) -> Option<EmailImapRuntimeConfig> {
    let imap = nested_object(object, "imap").unwrap_or(object);
    let host = plugin_string_alias(imap, &["host", "imapHost", "imap_host"])?;
    let username = plugin_string_alias(imap, &["username", "user"])?;
    let password = plugin_string_alias(imap, &["password"])?;
    Some(EmailImapRuntimeConfig {
        host,
        port: plugin_u64_alias(imap, &["port", "imapPort", "imap_port"])
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(993),
        username,
        password,
        mailbox: plugin_string_alias(imap, &["mailbox"]).unwrap_or_else(|| "INBOX".to_owned()),
        security: email_security(imap),
        poll_interval_seconds: plugin_u64_alias(
            imap,
            &["pollIntervalSeconds", "poll_interval_seconds"],
        )
        .unwrap_or(60)
        .max(1),
        timeout_seconds: plugin_u64_alias(imap, &["timeoutSeconds", "timeout_seconds"])
            .unwrap_or(30)
            .max(1),
        mark_seen: plugin_bool_alias(imap, &["markSeen", "mark_seen"]).unwrap_or(true),
    })
}

fn email_security(object: &Map<String, Value>) -> EmailSecurity {
    match plugin_string_alias(object, &["security", "tls"])
        .unwrap_or_else(|| "tls".to_owned())
        .to_ascii_lowercase()
        .as_str()
    {
        "plain" | "none" | "false" => EmailSecurity::Plain,
        "starttls" => EmailSecurity::StartTls,
        _ => EmailSecurity::Tls,
    }
}

fn whatsapp_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<WhatsAppRuntimeConfig> {
    let object = plugin_object(plugins, WHATSAPP_CHANNEL)?;
    let bridge_url =
        plugin_string_alias(object, &["bridgeUrl", "bridge_url", "baseUrl", "base_url"])?;
    let allowed = nested_object(object, "allowlist")
        .map(|allowlist| {
            plugin_string_array_alias(allowlist, &["allowedSenders", "allowed_senders"])
        })
        .unwrap_or_default();
    let group_policy = match plugin_string_alias(object, &["groupPolicy", "group_policy"])
        .unwrap_or_else(|| "open".to_owned())
        .to_ascii_lowercase()
        .as_str()
    {
        "mention" => WhatsAppGroupPolicy::Mention,
        _ => WhatsAppGroupPolicy::Open,
    };
    Some(WhatsAppRuntimeConfig {
        bridge_url,
        bridge_token: plugin_string_alias(object, &["bridgeToken", "bridge_token", "token"]),
        poll_path: plugin_string_alias(object, &["pollPath", "poll_path"])
            .unwrap_or_else(|| "/messages".to_owned()),
        send_path: plugin_string_alias(object, &["sendPath", "send_path"])
            .unwrap_or_else(|| "/send".to_owned()),
        poll_interval_seconds: plugin_u64_alias(
            object,
            &["pollIntervalSeconds", "poll_interval_seconds"],
        )
        .unwrap_or(2)
        .max(1),
        allowlist: if allowed.is_empty() {
            shacs_channels::ChannelAllowlist::allow_all()
        } else {
            shacs_channels::ChannelAllowlist::new(allowed)
        },
        group_policy,
    })
}

fn plugin_object<'a>(
    plugins: &'a BTreeMap<String, Value>,
    name: &str,
) -> Option<&'a Map<String, Value>> {
    plugins.get(name).and_then(Value::as_object)
}

fn nested_object<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    object.get(key).and_then(Value::as_object)
}

fn plugin_string_alias(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn plugin_string_array_alias(object: &Map<String, Value>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn plugin_u64_alias(object: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        let value = object.get(*key)?;
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
    })
}

fn plugin_bool_alias(object: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        let value = object.get(*key)?;
        value.as_bool().or_else(|| match value.as_str()?.trim() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        })
    })
}

fn resolve_websocket_addr(websocket: &WebsocketPreset) -> Result<SocketAddr, CliError> {
    let ip = websocket.host.parse::<IpAddr>().map_err(|error| {
        CliError::InvalidArguments(format!("websocket host must be an IP address: {error}"))
    })?;
    Ok(SocketAddr::new(ip, websocket.port))
}

fn validate_run_security(options: &RunOptions, addr: SocketAddr) -> Result<(), CliError> {
    if !addr.ip().is_loopback() && !options.allow_remote {
        return Err(CliError::InvalidArguments(
            "non-loopback WebSocket bind requires --allow-remote".to_owned(),
        ));
    }
    Ok(())
}

pub fn format_channel_runtime_plan(report: ChannelRuntimeReport) -> String {
    let mut lines = vec![
        "Channel runtime plan".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
        format!(
            "WebSocket: enabled={} ws://{}:{}{}",
            report.websocket.enabled,
            report.websocket.host,
            report.websocket.port,
            report.websocket.path
        ),
    ];
    for worker in report.workers {
        lines.push(format!(
            "- {} / {}: {} ({})",
            worker.descriptor.channel,
            worker.descriptor.label,
            worker.state.label(),
            worker.detail
        ));
    }
    if report.verbose {
        lines.push("Verbose: enabled".to_owned());
    }
    lines.join("\n")
}

impl ChannelRuntimeWorkerState {
    fn label(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::SkippedDisabled => "skipped-disabled",
            Self::SkippedMissingCredentials => "skipped-missing-credentials",
        }
    }
}

pub fn format_skills_list(report: SkillsListReport) -> String {
    let mut lines = vec![
        "Skills".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
    ];
    if report.entries.is_empty() {
        lines.push("No skills found.".to_owned());
        return lines.join("\n");
    }
    for entry in report.entries {
        let description = entry
            .descriptor
            .description
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("no description");
        let status = if report.all {
            format!(" [{}]", entry.status.label())
        } else {
            String::new()
        };
        lines.push(format!(
            "- {}{} — {} ({})",
            entry.descriptor.name,
            status,
            description,
            entry.descriptor.source_kind.label()
        ));
    }
    lines.join("\n")
}

pub fn format_skills_show(report: SkillsShowReport) -> String {
    let entry = report.entry;
    let mut lines = vec![
        format!("Skill: {}", entry.descriptor.name),
        format!("Status: {}", entry.status.label()),
        format!("Source: {}", entry.descriptor.source_kind.label()),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
        format!("Body hash: {}", entry.descriptor.body_hash),
    ];
    if let Some(path) = entry.descriptor.source_path.as_ref() {
        lines.push(format!("Path: {}", path.display()));
    }
    if let Some(description) = entry.descriptor.description.as_deref() {
        lines.push(format!("Description: {description}"));
    }
    if !entry.descriptor.requirements.is_empty() {
        lines.push(format!(
            "Requirements: {}",
            entry.descriptor.requirements.join(", ")
        ));
    }
    if let Some(install) = entry.descriptor.install_metadata.as_deref() {
        lines.push(format!("Install metadata: {install}"));
    }
    if !entry.diagnostics.is_empty() {
        lines.push(format!("Diagnostics: {}", entry.diagnostics.join("; ")));
    }
    lines.join("\n")
}

pub fn format_channels_list(report: ChannelsReport) -> String {
    let mut lines = vec![
        "Channels".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
    ];
    for item in report.channels {
        lines.push(format!(
            "- {} — {} [{}] configured={} workers={} capabilities={}",
            item.descriptor.name,
            item.descriptor.display_name,
            if item.enabled { "enabled" } else { "disabled" },
            item.configured,
            item.workers.len(),
            format_channel_capabilities(&item.descriptor.capabilities)
        ));
    }
    if !report.unknown_plugins.is_empty() {
        lines.push(format!(
            "Unknown configured plugins: {}",
            report.unknown_plugins.join(", ")
        ));
    }
    lines.join("\n")
}

pub fn format_channels_status(report: ChannelsReport) -> String {
    let configured = report
        .channels
        .iter()
        .filter(|channel| channel.configured)
        .count();
    let enabled = report
        .channels
        .iter()
        .filter(|channel| channel.enabled)
        .count();
    let mut lines = vec![
        "Channel runtime status".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
        format!("Known channels: {}", report.channels.len()),
        format!("Configured channels: {configured}"),
        format!("Enabled channels: {enabled}"),
        format!("Worker boundaries: {}", report.worker_count),
        format!("Send progress: {}", report.send_progress),
        format!("Send tool hints: {}", report.send_tool_hints),
        format!("Send max retries: {}", report.send_max_retries),
        "Live workers: websocket runnable; external channels start only when transport adapters and credentials are available".to_owned(),
    ];
    for item in report.channels {
        let worker_labels = if item.workers.is_empty() {
            "none".to_owned()
        } else {
            item.workers
                .iter()
                .map(|worker| worker.label.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };
        lines.push(format!(
            "- {}: enabled={} configured={} runtime={} workers={}",
            item.descriptor.name,
            item.enabled,
            item.configured,
            if item.workers.iter().any(|worker| worker.ready_for_runtime) {
                "runnable"
            } else {
                "credential-gated"
            },
            worker_labels
        ));
    }
    if !report.unknown_plugins.is_empty() {
        lines.push(format!(
            "Unknown configured plugins: {}",
            report.unknown_plugins.join(", ")
        ));
    }
    lines.join("\n")
}

fn format_channel_capabilities(capabilities: &ChannelCapabilities) -> String {
    let mut values = Vec::new();
    if capabilities.streaming {
        values.push("streaming");
    }
    if capabilities.media {
        values.push("media");
    }
    if capabilities.buttons {
        values.push("buttons");
    }
    if capabilities.external_bridge {
        values.push("external-bridge");
    }
    if values.is_empty() {
        "text".to_owned()
    } else {
        values.join("+")
    }
}

pub fn session_list(options: SessionListOptions) -> Result<SessionListReport, CliError> {
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    if !workspace.join("sessions").exists() {
        return Ok(SessionListReport {
            workspace,
            sessions: Vec::new(),
        });
    }
    let sessions = SessionManager::new(&workspace)?
        .list_sessions()?
        .into_iter()
        .map(|session| SessionListItem {
            key: session.key,
            created_at: session.created_at,
            updated_at: session.updated_at,
            path: session.path,
        })
        .collect::<Vec<_>>();
    Ok(SessionListReport {
        workspace,
        sessions,
    })
}

pub fn session_inspect(options: SessionInspectOptions) -> Result<SessionInspectReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session inspect requires --session <key>".to_owned(),
        ));
    }

    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    if !workspace.join("sessions").exists() {
        return Err(CliError::InvalidArguments(format!(
            "session `{}` was not found",
            options.session
        )));
    }
    let manager = SessionManager::new(&workspace)?;
    let path = manager.session_path(&options.session);
    let value = manager.read_session_file(&options.session).ok_or_else(|| {
        CliError::InvalidArguments(format!("session `{}` was not found", options.session))
    })?;
    let message_count = value
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let mut metadata_keys = value
        .get("metadata")
        .and_then(Value::as_object)
        .map(|metadata| metadata.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    metadata_keys.sort();
    let (recovery_markers, checkpoint_phase) = recovery_summary_from_session_value(&value);
    let last_consolidated = value
        .get("last_consolidated")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default();

    Ok(SessionInspectReport {
        workspace,
        key: value
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or(&options.session)
            .to_owned(),
        path,
        created_at: value
            .get("created_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        updated_at: value
            .get("updated_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        message_count,
        metadata_keys,
        last_consolidated,
        recovery_markers,
        checkpoint_phase,
    })
}

pub fn session_create(options: SessionCreateOptions) -> Result<SessionCreateReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session create requires --session <key>".to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let mut manager = SessionManager::new(&workspace)?;
    let path = manager.session_path(&options.session);
    let created = if manager.load_existing(&options.session).is_some() {
        false
    } else {
        manager.save_with_fsync(&Session::new(&options.session))?;
        true
    };
    Ok(SessionCreateReport {
        workspace,
        key: options.session,
        path,
        created,
    })
}

pub fn session_history(
    options: SessionHistoryCliOptions,
) -> Result<SessionHistoryCliReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session history requires --session <key>".to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    if !workspace.join("sessions").exists() {
        return Err(CliError::InvalidArguments(format!(
            "session `{}` was not found",
            options.session
        )));
    }
    let manager = SessionManager::new(&workspace)?;
    let session = manager.load_existing(&options.session).ok_or_else(|| {
        CliError::InvalidArguments(format!("session `{}` was not found", options.session))
    })?;
    let path = manager
        .existing_session_path(&options.session)
        .unwrap_or_else(|| manager.session_path(&options.session));
    let history = session.get_history_with_options(SessionHistoryOptions {
        max_messages: options.max_messages,
        max_tokens: options.max_tokens,
        include_timestamps: options.timestamps,
    });
    Ok(SessionHistoryCliReport {
        workspace,
        key: session.key,
        path,
        history,
        json: options.json,
    })
}

pub fn session_export(options: SessionExportOptions) -> Result<SessionExportReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session export requires --session <key>".to_owned(),
        ));
    }
    if !options.yes {
        return Err(CliError::InvalidArguments(
            "session export prints raw local session content; pass --yes to confirm".to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    if !workspace.join("sessions").exists() {
        return Err(CliError::InvalidArguments(format!(
            "session `{}` was not found",
            options.session
        )));
    }
    let manager = SessionManager::new(&workspace)?;
    let session = manager.load_existing(&options.session).ok_or_else(|| {
        CliError::InvalidArguments(format!("session `{}` was not found", options.session))
    })?;
    let content = match options.format {
        SessionExportFormat::Json => serde_json::to_string_pretty(&json!({
            "key": session.key,
            "created_at": session.created_at,
            "updated_at": session.updated_at,
            "metadata": session.metadata,
            "last_consolidated": session.last_consolidated,
            "messages": session.messages,
        }))
        .map_err(|error| {
            CliError::InvalidArguments(format!("session JSON export failed: {error}"))
        })?,
        SessionExportFormat::Jsonl => session_to_jsonl(&session)?,
    };
    Ok(SessionExportReport { content })
}

pub fn session_clear(options: SessionClearOptions) -> Result<SessionClearReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session clear requires --session <key>".to_owned(),
        ));
    }
    if !options.yes {
        return Err(CliError::InvalidArguments(
            "session clear removes all messages from the session file; pass --yes to confirm"
                .to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let path = session_path_without_creating_dir(&workspace, &options.session);
    if !workspace.join("sessions").exists() {
        return Ok(SessionClearReport {
            workspace,
            key: options.session,
            path,
            cleared: false,
            message_count_before: 0,
        });
    }
    let mut manager = SessionManager::new(&workspace)?;
    let path = manager
        .existing_session_path(&options.session)
        .unwrap_or(path);
    let message_count_before = manager.clear_session(&options.session)?.unwrap_or_default();
    Ok(SessionClearReport {
        workspace,
        key: options.session,
        path,
        cleared: message_count_before > 0,
        message_count_before,
    })
}

pub fn session_diagnostics(
    options: SessionDiagnosticsOptions,
) -> Result<SessionDiagnosticsReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session diagnostics requires --session <key>".to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let fallback_path = session_path_without_creating_dir(&workspace, &options.session);
    if !workspace.join("sessions").exists() {
        return Ok(SessionDiagnosticsReport {
            workspace,
            key: options.session,
            path: fallback_path,
            exists: false,
            message_count: 0,
            last_consolidated: 0,
            metadata_keys: Vec::new(),
            recovery_markers: Vec::new(),
            checkpoint_phase: None,
            legal_start: 0,
        });
    }
    let manager = SessionManager::new(&workspace)?;
    let path = manager
        .existing_session_path(&options.session)
        .unwrap_or(fallback_path);
    let Some(value) = manager.read_session_file(&options.session) else {
        return Ok(SessionDiagnosticsReport {
            workspace,
            key: options.session,
            path,
            exists: false,
            message_count: 0,
            last_consolidated: 0,
            metadata_keys: Vec::new(),
            recovery_markers: Vec::new(),
            checkpoint_phase: None,
            legal_start: 0,
        });
    };
    let messages = value
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut metadata_keys = value
        .get("metadata")
        .and_then(Value::as_object)
        .map(|metadata| metadata.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    metadata_keys.sort();
    let (recovery_markers, checkpoint_phase) = recovery_summary_from_session_value(&value);
    Ok(SessionDiagnosticsReport {
        workspace,
        key: value
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or(&options.session)
            .to_owned(),
        path,
        exists: true,
        message_count: messages.len(),
        last_consolidated: value
            .get("last_consolidated")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or_default(),
        metadata_keys,
        recovery_markers,
        checkpoint_phase,
        legal_start: find_legal_message_start(&messages),
    })
}

pub fn session_compact(options: SessionCompactOptions) -> Result<SessionCompactReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session compact requires --session <key>".to_owned(),
        ));
    }
    if !options.yes {
        return Err(CliError::InvalidArguments(
            "session compact rewrites the session file and drops old messages; pass --yes to confirm"
                .to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let fallback_path = session_path_without_creating_dir(&workspace, &options.session);
    if !workspace.join("sessions").exists() {
        return Ok(SessionCompactReport {
            workspace,
            key: options.session,
            path: fallback_path,
            compacted: false,
            kept_messages: 0,
            archived_messages: 0,
        });
    }
    let mut manager = SessionManager::new(&workspace)?;
    let path = manager
        .existing_session_path(&options.session)
        .unwrap_or(fallback_path);
    let Some(mut session) = manager.load_existing(&options.session) else {
        return Ok(SessionCompactReport {
            workspace,
            key: options.session,
            path,
            compacted: false,
            kept_messages: 0,
            archived_messages: 0,
        });
    };
    let before = session.messages.len();
    session.retain_recent_legal_suffix(options.keep_messages);
    let archived_messages = before.saturating_sub(session.messages.len());
    let removed_legacy_files = manager.save_with_fsync_pruning_legacy(&session)?;
    Ok(SessionCompactReport {
        workspace,
        key: session.key,
        path,
        compacted: archived_messages > 0 || removed_legacy_files > 0,
        kept_messages: session.messages.len(),
        archived_messages,
    })
}

pub fn session_delete(options: SessionDeleteOptions) -> Result<SessionDeleteReport, CliError> {
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session delete requires --session <key>".to_owned(),
        ));
    }
    if !options.yes {
        return Err(CliError::InvalidArguments(
            "session delete removes the session file from disk and cannot be undone; pass --yes to confirm"
                .to_owned(),
        ));
    }

    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let path = session_path_without_creating_dir(&workspace, &options.session);
    if !workspace.join("sessions").exists() {
        return Ok(SessionDeleteReport {
            workspace,
            key: options.session,
            path,
            deleted: false,
        });
    }

    let mut manager = SessionManager::new(&workspace)?;
    let path = manager
        .existing_session_path(&options.session)
        .unwrap_or(path);
    let deleted = manager.delete_session(&options.session)?;
    Ok(SessionDeleteReport {
        workspace,
        key: options.session,
        path,
        deleted,
    })
}

fn session_path_without_creating_dir(workspace: &Path, key: &str) -> PathBuf {
    workspace
        .join("sessions")
        .join(format!("{}.jsonl", SessionManager::safe_key(key)))
}

fn recovery_summary_from_session_value(value: &Value) -> (Vec<String>, Option<String>) {
    let Some(metadata) = value.get("metadata").and_then(Value::as_object) else {
        return (Vec::new(), None);
    };
    let mut markers = Vec::new();
    if metadata.contains_key("pending_user_turn") {
        markers.push("pending_user_turn".to_owned());
    }
    if metadata.contains_key("runtime_checkpoint") {
        markers.push("runtime_checkpoint".to_owned());
    }
    if metadata.contains_key("_last_summary") {
        markers.push("_last_summary".to_owned());
    }
    let phase = metadata
        .get("runtime_checkpoint")
        .and_then(|checkpoint| checkpoint.get("phase"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    (markers, phase)
}

fn session_to_jsonl(session: &Session) -> Result<String, CliError> {
    let mut lines = Vec::with_capacity(session.messages.len() + 1);
    lines.push(
        serde_json::to_string(&json!({
            "_type": "metadata",
            "key": session.key,
            "created_at": session.created_at,
            "updated_at": session.updated_at,
            "metadata": session.metadata,
            "last_consolidated": session.last_consolidated,
        }))
        .map_err(|error| {
            CliError::InvalidArguments(format!("session JSONL export failed: {error}"))
        })?,
    );
    for message in &session.messages {
        lines.push(serde_json::to_string(message).map_err(|error| {
            CliError::InvalidArguments(format!("session JSONL export failed: {error}"))
        })?);
    }
    Ok(lines.join("\n"))
}

fn load_session_workspace(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Result<PathBuf, CliError> {
    let config_path = config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path),
            workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    Ok(bundle.context.workspace)
}

fn run_provider_command(command: ProviderCommand) -> Result<String, CliError> {
    match command {
        ProviderCommand::CodexImportToken(options) => {
            import_codex_token(options).map(format_codex_import_outcome)
        }
        ProviderCommand::CodexLogin(options) => codex_login(options),
    }
}

pub fn import_codex_token(
    options: CodexImportTokenOptions,
) -> Result<CodexImportOutcome, CliError> {
    let token = read_token_source(&options.token_source)?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err(CliError::InvalidArguments(
            "Codex token must not be empty".to_owned(),
        ));
    }

    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: true,
        },
        &ProcessEnv,
    )?;
    bundle
        .config
        .providers
        .entry("openai_codex".to_owned())
        .or_insert_with(codex_provider_config);
    let selected_model = options.select.then(|| CODEX_DEFAULT_MODEL.to_owned());
    if let Some(model) = &selected_model {
        bundle.config.agents.defaults.provider = "openai_codex".to_owned();
        bundle.config.agents.defaults.model = model.clone();
    }

    let context = config_context(
        Some(config_path.clone()),
        Some(bundle.config.workspace_path()),
    );
    let auth_path = context.auth_path();
    let mut auth = load_auth_store(&auth_path)?;
    auth.providers.insert(
        "openai_codex".to_owned(),
        ProviderAuth::oauth_access(token, options.account_id.clone()),
    );
    save_auth_store_to_path(&auth, &auth_path)?;
    save_config_to_path(&bundle.config, &config_path)?;

    Ok(CodexImportOutcome {
        config_path,
        auth_path,
        provider: "openai_codex".to_owned(),
        selected_model,
        selected: options.select,
        has_account_id: options
            .account_id
            .as_ref()
            .is_some_and(|value| !value.is_empty()),
    })
}

fn codex_login(options: CodexLoginOptions) -> Result<String, CliError> {
    codex_login_with_transport(options, &UreqCodexAuthTransport::default())
        .map(format_codex_login_outcome)
}

fn codex_login_with_transport(
    options: CodexLoginOptions,
    transport: &impl CodexAuthTransport,
) -> Result<CodexLoginOutcome, CliError> {
    let token = if options.headless {
        codex_headless_login(transport)?
    } else {
        codex_browser_login(&options, transport)?
    };
    save_codex_login_token(options.config_path, token)
}

fn codex_browser_login(
    options: &CodexLoginOptions,
    transport: &impl CodexAuthTransport,
) -> Result<CodexTokenResponse, CliError> {
    let pkce = CodexPkce::generate()?;
    let listeners = bind_codex_callback_listeners()?;
    let authorize_url = codex_authorize_url(&pkce);
    eprintln!("Open this URL to authorize Codex:\n{authorize_url}");
    if !options.no_browser {
        open_browser(&authorize_url)?;
    }
    let code = wait_for_codex_callback(&listeners, &pkce.state)?;
    exchange_codex_authorization_code(transport, &code, &pkce.verifier, CODEX_REDIRECT_URI)
}

fn codex_headless_login(
    transport: &impl CodexAuthTransport,
) -> Result<CodexTokenResponse, CliError> {
    codex_headless_login_with_polling(transport, thread::sleep, CODEX_DEVICE_TIMEOUT)
}

fn codex_headless_login_with_polling(
    transport: &impl CodexAuthTransport,
    sleep: impl Fn(Duration),
    timeout: Duration,
) -> Result<CodexTokenResponse, CliError> {
    let response = transport.post_json(
        &format!("{CODEX_ISSUER}/api/accounts/deviceauth/usercode"),
        json!({ "client_id": CODEX_CLIENT_ID }),
    )?;
    ensure_success(&response, "Codex device auth request failed")?;
    let device_auth_id = json_string(&response.body, "device_auth_id")?;
    let user_code = json_string(&response.body, "user_code")?;
    let interval = json_interval_seconds(&response.body).saturating_add(3);
    eprintln!("Open {CODEX_DEVICE_URL} and enter code: {user_code}");

    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() >= deadline {
            return Err(CliError::InvalidArguments(
                "Codex device login timed out".to_owned(),
            ));
        }
        sleep(Duration::from_secs(interval));
        let poll = transport.post_json(
            &format!("{CODEX_ISSUER}/api/accounts/deviceauth/token"),
            json!({
                "device_auth_id": device_auth_id,
                "user_code": user_code,
            }),
        )?;
        if matches!(poll.status, 403 | 404) {
            continue;
        }
        ensure_success(&poll, "Codex device token polling failed")?;
        let code = json_string(&poll.body, "authorization_code")?;
        let verifier = json_string(&poll.body, "code_verifier")?;
        return exchange_codex_authorization_code(
            transport,
            &code,
            &verifier,
            CODEX_DEVICE_REDIRECT_URI,
        );
    }
}

fn codex_provider_config() -> ProviderConfig {
    ProviderConfig {
        api_key: None,
        api_base: Some("https://chatgpt.com/backend-api".to_owned()),
        extra_headers: None,
        extra_body: None,
    }
}

fn read_token_source(source: &TokenSource) -> Result<String, CliError> {
    match source {
        TokenSource::Stdin => {
            let mut token = String::new();
            std::io::stdin().read_to_string(&mut token)?;
            Ok(token)
        }
        TokenSource::Env(name) => std::env::var(name).map_err(|_| {
            CliError::InvalidArguments(format!("environment variable `{name}` is not set"))
        }),
        TokenSource::Literal(value) => Ok(value.clone()),
    }
}

fn save_codex_login_token(
    config_path: Option<PathBuf>,
    token: CodexTokenResponse,
) -> Result<CodexLoginOutcome, CliError> {
    let config_path = config_path.unwrap_or_else(default_config_path);
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: true,
        },
        &ProcessEnv,
    )?;
    bundle
        .config
        .providers
        .entry(CODEX_PROVIDER_ID.to_owned())
        .or_insert_with(codex_provider_config);
    bundle.config.agents.defaults.provider = CODEX_PROVIDER_ID.to_owned();
    bundle.config.agents.defaults.model = CODEX_DEFAULT_MODEL.to_owned();
    let context = config_context(
        Some(config_path.clone()),
        Some(bundle.config.workspace_path()),
    );
    let auth_path = context.auth_path();
    let mut auth = load_auth_store(&auth_path)?;
    auth.providers.insert(
        CODEX_PROVIDER_ID.to_owned(),
        ProviderAuth {
            kind: "oauth".to_owned(),
            access: token.access,
            refresh: token.refresh,
            expires: token.expires,
            account_id: token.account_id.clone(),
        },
    );
    save_auth_store_to_path(&auth, &auth_path)?;
    save_config_to_path(&bundle.config, &config_path)?;
    Ok(CodexLoginOutcome {
        config_path,
        auth_path,
        provider: CODEX_PROVIDER_ID.to_owned(),
        selected_model: CODEX_DEFAULT_MODEL.to_owned(),
        account_id: token.account_id,
        expires: token.expires,
    })
}

fn exchange_codex_authorization_code(
    transport: &impl CodexAuthTransport,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<CodexTokenResponse, CliError> {
    let response = transport.post_form(
        &format!("{CODEX_ISSUER}/oauth/token"),
        &[
            ("grant_type".to_owned(), "authorization_code".to_owned()),
            ("client_id".to_owned(), CODEX_CLIENT_ID.to_owned()),
            ("code".to_owned(), code.to_owned()),
            ("redirect_uri".to_owned(), redirect_uri.to_owned()),
            ("code_verifier".to_owned(), verifier.to_owned()),
        ],
    )?;
    ensure_success(&response, "Codex token exchange failed")?;
    token_response_from_json(&response.body)
}

fn refresh_codex_token(
    transport: &impl CodexAuthTransport,
    refresh: &str,
) -> Result<CodexTokenResponse, CliError> {
    let response = transport.post_form(
        &format!("{CODEX_ISSUER}/oauth/token"),
        &[
            ("grant_type".to_owned(), "refresh_token".to_owned()),
            ("client_id".to_owned(), CODEX_CLIENT_ID.to_owned()),
            ("refresh_token".to_owned(), refresh.to_owned()),
        ],
    )?;
    ensure_success(&response, "Codex token refresh failed")?;
    token_response_from_json(&response.body)
}

fn token_response_from_json(body: &Value) -> Result<CodexTokenResponse, CliError> {
    let access = json_string(body, "access_token")?;
    let refresh = body
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let expires = body
        .get("expires_in")
        .and_then(Value::as_u64)
        .or(Some(3600))
        .map(|seconds| now_millis().saturating_add(seconds.saturating_mul(1000)));
    let id_token = body.get("id_token").and_then(Value::as_str);
    let account_id = id_token
        .and_then(extract_codex_account_id)
        .or_else(|| extract_codex_account_id(&access));
    Ok(CodexTokenResponse {
        access,
        refresh,
        expires,
        account_id,
    })
}

fn json_interval_seconds(body: &Value) -> u64 {
    body.get("interval")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
        })
        .unwrap_or(5)
        .max(1)
}

fn ensure_success(response: &CodexAuthHttpResponse, message: &str) -> Result<(), CliError> {
    if matches!(response.status, 200..=299) {
        return Ok(());
    }
    Err(CliError::Provider(ProviderError::Api {
        status: Some(response.status),
        message: message.to_owned(),
        retryable: false,
        headers: BTreeMap::new(),
        body: Some(response.body.to_string()),
    }))
}

fn json_string(body: &Value, key: &str) -> Result<String, CliError> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CliError::InvalidArguments(format!("Codex auth response missing `{key}`")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexPkce {
    verifier: String,
    challenge: String,
    state: String,
}

impl CodexPkce {
    fn generate() -> Result<Self, CliError> {
        let verifier = random_base64_url(32)?;
        let state = random_base64_url(24)?;
        let challenge = sha256_base64_url(verifier.as_bytes());
        Ok(Self {
            verifier,
            challenge,
            state,
        })
    }
}

fn codex_authorize_url(pkce: &CodexPkce) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", CODEX_CLIENT_ID),
        ("redirect_uri", CODEX_REDIRECT_URI),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", pkce.challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", pkce.state.as_str()),
        ("originator", "shacs-bot"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{CODEX_ISSUER}/oauth/authorize?{query}")
}

fn bind_codex_callback_listeners() -> Result<Vec<TcpListener>, CliError> {
    let addresses = [
        SocketAddr::from(([127, 0, 0, 1], 1455)),
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 1455)),
    ];
    let mut listeners = Vec::new();
    let mut last_error = None;
    for address in addresses {
        match TcpListener::bind(address) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                listeners.push(listener);
            }
            Err(error) => last_error = Some(error),
        }
    }
    if listeners.is_empty() {
        return Err(CliError::Io(last_error.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "could not bind Codex OAuth callback listener",
            )
        })));
    }
    Ok(listeners)
}

fn wait_for_codex_callback(
    listeners: &[TcpListener],
    expected_state: &str,
) -> Result<String, CliError> {
    let deadline = Instant::now() + CODEX_BROWSER_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            return Err(CliError::InvalidArguments(
                "Codex browser login timed out".to_owned(),
            ));
        }
        let mut accepted = false;
        for listener in listeners {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    accepted = true;
                    if let Some(code) = handle_codex_callback(&mut stream, expected_state)? {
                        return Ok(code);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(CliError::Io(error)),
            }
        }
        if !accepted {
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn handle_codex_callback(
    stream: &mut TcpStream,
    expected_state: &str,
) -> Result<Option<String>, CliError> {
    let mut buffer = [0_u8; 4096];
    let read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    match extract_codex_callback_code(&request, expected_state) {
        Ok(code) => {
            write_plain_response(
                stream,
                "200 OK",
                "Codex login complete. You can close this window.",
            );
            Ok(Some(code))
        }
        Err(_) => {
            write_plain_response(
                stream,
                "400 Bad Request",
                "Codex login is still waiting for the correct callback. Return to the terminal if this keeps happening.",
            );
            Ok(None)
        }
    }
}

fn extract_codex_callback_code(
    request: &str,
    expected_state: &str,
) -> Result<String, &'static str> {
    let first_line = request.lines().next().ok_or("missing request line")?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next().ok_or("missing method")?;
    let path = parts.next().ok_or("missing path")?;
    if method != "GET" {
        return Err("unsupported method");
    }
    let (request_path, query) = path.split_once('?').unwrap_or((path, ""));
    if request_path != "/auth/callback" {
        return Err("unexpected path");
    }
    let params = parse_query_params(query);
    let state = params.get("state").map(String::as_str).unwrap_or_default();
    if state != expected_state {
        return Err("state mismatch");
    }
    params
        .get("code")
        .filter(|code| !code.is_empty())
        .cloned()
        .ok_or("missing code")
}

fn write_plain_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn open_browser(url: &str) -> Result<(), CliError> {
    let status = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .status()
    } else {
        Command::new("xdg-open").arg(url).status()
    };
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(_) | Err(_) => {
            eprintln!("Could not open browser automatically; copy the URL above.");
            Ok(())
        }
    }
}

fn random_base64_url(len: usize) -> Result<String, CliError> {
    let mut bytes = vec![0_u8; len];
    fill_random_bytes(&mut bytes)?;
    Ok(base64_url_no_pad(&bytes))
}

fn fill_random_bytes(bytes: &mut [u8]) -> Result<(), CliError> {
    let mut file = fs::File::open("/dev/urandom")?;
    file.read_exact(bytes)?;
    Ok(())
}

fn sha256_base64_url(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    base64_url_no_pad(&digest)
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | bytes[index + 2] as u32;
        output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        output.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        output.push(TABLE[(chunk & 0x3f) as usize] as char);
        index += 3;
    }
    match bytes.len() - index {
        1 => {
            let chunk = (bytes[index] as u32) << 16;
            output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let chunk = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
            output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        }
        _ => {}
    }
    output
}

fn base64_url_decode(input: &str) -> Option<Vec<u8>> {
    let mut bits = 0_u32;
    let mut bit_count = 0_u8;
    let mut output = Vec::new();
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        bits = (bits << 6) | value;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            output.push(((bits >> bit_count) & 0xff) as u8);
        }
    }
    Some(output)
}

fn extract_codex_account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64_url_decode(payload)?;
    let claims = serde_json::from_slice::<Value>(&decoded).ok()?;
    claims
        .get("chatgpt_account_id")
        .and_then(Value::as_str)
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth.chatgpt_account_id")
                .and_then(Value::as_str)
        })
        .or_else(|| {
            claims
                .get("organizations")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
}

fn parse_query_params(query: &str) -> BTreeMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some((percent_decode(key)?, percent_decode(value)?))
        })
        .collect()
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
                output.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).ok()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn codex_auth_transport_error(error: ureq::Error) -> CliError {
    CliError::Provider(ProviderError::Api {
        status: None,
        message: format!("Codex auth request failed: {error}"),
        retryable: true,
        headers: BTreeMap::new(),
        body: None,
    })
}

fn codex_auth_response(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<CodexAuthHttpResponse, CliError> {
    let status = response.status().as_u16();
    let body_text = response.body_mut().read_to_string().map_err(|error| {
        CliError::Provider(ProviderError::Api {
            status: Some(status),
            message: format!("Codex auth response could not be read: {error}"),
            retryable: false,
            headers: BTreeMap::new(),
            body: None,
        })
    })?;
    let body = if body_text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body_text).map_err(|error| {
            CliError::InvalidArguments(format!("Codex auth response was not JSON: {error}"))
        })?
    };
    Ok(CodexAuthHttpResponse { status, body })
}

pub fn ask(options: AskOptions) -> Result<String, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    let adapter = AgentLoopChatCompletionAdapter::from_bundle(bundle, options.allow_side_effects)?;
    complete_direct_message(&adapter, &options)
}

fn complete_direct_message(
    adapter: &AgentLoopChatCompletionAdapter,
    options: &AskOptions,
) -> Result<String, CliError> {
    let mut invocation = direct_message_invocation(adapter.configured_model(), options)?;
    invocation.session_key = cli_session_key(options.session.as_deref());
    let response = adapter.complete_direct(invocation)?;
    Ok(render_agent_response(
        response.content.unwrap_or_default(),
        options.markdown,
        None,
    ))
}

fn direct_message_invocation(
    configured_model: &str,
    options: &AskOptions,
) -> Result<ChatCompletionInvocation, ApiError> {
    let request = ChatCompletionRequest {
        model: Some(configured_model.to_owned()),
        messages: vec![ApiChatMessage {
            role: "user".to_owned(),
            content: ApiMessageContent::Text(options.message.clone()),
        }],
        stream: false,
        session_id: None,
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        tools: Vec::new(),
        tool_choice: None,
    };
    chat_completion_invocation(&request, configured_model)
}

fn sdk_message_invocation(
    configured_model: &str,
    options: &ShacsBotRunOptions,
) -> Result<ChatCompletionInvocation, ApiError> {
    let request = ChatCompletionRequest {
        model: Some(configured_model.to_owned()),
        messages: vec![ApiChatMessage {
            role: "user".to_owned(),
            content: ApiMessageContent::Text(options.message.clone()),
        }],
        stream: false,
        session_id: None,
        temperature: options.temperature,
        max_tokens: options.max_tokens,
        tools: Vec::new(),
        tool_choice: None,
    };
    chat_completion_invocation(&request, configured_model)
}

fn cli_session_key(session: Option<&str>) -> String {
    match session.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.contains(':') => value.to_owned(),
        Some(value) => format!("cli:{value}"),
        None => "cli:direct".to_owned(),
    }
}

pub fn serve(options: ServeOptions) -> Result<String, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    let addr = resolve_serve_addr(&options, &bundle.config.api)?;
    validate_serve_security(&options, addr)?;
    let timeout_seconds = options
        .timeout
        .unwrap_or(bundle.config.api.timeout)
        .max(0.001);
    let adapter = Arc::new(AgentLoopChatCompletionAdapter::from_bundle(
        bundle,
        options.allow_api_side_effects,
    )?);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    if options.verbose {
        eprintln!("Serving shacs-bot API on http://{addr} (timeout: {timeout_seconds}s)");
    } else {
        eprintln!("Serving shacs-bot API on http://{addr}");
    }
    runtime.block_on(shacs_api::serve_api_with_timeout(
        addr,
        adapter,
        Duration::from_secs_f64(timeout_seconds),
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    ))?;
    Ok(format!("API server stopped: http://{addr}"))
}

pub fn run_runtime(options: RunOptions) -> Result<String, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    let report = channel_runtime_report_from_bundle(&bundle, &options)?;
    let plan = format_channel_runtime_plan(report.clone());
    let timeout_seconds = options
        .timeout
        .unwrap_or(bundle.config.api.timeout)
        .max(0.001);
    let plugins = bundle.config.channels.plugins.clone();
    let specs = external_transport_specs(&plugins);
    if !runtime_needs_process(&report, &specs) {
        return Ok(plan);
    }
    let adapter = Arc::new(AgentLoopChatCompletionAdapter::from_bundle(
        bundle,
        options.allow_side_effects,
    )?);
    let supervisor = ExternalChannelSupervisor::start(adapter.clone(), specs);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    eprintln!("{plan}");
    let serve_result = if report.websocket.enabled {
        runtime.block_on(shacs_api::serve_websocket_with_timeout_and_path(
            report.websocket_addr,
            adapter,
            Duration::from_secs_f64(timeout_seconds),
            &report.websocket.path,
            async {
                let _ = tokio::signal::ctrl_c().await;
            },
        ))
    } else {
        runtime.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
        });
        Ok(())
    };
    supervisor.stop();
    serve_result?;
    Ok(format!(
        "Channel runtime stopped: websocket_enabled={} ws://{}:{}{}",
        report.websocket.enabled,
        report.websocket.host,
        report.websocket.port,
        report.websocket.path
    ))
}

struct ExternalChannelSupervisor {
    stop: Arc<AtomicBool>,
    handles: Vec<thread::JoinHandle<()>>,
}

impl ExternalChannelSupervisor {
    fn start(
        adapter: Arc<AgentLoopChatCompletionAdapter>,
        specs: Vec<ExternalTransportSpec>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        if specs.is_empty() {
            return Self {
                stop,
                handles: Vec::new(),
            };
        }
        let (inbound_tx, inbound_rx) = mpsc::channel::<InboundMessage>();
        let mut outbound_txs = BTreeMap::new();
        let mut handles = Vec::new();
        for spec in specs {
            let channel = spec.channel().to_owned();
            let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundMessage>();
            outbound_txs.insert(channel.clone(), outbound_tx);
            let worker_stop = stop.clone();
            let worker_inbound = inbound_tx.clone();
            handles.push(thread::spawn(move || {
                run_external_transport_worker(spec, worker_inbound, outbound_rx, worker_stop);
            }));
        }
        let processor_stop = stop.clone();
        handles.push(thread::spawn(move || {
            run_external_agent_processor(adapter, inbound_rx, outbound_txs, processor_stop);
        }));
        Self { stop, handles }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::SeqCst);
        for handle in self.handles {
            let _ = handle.join();
        }
    }
}

impl ExternalTransportSpec {
    fn channel(&self) -> &'static str {
        match self {
            Self::Telegram(_) => TELEGRAM_CHANNEL,
            Self::Discord(_) => DISCORD_CHANNEL,
            Self::Slack(_) => SLACK_CHANNEL,
            Self::Email(_) => EMAIL_CHANNEL,
            Self::WhatsApp(_) => WHATSAPP_CHANNEL,
        }
    }
}

fn run_external_agent_processor(
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    inbound_rx: mpsc::Receiver<InboundMessage>,
    outbound_txs: BTreeMap<String, mpsc::Sender<OutboundMessage>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        let message = match inbound_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(message) => message,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let outbound =
            match adapter.process_inbound_with_outbound(message, adapter.loop_config(), None) {
                Ok((_, outbound)) => outbound,
                Err(error) => {
                    eprintln!("external channel turn failed: {error}");
                    Vec::new()
                }
            };
        for message in outbound {
            if let Some(tx) = outbound_txs.get(&message.channel) {
                let _ = tx.send(message);
            }
        }
    }
}

fn run_external_transport_worker(
    spec: ExternalTransportSpec,
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
) {
    match spec {
        ExternalTransportSpec::Telegram(config) => {
            run_telegram_transport(config, inbound_tx, outbound_rx, stop)
        }
        ExternalTransportSpec::Discord(config) => {
            run_discord_transport(config, inbound_tx, outbound_rx, stop)
        }
        ExternalTransportSpec::Slack(config) => {
            run_slack_transport(config, inbound_tx, outbound_rx, stop)
        }
        ExternalTransportSpec::Email(config) => {
            run_email_transport(config, inbound_tx, outbound_rx, stop)
        }
        ExternalTransportSpec::WhatsApp(config) => {
            run_whatsapp_transport(config, inbound_tx, outbound_rx, stop)
        }
    }
}

fn run_telegram_transport(
    config: TelegramRuntimeConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
) {
    let agent = runtime_http_agent(Duration::from_secs(config.poll_timeout_seconds + 10));
    let mut offset = 0_i64;
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(&outbound_rx, |message| {
            send_telegram_message(&agent, &config, message)
        });
        let body = json!({
            "offset": offset,
            "timeout": config.poll_timeout_seconds,
            "limit": config.poll_limit,
            "allowed_updates": ["message", "edited_message"],
        });
        match post_json(
            &agent,
            &telegram_url(&config.token, "getUpdates"),
            None,
            body,
        ) {
            Ok(value) => {
                if let Some(updates) = value.get("result").and_then(Value::as_array) {
                    for update in updates {
                        if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
                            offset = offset.max(update_id + 1);
                        }
                        if let Some(inbound) = telegram_update_to_inbound(update) {
                            let _ = inbound_tx.send(inbound);
                        }
                    }
                }
            }
            Err(error) => eprintln!("telegram polling failed: {error}"),
        }
    }
}

fn send_telegram_message(
    agent: &ureq::Agent,
    config: &TelegramRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    post_json(
        agent,
        &telegram_url(&config.token, "sendMessage"),
        None,
        json!({"chat_id": message.chat_id, "text": message.content}),
    )
    .map(|_| ())
}

fn telegram_update_to_inbound(update: &Value) -> Option<InboundMessage> {
    let message = update
        .get("message")
        .or_else(|| update.get("edited_message"))?;
    let content = message
        .get("text")
        .or_else(|| message.get("caption"))
        .and_then(Value::as_str)?;
    let chat_id = json_id_string(message.get("chat")?.get("id")?)?;
    let sender_id = message
        .get("from")
        .and_then(|from| from.get("id"))
        .and_then(json_id_string)
        .unwrap_or_else(|| chat_id.clone());
    let message_id = message.get("message_id").and_then(json_id_string);
    Some(
        TelegramInbound {
            sender_id,
            chat_id,
            content: content.to_owned(),
            message_id,
            username: message
                .get("from")
                .and_then(|from| from.get("username"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            message_thread_id: message.get("message_thread_id").and_then(json_id_string),
            media: Vec::new(),
        }
        .into_message(),
    )
}

fn telegram_url(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/{method}")
}

fn redact_sensitive_url_text(text: &str) -> String {
    let marker = "api.telegram.org/bot";
    let mut redacted = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(marker) {
        let (before, after_before) = rest.split_at(index);
        redacted.push_str(before);
        redacted.push_str(marker);
        redacted.push_str("<redacted>");
        let token_start = marker.len();
        let token_tail = &after_before[token_start..];
        if let Some(path_index) = token_tail.find('/') {
            rest = &token_tail[path_index..];
        } else {
            rest = "";
        }
    }
    redacted.push_str(rest);
    redacted
}

fn run_discord_transport(
    config: DiscordRuntimeConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
) {
    let agent = runtime_http_agent(Duration::from_secs(30));
    let mut last_ids = BTreeMap::<String, String>::new();
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(&outbound_rx, |message| {
            send_discord_message(&agent, &config, message)
        });
        for channel_id in &config.channel_ids {
            match poll_discord_channel(&agent, &config, channel_id, last_ids.get(channel_id)) {
                Ok((messages, newest)) => {
                    if let Some(newest) = newest {
                        last_ids.insert(channel_id.clone(), newest);
                    }
                    for inbound in messages {
                        let _ = inbound_tx.send(inbound);
                    }
                }
                Err(error) => eprintln!("discord polling failed for {channel_id}: {error}"),
            }
        }
        sleep_with_stop(&stop, Duration::from_secs(config.poll_interval_seconds));
    }
}

fn poll_discord_channel(
    agent: &ureq::Agent,
    config: &DiscordRuntimeConfig,
    channel_id: &str,
    after: Option<&String>,
) -> Result<(Vec<InboundMessage>, Option<String>), String> {
    let mut url = format!("https://discord.com/api/v10/channels/{channel_id}/messages?limit=10");
    if let Some(after) = after {
        url.push_str("&after=");
        url.push_str(after);
    }
    let value = get_json(agent, &url, Some(discord_auth_header(&config.token)))?;
    let Some(items) = value.as_array() else {
        return Ok((Vec::new(), None));
    };
    let newest = items
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .max()
        .map(str::to_owned);
    let mut messages = Vec::new();
    for item in items.iter().rev() {
        if item
            .get("author")
            .and_then(|author| author.get("bot"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(content) = item
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let sender_id = item
            .get("author")
            .and_then(|author| author.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("discord-user")
            .to_owned();
        messages.push(
            DiscordInbound {
                sender_id,
                channel_id: channel_id.to_owned(),
                content: content.to_owned(),
                message_id: item.get("id").and_then(Value::as_str).map(str::to_owned),
                guild_id: item
                    .get("guild_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parent_channel_id: None,
                thread_id: item
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                attachments: Vec::new(),
            }
            .into_message(),
        );
    }
    Ok((messages, newest))
}

fn send_discord_message(
    agent: &ureq::Agent,
    config: &DiscordRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    post_json(
        agent,
        &format!(
            "https://discord.com/api/v10/channels/{}/messages",
            message.chat_id
        ),
        Some(discord_auth_header(&config.token)),
        json!({"content": message.content}),
    )
    .map(|_| ())
}

fn discord_auth_header(token: &str) -> String {
    format!("Bot {token}")
}

fn run_slack_transport(
    config: SlackRuntimeConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
) {
    let agent = runtime_http_agent(Duration::from_secs(30));
    let mut latest = BTreeMap::<String, String>::new();
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(&outbound_rx, |message| {
            send_slack_message(&agent, &config, message)
        });
        for channel_id in &config.channel_ids {
            match poll_slack_channel(&agent, &config, channel_id, latest.get(channel_id)) {
                Ok((messages, newest)) => {
                    if let Some(newest) = newest {
                        latest.insert(channel_id.clone(), newest);
                    }
                    for inbound in messages {
                        let _ = inbound_tx.send(inbound);
                    }
                }
                Err(error) => eprintln!("slack polling failed for {channel_id}: {error}"),
            }
        }
        sleep_with_stop(&stop, Duration::from_secs(config.poll_interval_seconds));
    }
}

fn poll_slack_channel(
    agent: &ureq::Agent,
    config: &SlackRuntimeConfig,
    channel_id: &str,
    oldest: Option<&String>,
) -> Result<(Vec<InboundMessage>, Option<String>), String> {
    let mut url =
        format!("https://slack.com/api/conversations.history?channel={channel_id}&limit=10");
    if let Some(oldest) = oldest {
        url.push_str("&oldest=");
        url.push_str(oldest);
        url.push_str("&inclusive=false");
    }
    let value = get_json(agent, &url, Some(bearer_header(&config.bot_token)))?;
    if !value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(format!(
            "Slack API error: {}",
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
    }
    let Some(items) = value.get("messages").and_then(Value::as_array) else {
        return Ok((Vec::new(), None));
    };
    let newest = items
        .iter()
        .filter_map(|item| item.get("ts").and_then(Value::as_str))
        .max_by(|left, right| left.cmp(right))
        .map(str::to_owned);
    let mut messages = Vec::new();
    for item in items.iter().rev() {
        if item.get("subtype").is_some() {
            continue;
        }
        let Some(content) = item
            .get("text")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        messages.push(
            SlackInbound {
                user_id: item
                    .get("user")
                    .and_then(Value::as_str)
                    .unwrap_or("slack-user")
                    .to_owned(),
                channel_id: channel_id.to_owned(),
                content: content.to_owned(),
                event_ts: item.get("ts").and_then(Value::as_str).map(str::to_owned),
                thread_ts: item
                    .get("thread_ts")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                channel_type: None,
                files: Vec::new(),
            }
            .into_message(),
        );
    }
    Ok((messages, newest))
}

fn send_slack_message(
    agent: &ureq::Agent,
    config: &SlackRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    let value = post_json(
        agent,
        "https://slack.com/api/chat.postMessage",
        Some(bearer_header(&config.bot_token)),
        json!({"channel": message.chat_id, "text": message.content}),
    )?;
    if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        Ok(())
    } else {
        Err(format!(
            "Slack API error: {}",
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ))
    }
}

fn run_email_transport(
    config: EmailRuntimeConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
) {
    let mut last_poll = Instant::now();
    let mut seen_uids = BTreeSet::new();
    let mut seen_uid_order = VecDeque::new();
    while !stop.load(Ordering::SeqCst) {
        if let Some(smtp) = config.smtp.as_ref() {
            drain_outbound(&outbound_rx, |message| send_email_message(smtp, message));
        } else {
            discard_outbound(&outbound_rx, "email smtp is not configured");
        }
        if let Some(imap) = config.imap.as_ref() {
            if last_poll.elapsed() >= Duration::from_secs(imap.poll_interval_seconds) {
                match poll_email_inbox(imap, &mut seen_uids, &mut seen_uid_order) {
                    Ok(messages) => {
                        for inbound in messages {
                            let _ = inbound_tx.send(inbound);
                        }
                    }
                    Err(error) => eprintln!("email imap polling failed: {error}"),
                }
                last_poll = Instant::now();
            }
        }
        sleep_with_stop(&stop, Duration::from_millis(250));
    }
}

fn send_email_message(
    config: &EmailSmtpRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    let from = config
        .from
        .parse::<Mailbox>()
        .map_err(|error| error.to_string())?;
    let to = message
        .chat_id
        .parse::<Mailbox>()
        .map_err(|error| error.to_string())?;
    let email = EmailMessage::builder()
        .from(from)
        .to(to)
        .subject("shacs-bot")
        .body(message.content)
        .map_err(|error| error.to_string())?;
    let mut builder = match config.security {
        EmailSecurity::Plain => SmtpTransport::builder_dangerous(&config.host).port(config.port),
        EmailSecurity::StartTls => SmtpTransport::starttls_relay(&config.host)
            .map_err(|error| error.to_string())?
            .port(config.port),
        EmailSecurity::Tls => SmtpTransport::relay(&config.host)
            .map_err(|error| error.to_string())?
            .port(config.port),
    };
    builder = builder.timeout(Some(Duration::from_secs(config.timeout_seconds)));
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        builder = builder.credentials(SmtpCredentials::new(username.clone(), password.clone()));
    }
    builder
        .build()
        .send(&email)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn poll_email_inbox(
    config: &EmailImapRuntimeConfig,
    seen_uids: &mut BTreeSet<String>,
    seen_uid_order: &mut VecDeque<String>,
) -> Result<Vec<InboundMessage>, String> {
    if !matches!(config.security, EmailSecurity::Tls) {
        return Err("only TLS IMAP polling is supported in this runtime".to_owned());
    }
    let client = connect_imap_tls(config)?;
    let mut session = client
        .login(&config.username, &config.password)
        .map_err(|(error, _)| error.to_string())?;
    session
        .select(&config.mailbox)
        .map_err(|error| error.to_string())?;
    let uids = session
        .uid_search("UNSEEN")
        .map_err(|error| error.to_string())?;
    let mut messages = Vec::new();
    for uid in uids.iter().take(10) {
        let uid = uid.to_string();
        if seen_uids.contains(&uid) {
            continue;
        }
        remember_seen_email_uid(seen_uids, seen_uid_order, uid.clone());
        let fetches = session
            .uid_fetch(uid.clone(), "RFC822")
            .map_err(|error| error.to_string())?;
        for fetch in fetches.iter() {
            let Some(body) = fetch.body() else {
                continue;
            };
            if let Some(inbound) = parse_email_body(body, uid.clone()) {
                messages.push(inbound);
            }
        }
        if config.mark_seen {
            let _ = session.uid_store(uid, "+FLAGS (\\Seen)");
        }
    }
    let _ = session.logout();
    Ok(messages)
}

fn connect_imap_tls(
    config: &EmailImapRuntimeConfig,
) -> Result<imap::Client<native_tls::TlsStream<TcpStream>>, String> {
    let timeout = Duration::from_secs(config.timeout_seconds);
    let tls = native_tls::TlsConnector::builder()
        .build()
        .map_err(|error| error.to_string())?;
    let addrs = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?;
    let mut last_error = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(timeout))
                    .map_err(|error| error.to_string())?;
                stream
                    .set_write_timeout(Some(timeout))
                    .map_err(|error| error.to_string())?;
                let tls_stream = tls
                    .connect(&config.host, stream)
                    .map_err(|error| error.to_string())?;
                let mut client = imap::Client::new(tls_stream);
                client.read_greeting().map_err(|error| error.to_string())?;
                return Ok(client);
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    Err(last_error.unwrap_or_else(|| "no IMAP socket addresses resolved".to_owned()))
}

fn remember_seen_email_uid(
    seen_uids: &mut BTreeSet<String>,
    seen_uid_order: &mut VecDeque<String>,
    uid: String,
) {
    if seen_uids.insert(uid.clone()) {
        seen_uid_order.push_back(uid);
    }
    while seen_uid_order.len() > 1024 {
        if let Some(oldest) = seen_uid_order.pop_front() {
            seen_uids.remove(&oldest);
        }
    }
}

fn parse_email_body(body: &[u8], uid: String) -> Option<InboundMessage> {
    let parsed = mailparse::parse_mail(body).ok()?;
    let headers = parsed.get_headers();
    let sender = headers
        .get_first_value("From")
        .unwrap_or_else(|| "unknown@example.invalid".to_owned());
    let subject = headers
        .get_first_value("Subject")
        .unwrap_or_else(|| "(no subject)".to_owned());
    let date = headers.get_first_value("Date").unwrap_or_default();
    let message_id = headers
        .get_first_value("Message-Id")
        .unwrap_or_else(|| uid.clone());
    let body = parsed.get_body().unwrap_or_default();
    Some(
        EmailInbound {
            sender_email: sender,
            subject,
            date,
            body,
            message_id,
            uid: Some(uid),
            attachments: Vec::new(),
        }
        .into_message(),
    )
}

fn run_whatsapp_transport(
    config: WhatsAppRuntimeConfig,
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
) {
    let agent = runtime_http_agent(Duration::from_secs(30));
    let mut recent = RecentMessageIds::default();
    let channel_config = WhatsAppChannelConfig {
        bridge_url: config.bridge_url.clone(),
        bridge_token: config.bridge_token.clone(),
        allowlist: config.allowlist.clone(),
        group_policy: config.group_policy.clone(),
    };
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(&outbound_rx, |message| {
            send_whatsapp_message(&agent, &config, message)
        });
        match get_json(
            &agent,
            &join_url_path(&config.bridge_url, &config.poll_path),
            config
                .bridge_token
                .as_ref()
                .map(|token| bearer_header(token)),
        ) {
            Ok(value) => {
                for item in whatsapp_message_items(&value) {
                    match serde_json::from_value::<WhatsAppBridgeMessage>(item.clone())
                        .map_err(|error| error.to_string())
                        .and_then(|message| {
                            normalize_whatsapp_bridge_message(message, &channel_config, &mut recent)
                                .map_err(|error| error.to_string())
                        }) {
                        Ok(Some(inbound)) => {
                            let _ = inbound_tx.send(inbound);
                        }
                        Ok(None) => {}
                        Err(error) => eprintln!("whatsapp bridge message failed: {error}"),
                    }
                }
            }
            Err(error) => eprintln!("whatsapp bridge polling failed: {error}"),
        }
        sleep_with_stop(&stop, Duration::from_secs(config.poll_interval_seconds));
    }
}

fn whatsapp_message_items(value: &Value) -> Vec<&Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn send_whatsapp_message(
    agent: &ureq::Agent,
    config: &WhatsAppRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    for frame in whatsapp_outbound_frames(message) {
        let body = match frame {
            WhatsAppOutboundFrame::Auth { token } => json!({"type": "auth", "token": token}),
            WhatsAppOutboundFrame::Send { to, text } => {
                json!({"type": "send", "to": to, "text": text})
            }
            WhatsAppOutboundFrame::SendMedia {
                to,
                file_path,
                mimetype,
                file_name,
            } => json!({
                "type": "send_media",
                "to": to,
                "filePath": file_path,
                "mimetype": mimetype,
                "fileName": file_name,
            }),
        };
        post_json(
            agent,
            &join_url_path(&config.bridge_url, &config.send_path),
            config
                .bridge_token
                .as_ref()
                .map(|token| bearer_header(token)),
            body,
        )?;
    }
    Ok(())
}

fn drain_outbound(
    outbound_rx: &mpsc::Receiver<OutboundMessage>,
    mut send: impl FnMut(OutboundMessage) -> Result<(), String>,
) {
    while let Ok(message) = outbound_rx.try_recv() {
        if let Err(error) = send(message) {
            eprintln!("external channel outbound failed: {error}");
        }
    }
}

fn discard_outbound(outbound_rx: &mpsc::Receiver<OutboundMessage>, reason: &str) {
    while let Ok(message) = outbound_rx.try_recv() {
        eprintln!(
            "external channel outbound dropped for {}: {reason}",
            message.channel
        );
    }
}

fn runtime_http_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .new_agent()
}

fn get_json(
    agent: &ureq::Agent,
    url: &str,
    authorization: Option<String>,
) -> Result<Value, String> {
    let mut request = agent.get(url);
    if let Some(authorization) = authorization {
        request = request.header("Authorization", authorization);
    }
    read_json_response(request.call().map_err(|error| {
        format!(
            "request to {} failed: {}",
            redact_sensitive_url_text(url),
            redact_sensitive_url_text(&error.to_string())
        )
    })?)
}

fn post_json(
    agent: &ureq::Agent,
    url: &str,
    authorization: Option<String>,
    body: Value,
) -> Result<Value, String> {
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some(authorization) = authorization {
        request = request.header("Authorization", authorization);
    }
    let body = serde_json::to_string(&body).map_err(|error| error.to_string())?;
    read_json_response(request.send(body).map_err(|error| {
        format!(
            "request to {} failed: {}",
            redact_sensitive_url_text(url),
            redact_sensitive_url_text(&error.to_string())
        )
    })?)
}

fn read_json_response(mut response: ureq::http::Response<ureq::Body>) -> Result<Value, String> {
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    if status >= 400 {
        return Err(format!("HTTP {status}: {body}"));
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

fn bearer_header(token: &str) -> String {
    format!("Bearer {token}")
}

fn json_id_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
}

fn join_url_path(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn sleep_with_stop(stop: &AtomicBool, duration: Duration) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::SeqCst) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn gateway_preset(options: GatewayOptions) -> Result<GatewayPresetReport, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    let addr = resolve_gateway_addr(&options, &bundle.config.gateway)?;
    Ok(GatewayPresetReport {
        config_path: bundle.context.config_path,
        workspace: bundle.context.workspace,
        addr,
        verbose: options.verbose,
    })
}

pub fn web_preset(options: WebOptions) -> Result<WebPresetReport, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    let gateway_options = GatewayOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        port: options.gateway_port,
        verbose: options.verbose,
    };
    let gateway_addr = resolve_gateway_addr(&gateway_options, &bundle.config.gateway)?;
    let websocket = websocket_preset(&bundle.config.channels.plugins, &options)?;
    let assets_dir = shacs_web::manifest_dist_dir();
    let assets_populated = shacs_web::dist_is_populated(&assets_dir);
    Ok(WebPresetReport {
        config_path: bundle.context.config_path,
        workspace: bundle.context.workspace,
        gateway_addr,
        websocket,
        assets_dir,
        assets_populated,
        verbose: options.verbose,
    })
}

pub fn format_gateway_preset_report(report: GatewayPresetReport) -> String {
    let mut lines = vec![
        "Gateway preset ready".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
        format!("Gateway URL: http://{}", report.addr),
    ];
    if report.verbose {
        lines.push("Verbose: enabled".to_owned());
    }
    lines.join("\n")
}

pub fn format_web_preset_report(report: WebPresetReport) -> String {
    let mut lines = vec![
        "Web UI preset ready (use `shacs-bot run` to start WebSocket runtime)".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
        format!("Gateway URL: http://{}", report.gateway_addr),
        format!(
            "WebSocket: enabled={} ws://{}:{}{}",
            report.websocket.enabled,
            report.websocket.host,
            report.websocket.port,
            report.websocket.path
        ),
        format!("Assets: {}", report.assets_dir.display()),
        format!("Assets populated: {}", report.assets_populated),
    ];
    if report.verbose {
        lines.push("Verbose: enabled".to_owned());
    }
    lines.join("\n")
}

pub fn help_text() -> String {
    [
        "shacs-bot",
        "",
        "Usage:",
        "  shacs-bot [--config <path>] <command>",
        "",
        "Commands:",
        "  onboard   Create or refresh config and workspace templates",
        "  status    Show config, workspace, model, and provider status",
        "  runtime   Inspect local runtime/workspace state",
        "  session   Manage local session files",
        "  skills    List and inspect local skill registry entries",
        "  channels  List channel registry/config status",
        "  ask       Send one message through the local AgentLoop",
        "  run       Start selected channel runtime workers",
        "  serve     Start the local OpenAI-compatible HTTP API",
        "  api serve Compatibility alias for serve",
        "  gateway   Report gateway preset boundary without starting channels",
        "  web       Report WebUI asset and WebSocket preset boundary",
        "  agent     Alias for one-shot direct AgentLoop messages with -m/--message",
        "  provider  Manage provider auth; Codex login/import-token are available",
        "  plugins   Reserved for a later plugin slice",
        "",
        "Options:",
        "  -c, --config <path>   Use an explicit config file",
        "  -w, --workspace <path> Override workspace for ask/agent/serve",
        "  -m, --message <text> Direct message for agent",
        "      --bind <addr>     Serve API on host:port",
        "      --host <ip>       Override API host for serve",
        "      --port <port>     Override API/gateway port for serve/gateway",
        "      --gateway-port <port> Override gateway port for web preset",
        "      --websocket-host <host> Override WebSocket host for web/run",
        "      --websocket-port <port> Override WebSocket port for web/run",
        "  -t, --timeout <sec>   Override API request timeout for serve",
        "      --temperature <n> Override generation temperature for ask/agent",
        "      --max-tokens <n>  Override max output tokens for ask/agent",
        "      --markdown/--no-markdown  Accept nanobot direct CLI render flags",
        "      --verbose         Print additional serve diagnostics",
        "      --allow-remote    Permit non-loopback API binding",
        "      --allow-api-side-effects  Enable write/edit/exec tools in API turns",
        "      --session <id>    Use a CLI session key for ask/agent",
        "      --session <key>   Select a session for session commands",
        "      --max-messages <n> Limit session history output",
        "      --format <json|jsonl> Select session export format",
        "      --keep-messages <n> Retain this many messages during session compact",
        "      --all             Include inactive skill diagnostics in skills list",
        "  -y, --yes            Confirm irreversible session delete",
        "      --allow-side-effects  Enable write/edit/exec tools in CLI turns",
        "      --token-stdin     Read Codex token from stdin for provider codex import-token",
        "      --token-env <var> Read Codex token from an environment variable",
        "      --account-id <id> Store optional ChatGPT account id for Codex",
        "  -h, --help            Show help",
        "  -v, --version         Show version",
    ]
    .join("\n")
}

fn parse_global_config(parser: &mut ArgParser) -> Result<Option<PathBuf>, CliError> {
    let mut config_path = None;
    loop {
        match parser.peek() {
            Some("--config") | Some("-c") => {
                let Some(flag) = parser.next() else {
                    return Err(CliError::InvalidArguments(
                        "missing config flag while parsing global options".to_owned(),
                    ));
                };
                config_path = Some(take_path(parser, &flag)?);
            }
            _ => return Ok(config_path),
        }
    }
}

fn parse_onboard(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = OnboardOptions {
        config_path: global_config,
        workspace: None,
        wizard: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => options.workspace = Some(take_path(&mut parser, &arg)?),
            "--wizard" => options.wizard = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown onboard argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Onboard(options))
}

fn parse_status(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = StatusOptions {
        config_path: global_config,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown status argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Status(options))
}

fn parse_runtime_inspect(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = RuntimeInspectOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown runtime inspect argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::RuntimeInspect(options))
}

fn parse_session(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "session requires `list`, `inspect`, `create`, `history`, `export`, `clear`, `diagnostics`, `compact`, or `delete`".to_owned(),
        ));
    };
    match action.as_str() {
        "list" => parse_session_list(parser, global_config),
        "inspect" => parse_session_inspect(parser, global_config),
        "create" => parse_session_create(parser, global_config),
        "history" => parse_session_history(parser, global_config),
        "export" => parse_session_export(parser, global_config),
        "clear" => parse_session_clear(parser, global_config),
        "diagnostics" | "diagnose" => parse_session_diagnostics(parser, global_config),
        "compact" => parse_session_compact(parser, global_config),
        "delete" => parse_session_delete(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown session action `{other}`"
        ))),
    }
}

fn parse_skills(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "skills requires `list` or `show <name>`".to_owned(),
        ));
    };
    match action.as_str() {
        "list" | "ls" => parse_skills_list(parser, global_config),
        "show" | "inspect" => parse_skills_show(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown skills subcommand `{other}`"
        ))),
    }
}

fn parse_skills_list(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SkillsListOptions {
        config_path: global_config,
        workspace_override: None,
        all: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--all" => options.all = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown skills list argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Skills(SkillsCommand::List(options)))
}

fn parse_skills_show(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SkillsShowOptions {
        config_path: global_config,
        workspace_override: None,
        name: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--name" | "-n" => options.name = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown skills show argument `{other}`"
                )))
            }
            other => {
                if !options.name.is_empty() {
                    return Err(CliError::InvalidArguments(
                        "skills show accepts exactly one skill name".to_owned(),
                    ));
                }
                options.name = other.to_owned();
            }
        }
    }
    if options.name.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "skills show requires a skill name".to_owned(),
        ));
    }
    Ok(CliCommand::Skills(SkillsCommand::Show(options)))
}

fn parse_channels(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "channels requires `list` or `status`".to_owned(),
        ));
    };
    match action.as_str() {
        "list" | "ls" => parse_channels_list(parser, global_config),
        "status" | "inspect" => parse_channels_status(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown channels subcommand `{other}`"
        ))),
    }
}

fn parse_channels_list(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = ChannelsListOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown channels list argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Channels(ChannelsCommand::List(options)))
}

fn parse_channels_status(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = ChannelsStatusOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown channels status argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Channels(ChannelsCommand::Status(options)))
}

fn parse_session_list(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionListOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session list argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Session(SessionCommand::List(options)))
}

fn parse_session_inspect(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionInspectOptions {
        config_path: global_config,
        workspace_override: None,
        session: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session inspect argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session inspect requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Inspect(options)))
}

fn parse_session_create(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionCreateOptions {
        config_path: global_config,
        workspace_override: None,
        session: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session create argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session create requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Create(options)))
}

fn parse_session_history(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionHistoryCliOptions {
        config_path: global_config,
        ..SessionHistoryCliOptions::default()
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--max-messages" | "-n" => {
                options.max_messages = take_positive_usize(&mut parser, &arg)?
            }
            "--max-tokens" => options.max_tokens = take_positive_usize(&mut parser, &arg)?,
            "--timestamps" => options.timestamps = true,
            "--json" => options.json = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session history argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session history requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::History(options)))
}

fn parse_session_export(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionExportOptions {
        config_path: global_config,
        ..SessionExportOptions::default()
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--format" => {
                let value = take_value(&mut parser, &arg)?;
                options.format = match value.as_str() {
                    "json" => SessionExportFormat::Json,
                    "jsonl" => SessionExportFormat::Jsonl,
                    other => {
                        return Err(CliError::InvalidArguments(format!(
                            "unknown session export format `{other}`"
                        )))
                    }
                };
            }
            "--yes" | "-y" => options.yes = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session export argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session export requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Export(options)))
}

fn parse_session_clear(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionClearOptions {
        config_path: global_config,
        workspace_override: None,
        session: String::new(),
        yes: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--yes" | "-y" => options.yes = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session clear argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session clear requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Clear(options)))
}

fn parse_session_diagnostics(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionDiagnosticsOptions {
        config_path: global_config,
        workspace_override: None,
        session: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session diagnostics argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session diagnostics requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Diagnostics(options)))
}

fn parse_session_compact(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionCompactOptions {
        config_path: global_config,
        ..SessionCompactOptions::default()
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--keep-messages" => options.keep_messages = take_positive_usize(&mut parser, &arg)?,
            "--yes" | "-y" => options.yes = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session compact argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session compact requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Compact(options)))
}

fn parse_session_delete(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = SessionDeleteOptions {
        config_path: global_config,
        workspace_override: None,
        session: String::new(),
        yes: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--session" | "-s" => options.session = take_value(&mut parser, &arg)?,
            "--yes" | "-y" => options.yes = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown session delete argument `{other}`"
                )))
            }
        }
    }
    if options.session.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "session delete requires --session <key>".to_owned(),
        ));
    }
    Ok(CliCommand::Session(SessionCommand::Delete(options)))
}

fn parse_provider(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(first) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "provider requires a subcommand".to_owned(),
        ));
    };
    match first.as_str() {
        "codex" | "openai-codex" | "openai_codex" => parse_provider_codex(parser, global_config),
        "login" => {
            let Some(provider) = parser.next() else {
                return Err(CliError::InvalidArguments(
                    "provider login requires a provider name".to_owned(),
                ));
            };
            if is_codex_provider_name(&provider) {
                parse_codex_login(parser, global_config)
            } else {
                Err(CliError::InvalidArguments(format!(
                    "unsupported provider login target `{provider}`"
                )))
            }
        }
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown provider subcommand `{other}`"
        ))),
    }
}

fn parse_provider_codex(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "provider codex requires `login` or `import-token`".to_owned(),
        ));
    };
    match action.as_str() {
        "import-token" => parse_codex_import_token(parser, global_config),
        "login" => parse_codex_login(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown provider codex action `{other}`"
        ))),
    }
}

fn parse_codex_import_token(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut token_source = None;
    let mut account_id = None;
    let mut select = true;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--token-stdin" => token_source = Some(TokenSource::Stdin),
            "--token-env" => token_source = Some(TokenSource::Env(take_value(&mut parser, &arg)?)),
            "--account-id" => account_id = Some(take_value(&mut parser, &arg)?),
            "--select" => select = true,
            "--no-select" => select = false,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown provider codex import-token argument `{other}`"
                )))
            }
        }
    }
    let token_source = token_source.ok_or_else(|| {
        CliError::InvalidArguments(
            "provider codex import-token requires --token-stdin or --token-env <name>".to_owned(),
        )
    })?;
    Ok(CliCommand::Provider(ProviderCommand::CodexImportToken(
        CodexImportTokenOptions {
            config_path,
            token_source,
            account_id,
            select,
        },
    )))
}

fn parse_codex_login(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = CodexLoginOptions {
        config_path: global_config,
        headless: false,
        no_browser: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--headless" => options.headless = true,
            "--no-browser" => options.no_browser = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown provider codex login argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Provider(ProviderCommand::CodexLogin(options)))
}

fn is_codex_provider_name(value: &str) -> bool {
    matches!(value, "codex" | "openai-codex" | "openai_codex")
}

fn parse_ask(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    agent_alias: bool,
) -> Result<CliCommand, CliError> {
    let mut options = AskOptions {
        config_path: global_config,
        ..AskOptions::default()
    };
    let mut message_parts = Vec::new();
    let mut positional_message = false;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--message" | "-m" => message_parts.push(take_value(&mut parser, &arg)?),
            "--session" | "-s" => options.session = Some(take_value(&mut parser, &arg)?),
            "--temperature" => options.temperature = Some(take_temperature(&mut parser, &arg)?),
            "--max-tokens" => options.max_tokens = Some(take_max_tokens(&mut parser, &arg)?),
            "--allow-side-effects" => options.allow_side_effects = true,
            "--markdown" => options.markdown = true,
            "--no-markdown" => options.markdown = false,
            "--help" | "-h" => return Ok(CliCommand::Help),
            "--" => {
                while let Some(value) = parser.next() {
                    positional_message = true;
                    message_parts.push(value);
                }
                break;
            }
            other if other.starts_with('-') => {
                let command = if agent_alias { "agent" } else { "ask" };
                return Err(CliError::InvalidArguments(format!(
                    "unknown {command} argument `{other}`"
                )));
            }
            other => {
                positional_message = true;
                message_parts.push(other.to_owned());
            }
        }
    }
    options.message = message_parts.join(" ").trim().to_owned();
    if agent_alias && positional_message {
        return Err(CliError::InvalidArguments(
            "agent direct messages require -m/--message; interactive agent mode is deferred"
                .to_owned(),
        ));
    }
    if options.message.is_empty() {
        if agent_alias {
            return Ok(CliCommand::Unsupported(UnsupportedCommand {
                name: "agent".to_owned(),
                reason: "interactive agent mode is deferred; use `agent -m <message>` or `ask <message>`"
                    .to_owned(),
            }));
        }
        return Err(CliError::InvalidArguments(
            "ask requires a message argument".to_owned(),
        ));
    }
    Ok(CliCommand::Ask(options))
}

fn parse_run(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = RunOptions {
        config_path: global_config,
        ..RunOptions::default()
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--websocket-host" => options.websocket_host = Some(take_value(&mut parser, &arg)?),
            "--websocket-port" => options.websocket_port = Some(take_port(&mut parser, &arg)?),
            "--timeout" | "-t" => options.timeout = Some(take_timeout(&mut parser, &arg)?),
            "--verbose" | "-v" => options.verbose = true,
            "--allow-remote" => options.allow_remote = true,
            "--allow-side-effects" | "--allow-api-side-effects" => {
                options.allow_side_effects = true
            }
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown run argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Run(options))
}

fn parse_serve(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = ServeOptions {
        config_path: global_config,
        workspace_override: None,
        bind: None,
        host: None,
        port: None,
        timeout: None,
        verbose: false,
        allow_remote: false,
        allow_api_side_effects: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--bind" => options.bind = Some(take_socket_addr(&mut parser, &arg)?),
            "--host" => options.host = Some(take_value(&mut parser, &arg)?),
            "--port" => options.port = Some(take_port(&mut parser, &arg)?),
            "--timeout" | "-t" => options.timeout = Some(take_timeout(&mut parser, &arg)?),
            "--verbose" => options.verbose = true,
            "--allow-remote" => options.allow_remote = true,
            "--allow-api-side-effects" => options.allow_api_side_effects = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown serve argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Serve(options))
}

fn parse_gateway(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = GatewayOptions {
        config_path: global_config,
        workspace_override: None,
        port: None,
        verbose: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--port" | "-p" => options.port = Some(take_port(&mut parser, &arg)?),
            "--verbose" | "-v" => options.verbose = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown gateway argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Gateway(options))
}

fn parse_web(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = WebOptions {
        config_path: global_config,
        workspace_override: None,
        gateway_port: None,
        websocket_host: None,
        websocket_port: None,
        verbose: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--gateway-port" => options.gateway_port = Some(take_port(&mut parser, &arg)?),
            "--websocket-host" => options.websocket_host = Some(take_value(&mut parser, &arg)?),
            "--websocket-port" => options.websocket_port = Some(take_port(&mut parser, &arg)?),
            "--verbose" | "-v" => options.verbose = true,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown web argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Web(options))
}

fn take_path(parser: &mut ArgParser, flag: &str) -> Result<PathBuf, CliError> {
    take_value(parser, flag).map(PathBuf::from)
}

fn take_value(parser: &mut ArgParser, flag: &str) -> Result<String, CliError> {
    parser
        .next()
        .ok_or_else(|| CliError::InvalidArguments(format!("missing value for `{flag}`")))
}

fn take_socket_addr(parser: &mut ArgParser, flag: &str) -> Result<SocketAddr, CliError> {
    let value = take_value(parser, flag)?;
    value.parse::<SocketAddr>().map_err(|error| {
        CliError::InvalidArguments(format!("invalid socket address for `{flag}`: {error}"))
    })
}

fn take_port(parser: &mut ArgParser, flag: &str) -> Result<u16, CliError> {
    let value = take_value(parser, flag)?;
    let port = value.parse::<u16>().map_err(|error| {
        CliError::InvalidArguments(format!("invalid port for `{flag}`: {error}"))
    })?;
    if port == 0 {
        return Err(CliError::InvalidArguments(format!(
            "port for `{flag}` must be positive"
        )));
    }
    Ok(port)
}

fn take_timeout(parser: &mut ArgParser, flag: &str) -> Result<f64, CliError> {
    let value = take_value(parser, flag)?;
    let timeout = value.parse::<f64>().map_err(|error| {
        CliError::InvalidArguments(format!("invalid timeout for `{flag}`: {error}"))
    })?;
    if timeout <= 0.0 {
        return Err(CliError::InvalidArguments(format!(
            "timeout for `{flag}` must be positive"
        )));
    }
    Ok(timeout)
}

fn take_temperature(parser: &mut ArgParser, flag: &str) -> Result<f64, CliError> {
    let value = take_value(parser, flag)?;
    let temperature = value.parse::<f64>().map_err(|error| {
        CliError::InvalidArguments(format!("invalid temperature for `{flag}`: {error}"))
    })?;
    if temperature < 0.0 {
        return Err(CliError::InvalidArguments(format!(
            "temperature for `{flag}` must be non-negative"
        )));
    }
    Ok(temperature)
}

fn take_max_tokens(parser: &mut ArgParser, flag: &str) -> Result<u32, CliError> {
    let value = take_value(parser, flag)?;
    let max_tokens = value.parse::<u32>().map_err(|error| {
        CliError::InvalidArguments(format!("invalid max tokens for `{flag}`: {error}"))
    })?;
    if max_tokens == 0 {
        return Err(CliError::InvalidArguments(format!(
            "max tokens for `{flag}` must be positive"
        )));
    }
    Ok(max_tokens)
}

fn take_positive_usize(parser: &mut ArgParser, flag: &str) -> Result<usize, CliError> {
    let value = take_value(parser, flag)?;
    let parsed = value.parse::<usize>().map_err(|error| {
        CliError::InvalidArguments(format!("invalid positive integer for `{flag}`: {error}"))
    })?;
    if parsed == 0 {
        return Err(CliError::InvalidArguments(format!(
            "value for `{flag}` must be positive"
        )));
    }
    Ok(parsed)
}

fn resolve_serve_addr(options: &ServeOptions, api: &ApiConfig) -> Result<SocketAddr, CliError> {
    if options.bind.is_some() && (options.host.is_some() || options.port.is_some()) {
        return Err(CliError::InvalidArguments(
            "--bind cannot be combined with --host or --port".to_owned(),
        ));
    }
    if let Some(addr) = options.bind {
        return Ok(addr);
    }
    let host = options.host.as_deref().unwrap_or(&api.host);
    let port = options.port.unwrap_or(api.port);
    let ip = host.parse::<IpAddr>().map_err(|error| {
        CliError::InvalidArguments(format!("API host must be an IP address: {error}"))
    })?;
    Ok(SocketAddr::new(ip, port))
}

fn resolve_gateway_addr(
    options: &GatewayOptions,
    gateway: &shacs_config::GatewayConfig,
) -> Result<SocketAddr, CliError> {
    let host = gateway.host.as_str();
    let port = options.port.unwrap_or(gateway.port);
    let ip = host.parse::<IpAddr>().map_err(|error| {
        CliError::InvalidArguments(format!("gateway host must be an IP address: {error}"))
    })?;
    Ok(SocketAddr::new(ip, port))
}

fn websocket_preset(
    plugins: &BTreeMap<String, Value>,
    options: &WebOptions,
) -> Result<WebsocketPreset, CliError> {
    let websocket = plugins.get("websocket").and_then(Value::as_object);
    let enabled = websocket
        .and_then(|object| object.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let host = options
        .websocket_host
        .clone()
        .or_else(|| websocket_string(websocket, "host"))
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    if host.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "websocket host must not be empty".to_owned(),
        ));
    }
    let port = options
        .websocket_port
        .or_else(|| websocket_port(websocket, "port"))
        .unwrap_or(8765);
    let path = normalize_websocket_path(
        websocket_string(websocket, "path").unwrap_or_else(|| "/".to_owned()),
    )?;
    Ok(WebsocketPreset {
        enabled,
        host,
        port,
        path,
    })
}

fn websocket_string(object: Option<&serde_json::Map<String, Value>>, key: &str) -> Option<String> {
    object
        .and_then(|object| object.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn websocket_port(object: Option<&serde_json::Map<String, Value>>, key: &str) -> Option<u16> {
    let value = object.and_then(|object| object.get(key))?;
    if let Some(port) = value.as_u64() {
        return u16::try_from(port).ok().filter(|port| *port > 0);
    }
    value
        .as_str()
        .and_then(|value| value.trim().parse::<u16>().ok())
        .filter(|port| *port > 0)
}

fn normalize_websocket_path(path: String) -> Result<String, CliError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(CliError::InvalidArguments(
            "websocket path must not be empty".to_owned(),
        ));
    }
    if trimmed.starts_with('/') {
        Ok(trimmed.to_owned())
    } else {
        Ok(format!("/{trimmed}"))
    }
}

fn validate_serve_security(options: &ServeOptions, addr: SocketAddr) -> Result<(), CliError> {
    if !addr.ip().is_loopback() && !options.allow_remote {
        return Err(CliError::InvalidArguments(
            "non-loopback API bind requires --allow-remote".to_owned(),
        ));
    }
    Ok(())
}

pub struct AgentLoopChatCompletionAdapter {
    configured_model: String,
    provider_id: String,
    defaults: AgentDefaults,
    resolved_model: String,
    client: Arc<dyn ProviderClient>,
    retry_mode: ProviderRetryMode,
    workspace: PathBuf,
    media_dir: PathBuf,
    tools: ToolRegistry,
    _mcp_runtime: Option<McpRuntime>,
    _mcp_reports: Vec<McpServerConnectionReport>,
    allow_side_effect_tools: bool,
}

impl AgentLoopChatCompletionAdapter {
    pub fn from_bundle(
        bundle: ConfigBundle,
        allow_side_effect_tools: bool,
    ) -> Result<Self, CliError> {
        let defaults = bundle.config.agents.defaults.clone();
        let registry = ProviderRegistry::new();
        let resolved = resolve_provider_client(
            &registry,
            &defaults.provider,
            &defaults.model,
            &bundle.config.providers,
        )?;
        let retry_mode = ProviderRetryMode::from_config(&defaults.provider_retry_mode);
        let media_dir = bundle.context.media_dir(Some("api"));
        fs::create_dir_all(&media_dir)?;
        let tooling = production_tool_registry(&bundle, allow_side_effect_tools)?;
        let provider_id = resolved.provider_id.clone();
        let resolved_model = resolved.model.clone();
        let client: Arc<dyn ProviderClient> = Arc::from(resolved.client);
        Ok(Self {
            configured_model: defaults.model.clone(),
            provider_id,
            defaults,
            resolved_model,
            client,
            retry_mode,
            workspace: bundle.context.workspace,
            media_dir,
            tools: tooling.registry,
            _mcp_runtime: tooling.mcp_runtime,
            _mcp_reports: tooling.mcp_reports,
            allow_side_effect_tools,
        })
    }

    fn loop_config(&self) -> AgentLoopConfig {
        let mut config = AgentLoopConfig::new(&self.workspace, self.resolved_model.clone());
        config.settings = shacs_providers::GenerationSettings {
            temperature: self.defaults.temperature,
            max_tokens: self.defaults.max_tokens,
            reasoning_effort: self.defaults.reasoning_effort.clone(),
        };
        config.retry_mode = self.retry_mode;
        config.max_iterations = self.defaults.max_tool_iterations as usize;
        config.max_tool_result_chars = self.defaults.max_tool_result_chars;
        config.context_window_tokens = Some(self.defaults.context_window_tokens as usize);
        config.context_block_limit = self
            .defaults
            .context_block_limit
            .map(|value| value as usize);
        config.history_options = SessionHistoryOptions {
            max_messages: self.defaults.max_messages as usize,
            max_tokens: replay_token_budget(
                self.defaults.context_window_tokens as usize,
                self.defaults.max_tokens as usize,
            ),
            include_timestamps: true,
        };
        config.unified_session_key = self
            .defaults
            .unified_session
            .then(|| "api:default".to_owned());
        config
    }

    fn run_agent_loop(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: Option<ApiProviderEventCallback>,
    ) -> Result<LlmResponse, ApiError> {
        self.run_agent_loop_with_origin(invocation, on_event, "api", "user", shacs_api::API_CHAT_ID)
    }

    fn complete_direct(
        &self,
        invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        self.run_agent_loop_with_origin(invocation, None, "cli", "user", "direct")
    }

    fn complete_sdk_run(
        &self,
        invocation: ChatCompletionInvocation,
    ) -> Result<RunResult, ApiError> {
        let turn =
            self.run_agent_loop_turn_with_origin(invocation, None, "sdk", "user", "default")?;
        let messages = SessionManager::new(&self.workspace)
            .map_err(|error| {
                ApiError::internal(format!("session manager could not be initialized: {error}"))
            })?
            .load_existing(&turn.session_key)
            .map(|session| session.messages)
            .unwrap_or_default();
        Ok(RunResult {
            content: turn.final_content.unwrap_or_default(),
            tools_used: turn.tools_used,
            messages,
        })
    }

    fn run_agent_loop_with_origin(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: Option<ApiProviderEventCallback>,
        channel: &str,
        sender_id: &str,
        chat_id: &str,
    ) -> Result<LlmResponse, ApiError> {
        let turn = self
            .run_agent_loop_turn_with_origin(invocation, on_event, channel, sender_id, chat_id)?;
        Ok(llm_response_from_turn(turn))
    }

    fn run_agent_loop_turn_with_origin(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: Option<ApiProviderEventCallback>,
        channel: &str,
        sender_id: &str,
        chat_id: &str,
    ) -> Result<AgentLoopTurnResult, ApiError> {
        let mut config = self.loop_config();
        config.settings.temperature = invocation
            .temperature
            .unwrap_or(config.settings.temperature);
        config.settings.max_tokens = invocation.max_tokens.unwrap_or(config.settings.max_tokens);
        let message =
            InboundMessage::new(channel, sender_id, chat_id, invocation_text(&invocation))
                .with_media(invocation.media_paths.clone())
                .with_session_key_override(invocation.session_key.clone());
        let (result, _) = self.process_inbound_with_outbound(message, config, on_event)?;
        Ok(result)
    }

    pub fn process_websocket_frame(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
    ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
        let action = match normalize_websocket_frame(frame, client_id, default_chat_id) {
            Ok(action) => action,
            Err(error) => {
                return Ok(vec![WebSocketServerEvent::Error {
                    chat_id: Some(default_chat_id.to_owned()),
                    detail: Some(error.to_string()),
                }])
            }
        };
        match action {
            WebSocketInboundAction::NewChat => Ok(vec![WebSocketServerEvent::Ready {
                chat_id: default_chat_id.to_owned(),
                client_id: client_id.to_owned(),
            }]),
            WebSocketInboundAction::Attach { chat_id } => {
                Ok(vec![WebSocketServerEvent::Attached { chat_id }])
            }
            WebSocketInboundAction::Message(mut inbound) => {
                let session_key = inbound.session_key();
                let media_paths =
                    <Self as ChatCompletionAdapter>::persist_media_data_urls(self, &inbound.media)?;
                inbound.media = media_paths;
                inbound.session_key_override = Some(session_key);
                let (_, outbound) =
                    self.process_inbound_with_outbound(inbound, self.loop_config(), None)?;
                Ok(outbound
                    .into_iter()
                    .filter(|message| message.channel == WEBSOCKET_CHANNEL)
                    .map(websocket_event_from_outbound)
                    .collect())
            }
        }
    }

    fn process_inbound_with_outbound(
        &self,
        message: InboundMessage,
        config: AgentLoopConfig,
        on_event: Option<ApiProviderEventCallback>,
    ) -> Result<(AgentLoopTurnResult, Vec<shacs_channels::OutboundMessage>), ApiError> {
        let sessions = SessionManager::new(&self.workspace).map_err(|error| {
            ApiError::internal(format!("session manager could not be initialized: {error}"))
        })?;
        let bus = MessageBus::new();
        let outbound_bus = bus.clone();
        let subagent_runtime = SubagentRuntime::with_bus(bus.clone());
        let spawn_config = self.subagent_execution_config(&config);
        let subagent_client = self.client.clone();
        let spawner_runtime = subagent_runtime.clone();
        let spawn_tool = SpawnTool::new(Arc::new(move |request| {
            spawner_runtime
                .spawn_and_run_background(request, subagent_client.clone(), spawn_config.clone())
                .map(|outcome| outcome.user_message)
        }));
        let mut tools = self.tools.clone();
        tools.register(spawn_tool.clone());
        let mut loop_runtime = AgentLoop::new(
            bus,
            sessions,
            ContextBuilder::new(&self.workspace),
            &tools,
            self.client.as_ref(),
            config,
        )
        .with_context_tools(shacs_core::runtime::RuntimeContextTools::new().with_spawn(spawn_tool));
        if let Some(callback) = on_event {
            loop_runtime = loop_runtime.with_provider_event_callback(callback);
        }
        let result = loop_runtime
            .process_message(message)
            .map_err(|error| ApiError::internal(format!("agent loop request failed: {error}")))?;
        let mut outbound = Vec::new();
        while let Some(message) = outbound_bus.consume_outbound() {
            outbound.push(message);
        }
        Ok((result, outbound))
    }

    fn subagent_execution_config(&self, config: &AgentLoopConfig) -> SubagentExecutionConfig {
        let mut subagent =
            SubagentExecutionConfig::new(&self.workspace, self.resolved_model.clone());
        subagent.settings = config.settings.clone();
        subagent.retry_mode = config.retry_mode;
        subagent.max_iterations = config.max_iterations;
        subagent.max_tool_result_chars = config.max_tool_result_chars;
        subagent.fail_on_tool_error = true;
        subagent.allow_side_effect_tools = self.allow_side_effect_tools;
        subagent.enable_exec = self.allow_side_effect_tools;
        subagent.enable_web = true;
        subagent.restrict_to_workspace = true;
        subagent
    }
}

fn replay_token_budget(context_window_tokens: usize, max_output_tokens: usize) -> usize {
    context_window_tokens
        .saturating_sub(max_output_tokens)
        .saturating_mul(3)
        / 4
}

fn llm_response_from_turn(turn: AgentLoopTurnResult) -> LlmResponse {
    let mut response = LlmResponse {
        content: turn.final_content,
        finish_reason: turn.stop_reason,
        ..LlmResponse::default()
    };
    if response.finish_reason == "error" {
        response.error_status_code = Some(500);
    }
    response
}

impl ChatCompletionAdapter for AgentLoopChatCompletionAdapter {
    fn configured_model(&self) -> &str {
        &self.configured_model
    }

    fn models(&self) -> Vec<ApiModel> {
        vec![ApiModel {
            id: self.configured_model.clone(),
            owned_by: self.provider_id.clone(),
        }]
    }

    fn complete_chat(&self, invocation: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        self.run_agent_loop(invocation, None)
    }

    fn stream_chat(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: &mut dyn FnMut(shacs_providers::ProviderEvent),
    ) -> Result<LlmResponse, ApiError> {
        let (event_tx, event_rx) = mpsc::channel();
        let callback = Arc::new(move |event: &shacs_providers::ProviderEvent| {
            let _ = event_tx.send(event.clone());
        });
        let result = thread::scope(|scope| {
            let handle = scope.spawn(move || self.run_agent_loop(invocation, Some(callback)));
            for event in event_rx {
                on_event(event);
            }
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(ApiError::internal("agent loop stream task panicked")),
            }
        });
        let response = result?;
        Ok(response)
    }

    fn persist_media_data_urls(&self, data_urls: &[String]) -> Result<Vec<String>, ApiError> {
        let mut paths = Vec::new();
        for data_url in data_urls {
            match save_base64_data_url(data_url, &self.media_dir, Some(DEFAULT_MAX_BYTES)) {
                Ok(Some(path)) => paths.push(path),
                Ok(None) => {}
                Err(MediaDecodeError::FileSizeExceeded { limit }) => {
                    return Err(ApiError::payload_too_large(format!(
                        "media data URL exceeds {limit} bytes"
                    )))
                }
                Err(MediaDecodeError::Malformed) => {}
                Err(MediaDecodeError::Io(error)) => {
                    return Err(ApiError::internal(format!(
                        "media data URL could not be saved: {error}"
                    )))
                }
            }
        }
        Ok(paths)
    }

    fn persist_uploaded_file(
        &self,
        filename: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        if bytes.len() > shacs_api::MAX_MEDIA_BYTES {
            return Err(ApiError::payload_too_large(format!(
                "uploaded file exceeds {} bytes",
                shacs_api::MAX_MEDIA_BYTES
            )));
        }
        fs::create_dir_all(&self.media_dir).map_err(|error| {
            ApiError::internal(format!("media directory could not be created: {error}"))
        })?;
        let stem = unique_upload_stem();
        let name = filename
            .map(safe_filename)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "upload.bin".to_owned());
        let path = self.media_dir.join(format!("{stem}-{name}"));
        fs::write(&path, bytes).map_err(|error| {
            ApiError::internal(format!("uploaded file could not be saved: {error}"))
        })?;
        Ok(path.to_string_lossy().to_string())
    }

    fn process_websocket_frame(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
    ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
        AgentLoopChatCompletionAdapter::process_websocket_frame(
            self,
            frame,
            client_id,
            default_chat_id,
        )
    }
}

fn invocation_text(invocation: &ChatCompletionInvocation) -> String {
    let Some(content) = invocation
        .provider_request
        .messages
        .first()
        .and_then(|message| message.get("content"))
    else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

struct ProductionTooling {
    registry: ToolRegistry,
    mcp_runtime: Option<McpRuntime>,
    mcp_reports: Vec<McpServerConnectionReport>,
}

fn production_tool_registry(
    bundle: &ConfigBundle,
    allow_side_effect_tools: bool,
) -> Result<ProductionTooling, CliError> {
    let workspace = &bundle.context.workspace;
    fs::create_dir_all(workspace)?;
    let media_dir = bundle.context.media_dir(Some("api"));
    fs::create_dir_all(&media_dir)?;
    let path_context = PathContext {
        workspace: Some(workspace.clone()),
        allowed_dir: Some(workspace.clone()),
        media_dir: Some(media_dir),
        extra_allowed_dirs: Vec::new(),
    };
    let file_state = Arc::new(Mutex::new(FileState::new()));
    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool::with_file_state(
        path_context.clone(),
        file_state.clone(),
    ));
    if allow_side_effect_tools {
        registry.register(WriteFileTool::with_file_state(
            path_context.clone(),
            file_state.clone(),
        ));
        registry.register(EditFileTool::with_file_state(
            path_context.clone(),
            file_state,
        ));
    }
    registry.register(ListDirTool::new(path_context.clone()));
    registry.register(GlobTool::new(path_context.clone()));
    registry.register(GrepTool::new(path_context.clone()));
    if allow_side_effect_tools && bundle.config.tools.exec.enable {
        let mut exec_config = ExecConfig::new(path_context.clone());
        exec_config.network_guard = NetworkGuard::with_ssrf_whitelist(
            bundle
                .config
                .tools
                .ssrf_whitelist
                .iter()
                .map(String::as_str),
        );
        exec_config.timeout_seconds = u64::from(bundle.config.tools.exec.timeout);
        exec_config.restrict_to_workspace = bundle.config.tools.restrict_to_workspace;
        exec_config.sandbox = non_empty(Some(bundle.config.tools.exec.sandbox.as_str()))
            .then(|| bundle.config.tools.exec.sandbox.clone());
        exec_config.path_append = non_empty(Some(bundle.config.tools.exec.path_append.as_str()))
            .then(|| bundle.config.tools.exec.path_append.clone());
        exec_config.allowed_env_keys = bundle.config.tools.exec.allowed_env_keys.clone();
        registry.register(ExecTool::new(exec_config));
    }
    if bundle.config.tools.web.enable {
        let network_guard = NetworkGuard::with_ssrf_whitelist(
            bundle
                .config
                .tools
                .ssrf_whitelist
                .iter()
                .map(String::as_str),
        );
        let user_agent = bundle
            .config
            .tools
            .web
            .user_agent
            .clone()
            .unwrap_or_else(|| "Mozilla/5.0 (shacs-bot)".to_owned());
        registry.register(WebFetchTool::with_config(
            WebFetchConfig {
                user_agent: user_agent.clone(),
                network_guard: network_guard.clone(),
                ..WebFetchConfig::default()
            },
            Arc::new(shacs_core::tools::UreqWebClient),
        ));
        registry.register(WebSearchTool::new(WebSearchConfig {
            provider: bundle.config.tools.web.search.provider.clone(),
            api_key: bundle.config.tools.web.search.api_key.clone(),
            base_url: bundle.config.tools.web.search.base_url.clone(),
            max_results: bundle.config.tools.web.search.max_results as usize,
            timeout: Duration::from_secs(u64::from(bundle.config.tools.web.search.timeout)),
            user_agent,
            network_guard,
        }));
    }
    registry.register(AskUserTool::new());
    registry.register(SelfTool::with_modify_allowed(
        Arc::new(Mutex::new(SelfRuntimeState::new())),
        allow_side_effect_tools && bundle.config.tools.my.allow_set,
    ));
    let specs = mcp_server_specs(bundle);
    let (mcp_runtime, mcp_reports) = if specs.is_empty() {
        (None, Vec::new())
    } else {
        let runtime = McpRuntime::new(Some(Arc::new(StdioMcpConnector::new())));
        let reports = runtime.connect_and_register(&mut registry, &specs);
        (Some(runtime), reports)
    };
    Ok(ProductionTooling {
        registry,
        mcp_runtime,
        mcp_reports,
    })
}

fn mcp_server_specs(bundle: &ConfigBundle) -> Vec<McpServerSpec> {
    bundle
        .config
        .tools
        .mcp_servers
        .iter()
        .map(|(name, config)| McpServerSpec {
            name: name.clone(),
            r#type: config.r#type.clone(),
            command: non_empty(Some(config.command.as_str())).then(|| config.command.clone()),
            args: config.args.clone(),
            env: config
                .env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            url: non_empty(Some(config.url.as_str())).then(|| config.url.clone()),
            headers: config
                .headers
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            timeout_seconds: u64::from(config.tool_timeout),
            enabled_tools: config.enabled_tools.clone(),
        })
        .collect()
}

fn unique_upload_stem() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:012x}", nanos & 0xffffffffffff)
}

pub struct ProviderChatCompletionAdapter {
    configured_model: String,
    defaults: AgentDefaults,
    resolved: ResolvedProviderClient,
    retry_mode: ProviderRetryMode,
}

impl ProviderChatCompletionAdapter {
    pub fn from_bundle(bundle: ConfigBundle) -> Result<Self, CliError> {
        let defaults = bundle.config.agents.defaults.clone();
        let registry = ProviderRegistry::new();
        let resolved = resolve_provider_client(
            &registry,
            &defaults.provider,
            &defaults.model,
            &bundle.config.providers,
        )?;
        let retry_mode = ProviderRetryMode::from_config(&defaults.provider_retry_mode);
        Ok(Self {
            configured_model: defaults.model.clone(),
            defaults,
            resolved,
            retry_mode,
        })
    }

    fn provider_settings(
        &self,
        invocation: &ChatCompletionInvocation,
    ) -> shacs_providers::GenerationSettings {
        shacs_providers::GenerationSettings {
            temperature: invocation.temperature.unwrap_or(self.defaults.temperature),
            max_tokens: invocation.max_tokens.unwrap_or(self.defaults.max_tokens),
            reasoning_effort: self.defaults.reasoning_effort.clone(),
        }
    }
}

impl ChatCompletionAdapter for ProviderChatCompletionAdapter {
    fn configured_model(&self) -> &str {
        &self.configured_model
    }

    fn models(&self) -> Vec<ApiModel> {
        vec![ApiModel {
            id: self.configured_model.clone(),
            owned_by: self.resolved.provider_id.clone(),
        }]
    }

    fn complete_chat(&self, invocation: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        let settings = self.provider_settings(&invocation);
        let request = prepare_provider_request(
            &self.resolved,
            invocation.provider_request.messages,
            invocation.provider_request.tools,
            &self.defaults,
            Some(settings),
            invocation.provider_request.tool_choice,
        );
        let response = chat_with_retry(self.resolved.client.as_ref(), request, self.retry_mode)
            .map_err(api_error_from_provider_error)?;
        if response.finish_reason == "error" {
            return Err(api_error_from_provider_response(&response));
        }
        Ok(response)
    }
}

fn api_error_from_provider_error(error: ProviderError) -> ApiError {
    match error {
        ProviderError::ProviderNotFound { provider_id, .. } => ApiError::invalid_request(format!(
            "provider `{provider_id}` is not configured or supported"
        )),
        ProviderError::ModelNotFound {
            provider_id,
            model_id,
            ..
        } => ApiError::invalid_request(format!(
            "model `{model_id}` is not available for provider `{provider_id}`"
        )),
        ProviderError::AuthRequired { provider_id } => ApiError {
            status: 401,
            message: format!("provider `{provider_id}` requires authentication"),
            error_type: "authentication_error".to_owned(),
        },
        ProviderError::Api {
            status, message, ..
        } => {
            let status = normalize_provider_status(status);
            let message = if status >= 500 {
                "provider API request failed".to_owned()
            } else {
                message
            };
            ApiError {
                status,
                message,
                error_type: "provider_api_error".to_owned(),
            }
        }
    }
}

fn api_error_from_provider_response(response: &LlmResponse) -> ApiError {
    let status = normalize_provider_status(response.error_status_code);
    let message = if status >= 500 {
        "provider API request failed".to_owned()
    } else {
        response
            .content
            .clone()
            .unwrap_or_else(|| "provider returned an error response".to_owned())
    };
    ApiError {
        status,
        message,
        error_type: "provider_api_error".to_owned(),
    }
}

fn normalize_provider_status(status: Option<u16>) -> u16 {
    status
        .filter(|status| matches!(status, 400..=599))
        .unwrap_or(500)
}

fn format_onboard_outcome(outcome: OnboardOutcome) -> String {
    let mut lines = vec![
        "Onboard complete.".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Workspace: {}", display_path(&outcome.workspace)),
        format!("Runtime dirs ensured: {}", outcome.runtime_dirs.len()),
        format!("Template files created: {}", outcome.template_files.len()),
        format!("Template dirs created: {}", outcome.template_dirs.len()),
    ];
    if !outcome.migrations.is_empty() {
        lines.push(format!(
            "Migrations applied: {}",
            outcome.migrations.join(", ")
        ));
    }
    lines.push(
        "Next: edit the config provider API key, then run `shacs-bot ask \"hello\"`.".to_owned(),
    );
    lines.join("\n")
}

fn format_status_report(report: StatusReport) -> String {
    let mut lines = vec![
        "shacs-bot status".to_owned(),
        format!(
            "Config: {} ({})",
            display_path(&report.config_path),
            exists_label(report.config_exists)
        ),
        format!(
            "Workspace: {} ({})",
            display_path(&report.workspace),
            exists_label(report.workspace_exists)
        ),
        format!("Model: {}", report.model),
        format!("Provider: {}", report.provider),
    ];
    if report.providers.is_empty() {
        lines.push("Configured providers: none".to_owned());
    } else {
        lines.push("Configured providers:".to_owned());
        for provider in report.providers {
            lines.push(format!(
                "  - {}: api_key={}, api_base={}",
                provider.name,
                configured_label(provider.has_api_key),
                configured_label(provider.has_api_base)
            ));
        }
    }
    lines.join("\n")
}

fn format_runtime_inspect(report: RuntimeInspectReport) -> String {
    let mut lines = vec![
        "shacs-bot runtime inspect".to_owned(),
        format!(
            "Config: {} ({})",
            display_path(&report.config_path),
            exists_label(report.config_exists)
        ),
        format!(
            "Workspace: {} ({})",
            display_path(&report.workspace),
            exists_label(report.workspace_exists)
        ),
        format!("Data dir: {}", display_path(&report.data_dir)),
        format!("Provider: {}", report.provider),
        format!("Model: {}", report.model),
        format!("Sessions: {}", report.sessions.count),
    ];
    if let Some(latest_key) = report.sessions.latest_key {
        let updated = report
            .sessions
            .latest_updated_at
            .unwrap_or_else(|| "unknown".to_owned());
        lines.push(format!("Latest session: {latest_key} ({updated})"));
    }
    if report.providers.is_empty() {
        lines.push("Configured providers: none".to_owned());
    } else {
        lines.push("Configured providers:".to_owned());
        for provider in report.providers {
            lines.push(format!(
                "  - {}: api_key={}, api_base={}",
                provider.name,
                configured_label(provider.has_api_key),
                configured_label(provider.has_api_base)
            ));
        }
    }
    lines.push("Runtime capabilities:".to_owned());
    for capability in report.capabilities {
        lines.push(format!(
            "  - {}: {} ({})",
            capability.component,
            runtime_capability_label(&capability.status),
            capability.reason
        ));
    }
    lines.join("\n")
}

fn format_session_list(report: SessionListReport) -> String {
    let mut lines = vec![
        "shacs-bot session list".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Sessions: {}", report.sessions.len()),
    ];
    if report.sessions.is_empty() {
        lines.push("No sessions found.".to_owned());
    } else {
        for session in report.sessions {
            let created = session.created_at.unwrap_or_else(|| "unknown".to_owned());
            let updated = session.updated_at.unwrap_or_else(|| "unknown".to_owned());
            lines.push(format!(
                "  - {}: created={}, updated={}, path={}",
                session.key,
                created,
                updated,
                display_path(&session.path)
            ));
        }
    }
    lines.join("\n")
}

fn format_session_inspect(report: SessionInspectReport) -> String {
    let metadata = if report.metadata_keys.is_empty() {
        "none".to_owned()
    } else {
        report.metadata_keys.join(", ")
    };
    let recovery = if report.recovery_markers.is_empty() {
        "no".to_owned()
    } else {
        format!("yes ({})", report.recovery_markers.join(", "))
    };
    let checkpoint = report.checkpoint_phase.unwrap_or_else(|| "none".to_owned());
    [
        "shacs-bot session inspect".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!(
            "Created: {}",
            report.created_at.unwrap_or_else(|| "unknown".to_owned())
        ),
        format!(
            "Updated: {}",
            report.updated_at.unwrap_or_else(|| "unknown".to_owned())
        ),
        format!("Messages: {}", report.message_count),
        format!("Last consolidated: {}", report.last_consolidated),
        format!("Metadata keys: {metadata}"),
        format!("Recovery required: {recovery}"),
        format!("Checkpoint phase: {checkpoint}"),
    ]
    .join("\n")
}

fn format_session_create(report: SessionCreateReport) -> String {
    [
        "shacs-bot session create".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!("Created: {}", yes_no_label(report.created)),
    ]
    .join("\n")
}

fn format_session_history(report: SessionHistoryCliReport) -> String {
    if report.json {
        return serde_json::to_string_pretty(&report.history).unwrap_or_else(|_| "[]".to_owned());
    }
    let mut lines = vec![
        "shacs-bot session history".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!("Messages: {}", report.history.len()),
    ];
    if report.history.is_empty() {
        lines.push("No conversation history yet.".to_owned());
    } else {
        for message in report.history {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if !matches!(role, "user" | "assistant") {
                continue;
            }
            let content = session_message_content_text(&message);
            if content.is_empty() {
                continue;
            }
            lines.push(format!(
                "  - {role}: {}",
                truncate_for_history(&content, 200)
            ));
        }
    }
    lines.join("\n")
}

fn format_session_clear(report: SessionClearReport) -> String {
    [
        "shacs-bot session clear".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!("Cleared: {}", yes_no_label(report.cleared)),
        format!("Messages removed: {}", report.message_count_before),
    ]
    .join("\n")
}

fn format_session_diagnostics(report: SessionDiagnosticsReport) -> String {
    let metadata = if report.metadata_keys.is_empty() {
        "none".to_owned()
    } else {
        report.metadata_keys.join(", ")
    };
    let recovery = if report.recovery_markers.is_empty() {
        "none".to_owned()
    } else {
        report.recovery_markers.join(", ")
    };
    [
        "shacs-bot session diagnostics".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!("Exists: {}", yes_no_label(report.exists)),
        format!("Messages: {}", report.message_count),
        format!("Last consolidated: {}", report.last_consolidated),
        format!("Metadata keys: {metadata}"),
        format!("Recovery markers: {recovery}"),
        format!(
            "Checkpoint phase: {}",
            report.checkpoint_phase.unwrap_or_else(|| "none".to_owned())
        ),
        format!("Legal history start: {}", report.legal_start),
    ]
    .join("\n")
}

fn format_session_compact(report: SessionCompactReport) -> String {
    [
        "shacs-bot session compact".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!("Compacted: {}", yes_no_label(report.compacted)),
        format!("Kept messages: {}", report.kept_messages),
        format!("Archived messages: {}", report.archived_messages),
    ]
    .join("\n")
}

fn format_session_delete(report: SessionDeleteReport) -> String {
    [
        "shacs-bot session delete".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Key: {}", report.key),
        format!("Path: {}", display_path(&report.path)),
        format!("Deleted: {}", yes_no_label(report.deleted)),
    ]
    .join("\n")
}

fn runtime_capability_label(status: &RuntimeCapabilityStatus) -> &'static str {
    match status {
        RuntimeCapabilityStatus::Available => "available",
        RuntimeCapabilityStatus::Unavailable => "unavailable",
        RuntimeCapabilityStatus::Unsupported => "unsupported",
    }
}

fn format_codex_import_outcome(outcome: CodexImportOutcome) -> String {
    let model_line = outcome
        .selected_model
        .as_deref()
        .map(|model| format!("Selected model: {model}"))
        .unwrap_or_else(|| "Selected model: unchanged".to_owned());
    [
        "Codex token imported.".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Auth: {}", display_path(&outcome.auth_path)),
        format!("Provider: {}", outcome.provider),
        model_line,
        format!("Selected: {}", configured_label(outcome.selected)),
        format!("Account id: {}", configured_label(outcome.has_account_id)),
    ]
    .join("\n")
}

fn format_codex_login_outcome(outcome: CodexLoginOutcome) -> String {
    [
        "Codex login complete.".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Auth: {}", display_path(&outcome.auth_path)),
        format!("Provider: {}", outcome.provider),
        format!("Selected model: {}", outcome.selected_model),
        format!(
            "Account id: {}",
            configured_label(
                outcome
                    .account_id
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
            )
        ),
        format!("Expires: {}", configured_label(outcome.expires.is_some())),
    ]
    .join("\n")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn exists_label(exists: bool) -> &'static str {
    if exists {
        "exists"
    } else {
        "missing"
    }
}

fn configured_label(configured: bool) -> &'static str {
    if configured {
        "configured"
    } else {
        "missing"
    }
}

fn yes_no_label(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn session_message_content_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.trim().to_owned(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("content"))
                    .and_then(Value::as_str)
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn truncate_for_history(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn non_empty(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
struct ArgParser {
    args: Vec<String>,
    index: usize,
}

impl ArgParser {
    fn new(args: Vec<String>) -> Self {
        Self { args, index: 0 }
    }

    fn next(&mut self) -> Option<String> {
        let value = self.args.get(self.index).cloned();
        if value.is_some() {
            self.index += 1;
        }
        value
    }

    fn peek(&self) -> Option<&str> {
        self.args.get(self.index).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use shacs_config::{save_config_to_path, AuthStore, Config};
    use shacs_core::runtime::Session;
    use shacs_providers::{GenerationSettings, ProviderClient, ProviderEvent, ProviderRequest};
    use shacs_templates::WorkspaceSyncOutcome;
    use std::collections::{BTreeMap, VecDeque};
    use std::error::Error;
    use std::fs;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parser_handles_global_and_command_config_paths() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "onboard",
            "--workspace",
            "/tmp/workspace",
        ])?;
        let CliCommand::Onboard(options) = parsed else {
            return Err("expected onboard command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(options.workspace, Some(PathBuf::from("/tmp/workspace")));

        let parsed = parse_cli_args(["status", "-c", "/tmp/b.json"])?;
        let CliCommand::Status(options) = parsed else {
            return Err("expected status command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/b.json")));
        Ok(())
    }

    #[test]
    fn parser_handles_serve_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "serve",
            "--workspace",
            "/tmp/workspace",
            "--bind",
            "127.0.0.1:9999",
            "--timeout",
            "30.5",
            "--verbose",
            "--allow-remote",
            "--allow-api-side-effects",
        ])?;
        let CliCommand::Serve(options) = parsed else {
            return Err("expected serve command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.bind, Some("127.0.0.1:9999".parse()?));
        assert_eq!(options.timeout, Some(30.5));
        assert!(options.verbose);
        assert!(options.allow_remote);
        assert!(options.allow_api_side_effects);

        let parsed = parse_cli_args(["serve", "--host", "127.0.0.1", "--port", "8901"])?;
        let CliCommand::Serve(options) = parsed else {
            return Err("expected serve command".into());
        };
        assert_eq!(options.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(options.port, Some(8901));
        Ok(())
    }

    #[test]
    fn parser_handles_gateway_and_web_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "gateway",
            "--workspace",
            "/tmp/workspace",
            "--port",
            "8902",
            "--verbose",
        ])?;
        let CliCommand::Gateway(options) = parsed else {
            return Err("expected gateway command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.port, Some(8902));
        assert!(options.verbose);

        let parsed = parse_cli_args([
            "web",
            "--gateway-port",
            "8903",
            "--websocket-host",
            "127.0.0.1",
            "--websocket-port",
            "8766",
            "-v",
        ])?;
        let CliCommand::Web(options) = parsed else {
            return Err("expected web command".into());
        };
        assert_eq!(options.gateway_port, Some(8903));
        assert_eq!(options.websocket_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(options.websocket_port, Some(8766));
        assert!(options.verbose);
        Ok(())
    }

    #[test]
    fn parser_handles_channels_list_and_status_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "channels",
            "list",
            "--workspace",
            "/tmp/workspace",
        ])?;
        let CliCommand::Channels(ChannelsCommand::List(options)) = parsed else {
            return Err("expected channels list command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let parsed = parse_cli_args(["channel", "status", "-c", "/tmp/b.json"])?;
        let CliCommand::Channels(ChannelsCommand::Status(options)) = parsed else {
            return Err("expected channels status command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/b.json")));

        let error = parse_cli_args(["channels"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("channels requires `list` or `status`"));
        Ok(())
    }

    #[test]
    fn parser_rejects_invalid_gateway_and_web_ports() {
        let gateway_error = parse_cli_args(["gateway", "--port", "0"])
            .unwrap_err()
            .to_string();
        assert!(gateway_error.contains("must be positive"));

        let web_error = parse_cli_args(["web", "--websocket-port", "70000"])
            .unwrap_err()
            .to_string();
        assert!(web_error.contains("invalid port"));
    }

    #[test]
    fn parser_handles_runtime_inspect_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "runtime",
            "inspect",
            "--workspace",
            "/tmp/workspace",
        ])?;
        let CliCommand::RuntimeInspect(options) = parsed else {
            return Err("expected runtime inspect command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let error = parse_cli_args(["runtime"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("runtime requires `inspect`"));
        Ok(())
    }

    #[test]
    fn parser_handles_session_list_and_inspect_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "session",
            "list",
            "--workspace",
            "/tmp/workspace",
        ])?;
        let CliCommand::Session(SessionCommand::List(options)) = parsed else {
            return Err("expected session list command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let parsed = parse_cli_args([
            "sessions",
            "inspect",
            "--session",
            "cli:direct",
            "-w",
            "/tmp/runtime",
        ])?;
        let CliCommand::Session(SessionCommand::Inspect(options)) = parsed else {
            return Err("expected session inspect command".into());
        };
        assert_eq!(options.session, "cli:direct");
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/runtime"))
        );

        let error = parse_cli_args(["session", "inspect"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("requires --session"));

        let parsed = parse_cli_args([
            "session",
            "delete",
            "--session",
            "cli:direct",
            "--workspace",
            "/tmp/runtime",
            "--yes",
        ])?;
        let CliCommand::Session(SessionCommand::Delete(options)) = parsed else {
            return Err("expected session delete command".into());
        };
        assert_eq!(options.session, "cli:direct");
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/runtime"))
        );
        assert!(options.yes);

        let error = parse_cli_args(["session", "delete"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("requires --session"));

        let parsed = parse_cli_args([
            "session",
            "history",
            "-s",
            "cli:direct",
            "-n",
            "5",
            "--json",
        ])?;
        let CliCommand::Session(SessionCommand::History(options)) = parsed else {
            return Err("expected session history command".into());
        };
        assert_eq!(options.session, "cli:direct");
        assert_eq!(options.max_messages, 5);
        assert!(options.json);

        let parsed = parse_cli_args([
            "session",
            "export",
            "-s",
            "cli:direct",
            "--format",
            "jsonl",
            "--yes",
        ])?;
        let CliCommand::Session(SessionCommand::Export(options)) = parsed else {
            return Err("expected session export command".into());
        };
        assert_eq!(options.format, SessionExportFormat::Jsonl);
        assert!(options.yes);

        assert!(matches!(
            parse_cli_args(["session", "create", "-s", "cli:new"]),
            Ok(CliCommand::Session(SessionCommand::Create(_)))
        ));
        assert!(matches!(
            parse_cli_args(["session", "clear", "-s", "cli:new", "-y"]),
            Ok(CliCommand::Session(SessionCommand::Clear(_)))
        ));
        assert!(matches!(
            parse_cli_args(["session", "diagnostics", "-s", "cli:new"]),
            Ok(CliCommand::Session(SessionCommand::Diagnostics(_)))
        ));
        assert!(matches!(
            parse_cli_args([
                "session",
                "compact",
                "-s",
                "cli:new",
                "--keep-messages",
                "3",
                "--yes"
            ]),
            Ok(CliCommand::Session(SessionCommand::Compact(_)))
        ));
        Ok(())
    }

    #[test]
    fn parser_handles_skills_list_and_show_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "skills",
            "list",
            "--workspace",
            "/tmp/workspace",
            "--all",
        ])?;
        let CliCommand::Skills(SkillsCommand::List(options)) = parsed else {
            return Err("expected skills list command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert!(options.all);

        let parsed = parse_cli_args(["skill", "show", "clawhub"])?;
        let CliCommand::Skills(SkillsCommand::Show(options)) = parsed else {
            return Err("expected skills show command".into());
        };
        assert_eq!(options.name, "clawhub");
        Ok(())
    }

    #[test]
    fn parser_handles_api_serve_alias() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "api",
            "serve",
            "--workspace",
            "/tmp/workspace",
            "--host",
            "127.0.0.1",
            "--port",
            "8902",
        ])?;
        let CliCommand::Serve(options) = parsed else {
            return Err("expected serve command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(options.port, Some(8902));

        let error = parse_cli_args(["api"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("api requires `serve`"));
        Ok(())
    }

    #[test]
    fn parser_handles_run_runtime_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "run",
            "--workspace",
            "/tmp/workspace",
            "--websocket-host",
            "127.0.0.1",
            "--websocket-port",
            "8766",
            "--timeout",
            "30",
            "--allow-side-effects",
            "--verbose",
        ])?;
        let CliCommand::Run(options) = parsed else {
            return Err("expected run command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.websocket_host.as_deref(), Some("127.0.0.1"));
        assert_eq!(options.websocket_port, Some(8766));
        assert_eq!(options.timeout, Some(30.0));
        assert!(options.allow_side_effects);
        assert!(options.verbose);
        Ok(())
    }

    #[test]
    fn channel_runtime_plan_starts_websocket_and_skips_unconfigured_external_workers(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config
            .channels
            .plugins
            .insert("telegram".to_owned(), json!({ "enabled": true }));
        save_config_to_path(&config, &config_path)?;

        let report = channel_runtime_plan(RunOptions {
            config_path: Some(config_path),
            websocket_port: Some(8766),
            ..RunOptions::default()
        })?;

        assert!(report.websocket.enabled);
        assert_eq!(report.websocket_addr, "127.0.0.1:8766".parse()?);
        let websocket = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "websocket")
            .ok_or("websocket worker missing")?;
        assert_eq!(websocket.state, ChannelRuntimeWorkerState::Started);
        let telegram = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "telegram")
            .ok_or("telegram worker missing")?;
        assert_eq!(
            telegram.state,
            ChannelRuntimeWorkerState::SkippedMissingCredentials
        );
        let formatted = format_channel_runtime_plan(report);
        assert!(formatted.contains("Channel runtime plan"));
        assert!(formatted.contains("started"));
        assert!(formatted.contains("skipped-missing-credentials"));
        Ok(())
    }

    #[test]
    fn channel_runtime_plan_starts_credentialed_external_workers() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config
            .channels
            .plugins
            .insert("websocket".to_owned(), json!({ "enabled": false }));
        config.channels.plugins.insert(
            "telegram".to_owned(),
            json!({ "enabled": true, "botToken": "telegram-token" }),
        );
        config.channels.plugins.insert(
            "discord".to_owned(),
            json!({ "enabled": true, "botToken": "discord-token", "channelIds": ["123"] }),
        );
        config.channels.plugins.insert(
            "slack".to_owned(),
            json!({ "enabled": true, "botToken": "slack-token", "channelIds": ["C123"] }),
        );
        config.channels.plugins.insert(
            "email".to_owned(),
            json!({
                "enabled": true,
                "smtp": {
                    "host": "smtp.example.com",
                    "from": "bot@example.com"
                },
                "imap": {
                    "host": "imap.example.com",
                    "username": "bot@example.com",
                    "password": "email-password"
                }
            }),
        );
        config.channels.plugins.insert(
            "whatsapp".to_owned(),
            json!({ "enabled": true, "bridgeUrl": "http://127.0.0.1:9001" }),
        );
        save_config_to_path(&config, &config_path)?;

        let report = channel_runtime_plan(RunOptions {
            config_path: Some(config_path),
            ..RunOptions::default()
        })?;
        let telegram = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "telegram")
            .ok_or("telegram worker missing")?;
        let discord = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "discord")
            .ok_or("discord worker missing")?;
        let slack = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "slack")
            .ok_or("slack worker missing")?;
        let email_smtp = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.kind == LiveChannelWorkerKind::EmailSmtp)
            .ok_or("email smtp worker missing")?;
        let email_imap = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.kind == LiveChannelWorkerKind::EmailImap)
            .ok_or("email imap worker missing")?;
        let whatsapp = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "whatsapp")
            .ok_or("whatsapp worker missing")?;
        assert_eq!(telegram.state, ChannelRuntimeWorkerState::Started);
        assert_eq!(discord.state, ChannelRuntimeWorkerState::Started);
        assert_eq!(slack.state, ChannelRuntimeWorkerState::Started);
        assert_eq!(email_smtp.state, ChannelRuntimeWorkerState::Started);
        assert_eq!(email_imap.state, ChannelRuntimeWorkerState::Started);
        assert_eq!(whatsapp.state, ChannelRuntimeWorkerState::Started);
        Ok(())
    }

    #[test]
    fn external_transport_specs_respect_enabled_and_external_only_runtime(
    ) -> Result<(), Box<dyn Error>> {
        let mut plugins = BTreeMap::new();
        plugins.insert(
            "telegram".to_owned(),
            json!({ "enabled": false, "botToken": "telegram-token" }),
        );
        plugins.insert(
            "slack".to_owned(),
            json!({ "enabled": true, "botToken": "slack-token", "channelIds": ["C123"] }),
        );
        let specs = external_transport_specs(&plugins);
        assert_eq!(specs.len(), 1);
        assert!(matches!(specs[0], ExternalTransportSpec::Slack(_)));

        let root = tempfile::tempdir()?;
        let report = ChannelRuntimeReport {
            config_path: root.path().join("config.json"),
            workspace: root.path().join("workspace"),
            websocket: WebsocketPreset {
                enabled: false,
                host: "127.0.0.1".to_owned(),
                port: 8765,
                path: "/ws".to_owned(),
            },
            websocket_addr: "127.0.0.1:8765".parse()?,
            workers: Vec::new(),
            verbose: false,
        };
        assert!(runtime_needs_process(&report, &specs));
        assert!(!runtime_needs_process(&report, &[]));
        Ok(())
    }

    #[test]
    fn channel_runtime_plan_allows_external_only_with_disabled_non_loopback_websocket(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.channels.plugins.insert(
            "websocket".to_owned(),
            json!({ "enabled": false, "host": "0.0.0.0", "port": 8765 }),
        );
        config.channels.plugins.insert(
            "slack".to_owned(),
            json!({ "enabled": true, "botToken": "slack-token", "channelIds": ["C123"] }),
        );
        save_config_to_path(&config, &config_path)?;

        let report = channel_runtime_plan(RunOptions {
            config_path: Some(config_path),
            ..RunOptions::default()
        })?;

        assert!(!report.websocket.enabled);
        assert_eq!(report.websocket_addr, "0.0.0.0:8765".parse()?);
        let slack = report
            .workers
            .iter()
            .find(|worker| worker.descriptor.channel == "slack")
            .ok_or("slack worker missing")?;
        assert_eq!(slack.state, ChannelRuntimeWorkerState::Started);
        Ok(())
    }

    #[test]
    fn transport_helpers_redact_tokens_and_default_email_imap_safely() {
        let redacted = redact_sensitive_url_text(
            "request to https://api.telegram.org/bot123:secret/getUpdates failed",
        );
        assert!(redacted.contains("bot<redacted>/getUpdates"));
        assert!(!redacted.contains("123:secret"));

        let mut plugins = BTreeMap::new();
        plugins.insert(
            "email".to_owned(),
            json!({
                "enabled": true,
                "imap": {
                    "host": "imap.example.com",
                    "username": "bot@example.com",
                    "password": "email-password"
                }
            }),
        );
        let config = email_runtime_config(&plugins).and_then(|config| config.imap);
        assert!(config
            .as_ref()
            .map(|config| config.mark_seen)
            .unwrap_or(false));
        assert_eq!(config.map(|config| config.timeout_seconds), Some(30));
    }

    #[test]
    fn parser_handles_ask_options() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "ask",
            "hello",
            "there",
            "--workspace",
            "/tmp/workspace",
            "--session",
            "work",
            "--temperature",
            "0.4",
            "--max-tokens",
            "200",
            "--allow-side-effects",
            "--no-markdown",
        ])?;
        let CliCommand::Ask(options) = parsed else {
            return Err("expected ask command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.message, "hello there");
        assert_eq!(options.session.as_deref(), Some("work"));
        assert_eq!(options.temperature, Some(0.4));
        assert_eq!(options.max_tokens, Some(200));
        assert!(options.allow_side_effects);
        assert!(!options.markdown);

        let parsed = parse_cli_args(["ask", "--", "-starts-with-dash"])?;
        let CliCommand::Ask(options) = parsed else {
            return Err("expected ask command".into());
        };
        assert_eq!(options.message, "-starts-with-dash");
        Ok(())
    }

    #[test]
    fn parser_handles_agent_message_alias() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args(["agent", "--message", "hello", "-s", "cli:direct"])?;
        let CliCommand::Ask(options) = parsed else {
            return Err("expected ask command".into());
        };
        assert_eq!(options.message, "hello");
        assert_eq!(options.session.as_deref(), Some("cli:direct"));
        Ok(())
    }

    #[test]
    fn cli_model_helpers_preserve_deferred_empty_database_contract() {
        assert!(get_all_models().is_empty());
        assert_eq!(find_model_info("gpt-test"), None);
        assert_eq!(get_model_context_limit("gpt-test", "auto"), None);
        assert!(get_model_suggestions("gpt", "auto", 20).is_empty());
        assert_eq!(format_token_count(200_000), "200,000");
    }

    #[test]
    fn cli_render_helpers_keep_non_tty_output_plain() {
        assert_eq!(cli_render_mode(false), CliRenderMode::PlainText);
        assert_eq!(cli_render_mode(true), CliRenderMode::InteractiveTerminal);
        assert_eq!(
            render_agent_response("hello\nworld", true, Some(&json!({"render_as": "text"}))),
            "hello\nworld"
        );
    }

    #[test]
    fn parser_rejects_agent_positional_message_until_interactive_mode_exists() {
        let error = parse_cli_args(["agent", "hello"]).unwrap_err().to_string();
        assert!(error.contains("require -m/--message"));
    }

    #[test]
    fn parser_handles_provider_codex_import_and_login_aliases() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/config.json",
            "provider",
            "codex",
            "import-token",
            "--token-env",
            "CODEX_TOKEN",
            "--account-id",
            "acct_123",
            "--no-select",
        ])?;
        let CliCommand::Provider(ProviderCommand::CodexImportToken(options)) = parsed else {
            return Err("expected codex import-token command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/config.json")));
        assert_eq!(
            options.token_source,
            TokenSource::Env("CODEX_TOKEN".to_owned())
        );
        assert_eq!(options.account_id.as_deref(), Some("acct_123"));
        assert!(!options.select);

        let parsed = parse_cli_args(["provider", "login", "openai-codex", "--headless"])?;
        let CliCommand::Provider(ProviderCommand::CodexLogin(options)) = parsed else {
            return Err("expected codex login command".into());
        };
        assert!(options.headless);
        Ok(())
    }

    #[test]
    fn ask_requires_prompt_but_agent_without_message_remains_deferred() -> Result<(), Box<dyn Error>>
    {
        let error = parse_cli_args(["ask"]).unwrap_err().to_string();
        assert!(error.contains("requires a message"));

        let parsed = parse_cli_args(["agent"])?;
        let CliCommand::Unsupported(command) = parsed else {
            return Err("expected unsupported interactive agent marker".into());
        };
        assert_eq!(command.name, "agent");
        assert!(command.reason.contains("interactive"));
        Ok(())
    }

    #[test]
    fn serve_rejects_non_loopback_without_explicit_remote_opt_in() -> Result<(), Box<dyn Error>> {
        let api = ApiConfig {
            host: "0.0.0.0".to_owned(),
            port: 8900,
            timeout: 120.0,
        };
        let options = ServeOptions::default();
        let addr = resolve_serve_addr(&options, &api)?;
        let error = validate_serve_security(&options, addr)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--allow-remote"));

        validate_serve_security(
            &ServeOptions {
                allow_remote: true,
                ..ServeOptions::default()
            },
            addr,
        )?;
        Ok(())
    }

    #[test]
    fn serve_resolves_bind_address_from_options_or_config() -> Result<(), Box<dyn Error>> {
        let api = ApiConfig {
            host: "127.0.0.1".to_owned(),
            port: 8900,
            timeout: 120.0,
        };

        assert_eq!(
            resolve_serve_addr(&ServeOptions::default(), &api)?,
            "127.0.0.1:8900".parse()?
        );
        assert_eq!(
            resolve_serve_addr(
                &ServeOptions {
                    host: Some("0.0.0.0".to_owned()),
                    port: Some(9000),
                    ..ServeOptions::default()
                },
                &api,
            )?,
            "0.0.0.0:9000".parse()?
        );
        let error = resolve_serve_addr(
            &ServeOptions {
                bind: Some("127.0.0.1:9000".parse()?),
                port: Some(9001),
                ..ServeOptions::default()
            },
            &api,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("--bind"));
        Ok(())
    }

    #[test]
    fn gateway_resolves_address_from_config_and_port_override() -> Result<(), Box<dyn Error>> {
        let gateway = shacs_config::GatewayConfig {
            host: "127.0.0.1".to_owned(),
            port: 8900,
            heartbeat: Default::default(),
        };

        assert_eq!(
            resolve_gateway_addr(&GatewayOptions::default(), &gateway)?,
            "127.0.0.1:8900".parse()?
        );
        assert_eq!(
            resolve_gateway_addr(
                &GatewayOptions {
                    port: Some(8902),
                    ..GatewayOptions::default()
                },
                &gateway,
            )?,
            "127.0.0.1:8902".parse()?
        );
        Ok(())
    }

    #[test]
    fn mcp_config_converts_to_runtime_server_specs() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let mut config = shacs_config::Config::default();
        config.tools.mcp_servers.insert(
            "docs".to_owned(),
            shacs_config::McpServerConfig {
                r#type: Some("stdio".to_owned()),
                command: "server".to_owned(),
                args: vec!["--stdio".to_owned()],
                env: BTreeMap::from([("MCP_TOKEN".to_owned(), "secret".to_owned())]),
                url: String::new(),
                headers: BTreeMap::new(),
                tool_timeout: 12,
                enabled_tools: vec!["*".to_owned()],
            },
        );
        let bundle = ConfigBundle {
            config,
            context: shacs_config::ConfigContext {
                config_path: root.path().join("config.json"),
                data_dir: root.path().join("data"),
                workspace,
            },
            migrations: Vec::new(),
        };

        let specs = mcp_server_specs(&bundle);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "docs");
        assert_eq!(specs[0].command.as_deref(), Some("server"));
        assert_eq!(specs[0].args, vec!["--stdio".to_owned()]);
        assert_eq!(
            specs[0].env,
            vec![("MCP_TOKEN".to_owned(), "secret".to_owned())]
        );
        assert_eq!(specs[0].timeout_seconds, 12);
        assert_eq!(
            runtime_capabilities(&bundle)[0].status,
            RuntimeCapabilityStatus::Available
        );
        Ok(())
    }

    #[test]
    fn web_preset_reports_assets_and_preserves_websocket_plugin() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.gateway.host = "127.0.0.1".to_owned();
        config.gateway.port = 8899;
        config.channels.plugins.insert(
            "websocket".to_owned(),
            json!({
                "enabled": false,
                "host": "0.0.0.0",
                "port": 8767,
                "path": "ws"
            }),
        );
        save_config_to_path(&config, &config_path)?;

        let report = web_preset(WebOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            gateway_port: Some(8901),
            websocket_host: None,
            websocket_port: None,
            verbose: true,
        })?;

        assert_eq!(report.config_path, config_path);
        assert_eq!(report.workspace, workspace);
        assert_eq!(report.gateway_addr, "127.0.0.1:8901".parse()?);
        assert!(!report.websocket.enabled);
        assert_eq!(report.websocket.host, "0.0.0.0");
        assert_eq!(report.websocket.port, 8767);
        assert_eq!(report.websocket.path, "/ws");
        assert!(report.assets_dir.ends_with(shacs_web::WEB_DIST_DIR_NAME));
        assert!(report.verbose);
        let formatted = format_web_preset_report(report);
        assert!(formatted.contains("shacs-bot run"));
        assert!(formatted.contains("ws://0.0.0.0:8767/ws"));
        Ok(())
    }

    #[test]
    fn channels_reports_registry_config_and_deferred_worker_boundaries(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.channels.send_progress = false;
        config.channels.send_tool_hints = true;
        config.channels.send_max_retries = 5;
        config.channels.plugins.insert(
            "websocket".to_owned(),
            json!({ "enabled": false, "host": "127.0.0.1" }),
        );
        config
            .channels
            .plugins
            .insert("custom-local".to_owned(), json!({ "enabled": true }));
        save_config_to_path(&config, &config_path)?;

        let report = channels_status(ChannelsStatusOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;

        assert_eq!(report.config_path, config_path);
        assert_eq!(report.workspace, workspace);
        assert!(!report.send_progress);
        assert!(report.send_tool_hints);
        assert_eq!(report.send_max_retries, 5);
        assert_eq!(report.unknown_plugins, vec!["custom-local".to_owned()]);
        let websocket = report
            .channels
            .iter()
            .find(|channel| channel.descriptor.name == "websocket")
            .ok_or("websocket channel missing")?;
        assert!(websocket.configured);
        assert!(!websocket.enabled);
        assert_eq!(websocket.workers.len(), 1);
        let telegram = report
            .channels
            .iter()
            .find(|channel| channel.descriptor.name == "telegram")
            .ok_or("telegram channel missing")?;
        assert!(!telegram.configured);
        assert!(!telegram.enabled);
        assert_eq!(telegram.workers[0].label, "Telegram long-polling worker");

        let status = format_channels_status(report.clone());
        assert!(status.contains("Channel runtime status"));
        assert!(status.contains("WebSocket server"));
        assert!(status.contains("runtime=runnable"));
        assert!(status.contains("custom-local"));
        let list = format_channels_list(report);
        assert!(list.contains("websocket"));
        assert!(list.contains("configured=true"));
        Ok(())
    }

    #[test]
    fn onboard_creates_config_runtime_dirs_and_templates() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("data").join("config.json");
        let workspace = root.path().join("workspace");

        let outcome = onboard(OnboardOptions {
            config_path: Some(config_path.clone()),
            workspace: Some(workspace.clone()),
            wizard: false,
        })?;

        assert_eq!(outcome.config_path, config_path);
        assert_eq!(outcome.workspace, workspace);
        assert!(outcome.config_path.exists());
        assert!(outcome.workspace.join("AGENTS.md").exists());
        assert!(outcome.workspace.join("memory").join("MEMORY.md").exists());
        assert!(outcome.workspace.join("skills").exists());
        assert!(outcome
            .workspace
            .join("builtin_skills")
            .join("skill-creator")
            .join("SKILL.md")
            .exists());
        assert!(outcome
            .template_files
            .iter()
            .any(|path| path == "AGENTS.md"));
        assert!(outcome
            .template_files
            .iter()
            .any(|path| path == "builtin_skills/skill-creator/SKILL.md"));

        let saved = fs::read_to_string(outcome.config_path)?;
        assert!(saved.contains(&outcome.workspace.to_string_lossy().to_string()));
        Ok(())
    }

    #[test]
    fn onboard_preserves_existing_workspace_templates() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        fs::write(workspace.join("AGENTS.md"), "custom agents")?;

        onboard(OnboardOptions {
            config_path: Some(config_path),
            workspace: Some(workspace.clone()),
            wizard: false,
        })?;

        assert_eq!(
            fs::read_to_string(workspace.join("AGENTS.md"))?,
            "custom agents"
        );
        Ok(())
    }

    #[test]
    fn runtime_workspace_override_does_not_persist_to_config() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config.providers.insert(
            "openrouter".to_owned(),
            shacs_config::ProviderConfig {
                api_key: Some("${OPENROUTER_API_KEY}".to_owned()),
                api_base: None,
                extra_headers: None,
                extra_body: None,
            },
        );
        config.agents.defaults.workspace = root
            .path()
            .join("saved-workspace")
            .to_string_lossy()
            .to_string();
        save_config_to_path(&config, &config_path)?;

        let mut env = BTreeMap::new();
        env.insert("OPENROUTER_API_KEY".to_owned(), "secret".to_owned());
        let runtime_workspace = root.path().join("runtime-workspace");
        let bundle = load_runtime_config_with_env(
            RuntimeConfigOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(runtime_workspace.clone()),
                resolve_env: true,
            },
            &env,
        )?;

        assert_eq!(bundle.context.workspace, runtime_workspace);
        assert_eq!(
            bundle.config.providers["openrouter"].api_key.as_deref(),
            Some("secret")
        );
        let saved = fs::read_to_string(config_path)?;
        assert!(saved.contains("${OPENROUTER_API_KEY}"));
        assert!(!saved.contains("secret"));
        assert!(!saved.contains("runtime-workspace"));
        Ok(())
    }

    #[test]
    fn skills_list_and_show_use_virtual_bundled_registry() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let list = skills_list(SkillsListOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            all: false,
        })?;
        assert_eq!(list.workspace, workspace);
        assert!(list.entries.iter().any(|entry| {
            entry.descriptor.name == "clawhub" && entry.status == SkillRegistryStatus::Active
        }));
        let output = format_skills_list(list);
        assert!(output.contains("clawhub"));

        let show = skills_show(SkillsShowOptions {
            config_path: Some(config_path),
            workspace_override: None,
            name: "skill-creator".to_owned(),
        })?;
        assert_eq!(show.entry.descriptor.name, "skill-creator");
        assert!(!show.entry.descriptor.body_hash.is_empty());
        let output = format_skills_show(show);
        assert!(output.contains("Skill: skill-creator"));
        assert!(output.contains("virtual-builtin"));
        Ok(())
    }

    #[test]
    fn status_reports_config_workspace_and_provider_fields() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.providers.insert(
            "openrouter".to_owned(),
            shacs_config::ProviderConfig {
                api_key: Some("sk-test".to_owned()),
                api_base: Some("https://example.invalid/v1".to_owned()),
                extra_headers: None,
                extra_body: None,
            },
        );
        save_config_to_path(&config, &config_path)?;

        let report = status(StatusOptions {
            config_path: Some(config_path.clone()),
        })?;

        assert_eq!(report.config_path, config_path);
        assert!(report.config_exists);
        assert_eq!(report.workspace, workspace);
        assert!(!report.workspace_exists);
        assert_eq!(report.providers.len(), 1);
        assert_eq!(report.providers[0].name, "openrouter");
        assert!(report.providers[0].has_api_key);
        assert!(report.providers[0].has_api_base);
        Ok(())
    }

    #[test]
    fn status_does_not_write_back_legacy_config_migrations() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let legacy = r#"{
  "agents": {
    "defaults": {
      "workspace": "~/legacy-workspace",
      "sessionTtlMinutes": 42
    }
  }
}
"#;
        fs::write(&config_path, legacy)?;

        let report = status(StatusOptions {
            config_path: Some(config_path.clone()),
        })?;

        assert!(report.config_exists);
        let saved = fs::read_to_string(config_path)?;
        assert_eq!(saved, legacy);
        assert!(saved.contains("sessionTtlMinutes"));
        assert!(!saved.contains("idleCompactAfterMinutes"));
        Ok(())
    }

    #[test]
    fn runtime_inspect_reports_capabilities_and_session_summary() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.agents.defaults.provider = "openai_codex".to_owned();
        config.agents.defaults.model = "gpt-5.4".to_owned();
        config
            .providers
            .insert("openai_codex".to_owned(), codex_provider_config());
        save_config_to_path(&config, &config_path)?;
        let mut sessions = SessionManager::new(&workspace)?;
        let mut session = Session::new("cli:direct");
        session.add_message("user", "hello", Default::default());
        sessions.save_with_fsync(&session)?;

        let report = runtime_inspect(RuntimeInspectOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;

        assert_eq!(report.config_path, config_path);
        assert_eq!(report.workspace, workspace);
        assert_eq!(report.provider, "openai_codex");
        assert_eq!(report.model, "gpt-5.4");
        assert_eq!(report.sessions.count, 1);
        assert_eq!(report.sessions.latest_key.as_deref(), Some("cli:direct"));
        assert!(report
            .capabilities
            .iter()
            .any(|capability| capability.component == "mcp_lifecycle"));
        assert!(report.capabilities.iter().any(|capability| {
            capability.component == "subagent_runtime"
                && capability.status == RuntimeCapabilityStatus::Available
        }));
        let output = format_runtime_inspect(report);
        assert!(output.contains("shacs-bot runtime inspect"));
        assert!(output.contains("Sessions: 1"));
        assert!(!output.contains("hello"));
        Ok(())
    }

    #[test]
    fn session_commands_list_and_inspect_without_raw_messages() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let mut sessions = SessionManager::new(&workspace)?;
        let mut session = Session::new("cli:direct");
        session.metadata.insert(
            "agent_configuration".to_owned(),
            json!({"model": "gpt-5.4"}),
        );
        session.add_message("user", "secret prompt body", Default::default());
        session.add_message("assistant", "secret answer body", Default::default());
        session.last_consolidated = 1;
        sessions.save_with_fsync(&session)?;

        let list = session_list(SessionListOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;
        assert_eq!(list.workspace, workspace);
        assert_eq!(list.sessions.len(), 1);
        assert_eq!(list.sessions[0].key, "cli:direct");
        let list_output = format_session_list(list);
        assert!(list_output.contains("shacs-bot session list"));
        assert!(list_output.contains("cli:direct"));
        assert!(!list_output.contains("secret prompt body"));

        let inspect = session_inspect(SessionInspectOptions {
            config_path: Some(config_path),
            workspace_override: None,
            session: "cli:direct".to_owned(),
        })?;
        assert_eq!(inspect.key, "cli:direct");
        assert_eq!(inspect.message_count, 2);
        assert_eq!(inspect.last_consolidated, 1);
        assert_eq!(
            inspect.metadata_keys,
            vec!["agent_configuration".to_owned()]
        );
        let inspect_output = format_session_inspect(inspect);
        assert!(inspect_output.contains("Messages: 2"));
        assert!(inspect_output.contains("Metadata keys: agent_configuration"));
        assert!(!inspect_output.contains("gpt-5.4"));
        assert!(!inspect_output.contains("secret answer body"));
        Ok(())
    }

    #[test]
    fn session_list_does_not_create_missing_sessions_dir() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let report = session_list(SessionListOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })?;

        assert!(report.sessions.is_empty());
        assert!(!workspace.join("sessions").exists());
        Ok(())
    }

    #[test]
    fn session_delete_requires_confirmation_and_removes_only_summary() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let mut sessions = SessionManager::new(&workspace)?;
        let mut session = Session::new("cli:direct");
        session
            .metadata
            .insert("secret_model".to_owned(), json!("gpt-5.4"));
        session.add_message("user", "secret prompt body", Default::default());
        sessions.save_with_fsync(&session)?;
        let path = sessions.session_path("cli:direct");
        assert!(path.exists());

        let error = session_delete(SessionDeleteOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:direct".to_owned(),
            yes: false,
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("--yes"));
        assert!(path.exists());

        let report = session_delete(SessionDeleteOptions {
            config_path: Some(config_path),
            workspace_override: None,
            session: "cli:direct".to_owned(),
            yes: true,
        })?;
        assert!(report.deleted);
        assert!(!path.exists());
        let output = format_session_delete(report);
        assert!(output.contains("Deleted: yes"));
        assert!(!output.contains("secret prompt body"));
        assert!(!output.contains("gpt-5.4"));
        Ok(())
    }

    #[test]
    fn session_delete_missing_dir_reports_false_without_creating_dir() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let report = session_delete(SessionDeleteOptions {
            config_path: Some(config_path),
            workspace_override: None,
            session: "cli:missing".to_owned(),
            yes: true,
        })?;

        assert!(!report.deleted);
        assert!(!workspace.join("sessions").exists());
        Ok(())
    }

    #[test]
    fn session_delete_missing_file_reports_false() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        SessionManager::new(&workspace)?;

        let report = session_delete(SessionDeleteOptions {
            config_path: Some(config_path),
            workspace_override: None,
            session: "cli:missing".to_owned(),
            yes: true,
        })?;

        assert!(!report.deleted);
        assert!(workspace.join("sessions").exists());
        let output = format_session_delete(report);
        assert!(output.contains("Deleted: no"));
        Ok(())
    }

    #[test]
    fn session_management_commands_cover_history_export_clear_diagnostics_and_compact(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let created = session_create(SessionCreateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
        })?;
        assert!(created.created);
        let created_again = session_create(SessionCreateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
        })?;
        assert!(!created_again.created);

        let mut manager = SessionManager::new(&workspace)?;
        let mut session = manager
            .load_existing("cli:managed")
            .ok_or("created session missing")?;
        session
            .metadata
            .insert("pending_user_turn".to_owned(), json!(true));
        session.metadata.insert(
            "runtime_checkpoint".to_owned(),
            json!({"phase": "awaiting_tools", "raw_secret": "do-not-print"}),
        );
        session.add_message("user", "alpha", Default::default());
        session.add_message("assistant", "beta", Default::default());
        session.add_message("user", "gamma", Default::default());
        session.add_message("assistant", "delta", Default::default());
        manager.save_with_fsync(&session)?;
        let legacy_path = manager.legacy_nanobot_session_path("cli:managed");
        std::fs::write(
            &legacy_path,
            concat!(
                "{\"_type\":\"metadata\",\"key\":\"cli:managed\"}\n",
                "{\"role\":\"user\",\"content\":\"stale raw legacy\"}\n"
            ),
        )?;

        let history = session_history(SessionHistoryCliOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            max_messages: 2,
            max_tokens: 0,
            timestamps: false,
            json: false,
        })?;
        let history_output = format_session_history(history);
        assert!(history_output.contains("user: gamma"));
        assert!(history_output.contains("assistant: delta"));
        assert!(!history_output.contains("alpha"));
        assert!(!history_output.contains("beta"));

        let diagnostics = session_diagnostics(SessionDiagnosticsOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
        })?;
        let diagnostics_output = format_session_diagnostics(diagnostics);
        assert!(
            diagnostics_output.contains("Recovery markers: pending_user_turn, runtime_checkpoint")
        );
        assert!(diagnostics_output.contains("Checkpoint phase: awaiting_tools"));
        assert!(!diagnostics_output.contains("do-not-print"));

        let export_error = session_export(SessionExportOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            format: SessionExportFormat::Json,
            yes: false,
        })
        .unwrap_err()
        .to_string();
        assert!(export_error.contains("--yes"));
        let exported = session_export(SessionExportOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            format: SessionExportFormat::Jsonl,
            yes: true,
        })?;
        assert!(exported.content.contains("alpha"));
        assert!(exported.content.lines().count() >= 4);

        let compact_error = session_compact(SessionCompactOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            keep_messages: 1,
            yes: false,
        })
        .unwrap_err()
        .to_string();
        assert!(compact_error.contains("--yes"));
        let compact = session_compact(SessionCompactOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            keep_messages: 1,
            yes: true,
        })?;
        assert!(compact.archived_messages > 0);
        assert!(!legacy_path.exists());

        let clear_error = session_clear(SessionClearOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            yes: false,
        })
        .unwrap_err()
        .to_string();
        assert!(clear_error.contains("--yes"));
        let cleared = session_clear(SessionClearOptions {
            config_path: Some(config_path),
            workspace_override: None,
            session: "cli:managed".to_owned(),
            yes: true,
        })?;
        assert!(cleared.message_count_before > 0);
        let manager = SessionManager::new(&workspace)?;
        let cleared_session = manager
            .load_existing("cli:managed")
            .ok_or("cleared session missing")?;
        assert!(cleared_session.messages.is_empty());
        assert_eq!(cleared_session.metadata["pending_user_turn"], true);
        Ok(())
    }

    #[test]
    fn unsupported_runtime_commands_are_reserved_not_silently_accepted(
    ) -> Result<(), Box<dyn Error>> {
        assert!(matches!(
            parse_cli_args(["channels", "status"]),
            Ok(CliCommand::Channels(ChannelsCommand::Status(_)))
        ));
        let parsed = parse_cli_args(["plugins"])?;
        let CliCommand::Unsupported(command) = parsed else {
            return Err("expected unsupported plugins marker".into());
        };
        assert_eq!(command.name, "plugins");
        assert!(command.reason.contains("later"));
        assert!(matches!(
            parse_cli_args(["gateway"]),
            Ok(CliCommand::Gateway(_))
        ));
        assert!(matches!(parse_cli_args(["web"]), Ok(CliCommand::Web(_))));
        assert!(matches!(
            parse_cli_args(["provider", "codex", "login"]),
            Ok(CliCommand::Provider(ProviderCommand::CodexLogin(_)))
        ));
        Ok(())
    }

    #[test]
    fn help_marks_serve_as_available() {
        let help = help_text();
        assert!(help.contains("serve     Start the local"));
        assert!(!help.contains("serve     Reserved"));
        assert!(help.contains("gateway   Report gateway"));
        assert!(!help.contains("gateway   Reserved"));
        assert!(help.contains("web       Report WebUI"));
        assert!(help.contains("runtime   Inspect local runtime"));
        assert!(help.contains("ask       Send one message"));
        assert!(help.contains("agent     Alias"));
        assert!(help.contains("provider  Manage provider auth"));
        assert!(help.contains("channels  List channel"));
        assert!(!help.contains("channels  Reserved"));
        assert!(help.contains("-m, --message"));
        assert!(help.contains("--temperature"));
        assert!(help.contains("--max-tokens"));
        assert!(help.contains("--gateway-port"));
        assert!(help.contains("--websocket-host"));
        assert!(help.contains("--token-stdin"));
    }

    #[test]
    fn provider_adapter_uses_resolved_model_and_config_defaults() -> Result<(), Box<dyn Error>> {
        let captured = std::sync::Arc::new(Mutex::new(Vec::new()));
        let adapter = ProviderChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                temperature: 0.2,
                max_tokens: 123,
                reasoning_effort: Some("medium".to_owned()),
                ..AgentDefaults::default()
            },
            resolved: ResolvedProviderClient {
                provider_id: "openai".to_owned(),
                model: "gpt-5".to_owned(),
                client: Box::new(FakeProviderClient {
                    captured: captured.clone(),
                    response: LlmResponse {
                        content: Some("ok".to_owned()),
                        ..LlmResponse::default()
                    },
                }),
            },
            retry_mode: ProviderRetryMode::Standard,
        };

        let response = adapter.complete_chat(ChatCompletionInvocation {
            provider_request: ProviderRequest {
                messages: vec![json!({"role": "user", "content": "hi"})],
                tools: Vec::new(),
                model: "openai/gpt-5".to_owned(),
                settings: GenerationSettings::default(),
                tool_choice: None,
            },
            requested_model: Some("openai/gpt-5".to_owned()),
            session_key: "api:default".to_owned(),
            media_data_urls: Vec::new(),
            media_paths: Vec::new(),
            temperature: None,
            max_tokens: None,
        })?;

        assert_eq!(response.content.as_deref(), Some("ok"));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "gpt-5");
        assert_eq!(requests[0].settings.temperature, 0.2);
        assert_eq!(requests[0].settings.max_tokens, 123);
        assert_eq!(
            requests[0].settings.reasoning_effort.as_deref(),
            Some("medium")
        );
        Ok(())
    }

    #[test]
    fn agent_loop_adapter_persists_data_urls_to_api_media_dir() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir: media_dir.clone(),
            tools: ToolRegistry::new(),
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
        };

        let paths = adapter.persist_media_data_urls(&[
            "data:text/plain;base64,aGk=".to_owned(),
            "not-a-data-url".to_owned(),
        ])?;

        assert_eq!(paths.len(), 1);
        assert!(PathBuf::from(&paths[0]).starts_with(&media_dir));
        assert_eq!(fs::read_to_string(&paths[0])?, "hi");
        Ok(())
    }

    #[test]
    fn websocket_frame_bridge_processes_message_through_agent_loop_without_socket(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            client: Arc::new(FakeProviderClient {
                captured: captured.clone(),
                response: LlmResponse {
                    content: Some("agent ok".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace: workspace.clone(),
            media_dir: media_dir.clone(),
            tools: ToolRegistry::new(),
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
        };

        let events = adapter.process_websocket_frame(
            json!({
                "type": "message",
                "chat_id": "chat-b",
                "text": "hello from websocket",
                "media": [{ "data_url": "data:text/plain;base64,aGk=", "name": "a.txt" }]
            }),
            "client-1",
            "chat-a",
        )?;

        assert_eq!(
            events,
            vec![WebSocketServerEvent::Message {
                chat_id: "chat-b".to_owned(),
                text: "agent ok".to_owned(),
                buttons: Vec::new(),
                button_prompt: None,
                media: Vec::new(),
                reply_to: None,
                kind: None,
            }]
        );
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        assert!(requests[0].messages.iter().any(|message| {
            message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| {
                    content.contains("Channel: websocket")
                        && content.contains("Chat ID: chat-b")
                        && content.contains("hello from websocket")
                })
        }));
        assert_eq!(fs::read_dir(&media_dir)?.count(), 1);
        let session = SessionManager::new(&workspace)?
            .load_existing("websocket:chat-b")
            .ok_or("websocket session missing")?;
        assert!(session
            .messages
            .iter()
            .any(|message| message.get("role").and_then(Value::as_str) == Some("assistant")));
        Ok(())
    }

    #[test]
    fn websocket_frame_bridge_handles_control_and_protocol_frames_without_agent_loop(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            client: Arc::new(FakeProviderClient {
                captured: captured.clone(),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
        };

        assert_eq!(
            adapter.process_websocket_frame(
                json!({ "type": "attach", "chat_id": "chat-b" }),
                "client-1",
                "chat-a",
            )?,
            vec![WebSocketServerEvent::Attached {
                chat_id: "chat-b".to_owned(),
            }]
        );
        assert_eq!(
            adapter.process_websocket_frame(json!({ "type": "new_chat" }), "client-1", "chat-a")?,
            vec![WebSocketServerEvent::Ready {
                chat_id: "chat-a".to_owned(),
                client_id: "client-1".to_owned(),
            }]
        );
        let error =
            adapter.process_websocket_frame(json!({ "type": "message" }), "client-1", "chat-a")?;
        assert!(matches!(
            error.as_slice(),
            [WebSocketServerEvent::Error { chat_id: Some(chat_id), detail: Some(detail) }]
                if chat_id == "chat-a" && detail.contains("message frame needs content")
        ));
        assert!(captured
            .lock()
            .map_err(|_| "captured lock poisoned")?
            .is_empty());
        Ok(())
    }

    #[test]
    fn agent_loop_adapter_constructs_and_runs_with_fake_provider() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            client: Arc::new(FakeProviderClient {
                captured: captured.clone(),
                response: LlmResponse {
                    content: Some("agent ok".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
        };

        let response = adapter.complete_chat(ChatCompletionInvocation {
            provider_request: ProviderRequest {
                messages: vec![json!({"role": "user", "content": "hi"})],
                tools: Vec::new(),
                model: "openai/gpt-5".to_owned(),
                settings: GenerationSettings::default(),
                tool_choice: None,
            },
            requested_model: Some("openai/gpt-5".to_owned()),
            session_key: "api:default".to_owned(),
            media_data_urls: Vec::new(),
            media_paths: Vec::new(),
            temperature: None,
            max_tokens: None,
        })?;

        assert_eq!(response.content.as_deref(), Some("agent ok"));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "gpt-5");
        assert!(
            requests[0].messages.iter().any(|message| {
                message
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| {
                        content.contains("Channel: api")
                            && content.contains("Chat ID: default")
                            && content.contains("hi")
                    })
            }),
            "captured messages: {:?}",
            requests[0].messages
        );
        Ok(())
    }

    #[test]
    fn direct_message_uses_cli_session_and_agent_loop() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            client: Arc::new(FakeProviderClient {
                captured: captured.clone(),
                response: LlmResponse {
                    content: Some("direct ok".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace: workspace.clone(),
            media_dir,
            tools: ToolRegistry::new(),
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
        };

        let output = complete_direct_message(
            &adapter,
            &AskOptions {
                message: "hello".to_owned(),
                session: Some("work".to_owned()),
                ..AskOptions::default()
            },
        )?;

        assert_eq!(output, "direct ok");
        let session_files = fs::read_dir(workspace.join("sessions"))?.count();
        assert_eq!(session_files, 1);
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "gpt-5");
        Ok(())
    }

    #[test]
    fn programmatic_facade_runs_with_sdk_session_and_returns_messages() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let bot = ShacsBot {
            adapter: AgentLoopChatCompletionAdapter {
                configured_model: "openai/gpt-5".to_owned(),
                provider_id: "openai".to_owned(),
                defaults: AgentDefaults {
                    model: "openai/gpt-5".to_owned(),
                    max_tool_iterations: 1,
                    ..AgentDefaults::default()
                },
                resolved_model: "gpt-5".to_owned(),
                client: Arc::new(FakeProviderClient {
                    captured: captured.clone(),
                    response: LlmResponse {
                        content: Some("sdk ok".to_owned()),
                        finish_reason: "stop".to_owned(),
                        ..LlmResponse::default()
                    },
                }),
                retry_mode: ProviderRetryMode::Standard,
                workspace: workspace.clone(),
                media_dir,
                tools: ToolRegistry::new(),
                _mcp_runtime: None,
                _mcp_reports: Vec::new(),
                allow_side_effect_tools: false,
            },
        };

        let result = bot.run_with_options(ShacsBotRunOptions {
            message: "hello from sdk".to_owned(),
            session_key: "sdk:work".to_owned(),
            ..ShacsBotRunOptions::default()
        })?;

        assert_eq!(result.content, "sdk ok");
        assert!(result.tools_used.is_empty());
        assert!(result
            .messages
            .iter()
            .any(|message| message.get("role").and_then(Value::as_str) == Some("user")));
        assert!(result
            .messages
            .iter()
            .any(|message| message.get("role").and_then(Value::as_str) == Some("assistant")));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "gpt-5");
        Ok(())
    }

    #[test]
    fn top_level_version_api_matches_cli_version() -> Result<(), Box<dyn Error>> {
        assert_eq!(version(), VERSION);
        assert_eq!(
            run_command(CliCommand::Version)?,
            format!("shacs-bot {VERSION}")
        );
        Ok(())
    }

    #[test]
    fn codex_import_token_writes_auth_without_leaking_token_to_config_or_output(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let outcome = import_codex_token(CodexImportTokenOptions {
            config_path: Some(config_path.clone()),
            token_source: TokenSource::Literal("secret-codex-token".to_owned()),
            account_id: Some("acct_123".to_owned()),
            select: true,
        })?;

        assert_eq!(outcome.config_path, config_path);
        assert_eq!(outcome.selected_model.as_deref(), Some(CODEX_DEFAULT_MODEL));
        assert!(outcome.has_account_id);
        let config_text = fs::read_to_string(&outcome.config_path)?;
        assert!(config_text.contains("openai_codex"));
        assert!(config_text.contains(CODEX_DEFAULT_MODEL));
        assert!(!config_text.contains("secret-codex-token"));
        let output = format_codex_import_outcome(outcome.clone());
        assert!(!output.contains("secret-codex-token"));

        let auth = load_auth_store(&outcome.auth_path)?;
        let codex = auth
            .providers
            .get("openai_codex")
            .ok_or("missing codex auth")?;
        assert_eq!(codex.access, "secret-codex-token");
        assert_eq!(codex.account_id.as_deref(), Some("acct_123"));
        Ok(())
    }

    #[test]
    fn codex_import_no_select_reports_unchanged_model() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let outcome = import_codex_token(CodexImportTokenOptions {
            config_path: Some(root.path().join("config.json")),
            token_source: TokenSource::Literal("token".to_owned()),
            account_id: None,
            select: false,
        })?;

        assert!(outcome.selected_model.is_none());
        let output = format_codex_import_outcome(outcome);
        assert!(output.contains("Selected model: unchanged"));
        Ok(())
    }

    #[test]
    fn codex_authorize_url_uses_open_code_pkce_parameters() {
        let pkce = CodexPkce {
            verifier: "verifier".to_owned(),
            challenge: "challenge".to_owned(),
            state: "state".to_owned(),
        };
        let url = codex_authorize_url(&pkce);
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(url.contains("code_challenge=challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("originator=shacs-bot"));
    }

    #[test]
    fn codex_callback_accepts_only_expected_path_state_and_code() -> Result<(), Box<dyn Error>> {
        let request =
            "GET /auth/callback?code=abc%20123&state=state HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(extract_codex_callback_code(request, "state")?, "abc 123");
        assert!(extract_codex_callback_code("GET /favicon.ico HTTP/1.1\r\n\r\n", "state").is_err());
        assert!(extract_codex_callback_code(
            "GET /auth/callback?code=abc&state=wrong HTTP/1.1\r\n\r\n",
            "state"
        )
        .is_err());
        assert!(extract_codex_callback_code(
            "GET /auth/callback?state=state HTTP/1.1\r\n\r\n",
            "state"
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn codex_token_response_extracts_account_id_and_default_expiry() -> Result<(), Box<dyn Error>> {
        let id_token = fake_jwt(json!({"chatgpt_account_id": "acct_claim"}));
        let response = token_response_from_json(&json!({
            "access_token": fake_jwt(json!({"organizations": [{"id": "org_fallback"}]})),
            "refresh_token": "refresh",
            "id_token": id_token
        }))?;
        assert_eq!(response.refresh.as_deref(), Some("refresh"));
        assert_eq!(response.account_id.as_deref(), Some("acct_claim"));
        assert!(response
            .expires
            .is_some_and(|expires| expires > now_millis()));
        Ok(())
    }

    #[test]
    fn codex_login_success_saves_oauth_session_without_config_secret() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let outcome = save_codex_login_token(
            Some(config_path.clone()),
            CodexTokenResponse {
                access: "access-secret".to_owned(),
                refresh: Some("refresh-secret".to_owned()),
                expires: Some(1234),
                account_id: Some("acct_login".to_owned()),
            },
        )?;

        assert_eq!(outcome.selected_model, CODEX_DEFAULT_MODEL);
        assert_eq!(outcome.account_id.as_deref(), Some("acct_login"));
        let config_text = fs::read_to_string(&config_path)?;
        assert!(config_text.contains(CODEX_PROVIDER_ID));
        assert!(!config_text.contains("access-secret"));
        assert!(!config_text.contains("refresh-secret"));
        let auth = load_auth_store(&outcome.auth_path)?;
        let codex = auth
            .providers
            .get(CODEX_PROVIDER_ID)
            .ok_or("missing codex auth")?;
        assert_eq!(codex.access, "access-secret");
        assert_eq!(codex.refresh.as_deref(), Some("refresh-secret"));
        assert_eq!(codex.expires, Some(1234));
        Ok(())
    }

    #[test]
    fn runtime_overlay_refreshes_expired_codex_auth() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config
            .providers
            .insert(CODEX_PROVIDER_ID.to_owned(), codex_provider_config());
        save_config_to_path(&config, &config_path)?;
        let mut bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let mut auth = AuthStore::default();
        auth.providers.insert(
            CODEX_PROVIDER_ID.to_owned(),
            ProviderAuth {
                kind: "oauth".to_owned(),
                access: "expired".to_owned(),
                refresh: Some("refresh-old".to_owned()),
                expires: Some(0),
                account_id: Some("acct_old".to_owned()),
            },
        );
        save_auth_store_to_path(&auth, &bundle.context.auth_path())?;
        let transport = MockCodexAuthTransport::new(vec![CodexAuthHttpResponse {
            status: 200,
            body: json!({
                "access_token": "access-new",
                "refresh_token": "refresh-new",
                "expires_in": 3600,
                "id_token": fake_jwt(json!({"https://api.openai.com/auth.chatgpt_account_id": "acct_new"}))
            }),
        }]);

        apply_codex_auth_overlay_with_transport(&mut bundle, &transport)?;
        let provider = bundle
            .config
            .providers
            .get(CODEX_PROVIDER_ID)
            .ok_or("missing provider")?;
        assert_eq!(provider.api_key.as_deref(), Some("access-new"));
        assert_eq!(
            provider
                .extra_headers
                .as_ref()
                .and_then(|headers| headers.get("ChatGPT-Account-Id"))
                .map(String::as_str),
            Some("acct_new")
        );
        let saved = load_auth_store(&bundle.context.auth_path())?;
        let saved_codex = saved
            .providers
            .get(CODEX_PROVIDER_ID)
            .ok_or("missing auth")?;
        assert_eq!(saved_codex.access, "access-new");
        assert_eq!(saved_codex.refresh.as_deref(), Some("refresh-new"));
        assert_eq!(saved_codex.account_id.as_deref(), Some("acct_new"));
        let forms = transport.forms.lock().map_err(|_| "forms lock poisoned")?;
        assert_eq!(forms.len(), 1);
        assert!(forms[0]
            .1
            .contains(&("grant_type".to_owned(), "refresh_token".to_owned())));
        assert!(forms[0]
            .1
            .contains(&("refresh_token".to_owned(), "refresh-old".to_owned())));
        Ok(())
    }

    #[test]
    fn codex_headless_login_polls_until_device_code_is_authorized() -> Result<(), Box<dyn Error>> {
        let transport = MockCodexAuthTransport::new(vec![
            CodexAuthHttpResponse {
                status: 200,
                body: json!({
                    "device_auth_id": "device-123",
                    "user_code": "USER-CODE",
                    "interval": 1,
                }),
            },
            CodexAuthHttpResponse {
                status: 403,
                body: json!({}),
            },
            CodexAuthHttpResponse {
                status: 200,
                body: json!({
                    "authorization_code": "authorized-code",
                    "code_verifier": "device-verifier",
                }),
            },
            CodexAuthHttpResponse {
                status: 200,
                body: json!({
                    "access_token": "device-access",
                    "refresh_token": "device-refresh",
                    "expires_in": 3600,
                }),
            },
        ]);

        let token = codex_headless_login_with_polling(&transport, |_| {}, Duration::from_secs(30))?;

        assert_eq!(token.access, "device-access");
        assert_eq!(token.refresh.as_deref(), Some("device-refresh"));
        let jsons = transport.jsons.lock().map_err(|_| "jsons lock poisoned")?;
        assert_eq!(jsons.len(), 3);
        assert_eq!(
            jsons[0].0,
            format!("{CODEX_ISSUER}/api/accounts/deviceauth/usercode")
        );
        assert_eq!(jsons[0].1, json!({ "client_id": CODEX_CLIENT_ID }));
        assert_eq!(
            jsons[1].0,
            format!("{CODEX_ISSUER}/api/accounts/deviceauth/token")
        );
        assert_eq!(jsons[1].1["device_auth_id"], "device-123");
        assert_eq!(jsons[1].1["user_code"], "USER-CODE");
        assert_eq!(jsons[2].1["device_auth_id"], "device-123");
        let forms = transport.forms.lock().map_err(|_| "forms lock poisoned")?;
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].0, format!("{CODEX_ISSUER}/oauth/token"));
        assert!(forms[0]
            .1
            .contains(&("grant_type".to_owned(), "authorization_code".to_owned())));
        assert!(forms[0]
            .1
            .contains(&("code".to_owned(), "authorized-code".to_owned())));
        assert!(forms[0]
            .1
            .contains(&("code_verifier".to_owned(), "device-verifier".to_owned())));
        assert!(forms[0].1.contains(&(
            "redirect_uri".to_owned(),
            CODEX_DEVICE_REDIRECT_URI.to_owned()
        )));
        Ok(())
    }

    #[test]
    fn runtime_config_overlays_codex_auth_without_persisting_secret() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config
            .providers
            .insert("openai_codex".to_owned(), codex_provider_config());
        save_config_to_path(&config, &config_path)?;
        let auth_path = config_context(Some(config_path.clone()), None).auth_path();
        let mut auth = AuthStore::default();
        auth.providers.insert(
            "openai_codex".to_owned(),
            ProviderAuth::oauth_access("runtime-token", Some("acct_runtime".to_owned())),
        );
        save_auth_store_to_path(&auth, &auth_path)?;

        let bundle = load_runtime_config(RuntimeConfigOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: false,
        })?;
        let provider = bundle
            .config
            .providers
            .get("openai_codex")
            .ok_or("missing provider")?;
        assert_eq!(provider.api_key.as_deref(), Some("runtime-token"));
        assert_eq!(
            provider
                .extra_headers
                .as_ref()
                .and_then(|headers| headers.get("ChatGPT-Account-Id"))
                .map(String::as_str),
            Some("acct_runtime")
        );
        assert!(!fs::read_to_string(config_path)?.contains("runtime-token"));
        Ok(())
    }

    #[test]
    fn cli_session_key_defaults_and_preserves_explicit_channel_prefix() {
        assert_eq!(cli_session_key(None), "cli:direct");
        assert_eq!(cli_session_key(Some("work")), "cli:work");
        assert_eq!(cli_session_key(Some("discord:123")), "discord:123");
    }

    #[test]
    fn wizard_is_explicitly_deferred() {
        let error = onboard(OnboardOptions {
            config_path: None,
            workspace: None,
            wizard: true,
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(error.contains("wizard"));
    }

    #[test]
    fn sync_outcome_fields_are_kept_public_for_callers() {
        let outcome = WorkspaceSyncOutcome {
            created_files: vec!["AGENTS.md".to_owned()],
            created_dirs: vec!["skills".to_owned()],
        };
        assert_eq!(outcome.created_files, ["AGENTS.md"]);
        assert_eq!(outcome.created_dirs, ["skills"]);
    }

    struct FakeProviderClient {
        captured: std::sync::Arc<Mutex<Vec<ProviderRequest>>>,
        response: LlmResponse,
    }

    type MockFormCalls = Vec<(String, Vec<(String, String)>)>;

    struct MockCodexAuthTransport {
        forms: Mutex<MockFormCalls>,
        jsons: Mutex<Vec<(String, Value)>>,
        responses: Mutex<VecDeque<CodexAuthHttpResponse>>,
    }

    impl MockCodexAuthTransport {
        fn new(responses: Vec<CodexAuthHttpResponse>) -> Self {
            Self {
                forms: Mutex::new(Vec::new()),
                jsons: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }

        fn next_response(&self) -> Result<CodexAuthHttpResponse, CliError> {
            self.responses
                .lock()
                .map_err(|_| CliError::InvalidArguments("responses lock poisoned".to_owned()))?
                .pop_front()
                .ok_or_else(|| CliError::InvalidArguments("missing mock response".to_owned()))
        }
    }

    impl CodexAuthTransport for MockCodexAuthTransport {
        fn post_form(
            &self,
            url: &str,
            fields: &[(String, String)],
        ) -> Result<CodexAuthHttpResponse, CliError> {
            self.forms
                .lock()
                .map_err(|_| CliError::InvalidArguments("forms lock poisoned".to_owned()))?
                .push((url.to_owned(), fields.to_vec()));
            self.next_response()
        }

        fn post_json(&self, url: &str, body: Value) -> Result<CodexAuthHttpResponse, CliError> {
            self.jsons
                .lock()
                .map_err(|_| CliError::InvalidArguments("jsons lock poisoned".to_owned()))?
                .push((url.to_owned(), body));
            self.next_response()
        }
    }

    fn fake_jwt(claims: Value) -> String {
        format!(
            "header.{}.sig",
            base64_url_no_pad(serde_json::to_string(&claims).unwrap().as_bytes())
        )
    }

    impl ProviderClient for FakeProviderClient {
        fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
            self.captured
                .lock()
                .map_err(|_| ProviderError::Api {
                    status: Some(500),
                    message: "fake lock failed".to_owned(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(self.response.clone())
        }

        fn chat_stream(
            &self,
            request: ProviderRequest,
            _on_event: &mut dyn FnMut(ProviderEvent),
        ) -> Result<LlmResponse, ProviderError> {
            self.chat(request)
        }
    }
}
