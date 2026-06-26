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
    builtin_channel_default_configs, builtin_live_worker_descriptors, normalize_websocket_frame,
    normalize_whatsapp_bridge_message, websocket_event_from_outbound, whatsapp_outbound_frames,
    ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError, ChannelManager,
    ChannelRegistry, ChannelRetryPolicy, DiscordInbound, EmailInbound, LiveChannelWorkerDescriptor,
    LiveChannelWorkerKind, OutboundMessage, RecentMessageIds, SlackInbound, TelegramInbound,
    WebSocketInboundAction, WebSocketServerEvent, WhatsAppBridgeMessage, WhatsAppChannelConfig,
    WhatsAppGroupPolicy, WhatsAppOutboundFrame, DISCORD_CHANNEL, EMAIL_CHANNEL, SLACK_CHANNEL,
    TELEGRAM_CHANNEL, WEBSOCKET_CHANNEL, WHATSAPP_CHANNEL,
};
use shacs_config::{
    config_context, default_config_path, ensure_runtime_dirs, load_auth_store,
    load_config_with_env, resolve_config_env_refs, save_auth_store_to_path, save_config_to_path,
    ApiConfig, ConfigBundle, ConfigError, EnvSource, LoadOptions, PermissionActivationContext,
    PermissionConfigSnapshot, PermissionModeSource, ProcessEnv, ProviderAuth, ProviderConfig,
};
use shacs_core::app::{AppError, AppId, AppLifecycleState, AppRegistryEntry, AppRegistryStore};
use shacs_core::app_authoring::{
    AppAuthoringError, AppAuthoringInitOutcome, AppAuthoringInitReport, AppAuthoringStore,
};
use shacs_core::runtime::{
    apply_context_safety_gate, build_context_diagnostics_summary, build_context_provider_handoff,
    build_plugin_runtime_snapshot, build_plugin_surface_projection, discover_context_files,
    discover_plugins, parse_context_references, plugin_hook_catalog, resolve_context_reference,
    AgentHook, AgentHookContext, AgentLoop, AgentLoopConfig, AgentLoopTurnResult, CompositeHook,
    ContainmentSnapshotRef, ContextBudgetInput, ContextBuilder, ContextDiagnosticsInput,
    ContextDiagnosticsSummary, ContextFileDiagnosticsSummary, ContextFileDiscoveryOptions,
    ContextReferenceDiagnosticsSummary, ContextReferenceResolverConfig, DiscoveredPlugin,
    DreamLifecycle, HeartbeatError, HeartbeatNotifier, HeartbeatResponseEvaluator,
    HeartbeatService, HeartbeatTaskExecutor, HeartbeatWorker, InboundMessage, McpLifecycle,
    MessageBus, PermissionModeSnapshot, PluginDiscoveryError, PluginHookCatalog,
    PluginHookDescriptor, PluginHookDispatchSink, PluginHookDispatchSummary,
    PluginRuntimeHookAgentHook, PluginRuntimeSnapshot, PluginState, PluginSurfaceProjection,
    ProviderNotificationEvaluator, RuntimeCapabilityReport, RuntimeCapabilityStatus,
    RuntimeToolCall, Session, SessionHistoryOptions, SessionManager, SessionTurnLock,
    StreamDeltaCoalescer, SubagentExecutionConfig, SubagentRuntime, ToolEvent, ToolSearchConfig,
    ToolSearchMode, ToolStatus, HEARTBEAT_FILE_NAME,
};
use shacs_core::tools::{
    AskUserTool, EditFileTool, ExecConfig, ExecTool, FileState, GlobTool, GrepTool,
    ImageGenerateTool, ImageGenerateToolConfig, ListDirTool, McpRuntime, McpServerConnectionReport,
    McpServerSpec, MessageTool, NetworkGuard, PathContext, ReadFileTool, SelfRuntimeState,
    SelfTool, SpawnTool, StdioMcpConnector, ToolRegistry, WebFetchConfig, WebFetchTool,
    WebSearchConfig, WebSearchTool, WriteFileTool,
};
use shacs_providers::{
    chat_with_retry, prepare_provider_request, resolve_image_generation_client,
    resolve_provider_client, AgentDefaults, LlmResponse, ProviderClient, ProviderError,
    ProviderEvent, ProviderRegistry, ProviderRetryMode, ResolvedProviderClient,
};
use shacs_redaction::redact_string;
use shacs_skills::{
    discover_skill_registry, sync_builtin_skills, SkillRegistryEntry, SkillRegistryOptions,
    SkillRegistryStatus,
};
use shacs_templates::sync_workspace_templates;
use shacs_utils::attachments::{
    normalize_channel_attachment_data_url, AttachmentIntakeService, AttachmentIntakeStatus,
    AttachmentLimitPolicy, ChannelAttachmentAdapterFailureReason, ChannelAttachmentIntakeRequest,
    DEFAULT_MAX_ATTACHMENTS_PER_MESSAGE, DEFAULT_MAX_BYTES_PER_TURN,
};
use shacs_utils::diagnostics::{
    write_diagnostics_bundle, CrashEvidence, DiagnosticsBundleManifest, DiagnosticsKind,
    DiagnosticsRecord, DiagnosticsSeverity, DiagnosticsSnapshot, OperationalLogRecord,
    RecoveryEvidence, TraceRecord, TraceStatus,
};
use shacs_utils::media_decode::DEFAULT_MAX_BYTES;
use shacs_utils::progress_events::{
    build_tool_event_start_payload, build_tool_progress_finish_payload,
    build_tool_progress_start_payload, project_tool_progress_arguments, ProgressEventStatus,
    ToolProgressEvent, ToolProgressPayload,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect as websocket_connect, Message as WebSocketMessage, WebSocket};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RUNTIME_DATA_SCHEMA_VERSION: u32 = 1;
const RUNTIME_DATA_SCHEMA_MIN_VERSION: u32 = 1;
const RUNTIME_OWNERSHIP_HEARTBEAT_TTL_MS: u64 = 30_000;
const RUNTIME_OWNERSHIP_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const RUNTIME_OWNERSHIP_MUTATION_LOCK_ERROR: &str =
    "runtime mutation blocked by concurrent runtime ownership mutation; try again";
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_ISSUER: &str = "https://auth.openai.com";
const CODEX_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const CODEX_DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const CODEX_DEVICE_URL: &str = "https://auth.openai.com/codex/device";
const CODEX_PROVIDER_ID: &str = "openai_codex";
const CODEX_DEFAULT_MODEL: &str = "gpt-5.4";
const GITHUB_COPILOT_PROVIDER_ID: &str = "github_copilot";
const GITHUB_COPILOT_DEFAULT_MODEL: &str = "github_copilot/gpt-4o";
const CODEX_BROWSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CODEX_DEVICE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const WEBSOCKET_STREAM_FLUSH_CHARS: usize = 32;
const EXTERNAL_SESSION_PENDING_LIMIT: usize = 20;
const EXTERNAL_TYPING_REFRESH_INTERVAL: Duration = Duration::from_secs(7);
const EXTERNAL_TYPING_INDICATOR_KEY: &str = "_typing_indicator";
type ApiProviderEventCallback = Arc<dyn Fn(&shacs_providers::ProviderEvent) + Send + Sync>;
type ExternalTransportRunner = Arc<
    dyn Fn(
            ExternalTransportSpec,
            MessageBus,
            mpsc::Receiver<OutboundMessage>,
            Arc<AtomicBool>,
            ExternalTransportRuntimeContext,
        ) + Send
        + Sync,
>;

#[derive(Debug, Clone, PartialEq)]
pub enum CliCommand {
    Help,
    Version,
    Onboard(OnboardOptions),
    Status(StatusOptions),
    RuntimeInspect(RuntimeInspectOptions),
    RuntimeDiagnostics(RuntimeDiagnosticsOptions),
    RuntimeUpdate(RuntimeUpdateOptions),
    RuntimeRecover(RuntimeRecoverOptions),
    RuntimeStart(RuntimeStartOptions),
    RuntimeStop(RuntimeStopOptions),
    RuntimeRestart(RuntimeStopOptions),
    Session(SessionCommand),
    Skills(SkillsCommand),
    Apps(AppsCommand),
    Plugins(PluginsCommand),
    Hooks(HooksCommand),
    Channels(ChannelsCommand),
    Context(ContextCommand),
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeDiagnosticsOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub bundle_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeUpdateOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub target_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeRecoverOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeStartOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeStopOptions {
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
pub enum AppsCommand {
    Init(AppsInitOptions),
    Install(AppsInstallOptions),
    List(AppsListOptions),
    Inspect(AppsInspectOptions),
    Enable(AppsIdOptions),
    Disable(AppsIdOptions),
    Uninstall(AppsIdOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginsCommand {
    List(PluginsListOptions),
    Inspect(PluginsInspectOptions),
    Doctor(PluginsListOptions),
    Enable(PluginsMutateOptions),
    Disable(PluginsMutateOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HooksCommand {
    List(HooksListOptions),
    Inspect(HooksInspectOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelsCommand {
    List(ChannelsListOptions),
    Status(ChannelsStatusOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCommand {
    Files(ContextFilesCommand),
    Refs(ContextRefsCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextFilesCommand {
    List(ContextFilesOptions),
    Inspect(ContextFilesOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextRefsCommand {
    Parse(ContextRefsParseOptions),
    Resolve(ContextRefsResolveOptions),
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
pub struct ContextFilesOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextRefsParseOptions {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextRefsResolveOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub message: String,
    pub network_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppsInitOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppsInstallOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub bundle_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppsListOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppsInspectOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppsIdOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginsListOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginsInspectOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PluginsMutateOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HooksListOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HooksInspectOptions {
    pub config_path: Option<PathBuf>,
    pub workspace_override: Option<PathBuf>,
    pub filter: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShacsBotLifecycleEvent {
    InitStarted,
    InitCompleted,
    InitFailed {
        error: String,
    },
    RunStarted {
        session_key: String,
    },
    RunCompleted {
        session_key: String,
        stop_reason: String,
    },
    RunFailed {
        session_key: String,
        error: String,
    },
    Shutdown,
}

pub type ShacsBotLifecycleHook = Arc<dyn Fn(&ShacsBotLifecycleEvent) + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub enum ShacsBotObservabilityEvent {
    Provider {
        event: Box<ProviderEvent>,
    },
    Tool {
        event: Box<ToolProgressEvent>,
        payload: Option<Box<ToolProgressPayload>>,
    },
}

pub type ShacsBotObservabilityHook = Arc<dyn Fn(&ShacsBotObservabilityEvent) + Send + Sync>;
type RuntimeToolEventCallback = Arc<dyn Fn(&ToolEvent) + Send + Sync>;
type RuntimeNotificationSink = Arc<dyn Fn(OutboundMessage) + Send + Sync>;

pub struct ShacsBot {
    adapter: AgentLoopChatCompletionAdapter,
    lifecycle_hooks: Vec<ShacsBotLifecycleHook>,
    observability_hooks: Vec<ShacsBotObservabilityHook>,
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
    CopilotImportToken(CopilotImportTokenOptions),
    ImportApiKey(ProviderApiKeyImportOptions),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodexImportTokenOptions {
    pub config_path: Option<PathBuf>,
    pub token_source: TokenSource,
    pub account_id: Option<String>,
    pub select: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CopilotImportTokenOptions {
    pub config_path: Option<PathBuf>,
    pub token_source: TokenSource,
    pub select: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderApiKeyImportOptions {
    pub config_path: Option<PathBuf>,
    pub provider: String,
    pub token_source: TokenSource,
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
pub struct CopilotImportOutcome {
    pub config_path: PathBuf,
    pub auth_path: PathBuf,
    pub provider: String,
    pub selected_model: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderApiKeyImportOutcome {
    pub config_path: PathBuf,
    pub auth_path: PathBuf,
    pub provider: String,
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
    pub generated_media: Vec<GeneratedMediaArtifactInspect>,
    pub capabilities: Vec<RuntimeCapabilityReport>,
    pub sessions: RuntimeSessionInspect,
    pub lifecycle: RuntimeLifecycleInspect,
    pub containment: RuntimeContainmentInspect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContainmentInspect {
    pub contained: Option<bool>,
    pub backend: Option<String>,
    pub summary: Option<String>,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedMediaArtifactInspect {
    pub artifact_id: String,
    pub media_ref: String,
    pub metadata_ref: String,
    pub mime_type: String,
    pub byte_len: u64,
    pub sha256: String,
    pub provider_id: String,
    pub model_id: String,
    pub created_at: String,
    pub redacted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeDiagnosticsReport {
    pub snapshot: DiagnosticsSnapshot,
    pub bundle: Option<DiagnosticsBundleManifest>,
    pub bundle_path: Option<PathBuf>,
    pub bundle_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginProjectionReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub plugins: Vec<DiscoveredPlugin>,
    pub projection: PluginSurfaceProjection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginInspectReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub plugin: DiscoveredPlugin,
    pub projection: PluginSurfaceProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginMutationReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub plugin_name: String,
    pub action: PluginMutationAction,
    pub changed: bool,
    pub next_session_notice: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMutationAction {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookProjectionReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub catalog: PluginHookCatalog,
    pub plugin_hooks: Vec<PluginHookDescriptor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookInspectReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub filter: String,
    pub catalog: PluginHookCatalog,
    pub plugin_hooks: Vec<PluginHookDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLifecycleInspect {
    pub binary_version: String,
    pub data_schema_version: u32,
    pub data_schema_min_version: u32,
    pub compatibility: RuntimeCompatibility,
    pub ownership: RuntimeOwnershipStatus,
    pub stop_request: Option<RuntimeStopRequestMarker>,
    pub update_marker: Option<RuntimeUpdateMarker>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCompatibility {
    FullyCompatible,
    MigrationRequired,
    InspectOnly,
    Incompatible,
}

impl RuntimeCompatibility {
    fn as_str(&self) -> &'static str {
        match self {
            Self::FullyCompatible => "fully_compatible",
            Self::MigrationRequired => "migration_required",
            Self::InspectOnly => "inspect_only",
            Self::Incompatible => "incompatible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeOwnershipState {
    None,
    Active,
    Stale,
}

impl RuntimeOwnershipState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Active => "active",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOwnershipStatus {
    pub state: RuntimeOwnershipState,
    pub marker: Option<RuntimeOwnershipMarker>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOwnershipMarker {
    pub pid: u32,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub binary_version: String,
    pub data_schema_version: u32,
    pub mode: String,
    pub config_path: String,
    pub workspace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStopRequestMarker {
    pub request: String,
    pub requested_at_ms: u64,
    pub owner_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUpdateMarker {
    pub phase: String,
    pub from_version: String,
    pub target_version: String,
    pub migration_required: bool,
    pub completed_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeUpdateOutcome {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub marker_path: PathBuf,
    pub from_version: String,
    pub target_version: String,
    pub phase: String,
    pub migration_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRecoverOutcome {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub marker_path: PathBuf,
    pub recovered: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStopOutcome {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub request_path: PathBuf,
    pub status: RuntimeStopOutcomeStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeStopOutcomeStatus {
    RequestWritten,
    NoActiveOwner,
    StaleOwnerOnly,
}

impl RuntimeStopOutcomeStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::RequestWritten => "request_written",
            Self::NoActiveOwner => "no_active_owner",
            Self::StaleOwnerOnly => "stale_owner_only",
        }
    }
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
    pub send_memory_hints: bool,
    pub send_max_retries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelReportItem {
    pub descriptor: ChannelDescriptor,
    pub configured: bool,
    pub enabled: bool,
    pub runtime_status: String,
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
    SkippedUnsupported,
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
    channel_filter: DiscordChannelFilter,
    allowed_senders: Vec<String>,
    group_policy: DiscordGroupPolicy,
    streaming: bool,
    poll_interval_seconds: u64,
    transport: DiscordTransportMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiscordTransportMode {
    Gateway,
    RestPolling,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiscordChannelFilter {
    AllVisible,
    Only(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscordGroupPolicy {
    Mention,
    Open,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SlackRuntimeConfig {
    app_token: String,
    bot_token: String,
    channel_ids: Vec<String>,
    allowed_senders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmailRuntimeConfig {
    smtp: Option<EmailSmtpRuntimeConfig>,
    imap: Option<EmailImapRuntimeConfig>,
    allowed_senders: Vec<String>,
    verify_spf: bool,
    verify_dkim: bool,
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
struct ParsedEmailInbound {
    inbound: EmailInbound,
    authentication_results: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WhatsAppRuntimeConfig {
    bridge_url: String,
    bridge_token: Option<String>,
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
    App(AppError),
    AppAuthoring(AppAuthoringError),
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
            Self::App(error) => write!(formatter, "{error}"),
            Self::AppAuthoring(error) => write!(formatter, "{error}"),
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

impl From<AppError> for CliError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl From<AppAuthoringError> for CliError {
    fn from(error: AppAuthoringError) -> Self {
        Self::AppAuthoring(error)
    }
}

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

impl From<PluginDiscoveryError> for CliError {
    fn from(error: PluginDiscoveryError) -> Self {
        Self::InvalidArguments(error.to_string())
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
        Self::from_options_with_lifecycle_hooks(options, Vec::new())
    }

    pub fn from_options_with_lifecycle_hooks(
        options: ShacsBotOptions,
        lifecycle_hooks: Vec<ShacsBotLifecycleHook>,
    ) -> Result<Self, CliError> {
        Self::from_options_with_hooks(options, lifecycle_hooks, Vec::new())
    }

    pub fn from_options_with_observability_hooks(
        options: ShacsBotOptions,
        observability_hooks: Vec<ShacsBotObservabilityHook>,
    ) -> Result<Self, CliError> {
        Self::from_options_with_hooks(options, Vec::new(), observability_hooks)
    }

    pub fn from_options_with_hooks(
        options: ShacsBotOptions,
        lifecycle_hooks: Vec<ShacsBotLifecycleHook>,
        observability_hooks: Vec<ShacsBotObservabilityHook>,
    ) -> Result<Self, CliError> {
        emit_lifecycle_hooks(&lifecycle_hooks, ShacsBotLifecycleEvent::InitStarted);
        let bundle = load_runtime_config(RuntimeConfigOptions {
            config_path: options.config_path,
            workspace_override: options.workspace_override,
            resolve_env: true,
        })
        .map_err(|error| {
            emit_lifecycle_hooks(
                &lifecycle_hooks,
                ShacsBotLifecycleEvent::InitFailed {
                    error: error.to_string(),
                },
            );
            error
        })?;
        let adapter =
            AgentLoopChatCompletionAdapter::from_bundle(bundle, options.allow_side_effects)
                .map_err(|error| {
                    emit_lifecycle_hooks(
                        &lifecycle_hooks,
                        ShacsBotLifecycleEvent::InitFailed {
                            error: error.to_string(),
                        },
                    );
                    error
                })?;
        emit_lifecycle_hooks(&lifecycle_hooks, ShacsBotLifecycleEvent::InitCompleted);
        Ok(Self {
            adapter,
            lifecycle_hooks,
            observability_hooks,
        })
    }

    pub fn with_lifecycle_hook(mut self, hook: ShacsBotLifecycleHook) -> Self {
        self.lifecycle_hooks.push(hook);
        self
    }

    pub fn with_observability_hook(mut self, hook: ShacsBotObservabilityHook) -> Self {
        self.observability_hooks.push(hook);
        self
    }

    pub fn run(&self, message: impl Into<String>) -> Result<RunResult, CliError> {
        self.run_with_options(ShacsBotRunOptions {
            message: message.into(),
            ..ShacsBotRunOptions::default()
        })
    }

    pub fn run_with_options(&self, options: ShacsBotRunOptions) -> Result<RunResult, CliError> {
        self.run_with_options_and_observability_hooks(options, Vec::new())
    }

    pub fn run_with_options_and_observability_hooks(
        &self,
        options: ShacsBotRunOptions,
        run_observability_hooks: Vec<ShacsBotObservabilityHook>,
    ) -> Result<RunResult, CliError> {
        let session_key = options.session_key.trim().to_owned();
        emit_lifecycle_hooks(
            &self.lifecycle_hooks,
            ShacsBotLifecycleEvent::RunStarted {
                session_key: session_key.clone(),
            },
        );
        let result = self.run_with_options_inner(options, &run_observability_hooks);
        match &result {
            Ok(result) => emit_lifecycle_hooks(
                &self.lifecycle_hooks,
                ShacsBotLifecycleEvent::RunCompleted {
                    session_key,
                    stop_reason: if result.content.is_empty() {
                        "empty".to_owned()
                    } else {
                        "stop".to_owned()
                    },
                },
            ),
            Err(error) => emit_lifecycle_hooks(
                &self.lifecycle_hooks,
                ShacsBotLifecycleEvent::RunFailed {
                    session_key,
                    error: error.to_string(),
                },
            ),
        }
        result
    }

    fn run_with_options_inner(
        &self,
        options: ShacsBotRunOptions,
        run_observability_hooks: &[ShacsBotObservabilityHook],
    ) -> Result<RunResult, CliError> {
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
        let mut observability_hooks = self.observability_hooks.clone();
        observability_hooks.extend(run_observability_hooks.iter().cloned());
        self.adapter
            .complete_sdk_run(invocation, &observability_hooks)
            .map_err(Into::into)
    }
}

impl Drop for ShacsBot {
    fn drop(&mut self) {
        emit_lifecycle_hooks(&self.lifecycle_hooks, ShacsBotLifecycleEvent::Shutdown);
    }
}

fn emit_lifecycle_hooks(hooks: &[ShacsBotLifecycleHook], event: ShacsBotLifecycleEvent) {
    for hook in hooks {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(&event))).is_err() {
            eprintln!(
                "shacs bot lifecycle hook panicked during {}",
                lifecycle_event_label(&event)
            );
        }
    }
}

fn lifecycle_event_label(event: &ShacsBotLifecycleEvent) -> &'static str {
    match event {
        ShacsBotLifecycleEvent::InitStarted => "init_started",
        ShacsBotLifecycleEvent::InitCompleted => "init_completed",
        ShacsBotLifecycleEvent::InitFailed { .. } => "init_failed",
        ShacsBotLifecycleEvent::RunStarted { .. } => "run_started",
        ShacsBotLifecycleEvent::RunCompleted { .. } => "run_completed",
        ShacsBotLifecycleEvent::RunFailed { .. } => "run_failed",
        ShacsBotLifecycleEvent::Shutdown => "shutdown",
    }
}

fn emit_observability_hooks(
    hooks: &[ShacsBotObservabilityHook],
    event: ShacsBotObservabilityEvent,
) {
    for hook in hooks {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook(&event))).is_err() {
            eprintln!(
                "shacs bot observability hook panicked during {}",
                observability_event_label(&event)
            );
        }
    }
}

fn observability_event_label(event: &ShacsBotObservabilityEvent) -> String {
    match event {
        ShacsBotObservabilityEvent::Provider { .. } => "provider".to_owned(),
        ShacsBotObservabilityEvent::Tool { event, .. } => {
            format!("tool:{}:{:?}", event.name, event.status)
        }
    }
}

#[derive(Debug, Clone)]
struct ToolCallProgressSnapshot {
    call_id: String,
    arguments: Value,
}

type PendingToolProgress = Arc<Mutex<BTreeMap<String, VecDeque<ToolCallProgressSnapshot>>>>;

#[derive(Clone)]
struct ObservabilityToolStartHook {
    hooks: Vec<ShacsBotObservabilityHook>,
    pending: PendingToolProgress,
}

impl ObservabilityToolStartHook {
    fn new(hooks: Vec<ShacsBotObservabilityHook>, pending: PendingToolProgress) -> Self {
        Self { hooks, pending }
    }
}

impl AgentHook for ObservabilityToolStartHook {
    fn before_execute_tools(&self, _context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        for call in calls {
            let projected_arguments = project_tool_progress_arguments(&call.name, &call.arguments);
            if let Ok(mut pending) = self.pending.lock() {
                pending
                    .entry(call.name.clone())
                    .or_default()
                    .push_back(ToolCallProgressSnapshot {
                        call_id: call.id.clone(),
                        arguments: projected_arguments.clone(),
                    });
            }
            let payload = build_tool_progress_start_payload(
                call.id.clone(),
                call.name.clone(),
                projected_arguments,
            );
            emit_observability_hooks(
                &self.hooks,
                ShacsBotObservabilityEvent::Tool {
                    event: Box::new(build_tool_event_start_payload(call.name.clone(), "started")),
                    payload: Some(Box::new(payload)),
                },
            );
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct RuntimeVerboseLogHook;

impl AgentHook for RuntimeVerboseLogHook {
    fn before_execute_tools(&self, _context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        for call in calls {
            eprintln!(
                "Tool call {} args={}",
                call.name,
                runtime_tool_args_preview(&call.name, &call.arguments)
            );
        }
    }

    fn after_response(&self, _context: &AgentHookContext, response: &LlmResponse) {
        eprintln!(
            "LLM response preview: {}",
            runtime_response_preview(response)
        );
        eprintln!("LLM usage {}", runtime_usage_preview(response));
    }
}

fn runtime_preview_text(value: &str, max_chars: usize) -> String {
    let redacted = redact_string(value.trim());
    let preview = truncate_for_history(&redacted, max_chars);
    if preview.is_empty() {
        "<empty>".to_owned()
    } else {
        preview
    }
}

fn runtime_message_preview(message: &InboundMessage) -> String {
    runtime_preview_text(&message.content, 80)
}

fn runtime_response_preview(response: &LlmResponse) -> String {
    runtime_preview_text(response.content.as_deref().unwrap_or(""), 120)
}

fn runtime_tool_args_preview(name: &str, arguments: &Value) -> String {
    runtime_preview_text(
        &project_tool_progress_arguments(name, arguments).to_string(),
        200,
    )
}

fn publish_runtime_notification(
    bus: &MessageBus,
    live_sink: Option<&RuntimeNotificationSink>,
    message: OutboundMessage,
) {
    if is_plugin_hook_runtime_notification(&message) {
        bus.publish_outbound(message);
        return;
    }
    if let Some(sink) = live_sink {
        sink(message);
    } else {
        bus.publish_outbound(message);
    }
}

fn skill_usage_notification_message(
    channel: &str,
    chat_id: &str,
    routing_metadata: &Map<String, Value>,
    reply_to: Option<&str>,
    skill_names: &[String],
) -> Option<OutboundMessage> {
    if skill_names.is_empty() {
        return None;
    }
    let content = if skill_names.len() == 1 {
        format!("Using skill: {}", skill_names[0])
    } else {
        format!("Using skills: {}", skill_names.join(", "))
    };
    let mut metadata = routing_metadata.clone();
    metadata.insert(
        "runtime_notification".to_owned(),
        json!({
            "kind": "skill",
            "phase": "start",
            "usage": "selected",
            "skill_names": skill_names,
        }),
    );
    let mut message = OutboundMessage::new(channel, chat_id, content).with_metadata(metadata);
    if let Some(reply_to) = reply_to {
        message.reply_to = Some(reply_to.to_owned());
    }
    Some(message)
}

fn subagent_start_notification_message(
    channel: &str,
    chat_id: &str,
    routing_metadata: &Map<String, Value>,
    reply_to: Option<&str>,
    outcome: &shacs_core::runtime::SubagentSpawnOutcome,
) -> OutboundMessage {
    let mut metadata = routing_metadata.clone();
    metadata.insert(
        "runtime_notification".to_owned(),
        json!({
            "kind": "subagent",
            "phase": "start",
            "child_task_id": outcome.envelope.child_task_id,
            "label": outcome.envelope.label,
        }),
    );
    let mut message = OutboundMessage::new(channel, chat_id, outcome.user_message.clone())
        .with_metadata(metadata);
    if let Some(reply_to) = reply_to {
        message.reply_to = Some(reply_to.to_owned());
    }
    message
}

fn plugin_hook_dispatch_notification_message(
    channel: &str,
    chat_id: &str,
    routing_metadata: &Map<String, Value>,
    reply_to: Option<&str>,
    summary: &PluginHookDispatchSummary,
) -> OutboundMessage {
    let mut metadata = routing_metadata.clone();
    let summary = match serde_json::to_value(summary) {
        Ok(summary) => summary,
        Err(error) => json!({
            "serialization_error": redact_string(&error.to_string()),
        }),
    };
    metadata.insert(
        "runtime_notification".to_owned(),
        json!({
            "kind": "plugin_hook",
            "phase": "dispatch",
            "visible": false,
            "summary": summary,
        }),
    );
    let mut message = OutboundMessage::new(
        channel,
        chat_id,
        "Plugin hook diagnostics recorded".to_owned(),
    )
    .with_metadata(metadata);
    if let Some(reply_to) = reply_to {
        message.reply_to = Some(reply_to.to_owned());
    }
    message
}

fn plugin_hook_dispatch_notification_sink(
    bus: MessageBus,
    live_sink: Option<RuntimeNotificationSink>,
    channel: String,
    chat_id: String,
    routing_metadata: Map<String, Value>,
    reply_to: Option<String>,
) -> PluginHookDispatchSink {
    Arc::new(move |summary| {
        let notification = plugin_hook_dispatch_notification_message(
            &channel,
            &chat_id,
            &routing_metadata,
            reply_to.as_deref(),
            &summary,
        );
        publish_runtime_notification(&bus, live_sink.as_ref(), notification);
    })
}

fn is_skill_runtime_notification(message: &shacs_channels::OutboundMessage) -> bool {
    message
        .metadata
        .get("runtime_notification")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        == Some("skill")
}

fn is_visible_runtime_notification(message: &shacs_channels::OutboundMessage) -> bool {
    is_skill_runtime_notification(message)
        || matches!(
            message
                .metadata
                .get("runtime_notification")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str),
            Some("subagent")
        )
}

fn is_plugin_hook_runtime_notification(message: &shacs_channels::OutboundMessage) -> bool {
    message
        .metadata
        .get("runtime_notification")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        == Some("plugin_hook")
}

fn should_dispatch_runtime_outbound(message: &shacs_channels::OutboundMessage) -> bool {
    !message.metadata.contains_key("runtime_notification")
        || is_visible_runtime_notification(message)
}

fn runtime_usage_value(response: &LlmResponse, key: &str) -> u64 {
    response.usage.get(key).copied().unwrap_or_default()
}

fn runtime_usage_preview(response: &LlmResponse) -> String {
    format!(
        "prompt={} completion={} cached={}",
        runtime_usage_value(response, "prompt_tokens"),
        runtime_usage_value(response, "completion_tokens"),
        runtime_usage_value(response, "cached_tokens"),
    )
}

fn observability_provider_callback(
    hooks: &[ShacsBotObservabilityHook],
    on_event: Option<ApiProviderEventCallback>,
) -> Option<ApiProviderEventCallback> {
    if hooks.is_empty() {
        return on_event;
    }
    let hooks = hooks.to_vec();
    Some(Arc::new(move |event: &ProviderEvent| {
        emit_observability_hooks(
            &hooks,
            ShacsBotObservabilityEvent::Provider {
                event: Box::new(event.clone()),
            },
        );
        if let Some(callback) = &on_event {
            callback(event);
        }
    }))
}

fn observability_tool_callback(
    hooks: &[ShacsBotObservabilityHook],
    pending: PendingToolProgress,
) -> Option<RuntimeToolEventCallback> {
    if hooks.is_empty() {
        return None;
    }
    let hooks = hooks.to_vec();
    Some(Arc::new(move |event: &ToolEvent| {
        let payload = match &event.status {
            ToolStatus::Ok | ToolStatus::Error => {
                let snapshot = pending.lock().ok().and_then(|mut pending| {
                    pending
                        .get_mut(&event.name)
                        .and_then(|queue| queue.pop_front())
                });
                let call_id = event
                    .call_id
                    .clone()
                    .or_else(|| snapshot.as_ref().map(|snapshot| snapshot.call_id.clone()))
                    .unwrap_or_else(|| event.name.clone());
                let arguments = event
                    .arguments
                    .as_ref()
                    .map(|arguments| project_tool_progress_arguments(&event.name, arguments))
                    .or_else(|| snapshot.map(|snapshot| snapshot.arguments))
                    .unwrap_or_else(|| Value::Object(Map::new()));
                let result = event
                    .result
                    .clone()
                    .unwrap_or_else(|| Value::String(event.detail.clone()));
                Some(build_tool_progress_finish_payload(
                    call_id,
                    event.name.clone(),
                    arguments,
                    result,
                    event.status == ToolStatus::Ok,
                    event.detail.clone(),
                ))
            }
            ToolStatus::Waiting | ToolStatus::Skipped => None,
        };
        emit_observability_hooks(
            &hooks,
            ShacsBotObservabilityEvent::Tool {
                event: Box::new(tool_progress_event_from_runtime(event)),
                payload: payload.map(Box::new),
            },
        );
    }))
}

fn selected_skill_notification_callback(
    context_builder: ContextBuilder,
    bus: MessageBus,
    live_sink: Option<RuntimeNotificationSink>,
    channel: String,
    chat_id: String,
    routing_metadata: Map<String, Value>,
    reply_to: Option<String>,
) -> RuntimeToolEventCallback {
    let notified = Arc::new(Mutex::new(BTreeSet::<String>::new()));
    Arc::new(move |event: &ToolEvent| {
        if event.name != "read_file" || event.status != ToolStatus::Ok {
            return;
        }
        let Some(path) = event
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("path"))
            .and_then(Value::as_str)
        else {
            return;
        };
        let Some(skill_name) = context_builder.skill_name_for_source_path(path) else {
            return;
        };
        let should_notify = notified
            .lock()
            .map(|mut notified| notified.insert(skill_name.clone()))
            .unwrap_or(false);
        if !should_notify {
            return;
        }
        if let Some(notification) = skill_usage_notification_message(
            &channel,
            &chat_id,
            &routing_metadata,
            reply_to.as_deref(),
            std::slice::from_ref(&skill_name),
        ) {
            publish_runtime_notification(&bus, live_sink.as_ref(), notification);
        }
    })
}

fn combine_tool_event_callbacks(
    callbacks: Vec<RuntimeToolEventCallback>,
) -> Option<RuntimeToolEventCallback> {
    match callbacks.as_slice() {
        [] => None,
        [_] => callbacks.into_iter().next(),
        _ => Some(Arc::new(move |event: &ToolEvent| {
            for callback in &callbacks {
                callback(event);
            }
        })),
    }
}

fn tool_progress_event_from_runtime(event: &ToolEvent) -> ToolProgressEvent {
    ToolProgressEvent {
        name: event.name.clone(),
        status: match &event.status {
            ToolStatus::Ok => ProgressEventStatus::Ok,
            ToolStatus::Error => ProgressEventStatus::Error,
            ToolStatus::Waiting => ProgressEventStatus::Waiting,
            ToolStatus::Skipped => ProgressEventStatus::Skipped,
        },
        detail: event.detail.clone(),
        metadata: None,
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
        CliCommand::RuntimeDiagnostics(options) => {
            runtime_diagnostics(options).map(format_runtime_diagnostics)
        }
        CliCommand::RuntimeUpdate(options) => runtime_update(options).map(format_runtime_update),
        CliCommand::RuntimeRecover(options) => runtime_recover(options).map(format_runtime_recover),
        CliCommand::RuntimeStart(options) => runtime_start(options),
        CliCommand::RuntimeStop(options) => runtime_stop(options).map(format_runtime_stop),
        CliCommand::RuntimeRestart(options) => runtime_restart(options).map(format_runtime_restart),
        CliCommand::Session(command) => run_session_command(command),
        CliCommand::Skills(command) => run_skills_command(command),
        CliCommand::Apps(command) => run_apps_command(command),
        CliCommand::Plugins(command) => run_plugins_command(command),
        CliCommand::Hooks(command) => run_hooks_command(command),
        CliCommand::Channels(command) => run_channels_command(command),
        CliCommand::Context(command) => run_context_command(command),
        CliCommand::Ask(options) => ask(options),
        CliCommand::Run(options) => run_runtime(options),
        CliCommand::Serve(options) => serve(options),
        CliCommand::Gateway(options) => gateway_preset(options).map(format_gateway_preset_report),
        CliCommand::Web(options) => serve_web_ui(options),
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
        "apps" | "app" => parse_apps(parser, global_config),
        "plugins" | "plugin" => parse_plugins(parser, global_config),
        "hooks" | "hook" => parse_hooks(parser, global_config),
        "channels" | "channel" => parse_channels(parser, global_config),
        "context" => parse_context(parser, global_config),
        "ask" => parse_ask(parser, global_config, false),
        "agent" => parse_ask(parser, global_config, true),
        "run" => parse_run(parser, global_config),
        "serve" => parse_serve(parser, global_config),
        "api" => parse_api(parser, global_config),
        "gateway" => parse_gateway(parser, global_config),
        "web" => parse_web(parser, global_config),
        "provider" => parse_provider(parser, global_config),
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
            "runtime requires `start`, `stop`, `restart`, `inspect`, `diagnostics`, `update`, or `recover`".to_owned(),
        ));
    };
    match action.as_str() {
        "start" => parse_runtime_start(parser, global_config),
        "stop" => parse_runtime_stop(parser, global_config, false),
        "restart" => parse_runtime_stop(parser, global_config, true),
        "inspect" => parse_runtime_inspect(parser, global_config),
        "diagnostics" | "diagnose" => parse_runtime_diagnostics(parser, global_config),
        "update" => parse_runtime_update(parser, global_config),
        "recover" => parse_runtime_recover(parser, global_config),
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
    let config_path = options.config_path;
    let workspace_override = options.workspace_override;
    let resolve_env = options.resolve_env;
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path,
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: false,
        },
        env,
    )?;
    guard_runtime_marker_for_mutation(&bundle.context.data_dir)?;
    if !bundle.migrations.is_empty() {
        save_config_to_path(&bundle.config, &bundle.context.config_path)?;
    }
    if let Some(workspace) = workspace_override {
        bundle.config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
    }
    if resolve_env {
        resolve_config_env_refs(&mut bundle.config, env)?;
    }
    bundle.context = config_context(
        Some(bundle.context.config_path.clone()),
        Some(bundle.config.workspace_path()),
    );
    apply_codex_auth_overlay(&mut bundle)?;
    apply_copilot_auth_overlay(&mut bundle)?;
    apply_api_key_auth_overlay(&mut bundle)?;
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
    if codex_auth.kind != "oauth" {
        return Ok(());
    }
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

fn apply_copilot_auth_overlay(bundle: &mut ConfigBundle) -> Result<(), CliError> {
    let auth = load_auth_store(&bundle.context.auth_path())?;
    let Some(copilot_auth) = auth.providers.get(GITHUB_COPILOT_PROVIDER_ID).cloned() else {
        return Ok(());
    };
    if copilot_auth.kind != "oauth" {
        return Ok(());
    }
    if codex_auth_is_expired(&copilot_auth) {
        return Err(CliError::Provider(ProviderError::AuthRequired {
            provider_id: GITHUB_COPILOT_PROVIDER_ID.to_owned(),
        }));
    }
    let provider = bundle
        .config
        .providers
        .entry(GITHUB_COPILOT_PROVIDER_ID.to_owned())
        .or_insert_with(copilot_provider_config);
    provider.api_key = Some(copilot_auth.access);
    Ok(())
}

fn apply_api_key_auth_overlay(bundle: &mut ConfigBundle) -> Result<(), CliError> {
    let auth = load_auth_store(&bundle.context.auth_path())?;
    for (provider_id, provider_auth) in auth.providers {
        if provider_auth.kind != "apiKey" {
            continue;
        }
        let access = provider_auth.access.trim();
        if access.is_empty() {
            continue;
        }
        let provider = bundle.config.providers.entry(provider_id).or_default();
        if non_empty(provider.api_key.as_deref()) {
            continue;
        }
        provider.api_key = Some(access.to_owned());
    }
    Ok(())
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
    inject_builtin_channel_defaults(&mut bundle.config.channels.plugins);

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

fn inject_builtin_channel_defaults(plugins: &mut BTreeMap<String, Value>) {
    for (name, default_config) in builtin_channel_default_configs() {
        match plugins.get_mut(&name) {
            Some(existing) => merge_missing_json_defaults(existing, default_config),
            None => {
                plugins.insert(name, default_config);
            }
        }
    }
}

fn merge_missing_json_defaults(existing: &mut Value, default_value: Value) {
    if let (Value::Object(existing), Value::Object(defaults)) = (existing, default_value) {
        for (key, default_child) in defaults {
            match existing.get_mut(&key) {
                Some(existing_child) => merge_missing_json_defaults(existing_child, default_child),
                None => {
                    existing.insert(key, default_child);
                }
            }
        }
    }
}

pub fn status(options: StatusOptions) -> Result<StatusReport, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let config_exists = config_path.exists();
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    apply_api_key_auth_overlay(&mut bundle)?;
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
    runtime_inspect_inner(options, true)
}

fn runtime_inspect_inner(
    options: RuntimeInspectOptions,
    ensure_dirs: bool,
) -> Result<RuntimeInspectReport, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let config_exists = config_path.exists();
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    apply_api_key_auth_overlay(&mut bundle)?;
    if ensure_dirs {
        let _runtime_dirs = ensure_runtime_dirs(&bundle.context)?;
    }
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
    let generated_media =
        inspect_generated_media(&bundle.context.media_dir(Some("image-generation")))?;
    let capabilities = runtime_capabilities(&bundle);
    let sessions = inspect_runtime_sessions(&workspace)?;
    let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
    let update_marker = read_runtime_update_marker(&marker_path)?;
    let ownership = inspect_runtime_ownership(&bundle.context.data_dir, now_millis())?;
    let stop_request = read_runtime_stop_request_marker(&runtime_stop_request_marker_path(
        &bundle.context.data_dir,
    ))?;
    let compatibility = evaluate_runtime_compatibility(RUNTIME_DATA_SCHEMA_VERSION);
    let containment = runtime_containment_inspect(&bundle);

    Ok(RuntimeInspectReport {
        config_path,
        config_exists,
        workspace,
        workspace_exists,
        data_dir: bundle.context.data_dir,
        model: bundle.config.agents.defaults.model,
        provider: bundle.config.agents.defaults.provider,
        providers,
        generated_media,
        capabilities,
        sessions,
        lifecycle: RuntimeLifecycleInspect {
            binary_version: VERSION.to_owned(),
            data_schema_version: RUNTIME_DATA_SCHEMA_VERSION,
            data_schema_min_version: RUNTIME_DATA_SCHEMA_MIN_VERSION,
            compatibility,
            ownership,
            stop_request,
            update_marker,
        },
        containment,
    })
}

fn runtime_containment_inspect(bundle: &ConfigBundle) -> RuntimeContainmentInspect {
    runtime_containment_classify(RuntimeContainmentEvidence::detect(bundle))
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RuntimeContainmentEvidence {
    exec_sandbox_backend: Option<String>,
    official_package_marker: bool,
    container_markers: Vec<String>,
    unsafe_markers: Vec<String>,
}

impl RuntimeContainmentEvidence {
    fn detect(bundle: &ConfigBundle) -> Self {
        let exec_sandbox_backend = non_empty(Some(bundle.config.tools.exec.sandbox.as_str()))
            .then(|| redact_string(bundle.config.tools.exec.sandbox.trim()));
        let mut evidence = Self {
            exec_sandbox_backend,
            official_package_marker: official_runtime_package_marker_observed(),
            container_markers: runtime_container_markers(),
            unsafe_markers: runtime_unsafe_markers(),
        };
        evidence.normalize();
        evidence
    }

    fn normalize(&mut self) {
        self.container_markers =
            runtime_sorted_limited_markers(std::mem::take(&mut self.container_markers));
        self.unsafe_markers =
            runtime_sorted_limited_markers(std::mem::take(&mut self.unsafe_markers));
    }

    #[cfg(test)]
    fn from_parts(
        exec_sandbox_backend: Option<&str>,
        official_package_marker: bool,
        container_markers: &[&str],
        unsafe_markers: &[&str],
    ) -> Self {
        let mut evidence = Self {
            exec_sandbox_backend: exec_sandbox_backend.map(redact_string),
            official_package_marker,
            container_markers: container_markers
                .iter()
                .map(|marker| (*marker).to_owned())
                .collect(),
            unsafe_markers: unsafe_markers
                .iter()
                .map(|marker| (*marker).to_owned())
                .collect(),
        };
        evidence.normalize();
        evidence
    }
}

fn runtime_containment_classify(evidence: RuntimeContainmentEvidence) -> RuntimeContainmentInspect {
    let (contained, backend, summary) = if !evidence.unsafe_markers.is_empty() {
        (
            Some(false),
            Some("unsafe-privileged".to_owned()),
            format!(
                "unsafe privileged runtime evidence observed ({}); containment not trusted",
                evidence.unsafe_markers.join(", ")
            ),
        )
    } else if evidence.official_package_marker && !evidence.container_markers.is_empty() {
        (
            Some(true),
            Some(runtime_containment_backend("official-container", &evidence.exec_sandbox_backend)),
            format!(
                "official package marker and container runtime evidence observed ({}); kernel-level isolation not claimed",
                evidence.container_markers.join(", ")
            ),
        )
    } else if !evidence.container_markers.is_empty() {
        (
            Some(true),
            Some(runtime_containment_backend("container", &evidence.exec_sandbox_backend)),
            format!(
                "recognized container runtime evidence observed ({}); kernel-level isolation not claimed",
                evidence.container_markers.join(", ")
            ),
        )
    } else if evidence.official_package_marker {
        (
            None,
            Some(runtime_containment_backend("official-package", &evidence.exec_sandbox_backend)),
            "official package marker observed without container runtime evidence; containment not observed".to_owned(),
        )
    } else if let Some(exec_sandbox_backend) = &evidence.exec_sandbox_backend {
        (
            None,
            Some(exec_sandbox_backend.clone()),
            "exec sandbox backend configured as optional hardening; runtime containment not observed".to_owned(),
        )
    } else {
        (
            None,
            None,
            "native runtime; containment not observed".to_owned(),
        )
    };

    let mut inspect = RuntimeContainmentInspect {
        contained,
        backend,
        summary: Some(summary),
        digest: None,
    };
    inspect.digest = Some(runtime_containment_digest(&inspect));
    inspect
}

fn runtime_containment_backend(kind: &str, exec_sandbox_backend: &Option<String>) -> String {
    match exec_sandbox_backend {
        Some(exec_sandbox_backend) => format!("{kind}+{exec_sandbox_backend}"),
        None => kind.to_owned(),
    }
}

fn official_runtime_package_marker_observed() -> bool {
    std::env::var("SHACS_RUNTIME_PACKAGE")
        .map(|value| value.trim() == "shacs-bot-official-container")
        .unwrap_or(false)
}

fn runtime_container_markers() -> Vec<String> {
    let mut markers = Vec::new();
    if Path::new("/.dockerenv").exists() {
        markers.push("dockerenv".to_owned());
    }
    if std::env::var("container").is_ok_and(|value| non_empty(Some(value.as_str()))) {
        markers.push("container-env".to_owned());
    }
    if runtime_proc_file_has_any_marker(
        "/proc/1/cgroup",
        &["docker", "containerd", "kubepods", "podman", "lxc"],
    ) || runtime_proc_file_has_any_marker(
        "/proc/self/cgroup",
        &["docker", "containerd", "kubepods", "podman", "lxc"],
    ) {
        markers.push("cgroup".to_owned());
    }
    runtime_sorted_limited_markers(markers)
}

fn runtime_unsafe_markers() -> Vec<String> {
    let mut markers = Vec::new();
    if runtime_privileged_env_observed() {
        markers.push("privileged-env".to_owned());
    }
    let container_observed = Path::new("/.dockerenv").exists()
        || std::env::var("container").is_ok_and(|value| non_empty(Some(value.as_str())))
        || runtime_proc_file_has_any_marker(
            "/proc/1/cgroup",
            &["docker", "containerd", "kubepods", "podman", "lxc"],
        );
    if container_observed && Path::new("/var/run/docker.sock").exists() {
        markers.push("docker-socket".to_owned());
    }
    if container_observed && runtime_effective_capability_enabled("/proc/self/status", 21) {
        markers.push("cap-sys-admin".to_owned());
    }
    runtime_sorted_limited_markers(markers)
}

fn runtime_privileged_env_observed() -> bool {
    ["SHACS_RUNTIME_PRIVILEGED", "SHACS_CONTAINER_PRIVILEGED"]
        .iter()
        .any(|name| {
            std::env::var(name)
                .is_ok_and(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        })
}

fn runtime_proc_file_has_any_marker(path: &str, markers: &[&str]) -> bool {
    let Ok(text) = runtime_read_bounded_file(path) else {
        return false;
    };
    let lower = text.to_ascii_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

fn runtime_effective_capability_enabled(path: &str, capability_bit: u32) -> bool {
    let Ok(text) = runtime_read_bounded_file(path) else {
        return false;
    };
    text.lines().any(|line| {
        line.strip_prefix("CapEff:")
            .and_then(|value| u64::from_str_radix(value.trim(), 16).ok())
            .map(|mask| mask & (1_u64 << capability_bit) != 0)
            .unwrap_or(false)
    })
}

fn runtime_read_bounded_file(path: &str) -> io::Result<String> {
    let file = fs::File::open(path)?;
    let mut text = String::new();
    file.take(4096).read_to_string(&mut text)?;
    Ok(text)
}

fn runtime_sorted_limited_markers(markers: Vec<String>) -> Vec<String> {
    markers
        .into_iter()
        .map(|marker| marker.chars().take(48).collect::<String>())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(8)
        .collect()
}

fn runtime_containment_snapshot_ref(inspect: &RuntimeContainmentInspect) -> ContainmentSnapshotRef {
    ContainmentSnapshotRef {
        contained: inspect.contained,
        digest: inspect.digest.clone(),
        summary: inspect.summary.clone(),
    }
}

fn runtime_containment_precondition_met(inspect: &RuntimeContainmentInspect) -> bool {
    inspect.contained == Some(true)
}

fn agent_loop_permission_config_snapshot(
    bundle: &ConfigBundle,
    containment: &RuntimeContainmentInspect,
) -> PermissionConfigSnapshot {
    bundle.config.permissions.normalized_snapshot(
        PermissionModeSource::UserLocalConfig,
        PermissionActivationContext {
            user_local_auto_opt_in: false,
            containment_precondition_met: runtime_containment_precondition_met(containment),
        },
    )
}

fn runtime_permission_mode_snapshot(snapshot: &PermissionConfigSnapshot) -> PermissionModeSnapshot {
    PermissionModeSnapshot {
        mode: snapshot.mode,
        source: Some(permission_mode_source_name(snapshot.source).to_owned()),
        scope_ref: None,
    }
}

fn permission_mode_source_name(source: PermissionModeSource) -> &'static str {
    match source {
        PermissionModeSource::UserLocalConfig => "user_local_config",
        PermissionModeSource::WorkspaceConfig => "workspace_config",
        PermissionModeSource::CliFlag => "cli_flag",
        PermissionModeSource::LocalApiRequest => "local_api_request",
        PermissionModeSource::SessionCommand => "session_command",
        PermissionModeSource::DefaultFallback => "default_fallback",
    }
}

fn runtime_containment_digest(inspect: &RuntimeContainmentInspect) -> String {
    let payload = json!({
        "contained": inspect.contained,
        "backend": &inspect.backend,
        "summary": &inspect.summary,
    });
    let mut hasher = Sha256::new();
    hasher.update(payload.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn inspect_generated_media(
    media_dir: &Path,
) -> Result<Vec<GeneratedMediaArtifactInspect>, CliError> {
    if !media_dir.exists() {
        return Ok(Vec::new());
    }
    let canonical_media_dir = fs::canonicalize(media_dir)?;
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(&canonical_media_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let canonical_path = fs::canonicalize(&path)?;
        if !canonical_path.starts_with(&canonical_media_dir) {
            continue;
        }
        let Ok(value) = fs::read_to_string(&canonical_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .ok_or(())
        else {
            continue;
        };
        let revised_redacted = value
            .get("revisedPrompt")
            .and_then(Value::as_object)
            .and_then(|object| object.get("redacted"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        artifacts.push(GeneratedMediaArtifactInspect {
            artifact_id: json_string_field(&value, "artifactId"),
            media_ref: json_string_field(&value, "mediaRef"),
            metadata_ref: format!(
                "media/image-generation/{}",
                canonical_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
            ),
            mime_type: json_string_field(&value, "mimeType"),
            byte_len: value.get("byteLen").and_then(Value::as_u64).unwrap_or(0),
            sha256: json_string_field(&value, "sha256"),
            provider_id: json_string_field(&value, "providerId"),
            model_id: json_string_field(&value, "modelId"),
            created_at: json_string_field(&value, "createdAt"),
            redacted: revised_redacted,
        });
    }
    artifacts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    Ok(artifacts)
}

fn json_string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub fn runtime_diagnostics(
    options: RuntimeDiagnosticsOptions,
) -> Result<RuntimeDiagnosticsReport, CliError> {
    let inspect = runtime_inspect_inner(
        RuntimeInspectOptions {
            config_path: options.config_path,
            workspace_override: options.workspace_override,
        },
        false,
    )?;
    let mut snapshot = diagnostics_snapshot_from_runtime_inspect(&inspect);
    let (bundle, bundle_path, bundle_error) = match options.bundle_path {
        Some(path) => match write_diagnostics_bundle(&path, &snapshot) {
            Ok(outcome) => (Some(outcome.manifest), Some(outcome.path), None),
            Err(error) => {
                let message = format!("diagnostics bundle could not be written: {error}");
                let mut record = DiagnosticsRecord::new(
                    DiagnosticsSeverity::Warning,
                    DiagnosticsKind::Runtime,
                    "diagnostics bundle generation failed",
                );
                record.detail = json!({
                    "path": redact_string(&path.to_string_lossy()),
                    "error": redact_string(&message),
                });
                snapshot.diagnostics.push(record);
                (None, Some(path), Some(message))
            }
        },
        None => (None, None, None),
    };
    Ok(RuntimeDiagnosticsReport {
        snapshot,
        bundle,
        bundle_path,
        bundle_error,
    })
}

fn diagnostics_snapshot_from_runtime_inspect(report: &RuntimeInspectReport) -> DiagnosticsSnapshot {
    let providers = report
        .providers
        .iter()
        .map(|provider| {
            json!({
                "name": provider.name,
                "api_key_configured": provider.has_api_key,
                "api_base_configured": provider.has_api_base,
            })
        })
        .collect::<Vec<_>>();
    let capabilities = report
        .capabilities
        .iter()
        .map(|capability| {
            json!({
                "component": capability.component,
                "status": runtime_capability_label(&capability.status),
                "reason": capability.reason,
            })
        })
        .collect::<Vec<_>>();
    let generated_media = report
        .generated_media
        .iter()
        .map(|artifact| {
            json!({
                "artifact_id": artifact.artifact_id,
                "media_ref": artifact.media_ref,
                "metadata_ref": artifact.metadata_ref,
                "mime_type": artifact.mime_type,
                "byte_len": artifact.byte_len,
                "sha256": artifact.sha256,
                "provider_id": artifact.provider_id,
                "model_id": artifact.model_id,
                "created_at": artifact.created_at,
                "redacted": artifact.redacted,
            })
        })
        .collect::<Vec<_>>();
    let mut diagnostics = vec![DiagnosticsRecord::new(
        DiagnosticsSeverity::Info,
        DiagnosticsKind::Runtime,
        "local runtime diagnostics snapshot generated",
    )];
    if !report.config_exists {
        diagnostics.push(DiagnosticsRecord::new(
            DiagnosticsSeverity::Warning,
            DiagnosticsKind::Configuration,
            "config file is missing",
        ));
    }
    if !report.workspace_exists {
        diagnostics.push(DiagnosticsRecord::new(
            DiagnosticsSeverity::Warning,
            DiagnosticsKind::Runtime,
            "workspace directory is missing",
        ));
    }
    if report.lifecycle.update_marker.is_some() {
        diagnostics.push(DiagnosticsRecord::new(
            DiagnosticsSeverity::Info,
            DiagnosticsKind::Recovery,
            "runtime update marker is present",
        ));
    }
    let mut crash_evidence = Vec::new();
    if !report.config_exists {
        crash_evidence.push(CrashEvidence {
            timestamp_ms: shacs_utils::diagnostics::current_time_ms(),
            summary: "config file is missing during diagnostics inspection".to_owned(),
            correlation: Default::default(),
            fields: json!({ "path": report.config_path }),
        });
    }
    if !report.workspace_exists {
        crash_evidence.push(CrashEvidence {
            timestamp_ms: shacs_utils::diagnostics::current_time_ms(),
            summary: "workspace directory is missing during diagnostics inspection".to_owned(),
            correlation: Default::default(),
            fields: json!({ "path": report.workspace }),
        });
    }
    let recovery_evidence = report
        .lifecycle
        .update_marker
        .as_ref()
        .map(|marker| RecoveryEvidence {
            timestamp_ms: shacs_utils::diagnostics::current_time_ms(),
            status: match marker.phase.as_str() {
                "completed_cleanup" => TraceStatus::Ok,
                "partial_migration" => TraceStatus::Error,
                _ => TraceStatus::Waiting,
            },
            summary: format!("runtime update marker phase `{}` observed", marker.phase),
            correlation: Default::default(),
            fields: json!({
                "phase": marker.phase,
                "from_version": marker.from_version,
                "target_version": marker.target_version,
                "migration_required": marker.migration_required,
                "completed_at_ms": marker.completed_at_ms,
            }),
        })
        .into_iter()
        .collect::<Vec<_>>();
    let provider_progress = report
        .providers
        .iter()
        .map(|provider| {
            json!({
                "name": provider.name,
                "api_key_configured": provider.has_api_key,
                "api_base_configured": provider.has_api_base,
                "source": "runtime diagnostics provider snapshot",
            })
        })
        .collect::<Vec<_>>();
    let tool_progress = report
        .capabilities
        .iter()
        .map(|capability| {
            json!({
                "component": capability.component,
                "status": runtime_capability_label(&capability.status),
                "reason": capability.reason,
                "source": "runtime capability snapshot",
            })
        })
        .collect::<Vec<_>>();
    let update_marker = report
        .lifecycle
        .update_marker
        .as_ref()
        .map(|marker| {
            json!({
                "phase": marker.phase,
                "from_version": marker.from_version,
                "target_version": marker.target_version,
                "migration_required": marker.migration_required,
                "completed_at_ms": marker.completed_at_ms,
            })
        })
        .unwrap_or(Value::Null);
    let ownership = json!({
        "state": report.lifecycle.ownership.state.as_str(),
        "reason": report.lifecycle.ownership.reason,
        "marker": report.lifecycle.ownership.marker.as_ref().map(|marker| json!({
            "pid": marker.pid,
            "started_at_ms": marker.started_at_ms,
            "updated_at_ms": marker.updated_at_ms,
            "binary_version": marker.binary_version,
            "data_schema_version": marker.data_schema_version,
            "mode": marker.mode,
            "config_path": marker.config_path,
            "workspace": marker.workspace,
        })).unwrap_or(Value::Null),
    });
    DiagnosticsSnapshot {
        generated_at_ms: shacs_utils::diagnostics::current_time_ms(),
        runtime: json!({
            "config": {
                "path": report.config_path,
                "exists": report.config_exists,
            },
            "workspace": {
                "path": report.workspace,
                "exists": report.workspace_exists,
            },
            "data_dir": report.data_dir,
            "defaults": {
                "provider": report.provider,
                "model": report.model,
            },
            "providers": providers,
            "capabilities": capabilities,
            "containment": {
                "contained": report.containment.contained,
                "backend": &report.containment.backend,
                "summary": &report.containment.summary,
                "digest": &report.containment.digest,
            },
            "generated_media": generated_media,
            "sessions": {
                "count": report.sessions.count,
                "latest_key": report.sessions.latest_key,
                "latest_updated_at": report.sessions.latest_updated_at,
            },
            "lifecycle": {
                "binary_version": report.lifecycle.binary_version,
                "data_schema_version": report.lifecycle.data_schema_version,
                "data_schema_min_version": report.lifecycle.data_schema_min_version,
                "compatibility": report.lifecycle.compatibility.as_str(),
                "ownership": ownership,
                "update_marker": update_marker.clone(),
            },
            "update_marker": update_marker,
            "cron": { "status": "not_running_in_cli_snapshot" },
        }),
        operational_logs: vec![OperationalLogRecord::new(
            DiagnosticsSeverity::Info,
            DiagnosticsKind::Runtime,
            "runtime diagnostics inspected local files only",
        )],
        traces: vec![TraceRecord {
            timestamp_ms: shacs_utils::diagnostics::current_time_ms(),
            name: "runtime_diagnostics".to_owned(),
            status: TraceStatus::Ok,
            correlation: Default::default(),
            fields: json!({ "surface": "cli" }),
        }],
        diagnostics,
        crash_evidence,
        recovery_evidence,
        provider_progress,
        tool_progress,
        subagent_progress: vec![json!({
            "status": "available_when_runtime_executes_subagents",
            "source": "runtime capability snapshot"
        })],
    }
}

pub fn runtime_update(options: RuntimeUpdateOptions) -> Result<RuntimeUpdateOutcome, CliError> {
    let target_version = validate_runtime_target_version(&options.target_version)?;
    if target_version != VERSION {
        return Err(CliError::InvalidArguments(format!(
            "runtime update target version `{target_version}` must match the running binary version `{VERSION}` in this source-install workflow"
        )));
    }
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
    let _runtime_dirs = ensure_runtime_dirs(&bundle.context)?;
    if let Some(existing) = read_runtime_update_marker(&marker_path)? {
        if runtime_marker_blocks_mutation(&existing.phase) {
            return Err(CliError::InvalidArguments(format!(
                "runtime update blocked by existing {} marker; run `shacs-bot runtime recover` or inspect first",
                existing.phase
            )));
        }
    }
    guard_runtime_non_update_admission(&bundle.context.data_dir)?;

    let started = runtime_update_marker_value(&target_version, "in_progress", None);
    write_runtime_marker_atomically(&marker_path, &started)?;
    let completed_at_ms = now_millis();
    let completed =
        runtime_update_marker_value(&target_version, "completed_cleanup", Some(completed_at_ms));
    write_runtime_marker_atomically(&marker_path, &completed)?;

    Ok(RuntimeUpdateOutcome {
        config_path,
        workspace: bundle.context.workspace,
        data_dir: bundle.context.data_dir,
        marker_path,
        from_version: VERSION.to_owned(),
        target_version,
        phase: "completed_cleanup".to_owned(),
        migration_required: false,
    })
}

pub fn runtime_start(options: RuntimeStartOptions) -> Result<String, CliError> {
    run_runtime_with_mode(
        RunOptions {
            config_path: options.config_path,
            workspace_override: options.workspace_override,
            ..RunOptions::default()
        },
        "runtime-start",
    )
}

pub fn runtime_stop(options: RuntimeStopOptions) -> Result<RuntimeStopOutcome, CliError> {
    runtime_stop_or_restart(options, "stop")
}

pub fn runtime_restart(options: RuntimeStopOptions) -> Result<RuntimeStopOutcome, CliError> {
    runtime_stop_or_restart(options, "restart")
}

fn runtime_stop_or_restart(
    options: RuntimeStopOptions,
    request: &str,
) -> Result<RuntimeStopOutcome, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let _runtime_dirs = ensure_runtime_dirs(&bundle.context)?;
    let ownership = inspect_runtime_ownership(&bundle.context.data_dir, now_millis())?;
    let request_path = runtime_stop_request_marker_path(&bundle.context.data_dir);
    let (status, detail) = match ownership.state {
        RuntimeOwnershipState::Active => {
            let owner_pid = ownership.marker.as_ref().map(|marker| marker.pid);
            write_runtime_marker_atomically(
                &request_path,
                &runtime_stop_request_marker_value(request, owner_pid),
            )?;
            (
                RuntimeStopOutcomeStatus::RequestWritten,
                format!("wrote {request} request for active runtime"),
            )
        }
        RuntimeOwnershipState::Stale => (
            RuntimeStopOutcomeStatus::StaleOwnerOnly,
            "stale ownership marker exists; run `shacs-bot runtime recover`".to_owned(),
        ),
        RuntimeOwnershipState::None => (
            RuntimeStopOutcomeStatus::NoActiveOwner,
            "no active runtime owner found".to_owned(),
        ),
    };
    Ok(RuntimeStopOutcome {
        config_path,
        workspace: bundle.context.workspace,
        data_dir: bundle.context.data_dir,
        request_path,
        status,
        detail,
    })
}

pub fn runtime_recover(options: RuntimeRecoverOptions) -> Result<RuntimeRecoverOutcome, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let _runtime_dirs = ensure_runtime_dirs(&bundle.context)?;
    let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
    let ownership = inspect_runtime_ownership(&bundle.context.data_dir, now_millis())?;
    if ownership.state == RuntimeOwnershipState::Active {
        return Err(active_runtime_owner_recover_error());
    }
    let update_marker = read_runtime_update_marker(&marker_path)?;
    if let Some(marker) = update_marker.as_ref() {
        if marker.phase == "partial_migration" {
            return Err(CliError::InvalidArguments(
                "runtime recover blocked: partial migration marker requires manual inspection"
                    .to_owned(),
            ));
        }
    }
    let mut details = Vec::new();
    if let Some(marker) = update_marker {
        fs::remove_file(&marker_path)?;
        sync_parent_dir(&marker_path)?;
        details.push(format!(
            "cleared runtime update marker phase={} target={}",
            marker.phase, marker.target_version
        ));
    }
    if ownership.state == RuntimeOwnershipState::Stale
        && remove_stale_runtime_ownership_marker(&bundle.context.data_dir, now_millis())?
    {
        details.push("cleared stale runtime ownership marker".to_owned());
    }
    if details.is_empty() {
        return Ok(RuntimeRecoverOutcome {
            config_path,
            workspace: bundle.context.workspace,
            data_dir: bundle.context.data_dir,
            marker_path,
            recovered: false,
            detail: "no runtime update or stale ownership marker found".to_owned(),
        });
    }
    Ok(RuntimeRecoverOutcome {
        config_path,
        workspace: bundle.context.workspace,
        data_dir: bundle.context.data_dir,
        marker_path,
        recovered: true,
        detail: details.join("; "),
    })
}

fn runtime_update_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("update-marker.json")
}

fn guard_runtime_marker_for_mutation(data_dir: &Path) -> Result<(), CliError> {
    guard_runtime_admission(data_dir)?;
    Ok(())
}

fn guard_runtime_admission(data_dir: &Path) -> Result<(), CliError> {
    guard_runtime_update_marker_for_admission(data_dir)?;
    guard_runtime_non_update_admission(data_dir)
}

fn guard_runtime_ownership_acquire_admission(data_dir: &Path) -> Result<(), CliError> {
    guard_runtime_update_marker_for_admission(data_dir)?;
    guard_runtime_compatibility_for_admission(evaluate_runtime_compatibility(
        RUNTIME_DATA_SCHEMA_VERSION,
    ))
}

fn guard_runtime_update_marker_for_admission(data_dir: &Path) -> Result<(), CliError> {
    let marker_path = runtime_update_marker_path(data_dir);
    if let Some(marker) = read_runtime_update_marker(&marker_path)? {
        if runtime_marker_blocks_mutation(&marker.phase) || marker.migration_required {
            return Err(CliError::InvalidArguments(format!(
                "runtime mutation blocked by {} update marker; run `shacs-bot runtime inspect` or `shacs-bot runtime recover` first",
                marker.phase
            )));
        }
    }
    Ok(())
}

fn guard_runtime_non_update_admission(data_dir: &Path) -> Result<(), CliError> {
    guard_runtime_compatibility_for_admission(evaluate_runtime_compatibility(
        RUNTIME_DATA_SCHEMA_VERSION,
    ))?;
    let ownership = inspect_runtime_ownership(data_dir, now_millis())?;
    if ownership.state == RuntimeOwnershipState::Active {
        return Err(active_runtime_ownership_error(&ownership));
    }
    Ok(())
}

fn runtime_marker_blocks_mutation(phase: &str) -> bool {
    matches!(phase, "in_progress" | "partial_migration")
}

fn evaluate_runtime_compatibility(stored_schema_version: u32) -> RuntimeCompatibility {
    evaluate_runtime_compatibility_with_bounds(
        stored_schema_version,
        RUNTIME_DATA_SCHEMA_VERSION,
        RUNTIME_DATA_SCHEMA_MIN_VERSION,
    )
}

fn evaluate_runtime_compatibility_with_bounds(
    stored_schema_version: u32,
    current_schema_version: u32,
    min_schema_version: u32,
) -> RuntimeCompatibility {
    if stored_schema_version == current_schema_version {
        RuntimeCompatibility::FullyCompatible
    } else if stored_schema_version > current_schema_version {
        RuntimeCompatibility::InspectOnly
    } else if stored_schema_version >= min_schema_version {
        RuntimeCompatibility::MigrationRequired
    } else {
        RuntimeCompatibility::Incompatible
    }
}

fn guard_runtime_compatibility_for_admission(
    compatibility: RuntimeCompatibility,
) -> Result<(), CliError> {
    match compatibility {
        RuntimeCompatibility::FullyCompatible => Ok(()),
        other => Err(CliError::InvalidArguments(format!(
            "runtime mutation blocked by {} data schema compatibility",
            other.as_str()
        ))),
    }
}

fn runtime_ownership_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("ownership-marker.json")
}

fn runtime_ownership_mutation_lock_path(marker_path: &Path) -> PathBuf {
    marker_path.with_extension("json.lock")
}

struct RuntimeOwnershipMutationLock {
    path: PathBuf,
}

impl RuntimeOwnershipMutationLock {
    fn acquire(marker_path: &Path) -> Result<Self, CliError> {
        let path = runtime_ownership_mutation_lock_path(marker_path);
        let parent = path.parent().ok_or_else(|| {
            CliError::InvalidArguments(
                "runtime ownership mutation lock path has no parent directory".to_owned(),
            )
        })?;
        fs::create_dir_all(parent)?;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_file) => Ok(Self { path }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(
                CliError::InvalidArguments(RUNTIME_OWNERSHIP_MUTATION_LOCK_ERROR.to_owned()),
            ),
            Err(error) => Err(CliError::Io(error)),
        }
    }
}

impl Drop for RuntimeOwnershipMutationLock {
    fn drop(&mut self) {
        let _remove_result = fs::remove_file(&self.path);
        let _sync_result = sync_parent_dir(&self.path);
    }
}

fn runtime_stop_request_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("stop-request.json")
}

fn runtime_ownership_marker_value(
    pid: u32,
    started_at_ms: u64,
    updated_at_ms: u64,
    mode: &str,
    config_path: &Path,
    workspace: &Path,
) -> Value {
    json!({
        "pid": pid,
        "startedAtMs": started_at_ms,
        "updatedAtMs": updated_at_ms,
        "binaryVersion": VERSION,
        "dataSchemaVersion": RUNTIME_DATA_SCHEMA_VERSION,
        "mode": mode,
        "configPath": config_path.to_string_lossy(),
        "workspace": workspace.to_string_lossy(),
    })
}

fn read_runtime_ownership_marker(path: &Path) -> Result<Option<RuntimeOwnershipMarker>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?).map_err(|error| {
        CliError::InvalidArguments(format!("invalid runtime ownership marker: {error}"))
    })?;
    let pid = value
        .get("pid")
        .and_then(Value::as_u64)
        .filter(|pid| *pid <= u32::MAX as u64)
        .ok_or_else(|| {
            CliError::InvalidArguments("runtime ownership marker missing `pid`".to_owned())
        })? as u32;
    let started_at_ms = required_marker_u64(&value, "startedAtMs")?;
    let updated_at_ms = required_marker_u64(&value, "updatedAtMs")?;
    Ok(Some(RuntimeOwnershipMarker {
        pid,
        started_at_ms,
        updated_at_ms,
        binary_version: required_marker_string(&value, "binaryVersion")?,
        data_schema_version: value
            .get("dataSchemaVersion")
            .and_then(Value::as_u64)
            .unwrap_or_default() as u32,
        mode: required_marker_string(&value, "mode")?,
        config_path: required_marker_string(&value, "configPath")?,
        workspace: required_marker_string(&value, "workspace")?,
    }))
}

fn inspect_runtime_ownership(
    data_dir: &Path,
    now_ms: u64,
) -> Result<RuntimeOwnershipStatus, CliError> {
    let marker_path = runtime_ownership_marker_path(data_dir);
    let Some(marker) = read_runtime_ownership_marker(&marker_path)? else {
        return Ok(RuntimeOwnershipStatus {
            state: RuntimeOwnershipState::None,
            marker: None,
            reason: "no ownership marker found".to_owned(),
        });
    };
    Ok(classify_runtime_ownership_marker(marker, now_ms))
}

fn classify_runtime_ownership_marker(
    marker: RuntimeOwnershipMarker,
    now_ms: u64,
) -> RuntimeOwnershipStatus {
    if !pid_is_alive(marker.pid) {
        return RuntimeOwnershipStatus {
            state: RuntimeOwnershipState::Stale,
            marker: Some(marker),
            reason: "owner pid is not alive".to_owned(),
        };
    }
    if now_ms.saturating_sub(marker.updated_at_ms) > RUNTIME_OWNERSHIP_HEARTBEAT_TTL_MS {
        return RuntimeOwnershipStatus {
            state: RuntimeOwnershipState::Stale,
            marker: Some(marker),
            reason: "ownership heartbeat is stale".to_owned(),
        };
    }
    RuntimeOwnershipStatus {
        state: RuntimeOwnershipState::Active,
        marker: Some(marker),
        reason: "owner pid and heartbeat are active".to_owned(),
    }
}

fn runtime_ownership_marker_matches(
    marker: &RuntimeOwnershipMarker,
    pid: u32,
    started_at_ms: u64,
) -> bool {
    marker.pid == pid && marker.started_at_ms == started_at_ms
}

fn active_runtime_ownership_error(ownership: &RuntimeOwnershipStatus) -> CliError {
    let pid = ownership
        .marker
        .as_ref()
        .map(|marker| marker.pid.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    CliError::InvalidArguments(format!(
        "runtime mutation blocked by active runtime ownership pid={pid}; request stop or inspect first"
    ))
}

fn active_runtime_owner_recover_error() -> CliError {
    CliError::InvalidArguments(
        "runtime recover blocked: active runtime owner must stop first".to_owned(),
    )
}

fn is_runtime_ownership_mutation_lock_error(error: &CliError) -> bool {
    matches!(
        error,
        CliError::InvalidArguments(message) if message == RUNTIME_OWNERSHIP_MUTATION_LOCK_ERROR
    )
}

fn write_runtime_marker_if_absent_or_concurrent(
    path: &Path,
    marker: &Value,
) -> Result<(), CliError> {
    write_runtime_marker_if_absent(path, marker).map_err(|error| {
        if matches!(&error, CliError::Io(io_error) if io_error.kind() == io::ErrorKind::AlreadyExists)
        {
            CliError::InvalidArguments(
                "runtime mutation blocked by concurrent runtime ownership; request stop or inspect first"
                    .to_owned(),
            )
        } else {
            error
        }
    })
}

fn acquire_runtime_ownership_marker(
    data_dir: &Path,
    path: &Path,
    marker: &Value,
) -> Result<(), CliError> {
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(path)?;
    let ownership = inspect_runtime_ownership(data_dir, now_millis())?;
    match ownership.state {
        RuntimeOwnershipState::None => {
            write_runtime_marker_if_absent_or_concurrent(path, marker)?;
        }
        RuntimeOwnershipState::Stale => {
            if path.exists() {
                fs::remove_file(path)?;
                sync_parent_dir(path)?;
            }
            write_runtime_marker_if_absent_or_concurrent(path, marker)?;
        }
        RuntimeOwnershipState::Active => return Err(active_runtime_ownership_error(&ownership)),
    }
    Ok(())
}

fn update_runtime_ownership_heartbeat_if_current(
    path: &Path,
    pid: u32,
    started_at_ms: u64,
    marker: &Value,
) -> Result<bool, CliError> {
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(path)?;
    let Some(current_marker) = read_runtime_ownership_marker(path)? else {
        return Ok(false);
    };
    if !runtime_ownership_marker_matches(&current_marker, pid, started_at_ms) {
        return Ok(false);
    }
    write_runtime_marker_atomically(path, marker)?;
    Ok(true)
}

fn remove_runtime_ownership_marker_if_current(
    path: &Path,
    pid: u32,
    started_at_ms: u64,
) -> Result<bool, CliError> {
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(path)?;
    let Some(marker) = read_runtime_ownership_marker(path)? else {
        return Ok(false);
    };
    if !runtime_ownership_marker_matches(&marker, pid, started_at_ms) {
        return Ok(false);
    }
    fs::remove_file(path)?;
    sync_parent_dir(path)?;
    Ok(true)
}

fn remove_stale_runtime_ownership_marker(data_dir: &Path, now_ms: u64) -> Result<bool, CliError> {
    let path = runtime_ownership_marker_path(data_dir);
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(&path)?;
    let ownership = inspect_runtime_ownership(data_dir, now_ms)?;
    match ownership.state {
        RuntimeOwnershipState::None => Ok(false),
        RuntimeOwnershipState::Active => Err(active_runtime_owner_recover_error()),
        RuntimeOwnershipState::Stale => {
            if path.exists() {
                fs::remove_file(&path)?;
                sync_parent_dir(&path)?;
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

fn pid_is_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

struct RuntimeOwnershipLease {
    path: PathBuf,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    pid: u32,
    started_at_ms: u64,
}

impl RuntimeOwnershipLease {
    fn acquire(bundle: &ConfigBundle, mode: &str) -> Result<Self, CliError> {
        guard_runtime_ownership_acquire_admission(&bundle.context.data_dir)?;
        let path = runtime_ownership_marker_path(&bundle.context.data_dir);
        let request_path = runtime_stop_request_marker_path(&bundle.context.data_dir);
        let pid = std::process::id();
        let started_at_ms = now_millis();
        let marker = runtime_ownership_marker_value(
            pid,
            started_at_ms,
            started_at_ms,
            mode,
            &bundle.context.config_path,
            &bundle.context.workspace,
        );
        acquire_runtime_ownership_marker(&bundle.context.data_dir, &path, &marker)?;
        let stop = Arc::new(AtomicBool::new(false));
        let mut lease = Self {
            path,
            stop,
            handle: None,
            pid,
            started_at_ms,
        };
        if request_path.exists() {
            fs::remove_file(&request_path)?;
            sync_parent_dir(&request_path)?;
        }
        let thread_stop = lease.stop.clone();
        let thread_path = lease.path.clone();
        let config_path = bundle.context.config_path.clone();
        let workspace = bundle.context.workspace.clone();
        let mode = mode.to_owned();
        let handle = thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                thread::sleep(RUNTIME_OWNERSHIP_HEARTBEAT_INTERVAL);
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let marker = runtime_ownership_marker_value(
                    pid,
                    started_at_ms,
                    now_millis(),
                    &mode,
                    &config_path,
                    &workspace,
                );
                match update_runtime_ownership_heartbeat_if_current(
                    &thread_path,
                    pid,
                    started_at_ms,
                    &marker,
                ) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(error) if is_runtime_ownership_mutation_lock_error(&error) => continue,
                    Err(_error) => break,
                }
            }
        });
        lease.handle = Some(handle);
        Ok(lease)
    }

    fn cleanup(mut self) -> Result<(), CliError> {
        self.cleanup_inner()
    }

    fn cleanup_inner(&mut self) -> Result<(), CliError> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| CliError::Api(ApiError::internal("runtime heartbeat panicked")))?;
        }
        remove_runtime_ownership_marker_if_current(&self.path, self.pid, self.started_at_ms)?;
        Ok(())
    }
}

impl Drop for RuntimeOwnershipLease {
    fn drop(&mut self) {
        let _cleanup_result = self.cleanup_inner();
    }
}

fn runtime_stop_request_marker_value(request: &str, owner_pid: Option<u32>) -> Value {
    json!({
        "request": request,
        "requestedAtMs": now_millis(),
        "ownerPid": owner_pid,
    })
}

fn read_runtime_stop_request_marker(
    path: &Path,
) -> Result<Option<RuntimeStopRequestMarker>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?).map_err(|error| {
        CliError::InvalidArguments(format!("invalid runtime stop request marker: {error}"))
    })?;
    Ok(Some(RuntimeStopRequestMarker {
        request: required_marker_string(&value, "request")?,
        requested_at_ms: required_marker_u64(&value, "requestedAtMs")?,
        owner_pid: value
            .get("ownerPid")
            .and_then(Value::as_u64)
            .filter(|pid| *pid <= u32::MAX as u64)
            .map(|pid| pid as u32),
    }))
}

fn runtime_stop_request_observed(data_dir: &Path) -> Result<Option<String>, CliError> {
    let marker_path = runtime_stop_request_marker_path(data_dir);
    Ok(read_runtime_stop_request_marker(&marker_path)?.map(|marker| marker.request))
}

fn required_marker_u64(value: &Value, key: &str) -> Result<u64, CliError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| CliError::InvalidArguments(format!("runtime marker missing `{key}`")))
}

fn validate_runtime_target_version(input: &str) -> Result<String, CliError> {
    let version = input.trim();
    if version.is_empty() {
        return Err(CliError::InvalidArguments(
            "runtime update requires --target-version".to_owned(),
        ));
    }
    if version.len() > 64
        || version
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '+')))
    {
        return Err(CliError::InvalidArguments(
            "runtime update --target-version must contain only ASCII letters, digits, '.', '-', '_', or '+' and be at most 64 characters".to_owned(),
        ));
    }
    Ok(version.to_owned())
}

fn runtime_update_marker_value(
    target_version: &str,
    phase: &str,
    completed_at_ms: Option<u64>,
) -> Value {
    let mut marker = Map::new();
    marker.insert("version".to_owned(), json!(1));
    marker.insert("phase".to_owned(), json!(phase));
    marker.insert("fromVersion".to_owned(), json!(VERSION));
    marker.insert("targetVersion".to_owned(), json!(target_version));
    marker.insert("binaryVersion".to_owned(), json!(VERSION));
    marker.insert(
        "dataSchemaVersion".to_owned(),
        json!(RUNTIME_DATA_SCHEMA_VERSION),
    );
    marker.insert("migrationRequired".to_owned(), json!(false));
    if let Some(completed_at_ms) = completed_at_ms {
        marker.insert("completedAtMs".to_owned(), json!(completed_at_ms));
    }
    Value::Object(marker)
}

fn read_runtime_update_marker(path: &Path) -> Result<Option<RuntimeUpdateMarker>, CliError> {
    if !path.exists() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&fs::read_to_string(path)?).map_err(|error| {
        CliError::InvalidArguments(format!("invalid runtime update marker: {error}"))
    })?;
    let phase = required_marker_string(&value, "phase")?;
    validate_runtime_marker_phase(&phase)?;
    let from_version = marker_string(&value, "fromVersion").unwrap_or_else(|| "unknown".to_owned());
    let target_version = required_marker_string(&value, "targetVersion")?;
    let migration_required = value
        .get("migrationRequired")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let completed_at_ms = value.get("completedAtMs").and_then(Value::as_u64);
    Ok(Some(RuntimeUpdateMarker {
        phase,
        from_version,
        target_version,
        migration_required,
        completed_at_ms,
    }))
}

fn validate_runtime_marker_phase(phase: &str) -> Result<(), CliError> {
    match phase {
        "in_progress" | "completed_cleanup" | "partial_migration" => Ok(()),
        other => Err(CliError::InvalidArguments(format!(
            "runtime update marker has unknown phase `{other}`"
        ))),
    }
}

fn required_marker_string(value: &Value, key: &str) -> Result<String, CliError> {
    marker_string(value, key)
        .ok_or_else(|| CliError::InvalidArguments(format!("runtime update marker missing `{key}`")))
}

fn marker_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn write_runtime_marker_atomically(path: &Path, value: &Value) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::InvalidArguments("runtime marker path has no parent directory".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    let temp_path = path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        serde_json::to_writer_pretty(&mut file, value).map_err(|error| {
            CliError::InvalidArguments(format!("runtime marker could not be serialized: {error}"))
        })?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _cleanup_result = fs::remove_file(&temp_path);
        let _sync_result = sync_parent_dir(path);
        return Err(error);
    }
    sync_parent_dir(path)?;
    Ok(())
}

fn write_runtime_marker_if_absent(path: &Path, value: &Value) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        CliError::InvalidArguments("runtime marker path has no parent directory".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    let temp_path = path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        serde_json::to_writer_pretty(&mut file, value).map_err(|error| {
            CliError::InvalidArguments(format!("runtime marker could not be serialized: {error}"))
        })?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::hard_link(&temp_path, path)?;
        Ok(())
    })();
    let temp_cleanup_result = fs::remove_file(&temp_path);
    match write_result {
        Ok(()) => {
            let _ = temp_cleanup_result;
            sync_parent_dir(path)?;
            Ok(())
        }
        Err(error) => {
            let _ = temp_cleanup_result;
            let _ = sync_parent_dir(path);
            Err(error)
        }
    }
}

fn sync_parent_dir(path: &Path) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
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

fn run_apps_command(command: AppsCommand) -> Result<String, CliError> {
    match command {
        AppsCommand::Init(options) => apps_init(options).map(format_apps_init),
        AppsCommand::Install(options) => apps_install(options).map(format_apps_entry_report),
        AppsCommand::List(options) => apps_list(options).map(format_apps_list),
        AppsCommand::Inspect(options) => apps_inspect(options).map(format_apps_inspect),
        AppsCommand::Enable(options) => apps_enable(options).map(format_apps_entry_report),
        AppsCommand::Disable(options) => apps_disable(options).map(format_apps_entry_report),
        AppsCommand::Uninstall(options) => apps_uninstall(options).map(format_apps_uninstall),
    }
}

fn run_plugins_command(command: PluginsCommand) -> Result<String, CliError> {
    match command {
        PluginsCommand::List(options) => plugins_list(options).map(format_plugins_list),
        PluginsCommand::Inspect(options) => plugins_inspect(options).map(format_plugins_inspect),
        PluginsCommand::Doctor(options) => plugins_doctor(options).map(format_plugins_doctor),
        PluginsCommand::Enable(options) => plugins_enable(options).map(format_plugin_mutation),
        PluginsCommand::Disable(options) => plugins_disable(options).map(format_plugin_mutation),
    }
}

fn run_hooks_command(command: HooksCommand) -> Result<String, CliError> {
    match command {
        HooksCommand::List(options) => hooks_list(options).map(format_hooks_list),
        HooksCommand::Inspect(options) => hooks_inspect(options).map(format_hooks_inspect),
    }
}

fn run_channels_command(command: ChannelsCommand) -> Result<String, CliError> {
    match command {
        ChannelsCommand::List(options) => channels_list(options).map(format_channels_list),
        ChannelsCommand::Status(options) => channels_status(options).map(format_channels_status),
    }
}

fn run_context_command(command: ContextCommand) -> Result<String, CliError> {
    match command {
        ContextCommand::Files(ContextFilesCommand::List(options)) => context_files_report(options)
            .map(|report| format_context_files_report("context files list", report)),
        ContextCommand::Files(ContextFilesCommand::Inspect(options)) => {
            context_files_report(options)
                .map(|report| format_context_files_report("context files inspect", report))
        }
        ContextCommand::Refs(ContextRefsCommand::Parse(options)) => {
            context_refs_parse(options).map(format_context_refs_parse_report)
        }
        ContextCommand::Refs(ContextRefsCommand::Resolve(options)) => {
            context_refs_resolve(options).map(format_context_refs_resolve_report)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFilesCliReport {
    pub workspace: PathBuf,
    pub summary: ContextFileDiagnosticsSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRefsParseCliReport {
    pub summary: Option<ContextReferenceDiagnosticsSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRefsResolveCliReport {
    pub workspace: PathBuf,
    pub summary: ContextDiagnosticsSummary,
}

pub fn context_files_report(
    options: ContextFilesOptions,
) -> Result<ContextFilesCliReport, CliError> {
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let discovery = discover_context_files(&workspace, ContextFileDiscoveryOptions::default());
    let summary = build_context_diagnostics_summary(ContextDiagnosticsInput {
        reference_parse: None,
        context_files: &discovery.entries,
        resolved_artifacts: &[],
        safety_report: None,
        provider_handoff: None,
    })
    .context_files;
    Ok(ContextFilesCliReport { workspace, summary })
}

pub fn context_refs_parse(
    options: ContextRefsParseOptions,
) -> Result<ContextRefsParseCliReport, CliError> {
    if options.message.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "context refs parse requires a message".to_owned(),
        ));
    }
    let parse = parse_context_references(&options.message);
    let summary = build_context_diagnostics_summary(ContextDiagnosticsInput {
        reference_parse: Some(&parse),
        context_files: &[],
        resolved_artifacts: &[],
        safety_report: None,
        provider_handoff: None,
    })
    .references;
    Ok(ContextRefsParseCliReport { summary })
}

pub fn context_refs_resolve(
    options: ContextRefsResolveOptions,
) -> Result<ContextRefsResolveCliReport, CliError> {
    if options.message.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "context refs resolve requires a message".to_owned(),
        ));
    }
    let workspace = load_session_workspace(options.config_path, options.workspace_override)?;
    let parse = parse_context_references(&options.message);
    let discovery = discover_context_files(&workspace, ContextFileDiscoveryOptions::default());
    let resolver_config = ContextReferenceResolverConfig::new(&workspace)
        .with_network_enabled(options.network_enabled);
    let resolved = parse
        .references
        .iter()
        .map(|reference| resolve_context_reference(reference, &resolver_config))
        .collect::<Vec<_>>();
    let safety = apply_context_safety_gate(&resolved);
    let handoff = build_context_provider_handoff(
        &safety.artifacts,
        &discovery.entries,
        ContextBudgetInput::default(),
    );
    let summary = build_context_diagnostics_summary(ContextDiagnosticsInput {
        reference_parse: Some(&parse),
        context_files: &discovery.entries,
        resolved_artifacts: &safety.artifacts,
        safety_report: Some(&safety),
        provider_handoff: Some(&handoff),
    });
    Ok(ContextRefsResolveCliReport { workspace, summary })
}

pub fn channels_list(options: ChannelsListOptions) -> Result<ChannelsReport, CliError> {
    load_channels_report(options.config_path, options.workspace_override)
}

pub fn channels_status(options: ChannelsStatusOptions) -> Result<ChannelsReport, CliError> {
    load_channels_report(options.config_path, options.workspace_override)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsEntryReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub registry_path: PathBuf,
    pub entry: AppRegistryEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsListReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub registry_path: PathBuf,
    pub entries: Vec<AppRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsUninstallReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub registry_path: PathBuf,
    pub app_id: String,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsInitReport {
    pub config_path: PathBuf,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub authoring: AppAuthoringInitReport,
}

pub fn apps_init(options: AppsInitOptions) -> Result<AppsInitReport, CliError> {
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let data_dir = bundle.context.data_dir;
    let authoring = AppAuthoringStore::new(&data_dir).init_app(options.app_id)?;
    Ok(AppsInitReport {
        config_path,
        workspace: bundle.context.workspace,
        data_dir,
        authoring,
    })
}

pub fn apps_install(options: AppsInstallOptions) -> Result<AppsEntryReport, CliError> {
    let (config_path, workspace, store) =
        apps_store(options.config_path, options.workspace_override)?;
    let entry = store.install_local_bundle(options.bundle_path)?;
    Ok(AppsEntryReport {
        config_path,
        workspace,
        registry_path: store.registry_path(),
        entry,
    })
}

pub fn apps_list(options: AppsListOptions) -> Result<AppsListReport, CliError> {
    let (config_path, workspace, store) =
        apps_store(options.config_path, options.workspace_override)?;
    Ok(AppsListReport {
        config_path,
        workspace,
        registry_path: store.registry_path(),
        entries: store.list()?,
    })
}

pub fn apps_inspect(options: AppsInspectOptions) -> Result<AppsEntryReport, CliError> {
    let app_id = AppId::parse(options.app_id)?;
    let (config_path, workspace, store) =
        apps_store(options.config_path, options.workspace_override)?;
    let entry = store
        .inspect(&app_id)?
        .ok_or_else(|| AppError::UnknownApp(app_id.clone()))?;
    Ok(AppsEntryReport {
        config_path,
        workspace,
        registry_path: store.registry_path(),
        entry,
    })
}

pub fn apps_enable(options: AppsIdOptions) -> Result<AppsEntryReport, CliError> {
    let app_id = AppId::parse(options.app_id)?;
    let (config_path, workspace, store) =
        apps_store(options.config_path, options.workspace_override)?;
    let entry = store.enable(&app_id)?;
    Ok(AppsEntryReport {
        config_path,
        workspace,
        registry_path: store.registry_path(),
        entry,
    })
}

pub fn apps_disable(options: AppsIdOptions) -> Result<AppsEntryReport, CliError> {
    let app_id = AppId::parse(options.app_id)?;
    let (config_path, workspace, store) =
        apps_store(options.config_path, options.workspace_override)?;
    let entry = store.disable(&app_id)?;
    Ok(AppsEntryReport {
        config_path,
        workspace,
        registry_path: store.registry_path(),
        entry,
    })
}

pub fn apps_uninstall(options: AppsIdOptions) -> Result<AppsUninstallReport, CliError> {
    let app_id = AppId::parse(options.app_id.clone())?;
    let (config_path, workspace, store) =
        apps_store(options.config_path, options.workspace_override)?;
    let removed = store.uninstall_local_app(&app_id)?.is_some();
    Ok(AppsUninstallReport {
        config_path,
        workspace,
        registry_path: store.registry_path(),
        app_id: options.app_id,
        removed,
    })
}

fn apps_store(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Result<(PathBuf, PathBuf, AppRegistryStore), CliError> {
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
    Ok((
        config_path,
        bundle.context.workspace,
        AppRegistryStore::new(bundle.context.data_dir),
    ))
}

pub fn plugins_list(options: PluginsListOptions) -> Result<PluginProjectionReport, CliError> {
    load_plugin_projection(options.config_path, options.workspace_override)
}

pub fn plugins_doctor(options: PluginsListOptions) -> Result<PluginProjectionReport, CliError> {
    load_plugin_projection(options.config_path, options.workspace_override)
}

pub fn plugins_inspect(options: PluginsInspectOptions) -> Result<PluginInspectReport, CliError> {
    if options.name.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "plugins inspect requires a plugin name".to_owned(),
        ));
    }
    let report = load_plugin_projection(options.config_path, options.workspace_override)?;
    let plugin = find_discovered_plugin(&report.plugins, &options.name)
        .cloned()
        .ok_or_else(|| CliError::InvalidArguments(format!("unknown plugin `{}`", options.name)))?;
    Ok(PluginInspectReport {
        config_path: report.config_path,
        workspace: report.workspace,
        plugin,
        projection: report.projection,
    })
}

pub fn plugins_enable(options: PluginsMutateOptions) -> Result<PluginMutationReport, CliError> {
    mutate_plugin_config(options, PluginMutationAction::Enabled)
}

pub fn plugins_disable(options: PluginsMutateOptions) -> Result<PluginMutationReport, CliError> {
    mutate_plugin_config(options, PluginMutationAction::Disabled)
}

pub fn hooks_list(options: HooksListOptions) -> Result<HookProjectionReport, CliError> {
    let report = load_plugin_projection(options.config_path, options.workspace_override)?;
    Ok(HookProjectionReport {
        config_path: report.config_path,
        workspace: report.workspace,
        catalog: plugin_hook_catalog(),
        plugin_hooks: report.projection.hooks,
    })
}

pub fn hooks_inspect(options: HooksInspectOptions) -> Result<HookInspectReport, CliError> {
    if options.filter.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "hooks inspect requires a filter".to_owned(),
        ));
    }
    let report = load_plugin_projection(options.config_path, options.workspace_override)?;
    let filter = options.filter.trim().to_owned();
    let plugin_hooks = report
        .projection
        .hooks
        .into_iter()
        .filter(|hook| hook.plugin_id == filter || hook.event == filter)
        .collect::<Vec<_>>();
    let catalog = PluginHookCatalog {
        entries: plugin_hook_catalog()
            .entries
            .into_iter()
            .filter(|entry| entry.event.as_str() == filter)
            .collect(),
    };
    Ok(HookInspectReport {
        config_path: report.config_path,
        workspace: report.workspace,
        filter,
        catalog,
        plugin_hooks,
    })
}

fn load_plugin_projection(
    config_path: Option<PathBuf>,
    workspace_override: Option<PathBuf>,
) -> Result<PluginProjectionReport, CliError> {
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
    let discovery = discover_plugins(&bundle.config, &bundle.context, &ProcessEnv)?;
    let projection = build_plugin_surface_projection(&discovery.plugins);
    Ok(PluginProjectionReport {
        config_path,
        workspace: bundle.context.workspace,
        data_dir: bundle.context.data_dir,
        plugins: discovery.plugins,
        projection,
    })
}

fn mutate_plugin_config(
    options: PluginsMutateOptions,
    action: PluginMutationAction,
) -> Result<PluginMutationReport, CliError> {
    if options.name.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "plugin enable/disable requires a plugin name".to_owned(),
        ));
    }
    let name = options.name.trim().to_owned();
    let config_path = options.config_path.unwrap_or_else(default_config_path);
    let mut bundle = load_config_with_env(
        LoadOptions {
            config_path: Some(config_path.clone()),
            workspace_override: options.workspace_override,
            resolve_env: false,
            write_back_migrations: false,
        },
        &ProcessEnv,
    )?;
    let discovery = discover_plugins(&bundle.config, &bundle.context, &ProcessEnv)?;
    let discovered = find_discovered_plugin(&discovery.plugins, &name).is_some();
    let configured = plugin_config_contains(&bundle.config.plugins.enabled, &name)
        || plugin_config_contains(&bundle.config.plugins.disabled, &name);
    if !(discovered || action == PluginMutationAction::Disabled && configured) {
        return Err(CliError::InvalidArguments(format!(
            "unknown plugin `{name}`"
        )));
    }

    match action {
        PluginMutationAction::Enabled => {
            let mut candidate = bundle.config.clone();
            remove_string_value(&mut candidate.plugins.disabled, &name);
            push_unique_string(&mut candidate.plugins.enabled, name.clone());
            let candidate_discovery = discover_plugins(&candidate, &bundle.context, &ProcessEnv)?;
            let candidate_plugin = find_discovered_plugin(&candidate_discovery.plugins, &name)
                .ok_or_else(|| CliError::InvalidArguments(format!("unknown plugin `{name}`")))?;
            if candidate_plugin.state == PluginState::Blocked {
                return Err(CliError::InvalidArguments(format!(
                    "plugin `{name}` is blocked and was not enabled: {}",
                    plugin_block_summary(candidate_plugin)
                )));
            }
            let was_enabled = bundle
                .config
                .plugins
                .enabled
                .iter()
                .any(|value| value == &name)
                && !bundle
                    .config
                    .plugins
                    .disabled
                    .iter()
                    .any(|value| value == &name);
            remove_string_value(&mut bundle.config.plugins.disabled, &name);
            push_unique_string(&mut bundle.config.plugins.enabled, name.clone());
            patch_plugin_gate_config(&config_path, &name, PluginGateMutation::EnableDiscovered)?;
            Ok(plugin_mutation_report(
                config_path,
                bundle.context.workspace,
                name,
                action,
                !was_enabled,
            ))
        }
        PluginMutationAction::Disabled => {
            let was_disabled = bundle
                .config
                .plugins
                .disabled
                .iter()
                .any(|value| value == &name);
            remove_string_value(&mut bundle.config.plugins.enabled, &name);
            if discovered {
                push_unique_string(&mut bundle.config.plugins.disabled, name.clone());
                patch_plugin_gate_config(
                    &config_path,
                    &name,
                    PluginGateMutation::DisableDiscovered,
                )?;
            } else {
                remove_string_value(&mut bundle.config.plugins.disabled, &name);
                patch_plugin_gate_config(&config_path, &name, PluginGateMutation::RemoveStale)?;
            }
            Ok(plugin_mutation_report(
                config_path,
                bundle.context.workspace,
                name,
                action,
                if discovered {
                    !was_disabled
                } else {
                    configured
                },
            ))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginGateMutation {
    EnableDiscovered,
    DisableDiscovered,
    RemoveStale,
}

fn patch_plugin_gate_config(
    path: &Path,
    name: &str,
    mutation: PluginGateMutation,
) -> Result<(), CliError> {
    let mut value = read_config_value_for_patch(path)?;
    let root = value.as_object_mut().ok_or_else(|| {
        CliError::InvalidArguments("config root must be a JSON object".to_owned())
    })?;
    let plugins = root
        .entry("plugins".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let plugins = plugins.as_object_mut().ok_or_else(|| {
        CliError::InvalidArguments("config `plugins` must be a JSON object".to_owned())
    })?;

    let mut enabled = plugin_gate_array(plugins.remove("enabled"));
    let mut disabled = plugin_gate_array(plugins.remove("disabled"));
    match mutation {
        PluginGateMutation::EnableDiscovered => {
            remove_string_value(&mut disabled, name);
            push_unique_string(&mut enabled, name.to_owned());
        }
        PluginGateMutation::DisableDiscovered => {
            remove_string_value(&mut enabled, name);
            push_unique_string(&mut disabled, name.to_owned());
        }
        PluginGateMutation::RemoveStale => {
            remove_string_value(&mut enabled, name);
            remove_string_value(&mut disabled, name);
        }
    }

    if enabled.is_empty() {
        plugins.remove("enabled");
    } else {
        plugins.insert("enabled".to_owned(), json!(enabled));
    }
    if disabled.is_empty() {
        plugins.remove("disabled");
    } else {
        plugins.insert("disabled".to_owned(), json!(disabled));
    }
    if plugins.is_empty() {
        root.remove("plugins");
    }
    write_config_value_for_patch(path, &value)?;
    Ok(())
}

fn read_config_value_for_patch(path: &Path) -> Result<Value, CliError> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| CliError::InvalidArguments(format!("invalid config JSON: {error}"))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
        Err(error) => Err(error.into()),
    }
}

fn write_config_value_for_patch(path: &Path, value: &Value) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| CliError::InvalidArguments(format!("invalid config JSON: {error}")))?;
    fs::write(path, format!("{text}\n"))?;
    Ok(())
}

fn plugin_gate_array(value: Option<Value>) -> Vec<String> {
    value
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn find_discovered_plugin<'a>(
    plugins: &'a [DiscoveredPlugin],
    name: &str,
) -> Option<&'a DiscoveredPlugin> {
    plugins.iter().find(|plugin| plugin.id == name)
}

fn plugin_config_contains(values: &[String], name: &str) -> bool {
    values.iter().any(|value| value == name)
}

fn remove_string_value(values: &mut Vec<String>, name: &str) {
    values.retain(|value| value != name);
}

fn push_unique_string(values: &mut Vec<String>, name: String) {
    if !values.iter().any(|value| value == &name) {
        values.push(name);
    }
    values.sort();
}

fn plugin_mutation_report(
    config_path: PathBuf,
    workspace: PathBuf,
    plugin_name: String,
    action: PluginMutationAction,
    changed: bool,
) -> PluginMutationReport {
    PluginMutationReport {
        config_path,
        workspace,
        plugin_name,
        action,
        changed,
        next_session_notice:
            "Changes apply on the next session or runtime reload; no plugin code was executed."
                .to_owned(),
    }
}

fn plugin_block_summary(plugin: &DiscoveredPlugin) -> String {
    if !plugin.block_reasons.is_empty() {
        return plugin
            .block_reasons
            .iter()
            .map(|reason| reason.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    if !plugin.diagnostics.is_empty() {
        return plugin.diagnostics.join("; ");
    }
    "blocked".to_owned()
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
            let runtime_status = channel_runtime_status(
                &bundle.config.channels.plugins,
                &workers,
                enabled,
                descriptor.enabled_by_default,
            );
            ChannelReportItem {
                descriptor,
                configured,
                enabled,
                runtime_status,
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
        send_memory_hints: bundle.config.channels.send_memory_hints,
        send_max_retries: bundle.config.channels.send_max_retries,
    })
}

fn channel_runtime_status(
    plugins: &BTreeMap<String, Value>,
    workers: &[LiveChannelWorkerDescriptor],
    enabled: bool,
    default_enabled: bool,
) -> String {
    if !enabled {
        return "disabled".to_owned();
    }
    if workers.is_empty() {
        return "no-worker".to_owned();
    }
    let mut labels = Vec::new();
    for worker in workers {
        let status = if worker.channel == WEBSOCKET_CHANNEL {
            if channel_enabled_from_plugins(plugins, WEBSOCKET_CHANNEL, default_enabled) {
                "ready"
            } else {
                "disabled"
            }
        } else if !worker.requires_external_credentials {
            "ready"
        } else {
            match worker_config_state(plugins, worker) {
                WorkerConfigState::Ready => "ready",
                WorkerConfigState::MissingCredentials => "missing-credentials",
                WorkerConfigState::Unsupported(_) => "unsupported-config",
            }
        };
        labels.push((worker.label.clone(), status.to_owned()));
    }
    if labels.len() == 1 {
        labels
            .pop()
            .map(|(_, status)| status)
            .unwrap_or_else(|| "no-worker".to_owned())
    } else {
        labels
            .into_iter()
            .map(|(label, status)| format!("{label}={status}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
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
    if descriptor.requires_external_credentials {
        match worker_config_state(plugins, descriptor) {
            WorkerConfigState::Ready => {}
            WorkerConfigState::MissingCredentials => {
                return (
                    ChannelRuntimeWorkerState::SkippedMissingCredentials,
                    "missing channel credentials/config".to_owned(),
                );
            }
            WorkerConfigState::Unsupported(detail) => {
                return (ChannelRuntimeWorkerState::SkippedUnsupported, detail);
            }
        }
    }
    (
        ChannelRuntimeWorkerState::Started,
        "worker eligible for startup".to_owned(),
    )
}

enum WorkerConfigState {
    Ready,
    MissingCredentials,
    Unsupported(String),
}

fn worker_config_state(
    plugins: &BTreeMap<String, Value>,
    descriptor: &LiveChannelWorkerDescriptor,
) -> WorkerConfigState {
    match descriptor.kind {
        LiveChannelWorkerKind::TelegramLongPolling => {
            if telegram_runtime_config(plugins).is_some() {
                WorkerConfigState::Ready
            } else {
                WorkerConfigState::MissingCredentials
            }
        }
        LiveChannelWorkerKind::DiscordGateway => discord_worker_config_state(plugins),
        LiveChannelWorkerKind::SlackSocketMode => slack_worker_config_state(plugins),
        LiveChannelWorkerKind::EmailSmtp | LiveChannelWorkerKind::EmailImap => {
            email_worker_config_state(plugins, descriptor.kind.clone())
        }
        LiveChannelWorkerKind::WhatsAppBridge => {
            if whatsapp_runtime_config(plugins).is_some() {
                WorkerConfigState::Ready
            } else {
                WorkerConfigState::MissingCredentials
            }
        }
        LiveChannelWorkerKind::WebSocketServer => WorkerConfigState::Ready,
    }
}

fn discord_worker_config_state(plugins: &BTreeMap<String, Value>) -> WorkerConfigState {
    if discord_runtime_config(plugins).is_some() {
        return WorkerConfigState::Ready;
    }
    let Some(object) = plugin_object(plugins, DISCORD_CHANNEL) else {
        return WorkerConfigState::MissingCredentials;
    };
    let _ = object;
    WorkerConfigState::MissingCredentials
}

fn slack_worker_config_state(plugins: &BTreeMap<String, Value>) -> WorkerConfigState {
    if slack_runtime_config(plugins).is_some() {
        return WorkerConfigState::Ready;
    }
    let Some(object) = plugin_object(plugins, SLACK_CHANNEL) else {
        return WorkerConfigState::MissingCredentials;
    };
    let has_bot_token = plugin_string_alias(object, &["botToken", "bot_token", "token"]).is_some();
    let has_app_token = plugin_string_alias(object, &["appToken", "app_token"]).is_some();
    if has_bot_token || has_app_token {
        WorkerConfigState::Unsupported(
            "Slack Socket Mode requires both appToken/app_token and botToken/bot_token/token"
                .to_owned(),
        )
    } else {
        WorkerConfigState::MissingCredentials
    }
}

fn email_worker_config_state(
    plugins: &BTreeMap<String, Value>,
    kind: LiveChannelWorkerKind,
) -> WorkerConfigState {
    let Some(object) = plugin_object(plugins, EMAIL_CHANNEL) else {
        return WorkerConfigState::MissingCredentials;
    };
    if !plugin_bool_alias(object, &["consentGranted", "consent_granted"]).unwrap_or(false) {
        return WorkerConfigState::Unsupported(
            "Email channel requires consentGranted=true before SMTP/IMAP startup".to_owned(),
        );
    }
    match kind {
        LiveChannelWorkerKind::EmailSmtp => {
            if email_smtp_runtime_config(object).is_some() {
                WorkerConfigState::Ready
            } else {
                WorkerConfigState::MissingCredentials
            }
        }
        LiveChannelWorkerKind::EmailImap => {
            let Some(imap) = email_imap_runtime_config(object) else {
                return WorkerConfigState::MissingCredentials;
            };
            if !matches!(imap.security, EmailSecurity::Tls) {
                return WorkerConfigState::Unsupported(
                    "Email IMAP polling supports TLS security only".to_owned(),
                );
            }
            if allowed_sender_config(object).is_empty() {
                return WorkerConfigState::Unsupported(
                    "Email IMAP requires allowFrom/allowedSenders before polling".to_owned(),
                );
            }
            WorkerConfigState::Ready
        }
        _ => WorkerConfigState::MissingCredentials,
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

fn heartbeat_runtime_enabled(bundle: &ConfigBundle) -> bool {
    bundle.config.gateway.heartbeat.enabled
        && fs::read_to_string(bundle.context.workspace.join(HEARTBEAT_FILE_NAME))
            .map(|content| !content.trim().is_empty())
            .unwrap_or(false)
}

fn start_heartbeat_runtime(
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    bundle: &ConfigBundle,
) -> Result<Option<HeartbeatWorker>, CliError> {
    if !heartbeat_runtime_enabled(bundle) {
        return Ok(None);
    }
    let heartbeat = &bundle.config.gateway.heartbeat;
    let service = HeartbeatService::new(
        bundle.context.workspace.clone(),
        adapter.resolved_model.clone(),
        heartbeat.interval_s as u64,
        heartbeat.enabled,
        Some(bundle.config.agents.defaults.timezone.clone()),
    );
    let settings = adapter.loop_config().settings;
    let provider = adapter.client.clone();
    let executor_adapter = adapter.clone();
    let executor: Arc<dyn HeartbeatTaskExecutor> =
        Arc::new(move |tasks: &str| executor_adapter.execute_heartbeat_tasks(tasks));
    let evaluator: Arc<dyn HeartbeatResponseEvaluator> = Arc::new(
        ProviderNotificationEvaluator::new(provider.clone(), adapter.resolved_model.clone()),
    );
    let notifier: Arc<dyn HeartbeatNotifier> = Arc::new(|response: &str| {
        eprintln!("heartbeat: {response}");
        Ok::<(), HeartbeatError>(())
    });
    HeartbeatWorker::start(service, provider, settings, executor, evaluator, notifier)
        .map_err(|error| CliError::Api(ApiError::internal(error.to_string())))
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
    let (channel_ids, gateway_style) = discord_channel_config(object);
    let transport = if gateway_style || channel_ids.is_empty() {
        DiscordTransportMode::Gateway
    } else {
        DiscordTransportMode::RestPolling
    };
    let channel_filter = if channel_ids.is_empty() {
        DiscordChannelFilter::AllVisible
    } else {
        DiscordChannelFilter::Only(channel_ids)
    };
    Some(DiscordRuntimeConfig {
        token,
        channel_filter,
        allowed_senders: allowed_sender_config(object),
        group_policy: discord_group_policy(object),
        streaming: plugin_bool_alias(object, &["streaming"]).unwrap_or(true),
        poll_interval_seconds: plugin_u64_alias(
            object,
            &["pollIntervalSeconds", "poll_interval_seconds"],
        )
        .unwrap_or(5)
        .max(1),
        transport,
    })
}

fn slack_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<SlackRuntimeConfig> {
    let object = plugin_object(plugins, SLACK_CHANNEL)?;
    let app_token = plugin_string_alias(object, &["appToken", "app_token"])?;
    let bot_token = plugin_string_alias(object, &["botToken", "bot_token", "token"])?;
    let channel_ids = channel_id_config(object);
    Some(SlackRuntimeConfig {
        app_token,
        bot_token,
        channel_ids,
        allowed_senders: allowed_sender_config(object),
    })
}

fn allowed_sender_config(object: &Map<String, Value>) -> Vec<String> {
    plugin_string_array_alias(
        object,
        &[
            "allowFrom",
            "allowedSenders",
            "allow_from",
            "allowed_senders",
        ],
    )
}

fn channel_id_config(object: &Map<String, Value>) -> Vec<String> {
    let mut channel_ids = plugin_string_array_alias(
        object,
        &[
            "channelIds",
            "allowedChannelIds",
            "allowChannels",
            "channel_ids",
            "allowed_channel_ids",
            "allow_channels",
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
    channel_ids
}

fn discord_channel_config(object: &Map<String, Value>) -> (Vec<String>, bool) {
    let gateway_style =
        object.contains_key("allowChannels") || object.contains_key("allow_channels");
    (channel_id_config(object), gateway_style)
}

fn discord_group_policy(object: &Map<String, Value>) -> DiscordGroupPolicy {
    match plugin_string_alias(object, &["groupPolicy", "group_policy"])
        .as_deref()
        .map(str::trim)
    {
        Some("open") => DiscordGroupPolicy::Open,
        _ => DiscordGroupPolicy::Mention,
    }
}

fn email_runtime_config(plugins: &BTreeMap<String, Value>) -> Option<EmailRuntimeConfig> {
    let object = plugin_object(plugins, EMAIL_CHANNEL)?;
    if !plugin_bool_alias(object, &["consentGranted", "consent_granted"]).unwrap_or(false) {
        return None;
    }
    let allowed_senders = allowed_sender_config(object);
    let imap = email_imap_runtime_config(object)
        .filter(|imap| matches!(imap.security, EmailSecurity::Tls) && !allowed_senders.is_empty());
    Some(EmailRuntimeConfig {
        smtp: email_smtp_runtime_config(object),
        imap,
        allowed_senders,
        verify_spf: plugin_bool_alias(object, &["verifySpf", "verifySPF", "verify_spf"])
            .unwrap_or(true),
        verify_dkim: plugin_bool_alias(object, &["verifyDkim", "verifyDKIM", "verify_dkim"])
            .unwrap_or(true),
    })
}

fn email_smtp_runtime_config(object: &Map<String, Value>) -> Option<EmailSmtpRuntimeConfig> {
    let smtp = nested_object(object, "smtp").unwrap_or(object);
    let host = plugin_string_alias(smtp, &["host", "smtpHost", "smtp_host"])?;
    let from = plugin_string_alias(
        smtp,
        &[
            "from",
            "fromAddress",
            "fromEmail",
            "from_address",
            "from_email",
        ],
    )?;
    Some(EmailSmtpRuntimeConfig {
        host,
        port: plugin_u64_alias(smtp, &["port", "smtpPort", "smtp_port"])
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(587),
        username: plugin_string_alias(smtp, &["username", "user", "smtpUsername", "smtp_username"]),
        password: plugin_string_alias(smtp, &["password", "smtpPassword", "smtp_password"]),
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
    let username =
        plugin_string_alias(imap, &["username", "user", "imapUsername", "imap_username"])?;
    let password = plugin_string_alias(imap, &["password", "imapPassword", "imap_password"])?;
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
    let bridge_url = normalize_whatsapp_bridge_websocket_url(&plugin_string_alias(
        object,
        &["bridgeUrl", "bridge_url", "baseUrl", "base_url"],
    )?)?;
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
        allowlist: if allowed.is_empty() {
            shacs_channels::ChannelAllowlist::allow_all()
        } else {
            shacs_channels::ChannelAllowlist::new(allowed)
        },
        group_policy,
    })
}

fn normalize_whatsapp_bridge_websocket_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        return Some(trimmed.to_owned());
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return Some(format!("ws://{rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return Some(format!("wss://{rest}"));
    }
    None
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
            Self::SkippedUnsupported => "skipped-unsupported",
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

pub fn format_apps_list(report: AppsListReport) -> String {
    let mut lines = vec![
        "Apps".to_owned(),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Registry: {}", display_path(&report.registry_path)),
    ];
    if report.entries.is_empty() {
        lines.push("No apps installed.".to_owned());
        return lines.join("\n");
    }
    for entry in report.entries {
        lines.push(format!(
            "- {} {} [{}] {}",
            entry.app_id,
            entry.version,
            app_lifecycle_label(&entry.lifecycle_state),
            entry.digest
        ));
    }
    lines.join("\n")
}

pub fn format_apps_init(report: AppsInitReport) -> String {
    let outcome = match &report.authoring.outcome {
        AppAuthoringInitOutcome::Created => "created",
        AppAuthoringInitOutcome::AlreadyExistsSameContent => "already-exists-same-content",
    };
    let file_list = report
        .authoring
        .generated_files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    [
        format!("App authoring draft: {}", report.authoring.app_id),
        format!("Outcome: {outcome}"),
        format!("Draft id: {}", report.authoring.draft_id),
        format!(
            "Draft path: {}",
            display_path_escaped(&report.authoring.draft_path)
        ),
        format!(
            "Manifest candidate: {}",
            display_path_escaped(&report.authoring.manifest_candidate_path)
        ),
        format!(
            "README candidate: {}",
            display_path_escaped(&report.authoring.readme_candidate_path)
        ),
        format!(
            "Scaffold plan: {}",
            display_path_escaped(&report.authoring.scaffold_plan_path)
        ),
        format!(
            "Draft metadata: {}",
            display_path_escaped(&report.authoring.draft_metadata_path)
        ),
        format!("Config: {}", display_path_escaped(&report.config_path)),
        format!("Workspace: {}", display_path_escaped(&report.workspace)),
        format!("Data dir: {}", display_path_escaped(&report.data_dir)),
        format!(
            "Revision digest: {}",
            report.authoring.current_revision_digest
        ),
        format!("Generated files: {file_list}"),
        format!("Validation: {}", report.authoring.validation_status),
        format!("Next action: {}", report.authoring.next_action),
    ]
    .join("\n")
}

pub fn format_apps_inspect(report: AppsEntryReport) -> String {
    format_apps_entry("App", report)
}

pub fn format_apps_entry_report(report: AppsEntryReport) -> String {
    format_apps_entry("App", report)
}

fn format_apps_entry(title: &str, report: AppsEntryReport) -> String {
    let entry = report.entry;
    let mut lines = vec![
        format!("{title}: {}", entry.app_id),
        format!("Version: {}", entry.version),
        format!("State: {}", app_lifecycle_label(&entry.lifecycle_state)),
        format!("Digest: {}", entry.digest),
        format!("Bundle: {}", display_path(&entry.bundle_path)),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Registry: {}", display_path(&report.registry_path)),
        format!("Permission requests: {}", entry.permission_requests.len()),
        format!("Secret requests: {}", entry.secret_requests.len()),
        format!("Process snapshots: {}", entry.process_snapshots.len()),
    ];
    if let Some(grant_reference) = entry.grant_reference.as_deref() {
        lines.push(format!("Grant reference: {grant_reference}"));
    }
    if !entry.unavailable_reasons.is_empty() {
        lines.push(format!(
            "Unavailable: {}",
            entry.unavailable_reasons.join("; ")
        ));
    }
    lines.join("\n")
}

pub fn format_apps_uninstall(report: AppsUninstallReport) -> String {
    [
        format!("App: {}", report.app_id),
        format!("Removed: {}", yes_no_label(report.removed)),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Registry: {}", display_path(&report.registry_path)),
    ]
    .join("\n")
}

pub fn format_plugins_list(report: PluginProjectionReport) -> String {
    let mut lines = plugin_projection_header("Plugins", &report);
    if report.plugins.is_empty() {
        lines.push("No plugins discovered.".to_owned());
        return lines.join("\n");
    }
    for descriptor in &report.projection.plugins {
        lines.push(format!(
            "- {} [{}] source={} declared={} projected={} execution=disabled",
            descriptor.id,
            descriptor.state,
            descriptor.source,
            descriptor.declared_surface_count,
            descriptor.active_surface_count
        ));
    }
    lines.join("\n")
}

pub fn format_plugins_inspect(report: PluginInspectReport) -> String {
    let plugin = report.plugin;
    let descriptor = report
        .projection
        .plugins
        .iter()
        .find(|descriptor| descriptor.id == plugin.id);
    let mut lines = vec![
        format!("Plugin: {}", plugin.id),
        "Boundary: descriptor-only projection; no hooks, MCP, tools, commands, or processes were executed".to_owned(),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("State: {}", plugin.state.as_str()),
        format!("Source: {}", plugin.source.as_str()),
        format!("Manifest: {}", display_path(&plugin.manifest_path)),
    ];
    if let Some(digest) = plugin.digest.as_deref() {
        lines.push(format!("Digest: {digest}"));
    }
    if let Some(manifest) = plugin.manifest.as_ref() {
        lines.push(format!("Version: {}", manifest.version));
        if let Some(description) = manifest.description.as_deref() {
            lines.push(format!("Description: {description}"));
        }
    }
    if let Some(descriptor) = descriptor {
        lines.push(format!(
            "Surfaces: declared={} projected={} execution=disabled",
            descriptor.declared_surface_count, descriptor.active_surface_count
        ));
        if !descriptor.secret_refs.is_empty() {
            lines.push("Secret refs: names only, values redacted".to_owned());
            for secret in &descriptor.secret_refs {
                lines.push(format!(
                    "- {:?}: {} present={}",
                    secret.kind, secret.name, secret.present
                ));
            }
        }
    }
    let plugin_tools = report
        .projection
        .tools
        .iter()
        .filter(|item| item.plugin_id == plugin.id)
        .map(|item| format!("tool:{} execution={}", item.name, item.execution_enabled));
    let plugin_hooks = report
        .projection
        .hooks
        .iter()
        .filter(|item| item.plugin_id == plugin.id)
        .map(|item| format!("hook:{} execution={}", item.event, item.execution_enabled));
    let plugin_skills = report
        .projection
        .skills
        .iter()
        .filter(|item| item.plugin_id == plugin.id)
        .map(|item| format!("skill:{} execution={}", item.name, item.execution_enabled));
    let plugin_commands = report
        .projection
        .commands
        .iter()
        .filter(|item| item.plugin_id == plugin.id)
        .map(|item| format!("command:{} execution={}", item.name, item.execution_enabled));
    let plugin_mcp = report
        .projection
        .mcp
        .iter()
        .filter(|item| item.plugin_id == plugin.id)
        .map(|item| format!("mcp:{} execution={}", item.name, item.execution_enabled));
    let surfaces = plugin_tools
        .chain(plugin_hooks)
        .chain(plugin_skills)
        .chain(plugin_commands)
        .chain(plugin_mcp)
        .collect::<Vec<_>>();
    if !surfaces.is_empty() {
        lines.push(format!("Projected surfaces: {}", surfaces.join(", ")));
    }
    if !plugin.block_reasons.is_empty() {
        lines.push(format!(
            "Block reasons: {}",
            plugin
                .block_reasons
                .iter()
                .map(|reason| reason.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !plugin.diagnostics.is_empty() {
        lines.push(format!("Diagnostics: {}", plugin.diagnostics.join("; ")));
    }
    lines.join("\n")
}

pub fn format_plugins_doctor(report: PluginProjectionReport) -> String {
    let mut lines = plugin_projection_header("Plugin doctor", &report);
    lines.push("Boundary: descriptor-only checks; no live plugin execution, hook dispatch, MCP startup, tool registration, or process execution".to_owned());
    let blocked = report
        .plugins
        .iter()
        .filter(|plugin| plugin.state == PluginState::Blocked)
        .count();
    lines.push(format!("Discovered plugins: {}", report.plugins.len()));
    lines.push(format!("Blocked plugins: {blocked}"));
    lines.push(format!(
        "Projection diagnostics: {}",
        report.projection.diagnostics.len()
    ));
    for plugin in report.plugins {
        if plugin.state == PluginState::Blocked || !plugin.diagnostics.is_empty() {
            lines.push(format!(
                "- {} [{}]: {}",
                plugin.id,
                plugin.state.as_str(),
                plugin_block_summary(&plugin)
            ));
        }
    }
    for diagnostic in report.projection.diagnostics {
        lines.push(format!(
            "- projection {} {}: {}",
            diagnostic.plugin_id, diagnostic.code, diagnostic.message
        ));
    }
    lines.join("\n")
}

pub fn format_plugin_mutation(report: PluginMutationReport) -> String {
    let action = match report.action {
        PluginMutationAction::Enabled => "enabled",
        PluginMutationAction::Disabled => "disabled",
    };
    [
        format!("Plugin: {}", report.plugin_name),
        format!("Action: {action}"),
        format!("Changed: {}", yes_no_label(report.changed)),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        report.next_session_notice,
    ]
    .join("\n")
}

pub fn format_hooks_list(report: HookProjectionReport) -> String {
    let mut lines = vec![
        "Hooks".to_owned(),
        "Boundary: descriptor-only metadata; no hook dispatch or plugin process execution"
            .to_owned(),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Catalog events: {}", report.catalog.entries.len()),
        format!("Plugin hook descriptors: {}", report.plugin_hooks.len()),
    ];
    for entry in report.catalog.entries {
        lines.push(format!(
            "- catalog {} policy={:?} timeout_ms={} permission_approval={}",
            entry.event.as_str(),
            entry.output_policy,
            entry.timeout_ms,
            entry.can_request_permission_approval
        ));
    }
    for hook in report.plugin_hooks {
        lines.push(format!(
            "- plugin {} event={} execution={}",
            hook.plugin_id, hook.event, hook.execution_enabled
        ));
    }
    lines.join("\n")
}

pub fn format_hooks_inspect(report: HookInspectReport) -> String {
    let mut lines = vec![
        format!("Hook filter: {}", report.filter),
        "Boundary: descriptor-only metadata; no hook dispatch or plugin process execution"
            .to_owned(),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
    ];
    if report.catalog.entries.is_empty() && report.plugin_hooks.is_empty() {
        lines.push("No matching hook metadata found.".to_owned());
        return lines.join("\n");
    }
    for entry in report.catalog.entries {
        lines.push(format!(
            "Catalog: {} policy={:?} timeout_ms={} permission_approval={}",
            entry.event.as_str(),
            entry.output_policy,
            entry.timeout_ms,
            entry.can_request_permission_approval
        ));
    }
    for hook in report.plugin_hooks {
        lines.push(format!(
            "Plugin hook: {} event={} execution={}",
            hook.plugin_id, hook.event, hook.execution_enabled
        ));
    }
    lines.join("\n")
}

fn plugin_projection_header(title: &str, report: &PluginProjectionReport) -> Vec<String> {
    vec![
        title.to_owned(),
        "Boundary: descriptor-only projection; execution is disabled".to_owned(),
        format!("Config: {}", display_path(&report.config_path)),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Data dir: {}", display_path(&report.data_dir)),
    ]
}

fn app_lifecycle_label(state: &AppLifecycleState) -> &'static str {
    match state {
        AppLifecycleState::Installed => "installed",
        AppLifecycleState::Enabled => "enabled",
        AppLifecycleState::Disabled => "disabled",
        AppLifecycleState::Unavailable => "unavailable",
        AppLifecycleState::Uninstalling => "uninstalling",
    }
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
        format!("Send memory hints: {}", report.send_memory_hints),
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
            item.descriptor.name, item.enabled, item.configured, item.runtime_status, worker_labels
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

pub fn format_context_files_report(title: &str, report: ContextFilesCliReport) -> String {
    let summary = report.summary;
    let mut lines = vec![
        title.to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!("Context files: {}", summary.total_count),
        format!("Included: {}", summary.included_count),
        format!("Skipped: {}", summary.skipped_count),
        format!("Truncated: {}", summary.truncated_count),
        format!("Denied: {}", summary.denied_count),
    ];
    for entry in summary.entries {
        let reason = entry
            .reason
            .as_deref()
            .map(|reason| format!(" reason={reason}"))
            .unwrap_or_default();
        let digest = entry
            .digest
            .as_deref()
            .map(|digest| format!(" digest={digest}"))
            .unwrap_or_default();
        lines.push(format!(
            "- order={} status={:?} path={} bytes={:?} tokens={:?}{digest}{reason}",
            entry.order, entry.status, entry.source_label, entry.byte_count, entry.token_estimate
        ));
    }
    lines.join("\n")
}

pub fn format_context_refs_parse_report(report: ContextRefsParseCliReport) -> String {
    let Some(summary) = report.summary else {
        return "context refs parse\nReferences: 0".to_owned();
    };
    let mut lines = vec![
        "context refs parse".to_owned(),
        format!("References: {}", summary.reference_count),
        format!("Parse diagnostics: {}", summary.diagnostic_count),
    ];
    for reference in summary.references {
        lines.push(format!(
            "- span={}..{} kind={:?} target={}",
            reference.start, reference.end, reference.kind, reference.source_label
        ));
    }
    for diagnostic in summary.diagnostics {
        lines.push(format!(
            "- diagnostic span={}..{} kind={} message={}",
            diagnostic.start, diagnostic.end, diagnostic.kind, diagnostic.message
        ));
    }
    lines.join("\n")
}

pub fn format_context_refs_resolve_report(report: ContextRefsResolveCliReport) -> String {
    let summary = report.summary;
    let mut lines = vec![
        "context refs resolve".to_owned(),
        format!("Workspace: {}", display_path(&report.workspace)),
        format!(
            "References: {}",
            summary
                .references
                .as_ref()
                .map_or(0, |refs| refs.reference_count)
        ),
        format!("Artifacts: {}", summary.artifacts.total_count),
        format!("Resolved: {}", summary.artifacts.resolved_count),
        format!("Skipped: {}", summary.artifacts.skipped_count),
        format!("Denied: {}", summary.artifacts.denied_count),
        format!("Failed: {}", summary.artifacts.failed_count),
        format!("Redacted: {}", summary.artifacts.redacted_count),
        format!("Truncated: {}", summary.artifacts.truncated_count),
    ];
    if let Some(budget) = summary.budget.as_ref() {
        lines.push(format!(
            "Budget bytes: {}/{}",
            budget.used_context_bytes, budget.budget_bytes
        ));
        lines.push(format!(
            "Budget decisions: included={} skipped={} truncated={}",
            budget.included_count, budget.skipped_count, budget.truncated_count
        ));
    }
    for artifact in summary.artifacts.entries {
        let reason = artifact
            .permission_evidence
            .as_deref()
            .map(|reason| format!(" reason={reason}"))
            .unwrap_or_default();
        lines.push(format!(
            "- artifact kind={:?} state={:?} source={} redaction={:?} truncation={:?} permission={}{reason}",
            artifact.kind,
            artifact.state,
            artifact.source_label,
            artifact.redaction_status,
            artifact.truncation_status,
            artifact.permission_status
        ));
    }
    if let Some(safety) = summary.safety.as_ref() {
        for diagnostic in &safety.diagnostics {
            lines.push(format!(
                "- safety source={} decision={:?} trust={:?} redaction={:?} message={}",
                diagnostic.source_label,
                diagnostic.permission_decision,
                diagnostic.trust_label,
                diagnostic.redaction_status,
                diagnostic.message
            ));
        }
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
        .list_session_ux()?
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
    let detail = manager.session_ux_detail(&options.session).ok_or_else(|| {
        CliError::InvalidArguments(format!("session `{}` was not found", options.session))
    })?;

    Ok(SessionInspectReport {
        workspace,
        key: detail.key,
        path: detail.path,
        created_at: detail.created_at,
        updated_at: detail.updated_at,
        message_count: detail.message_count,
        metadata_keys: detail.metadata_keys,
        last_consolidated: detail.last_consolidated,
        recovery_markers: detail.recovery_markers,
        checkpoint_phase: detail.checkpoint_phase,
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
    let history = manager
        .session_ux_history_with_options(
            &options.session,
            SessionHistoryOptions {
                max_messages: options.max_messages,
                max_tokens: options.max_tokens,
                include_timestamps: options.timestamps,
            },
        )
        .ok_or_else(|| {
            CliError::InvalidArguments(format!("session `{}` was not found", options.session))
        })?;
    Ok(SessionHistoryCliReport {
        workspace,
        key: history.key,
        path: history.path,
        history: history.history,
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
    let diagnostics = manager.session_ux_diagnostics(&options.session);
    let path = if diagnostics.exists {
        diagnostics.path
    } else {
        fallback_path
    };
    Ok(SessionDiagnosticsReport {
        workspace,
        key: diagnostics.key,
        path,
        exists: diagnostics.exists,
        message_count: diagnostics.message_count,
        last_consolidated: diagnostics.last_consolidated,
        metadata_keys: diagnostics.metadata_keys,
        recovery_markers: diagnostics.recovery_markers,
        checkpoint_phase: diagnostics.checkpoint_phase,
        legal_start: diagnostics.legal_start,
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
        ProviderCommand::CopilotImportToken(options) => {
            import_copilot_token(options).map(format_copilot_import_outcome)
        }
        ProviderCommand::ImportApiKey(options) => {
            import_provider_api_key(options).map(format_provider_api_key_import_outcome)
        }
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

fn copilot_provider_config() -> ProviderConfig {
    ProviderConfig {
        api_key: None,
        api_base: Some("https://api.githubcopilot.com".to_owned()),
        extra_headers: None,
        extra_body: None,
    }
}

pub fn import_copilot_token(
    options: CopilotImportTokenOptions,
) -> Result<CopilotImportOutcome, CliError> {
    let token = read_token_source(&options.token_source)?.trim().to_owned();
    if token.is_empty() {
        return Err(CliError::InvalidArguments(
            "GitHub Copilot token must not be empty".to_owned(),
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
        .entry(GITHUB_COPILOT_PROVIDER_ID.to_owned())
        .or_insert_with(copilot_provider_config);
    let selected_model = options
        .select
        .then(|| GITHUB_COPILOT_DEFAULT_MODEL.to_owned());
    if let Some(model) = &selected_model {
        bundle.config.agents.defaults.provider = GITHUB_COPILOT_PROVIDER_ID.to_owned();
        bundle.config.agents.defaults.model = model.clone();
    }

    let context = config_context(
        Some(config_path.clone()),
        Some(bundle.config.workspace_path()),
    );
    let auth_path = context.auth_path();
    let mut auth = load_auth_store(&auth_path)?;
    auth.providers.insert(
        GITHUB_COPILOT_PROVIDER_ID.to_owned(),
        ProviderAuth::oauth_access(token, None),
    );
    save_auth_store_to_path(&auth, &auth_path)?;
    save_config_to_path(&bundle.config, &config_path)?;

    Ok(CopilotImportOutcome {
        config_path,
        auth_path,
        provider: GITHUB_COPILOT_PROVIDER_ID.to_owned(),
        selected_model,
        selected: options.select,
    })
}

pub fn import_provider_api_key(
    options: ProviderApiKeyImportOptions,
) -> Result<ProviderApiKeyImportOutcome, CliError> {
    let token = read_token_source(&options.token_source)?.trim().to_owned();
    if token.is_empty() {
        return Err(CliError::InvalidArguments(
            "provider API key must not be empty".to_owned(),
        ));
    }

    let registry = ProviderRegistry::new();
    let spec = registry.find_by_name(&options.provider).ok_or_else(|| {
        CliError::Provider(ProviderError::ProviderNotFound {
            provider_id: options.provider.clone(),
            suggestions: registry
                .specs()
                .iter()
                .map(|spec| spec.name.to_owned())
                .collect(),
        })
    })?;
    if spec.is_oauth {
        return Err(CliError::InvalidArguments(format!(
            "provider `{}` uses an OAuth auth workflow; use its provider-specific import/login command",
            spec.name
        )));
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
        .entry(spec.name.to_owned())
        .or_default();

    let context = config_context(
        Some(config_path.clone()),
        Some(bundle.config.workspace_path()),
    );
    let auth_path = context.auth_path();
    let mut auth = load_auth_store(&auth_path)?;
    auth.providers
        .insert(spec.name.to_owned(), ProviderAuth::api_key(token));
    save_auth_store_to_path(&auth, &auth_path)?;
    save_config_to_path(&bundle.config, &config_path)?;

    Ok(ProviderApiKeyImportOutcome {
        config_path,
        auth_path,
        provider: spec.name.to_owned(),
    })
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

fn base64_standard(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
            output.push('=');
            output.push('=');
        }
        2 => {
            let chunk = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
            output.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            output.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
            output.push('=');
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
    let mut config = adapter.loop_config();
    config.settings.temperature = invocation
        .temperature
        .unwrap_or(config.settings.temperature);
    config.settings.max_tokens = invocation.max_tokens.unwrap_or(config.settings.max_tokens);
    let message = InboundMessage::new("cli", "user", "direct", invocation_text(&invocation))
        .with_media(invocation.media_paths.clone())
        .with_session_key_override(invocation.session_key.clone());
    let (turn, outbound) = adapter.process_inbound_with_outbound(message, config, None, &[])?;
    let content = render_direct_turn_content(turn.final_content.unwrap_or_default(), outbound);
    Ok(render_agent_response(content, options.markdown, None))
}

fn render_direct_turn_content(
    final_content: String,
    outbound: Vec<shacs_channels::OutboundMessage>,
) -> String {
    let mut parts = outbound
        .into_iter()
        .filter(is_visible_runtime_notification)
        .filter_map(|message| {
            let content = message.content.trim();
            (!content.is_empty()).then(|| content.to_owned())
        })
        .collect::<Vec<_>>();
    if !final_content.trim().is_empty() {
        parts.push(final_content);
    }
    parts.join("\n\n")
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
    let ownership = RuntimeOwnershipLease::acquire(&bundle, "serve")?;
    let data_dir = bundle.context.data_dir.clone();
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
    let serve_result = runtime.block_on(shacs_api::serve_api_with_timeout(
        addr,
        adapter,
        Duration::from_secs_f64(timeout_seconds),
        async {
            wait_for_ctrl_c_or_runtime_request(data_dir).await;
        },
    ));
    ownership.cleanup()?;
    serve_result?;
    Ok(format!("API server stopped: http://{addr}"))
}

pub fn run_runtime(options: RunOptions) -> Result<String, CliError> {
    run_runtime_with_mode(options, "run")
}

fn run_runtime_with_mode(options: RunOptions, mode: &str) -> Result<String, CliError> {
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
    let send_max_retries = bundle.config.channels.send_max_retries;
    let _runtime_dirs = ensure_runtime_dirs(&bundle.context)?;
    let worker_metadata_dir = bundle
        .context
        .runtime_subdir("channels")
        .join("worker-metadata");
    let external_context = ExternalTransportRuntimeContext::new(
        worker_metadata_dir,
        channel_retry_policy(send_max_retries).max_attempts,
    );
    let specs = external_transport_specs(&plugins);
    let heartbeat_enabled = heartbeat_runtime_enabled(&bundle);
    if !runtime_needs_process(&report, &specs) && !heartbeat_enabled {
        return Ok(plan);
    }
    let ownership = RuntimeOwnershipLease::acquire(&bundle, mode)?;
    let data_dir = bundle.context.data_dir.clone();
    let adapter = Arc::new(
        AgentLoopChatCompletionAdapter::from_bundle(bundle.clone(), options.allow_side_effects)?
            .with_runtime_verbose(options.verbose),
    );
    let heartbeat_worker = start_heartbeat_runtime(adapter.clone(), &bundle)?;
    let supervisor = ExternalChannelSupervisor::start(
        adapter.clone(),
        specs,
        send_max_retries,
        external_context,
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    eprintln!("{plan}");
    let serve_result = if report.websocket.enabled {
        let request_data_dir = data_dir.clone();
        runtime.block_on(shacs_api::serve_websocket_with_timeout_and_path(
            report.websocket_addr,
            adapter,
            Duration::from_secs_f64(timeout_seconds),
            &report.websocket.path,
            async move {
                wait_for_ctrl_c_or_runtime_request(request_data_dir).await;
            },
        ))
    } else {
        runtime.block_on(async {
            wait_for_ctrl_c_or_runtime_request(data_dir).await;
        });
        Ok(())
    };
    supervisor.stop();
    if let Some(worker) = heartbeat_worker {
        worker
            .join()
            .map_err(|_| CliError::Api(ApiError::internal("heartbeat worker panicked")))?;
    }
    ownership.cleanup()?;
    serve_result?;
    Ok(format!(
        "Channel runtime stopped: websocket_enabled={} ws://{}:{}{}",
        report.websocket.enabled,
        report.websocket.host,
        report.websocket.port,
        report.websocket.path
    ))
}

async fn wait_for_ctrl_c_or_runtime_request(data_dir: PathBuf) {
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if runtime_stop_request_observed(&data_dir).ok().flatten().is_some() {
                    break;
                }
            }
        }
    }
}

struct ExternalChannelSupervisor {
    stop: Arc<AtomicBool>,
    handles: Vec<thread::JoinHandle<()>>,
}

#[derive(Debug, Default)]
struct ExternalSessionTurnCoordinator {
    active_sessions: BTreeSet<String>,
    pending_by_session: BTreeMap<String, VecDeque<InboundMessage>>,
}

struct ExternalAgentTurnResult {
    session_key: String,
    outbound: Vec<OutboundMessage>,
    error: Option<String>,
    retry_message: Option<InboundMessage>,
    subagent_runtime: Option<SubagentRuntime>,
}

#[derive(Clone)]
struct ExternalTypingIndicator {
    session_key: String,
    message: OutboundMessage,
    turn_active: bool,
    subagent_runtimes: Vec<SubagentRuntime>,
    next_at: Instant,
}

struct ExternalStreamRouting<'a> {
    channel: &'a str,
    chat_id: &'a str,
    stream_id: &'a str,
    metadata: &'a Map<String, Value>,
    reply_to: Option<&'a str>,
}

impl ExternalSessionTurnCoordinator {
    fn start_or_enqueue(
        &mut self,
        session_key: String,
        message: InboundMessage,
    ) -> Option<InboundMessage> {
        if self.active_sessions.insert(session_key.clone()) {
            return Some(message);
        }
        let queue = self.pending_by_session.entry(session_key).or_default();
        if queue.len() >= EXTERNAL_SESSION_PENDING_LIMIT {
            queue.pop_front();
        }
        queue.push_back(message);
        None
    }

    fn enqueue(&mut self, session_key: String, message: InboundMessage) {
        let queue = self.pending_by_session.entry(session_key).or_default();
        if queue.len() >= EXTERNAL_SESSION_PENDING_LIMIT {
            queue.pop_front();
        }
        queue.push_back(message);
    }

    fn defer_turn(&mut self, session_key: String, message: InboundMessage) {
        self.active_sessions.remove(&session_key);
        let queue = self.pending_by_session.entry(session_key).or_default();
        if queue.len() >= EXTERNAL_SESSION_PENDING_LIMIT {
            queue.pop_back();
        }
        queue.push_front(message);
    }

    fn start_next_ready(
        &mut self,
        mut is_busy: impl FnMut(&str) -> bool,
    ) -> Option<(String, InboundMessage)> {
        let session_key = self
            .pending_by_session
            .keys()
            .find(|key| !self.active_sessions.contains(*key) && !is_busy(key))
            .cloned()?;
        let queue = self.pending_by_session.get_mut(&session_key)?;
        let message = queue.pop_front()?;
        if queue.is_empty() {
            self.pending_by_session.remove(&session_key);
        }
        self.active_sessions.insert(session_key.clone());
        Some((session_key, message))
    }

    fn finish_turn(&mut self, session_key: &str) -> Option<InboundMessage> {
        if let Some(queue) = self.pending_by_session.get_mut(session_key) {
            if let Some(message) = queue.pop_front() {
                return Some(message);
            }
        }
        self.pending_by_session.remove(session_key);
        self.active_sessions.remove(session_key);
        None
    }
}

#[derive(Debug, Clone)]
struct ExternalTransportRuntimeContext {
    metadata_dir: PathBuf,
    send_attempts: usize,
}

impl ExternalTransportRuntimeContext {
    fn new(metadata_dir: PathBuf, send_attempts: usize) -> Self {
        Self {
            metadata_dir,
            send_attempts: send_attempts.clamp(1, 10),
        }
    }

    fn metadata_path(&self, name: &str) -> PathBuf {
        self.metadata_dir.join(format!("{name}.json"))
    }
}

impl ExternalChannelSupervisor {
    fn start(
        adapter: Arc<AgentLoopChatCompletionAdapter>,
        specs: Vec<ExternalTransportSpec>,
        send_max_retries: u32,
        transport_context: ExternalTransportRuntimeContext,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        if specs.is_empty() {
            return Self {
                stop,
                handles: Vec::new(),
            };
        }
        let runtime_bus = MessageBus::new();
        let processor_stop = stop.clone();
        let handles = vec![thread::spawn(move || {
            run_external_agent_processor(
                adapter,
                runtime_bus,
                specs,
                send_max_retries,
                processor_stop,
                transport_context,
            );
        })];
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

    fn supports_streaming(&self) -> bool {
        matches!(self, Self::Telegram(_) | Self::Discord(_) | Self::Slack(_))
    }
}

fn run_external_agent_processor(
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    runtime_bus: MessageBus,
    specs: Vec<ExternalTransportSpec>,
    send_max_retries: u32,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let mut channels = external_transport_channel_manager(
        specs,
        runtime_bus.clone(),
        send_max_retries,
        transport_context,
    );
    if let Err(error) = channels.start_all() {
        eprintln!("external channel start failed: {error}");
    }
    let (turn_tx, turn_rx) = mpsc::channel::<ExternalAgentTurnResult>();
    let mut turn_coordinator = ExternalSessionTurnCoordinator::default();
    let mut turn_handles = Vec::new();
    let mut typing_indicators = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        join_finished_turns(&mut turn_handles);
        let mut progressed = false;
        while let Ok(result) = turn_rx.try_recv() {
            progressed = true;
            finish_external_typing_indicator(
                &mut typing_indicators,
                &result.session_key,
                result.subagent_runtime.as_ref(),
            );
            if let Some(error) = result.error {
                eprintln!("external channel turn failed: {error}");
            }
            for message in result.outbound {
                runtime_bus.publish_outbound(message);
            }
            if let Some(message) = result.retry_message {
                turn_coordinator.defer_turn(result.session_key, message);
                continue;
            } else if let Some(next) = turn_coordinator.finish_turn(&result.session_key) {
                let session_key = adapter.external_effective_session_key(&next);
                start_external_typing_indicator(
                    &adapter,
                    &runtime_bus,
                    &mut typing_indicators,
                    &session_key,
                    &next,
                );
                turn_handles.push(spawn_external_agent_turn(
                    adapter.clone(),
                    session_key,
                    next,
                    runtime_bus.clone(),
                    turn_tx.clone(),
                ));
            }
        }
        while let Some(message) = runtime_bus.try_consume_inbound() {
            progressed = true;
            let session_key = adapter.external_effective_session_key(&message);
            if adapter.external_session_is_active(&session_key) {
                turn_coordinator.enqueue(session_key, message);
            } else if let Some(next) =
                turn_coordinator.start_or_enqueue(session_key.clone(), message)
            {
                start_external_typing_indicator(
                    &adapter,
                    &runtime_bus,
                    &mut typing_indicators,
                    &session_key,
                    &next,
                );
                turn_handles.push(spawn_external_agent_turn(
                    adapter.clone(),
                    session_key,
                    next,
                    runtime_bus.clone(),
                    turn_tx.clone(),
                ));
            }
        }
        while let Some((session_key, message)) = turn_coordinator
            .start_next_ready(|session_key| adapter.external_session_is_active(session_key))
        {
            progressed = true;
            start_external_typing_indicator(
                &adapter,
                &runtime_bus,
                &mut typing_indicators,
                &session_key,
                &message,
            );
            turn_handles.push(spawn_external_agent_turn(
                adapter.clone(),
                session_key,
                message,
                runtime_bus.clone(),
                turn_tx.clone(),
            ));
        }
        progressed |= publish_due_external_typing_indicators(&mut typing_indicators, &runtime_bus);
        progressed |= drain_runtime_outbound(&runtime_bus, &mut channels);
        if !progressed {
            sleep_with_stop(&stop, Duration::from_millis(50));
        }
    }
    for handle in turn_handles {
        let _ = handle.join();
    }
    while let Ok(result) = turn_rx.try_recv() {
        if let Some(error) = result.error {
            eprintln!("external channel turn failed: {error}");
        }
        for message in result.outbound {
            runtime_bus.publish_outbound(message);
        }
    }
    drain_runtime_outbound(&runtime_bus, &mut channels);
    if let Err(error) = channels.stop_all() {
        eprintln!("external channel stop failed: {error}");
    }
}

fn start_external_typing_indicator(
    adapter: &AgentLoopChatCompletionAdapter,
    runtime_bus: &MessageBus,
    active: &mut Vec<ExternalTypingIndicator>,
    session_key: &str,
    inbound: &InboundMessage,
) {
    if !adapter.send_progress {
        return;
    }
    if let Some(indicator) = active
        .iter_mut()
        .find(|indicator| indicator.session_key == session_key)
    {
        let message =
            external_typing_indicator_message(inbound).unwrap_or_else(|| indicator.message.clone());
        runtime_bus.publish_outbound(message.clone());
        indicator.message = message;
        indicator.turn_active = true;
        indicator.next_at = Instant::now() + EXTERNAL_TYPING_REFRESH_INTERVAL;
        return;
    }
    let Some(message) = external_typing_indicator_message(inbound) else {
        return;
    };
    runtime_bus.publish_outbound(message.clone());
    active.push(ExternalTypingIndicator {
        session_key: session_key.to_owned(),
        message,
        turn_active: true,
        subagent_runtimes: Vec::new(),
        next_at: Instant::now() + EXTERNAL_TYPING_REFRESH_INTERVAL,
    });
}

fn finish_external_typing_indicator(
    active: &mut Vec<ExternalTypingIndicator>,
    session_key: &str,
    subagent_runtime: Option<&SubagentRuntime>,
) {
    for indicator in active.iter_mut() {
        if indicator.session_key != session_key {
            continue;
        }
        indicator.turn_active = false;
        if let Some(runtime) =
            subagent_runtime.filter(|runtime| runtime.running_count_by_session(session_key) > 0)
        {
            indicator.subagent_runtimes.push(runtime.clone());
        }
    }
    active.retain_mut(external_typing_indicator_still_active);
}

fn publish_due_external_typing_indicators(
    active: &mut Vec<ExternalTypingIndicator>,
    runtime_bus: &MessageBus,
) -> bool {
    let now = Instant::now();
    let mut published = false;
    active.retain_mut(|indicator| {
        if !external_typing_indicator_still_active(indicator) {
            return false;
        }
        if indicator.next_at > now {
            return true;
        }
        runtime_bus.publish_outbound(indicator.message.clone());
        indicator.next_at = now + EXTERNAL_TYPING_REFRESH_INTERVAL;
        published = true;
        true
    });
    published
}

fn external_typing_indicator_still_active(indicator: &mut ExternalTypingIndicator) -> bool {
    let session_key = indicator.session_key.clone();
    indicator
        .subagent_runtimes
        .retain(|runtime| runtime.running_count_by_session(&session_key) > 0);
    indicator.turn_active || !indicator.subagent_runtimes.is_empty()
}

fn external_typing_indicator_message(inbound: &InboundMessage) -> Option<OutboundMessage> {
    if inbound.channel != DISCORD_CHANNEL {
        return None;
    }
    let mut metadata = Map::new();
    metadata.insert(EXTERNAL_TYPING_INDICATOR_KEY.to_owned(), Value::Bool(true));
    Some(OutboundMessage::new(&inbound.channel, &inbound.chat_id, "").with_metadata(metadata))
}

fn spawn_external_agent_turn(
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    session_key: String,
    message: InboundMessage,
    runtime_bus: MessageBus,
    result_tx: mpsc::Sender<ExternalAgentTurnResult>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let retry_message = message.clone();
        let result = match adapter.process_external_inbound_with_streaming(
            message,
            adapter.loop_config(),
            &runtime_bus,
        ) {
            Ok((_, outbound, subagent_runtime)) => ExternalAgentTurnResult {
                session_key,
                outbound,
                error: None,
                retry_message: None,
                subagent_runtime: Some(subagent_runtime),
            },
            Err(error) => ExternalAgentTurnResult {
                session_key,
                outbound: Vec::new(),
                retry_message: (error.status == 409 && error.error_type == "session_busy")
                    .then_some(retry_message),
                error: Some(error.to_string()),
                subagent_runtime: None,
            },
        };
        let _ = result_tx.send(result);
    })
}

fn join_finished_turns(handles: &mut Vec<thread::JoinHandle<()>>) {
    let mut pending = Vec::new();
    for handle in handles.drain(..) {
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            pending.push(handle);
        }
    }
    *handles = pending;
}

fn drain_runtime_outbound(runtime_bus: &MessageBus, channels: &mut ChannelManager) -> bool {
    let mut drained = false;
    while let Some(message) = runtime_bus.try_consume_outbound() {
        drained = true;
        if !should_dispatch_runtime_outbound(&message) {
            continue;
        }
        if let Err(error) = channels.dispatch_outbound(message) {
            let statuses = channels
                .status_report()
                .into_iter()
                .filter_map(|(name, status)| {
                    status.last_error.map(|error| format!("{name}={error}"))
                })
                .collect::<Vec<_>>()
                .join(", ");
            if statuses.is_empty() {
                eprintln!("external channel outbound dispatch failed: {error}");
            } else {
                eprintln!("external channel outbound dispatch failed: {error}; status: {statuses}");
            }
        }
    }
    drained
}

fn external_transport_channel_manager(
    specs: Vec<ExternalTransportSpec>,
    inbound_bus: MessageBus,
    send_max_retries: u32,
    transport_context: ExternalTransportRuntimeContext,
) -> ChannelManager {
    external_transport_channel_manager_with_runner(
        specs,
        inbound_bus,
        send_max_retries,
        transport_context,
        Arc::new(run_external_transport_worker),
    )
}

fn external_transport_channel_manager_with_runner(
    specs: Vec<ExternalTransportSpec>,
    inbound_bus: MessageBus,
    send_max_retries: u32,
    transport_context: ExternalTransportRuntimeContext,
    runner: ExternalTransportRunner,
) -> ChannelManager {
    let mut manager =
        ChannelManager::new().with_retry_policy(channel_retry_policy(send_max_retries));
    for spec in specs {
        let channel = spec.channel().to_owned();
        if let Err(error) = manager.register_adapter(
            Box::new(ExternalTransportChannelAdapter::new(
                spec,
                inbound_bus.clone(),
                transport_context.clone(),
                runner.clone(),
            )),
            true,
        ) {
            eprintln!("external channel adapter registration failed for {channel}: {error}");
        }
    }
    manager
}

fn channel_retry_policy(send_max_retries: u32) -> ChannelRetryPolicy {
    ChannelRetryPolicy {
        max_attempts: send_max_retries.clamp(1, 10) as usize,
    }
}

struct ExternalTransportChannelAdapter {
    channel: String,
    spec: ExternalTransportSpec,
    inbound_bus: MessageBus,
    transport_context: ExternalTransportRuntimeContext,
    runner: ExternalTransportRunner,
    outbound_tx: Option<mpsc::Sender<OutboundMessage>>,
    worker_stop: Option<Arc<AtomicBool>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ExternalTransportChannelAdapter {
    fn new(
        spec: ExternalTransportSpec,
        inbound_bus: MessageBus,
        transport_context: ExternalTransportRuntimeContext,
        runner: ExternalTransportRunner,
    ) -> Self {
        let channel = spec.channel().to_owned();
        Self {
            channel,
            spec,
            inbound_bus,
            transport_context,
            runner,
            outbound_tx: None,
            worker_stop: None,
            handle: None,
        }
    }
}

impl ChannelAdapter for ExternalTransportChannelAdapter {
    fn name(&self) -> &str {
        &self.channel
    }

    fn supports_streaming(&self) -> bool {
        self.spec.supports_streaming()
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        if self.handle.is_some() {
            return Ok(());
        }
        let spec = self.spec.clone();
        let inbound_bus = self.inbound_bus.clone();
        let transport_context = self.transport_context.clone();
        let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundMessage>();
        let runner = self.runner.clone();
        let stop = Arc::new(AtomicBool::new(false));
        self.outbound_tx = Some(outbound_tx);
        self.worker_stop = Some(stop.clone());
        self.handle = Some(thread::spawn(move || {
            runner(spec, inbound_bus, outbound_rx, stop, transport_context);
        }));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        if let Some(stop) = self.worker_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.outbound_tx = None;
        if let Some(handle) = self.handle.take() {
            handle.join().map_err(|_| {
                ChannelError::Delivery(format!("channel worker panicked: {}", self.channel))
            })?;
        }
        Ok(())
    }

    fn send(&self, message: OutboundMessage) -> Result<(), ChannelError> {
        self.outbound_tx
            .as_ref()
            .ok_or_else(|| {
                ChannelError::Delivery(format!("channel worker is not started: {}", self.channel))
            })?
            .send(message)
            .map_err(|error| ChannelError::Delivery(error.to_string()))
    }

    fn send_delta(
        &self,
        chat_id: &str,
        delta: &str,
        metadata: Map<String, Value>,
    ) -> Result<(), ChannelError> {
        let reply_to = metadata_string(&metadata, "reply_to");
        let mut message =
            OutboundMessage::new(&self.channel, chat_id, delta).with_metadata(metadata);
        message.reply_to = reply_to;
        self.send(message)
    }
}

#[derive(Clone, Default)]
struct WebSocketEventSink {
    events: Arc<Mutex<Vec<WebSocketServerEvent>>>,
}

impl WebSocketEventSink {
    fn push(&self, event: WebSocketServerEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    fn take_events(&self) -> Vec<WebSocketServerEvent> {
        self.events
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }
}

struct WebSocketEventChannelAdapter {
    sink: WebSocketEventSink,
}

impl WebSocketEventChannelAdapter {
    fn new(sink: WebSocketEventSink) -> Self {
        Self { sink }
    }
}

impl ChannelAdapter for WebSocketEventChannelAdapter {
    fn name(&self) -> &str {
        WEBSOCKET_CHANNEL
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        Ok(())
    }

    fn send(&self, message: OutboundMessage) -> Result<(), ChannelError> {
        self.sink.push(websocket_event_from_outbound(message));
        Ok(())
    }

    fn send_delta(
        &self,
        chat_id: &str,
        delta: &str,
        metadata: Map<String, Value>,
    ) -> Result<(), ChannelError> {
        self.send(OutboundMessage::new(WEBSOCKET_CHANNEL, chat_id, delta).with_metadata(metadata))
    }
}

fn websocket_event_channel_manager(
    send_max_retries: u32,
    sink: WebSocketEventSink,
) -> ChannelManager {
    let mut manager =
        ChannelManager::new().with_retry_policy(channel_retry_policy(send_max_retries));
    if let Err(error) =
        manager.register_adapter(Box::new(WebSocketEventChannelAdapter::new(sink)), true)
    {
        eprintln!("websocket channel adapter registration failed: {error}");
    }
    if let Err(error) = manager.start_all() {
        eprintln!("websocket channel adapter startup failed: {error}");
    }
    manager
}

fn stream_metadata(stream_id: &str, end: bool) -> Map<String, Value> {
    let mut metadata = Map::new();
    metadata.insert("_stream_id".to_owned(), json!(stream_id));
    if end {
        metadata.insert("_stream_end".to_owned(), json!(true));
    } else {
        metadata.insert("_stream_delta".to_owned(), json!(true));
    }
    metadata
}

fn stream_outbound_message(
    channel: &str,
    chat_id: &str,
    stream_id: &str,
    content: String,
    end: bool,
) -> OutboundMessage {
    OutboundMessage::new(channel, chat_id, content).with_metadata(stream_metadata(stream_id, end))
}

fn stream_outbound_message_with_routing(
    channel: &str,
    chat_id: &str,
    stream_id: &str,
    content: String,
    end: bool,
    routing_metadata: &Map<String, Value>,
    reply_to: Option<&str>,
) -> OutboundMessage {
    let mut metadata = stream_metadata(stream_id, end);
    metadata.extend(routing_metadata.clone());
    let mut message = OutboundMessage::new(channel, chat_id, content).with_metadata(metadata);
    if let Some(reply_to) = reply_to {
        message.reply_to = Some(reply_to.to_owned());
    }
    message
}

fn stream_routing_metadata_from_inbound(inbound: &InboundMessage) -> Map<String, Value> {
    let mut metadata = Map::new();
    for key in [
        "message_thread_id",
        "subject",
        "parent_channel_id",
        "thread_id",
    ] {
        if let Some(value) = inbound.metadata.get(key).cloned() {
            metadata.insert(key.to_owned(), value);
        }
    }
    if let Some(value) = inbound.metadata.get("slack").cloned() {
        metadata.insert("slack".to_owned(), value);
    }
    metadata
}

fn inbound_reply_to(inbound: &InboundMessage) -> Option<String> {
    inbound
        .metadata
        .get("message_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn effective_external_session_key(config: &AgentLoopConfig, message: &InboundMessage) -> String {
    if message.session_key_override.is_some() {
        message.session_key()
    } else {
        config
            .unified_session_key
            .clone()
            .unwrap_or_else(|| message.session_key())
    }
}

fn provider_streaming_channel(channel: &str) -> bool {
    matches!(
        channel,
        WEBSOCKET_CHANNEL | TELEGRAM_CHANNEL | DISCORD_CHANNEL | SLACK_CHANNEL
    )
}

fn message_metadata_bool(message: &OutboundMessage, key: &str) -> bool {
    match message.metadata.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => matches!(value.as_str(), "true" | "1" | "yes"),
        Some(Value::Number(value)) => value.as_i64().is_some_and(|number| number != 0),
        _ => false,
    }
}

fn metadata_string(metadata: &Map<String, Value>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn websocket_media_names(metadata: &Map<String, Value>) -> Vec<Option<String>> {
    metadata
        .get("media_names")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|value| !value.trim().is_empty())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn message_is_stream_delta(message: &OutboundMessage) -> bool {
    message_metadata_bool(message, "_stream_delta")
}

fn message_is_stream_end(message: &OutboundMessage) -> bool {
    message_metadata_bool(message, "_stream_end")
}

fn message_is_typing_indicator(message: &OutboundMessage) -> bool {
    message_metadata_bool(message, EXTERNAL_TYPING_INDICATOR_KEY)
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct EmailSeenUidState {
    uid_validity: Option<u32>,
    seen_uids: BTreeSet<String>,
    seen_uid_order: VecDeque<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct DiscordGatewayResumeState {
    session_id: Option<String>,
    sequence: Option<i64>,
    resume_gateway_url: Option<String>,
    bot_user_id: Option<String>,
    token_hash: Option<String>,
}

struct DiscordGatewaySessionContext<'a> {
    send_attempts: usize,
    metadata_path: &'a Path,
    resume_state: &'a mut DiscordGatewayResumeState,
}

const DISCORD_EXTERNAL_MESSAGE_LIMIT: usize = 2000;
const TELEGRAM_EXTERNAL_MESSAGE_LIMIT: usize = 4000;
const SLACK_EXTERNAL_MESSAGE_LIMIT: usize = 39000;

fn external_message_chunks(content: &str, max_chars: usize) -> Vec<String> {
    if content.is_empty() {
        return vec![String::new()];
    }
    if max_chars == 0 || content.chars().count() <= max_chars {
        return vec![content.to_owned()];
    }

    let mut chunks = Vec::new();
    let mut remaining = content;
    while !remaining.is_empty() {
        if remaining.chars().count() <= max_chars {
            chunks.push(remaining.to_owned());
            break;
        }

        let cut_byte = byte_index_after_chars(remaining, max_chars);
        let candidate = &remaining[..cut_byte];
        let split = boundary_split_index(candidate).unwrap_or(cut_byte);
        chunks.push(remaining[..split].to_owned());
        remaining = remaining[split..].trim_start();
    }
    chunks
}

fn byte_index_after_chars(content: &str, max_chars: usize) -> usize {
    content
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(content.len())
}

fn boundary_split_index(candidate: &str) -> Option<usize> {
    ["\n\n", "\n", " "]
        .into_iter()
        .find_map(|delimiter| candidate.rfind(delimiter).filter(|index| *index > 0))
}

fn send_split_external_messages<S>(
    text: &str,
    max_chars: usize,
    mut send_new: S,
) -> Result<(String, String), String>
where
    S: FnMut(&str) -> Result<String, String>,
{
    let chunks = external_message_chunks(text, max_chars);
    let mut active_remote_id = String::new();
    let mut active_text = String::new();
    for chunk in chunks {
        active_remote_id = send_new(&chunk)?;
        active_text = chunk;
    }
    Ok((active_remote_id, active_text))
}

#[cfg(test)]
fn outbound_route_key(message: &OutboundMessage) -> String {
    let slack_thread = slack_outbound_thread_ts(message).unwrap_or_default();
    let telegram_thread =
        metadata_string(&message.metadata, "message_thread_id").unwrap_or_default();
    let discord_thread = metadata_string(&message.metadata, "thread_id").unwrap_or_default();
    let discord_parent =
        metadata_string(&message.metadata, "parent_channel_id").unwrap_or_default();
    let reply_to = message.reply_to.clone().unwrap_or_default();
    format!(
        "{}\u{1f}{}\u{1f}tg={}\u{1f}sl={}\u{1f}dp={}\u{1f}dt={}\u{1f}rp={}",
        message.channel,
        message.chat_id,
        telegram_thread,
        slack_thread,
        discord_parent,
        discord_thread,
        reply_to
    )
}

fn load_metadata_json(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn save_metadata_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension(format!("json.tmp-{}", now_millis()));
    let payload = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    {
        let mut file = fs::File::create(&temp).map_err(|error| error.to_string())?;
        file.write_all(&payload)
            .map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    fs::rename(&temp, path).map_err(|error| error.to_string())?;
    Ok(())
}

fn delivery_record_id(message: &OutboundMessage) -> String {
    let mut digest = Sha256::new();
    digest.update(message.channel.as_bytes());
    digest.update(b"\0");
    digest.update(message.chat_id.as_bytes());
    digest.update(b"\0");
    digest.update(message.content.as_bytes());
    if let Some(reply_to) = message.reply_to.as_deref() {
        digest.update(b"\0");
        digest.update(reply_to.as_bytes());
    }
    hex_lower(&digest.finalize())
}

fn record_delivery_state(
    path: &Path,
    message: &OutboundMessage,
    status: &str,
    last_error: Option<&str>,
) {
    let mut root = load_metadata_json(path);
    if !root.get("deliveries").is_some_and(Value::is_array) {
        root["deliveries"] = json!([]);
    }
    let record = json!({
        "id": delivery_record_id(message),
        "channel": message.channel,
        "chat_id": message.chat_id,
        "reply_to": message.reply_to,
        "content_sha256": sha256_hex(&message.content),
        "status": status,
        "updated_at": now_millis(),
        "last_error": last_error,
    });
    if let Some(deliveries) = root["deliveries"].as_array_mut() {
        deliveries.push(record);
        let keep_from = deliveries.len().saturating_sub(128);
        if keep_from > 0 {
            deliveries.drain(0..keep_from);
        }
    }
    if let Err(error) = save_metadata_json(path, &root) {
        eprintln!("delivery metadata save failed: {error}");
    }
}

fn sha256_hex(input: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(input.as_bytes());
    hex_lower(&digest.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}

fn load_telegram_offset(path: &Path) -> i64 {
    load_metadata_json(path)
        .get("offset")
        .and_then(Value::as_i64)
        .unwrap_or_default()
}

fn save_telegram_offset(path: &Path, offset: i64) {
    let mut root = load_metadata_json(path);
    root["offset"] = json!(offset);
    if let Err(error) = save_metadata_json(path, &root) {
        eprintln!("telegram metadata save failed: {error}");
    }
}

fn load_discord_last_ids(path: &Path) -> BTreeMap<String, String> {
    load_metadata_json(path)
        .get("last_ids")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(channel, message)| {
            message
                .as_str()
                .map(|message| (channel.clone(), message.to_owned()))
        })
        .collect()
}

fn save_discord_last_ids(path: &Path, last_ids: &BTreeMap<String, String>) {
    let mut root = load_metadata_json(path);
    root["last_ids"] = json!(last_ids);
    if let Err(error) = save_metadata_json(path, &root) {
        eprintln!("discord metadata save failed: {error}");
    }
}

fn email_metadata_key(config: &EmailImapRuntimeConfig) -> String {
    format!(
        "{}:{}:{}:{}",
        config.host, config.port, config.username, config.mailbox
    )
}

fn load_email_seen_uid_state(path: &Path, key: &str) -> EmailSeenUidState {
    let root = load_metadata_json(path);
    if let Some(entry) = root
        .get("mailboxes")
        .and_then(|mailboxes| mailboxes.get(key))
        .and_then(Value::as_object)
    {
        let order = entry
            .get("seen")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<VecDeque<_>>();
        return EmailSeenUidState {
            uid_validity: entry
                .get("uid_validity")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            seen_uids: order.iter().cloned().collect(),
            seen_uid_order: order,
        };
    }
    let order = root
        .get("seen")
        .and_then(|seen| seen.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<VecDeque<_>>();
    EmailSeenUidState {
        uid_validity: None,
        seen_uids: order.iter().cloned().collect(),
        seen_uid_order: order,
    }
}

fn save_email_seen_uid_state(path: &Path, key: &str, state: &EmailSeenUidState) {
    let mut root = load_metadata_json(path);
    if !root.get("mailboxes").is_some_and(Value::is_object) {
        root["mailboxes"] = json!({});
    }
    root["mailboxes"][key] = json!({
        "uid_validity": state.uid_validity,
        "seen": state
            .seen_uid_order
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>()
    });
    if let Err(error) = save_metadata_json(path, &root) {
        eprintln!("email metadata save failed: {error}");
    }
}

fn apply_email_uid_validity(state: &mut EmailSeenUidState, current: Option<u32>) {
    if state.uid_validity.is_some() && current.is_some() && state.uid_validity != current {
        state.seen_uids.clear();
        state.seen_uid_order.clear();
    }
    if current.is_some() {
        state.uid_validity = current;
    }
}

fn load_discord_gateway_resume_state(path: &Path, token: &str) -> DiscordGatewayResumeState {
    let root = load_metadata_json(path);
    let expected_hash = sha256_hex(token);
    if root
        .get("token_hash")
        .and_then(Value::as_str)
        .is_some_and(|hash| hash != expected_hash)
    {
        return DiscordGatewayResumeState::default();
    }
    let state = DiscordGatewayResumeState {
        session_id: root
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        sequence: root.get("sequence").and_then(Value::as_i64),
        resume_gateway_url: root
            .get("resume_gateway_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
        bot_user_id: root
            .get("bot_user_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        token_hash: Some(expected_hash),
    };
    if state.session_id.is_none()
        && state.sequence.is_none()
        && state.resume_gateway_url.is_none()
        && state.bot_user_id.is_none()
    {
        DiscordGatewayResumeState::default()
    } else {
        state
    }
}

fn save_discord_gateway_resume_state(path: &Path, state: &DiscordGatewayResumeState, token: &str) {
    let mut root = load_metadata_json(path);
    root["session_id"] = json!(state.session_id);
    root["sequence"] = json!(state.sequence);
    root["resume_gateway_url"] = json!(state.resume_gateway_url);
    root["bot_user_id"] = json!(state.bot_user_id);
    root["token_hash"] = json!(state
        .token_hash
        .clone()
        .unwrap_or_else(|| sha256_hex(token)));
    root["updated_at"] = json!(now_millis());
    if let Err(error) = save_metadata_json(path, &root) {
        eprintln!("discord gateway metadata save failed: {error}");
    }
}

fn clear_discord_gateway_resume_state(path: &Path) {
    let mut root = load_metadata_json(path);
    if let Some(object) = root.as_object_mut() {
        for key in [
            "session_id",
            "sequence",
            "resume_gateway_url",
            "bot_user_id",
            "token_hash",
            "updated_at",
        ] {
            object.remove(key);
        }
    }
    if let Err(error) = save_metadata_json(path, &root) {
        eprintln!("discord gateway metadata clear failed: {error}");
    }
}

fn discord_gateway_identify_payload(token: &str) -> Value {
    json!({
        "op": 2,
        "d": {
            "token": token,
            "intents": DISCORD_GATEWAY_INTENTS,
            "properties": {
                "os": std::env::consts::OS,
                "browser": "shacs-bot",
                "device": "shacs-bot"
            }
        }
    })
}

fn discord_gateway_resume_payload(token: &str, session_id: &str, sequence: i64) -> Value {
    json!({
        "op": 6,
        "d": {
            "token": token,
            "session_id": session_id,
            "seq": sequence
        }
    })
}

fn discord_gateway_can_resume(state: &DiscordGatewayResumeState) -> Option<(&str, i64)> {
    Some((state.session_id.as_deref()?, state.sequence?))
}

fn run_external_transport_worker(
    spec: ExternalTransportSpec,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    match spec {
        ExternalTransportSpec::Telegram(config) => {
            run_telegram_transport(config, inbound_bus, outbound_rx, stop, transport_context)
        }
        ExternalTransportSpec::Discord(config) => {
            run_discord_transport(config, inbound_bus, outbound_rx, stop, transport_context)
        }
        ExternalTransportSpec::Slack(config) => {
            run_slack_transport(config, inbound_bus, outbound_rx, stop, transport_context)
        }
        ExternalTransportSpec::Email(config) => {
            run_email_transport(config, inbound_bus, outbound_rx, stop, transport_context)
        }
        ExternalTransportSpec::WhatsApp(config) => {
            run_whatsapp_transport(config, inbound_bus, outbound_rx, stop, transport_context)
        }
    }
}

fn run_telegram_transport(
    config: TelegramRuntimeConfig,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let agent = runtime_http_agent(Duration::from_secs(config.poll_timeout_seconds + 10));
    let metadata_path = transport_context.metadata_path("telegram");
    let mut offset = load_telegram_offset(&metadata_path);
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(
            &outbound_rx,
            transport_context.send_attempts,
            &stop,
            Some(&metadata_path),
            |message| send_telegram_message(&agent, &config, message),
        );
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
                        if let Some(inbound) =
                            telegram_update_to_inbound_with_download(&agent, &config.token, update)
                        {
                            inbound_bus.publish_inbound(inbound);
                        }
                    }
                    save_telegram_offset(&metadata_path, offset);
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
    if message_is_stream_delta(&message) || message_is_stream_end(&message) {
        return Ok(());
    }
    let mut chunk_index = 0usize;
    send_split_external_messages(&message.content, TELEGRAM_EXTERNAL_MESSAGE_LIMIT, |text| {
        let include_reply = chunk_index == 0;
        chunk_index += 1;
        let value = post_json(
            agent,
            &telegram_url(&config.token, "sendMessage"),
            None,
            telegram_message_body(&message, text, None, include_reply),
        )?;
        value
            .get("result")
            .and_then(|result| result.get("message_id"))
            .and_then(json_id_string)
            .ok_or_else(|| "Telegram sendMessage response missing message_id".to_owned())
    })?;
    Ok(())
}

fn telegram_message_body(
    message: &OutboundMessage,
    text: &str,
    remote_message_id: Option<&str>,
    include_reply: bool,
) -> Value {
    let mut body = Map::new();
    body.insert("chat_id".to_owned(), Value::String(message.chat_id.clone()));
    body.insert("text".to_owned(), Value::String(text.to_owned()));
    if let Some(remote_message_id) = remote_message_id {
        body.insert(
            "message_id".to_owned(),
            Value::String(remote_message_id.to_owned()),
        );
    }
    if let Some(thread_id) = metadata_string(&message.metadata, "message_thread_id") {
        body.insert("message_thread_id".to_owned(), Value::String(thread_id));
    }
    if include_reply {
        if let Some(reply_to) = message.reply_to.as_deref() {
            body.insert(
                "reply_parameters".to_owned(),
                json!({ "message_id": reply_to }),
            );
        }
    }
    Value::Object(body)
}

fn telegram_update_to_inbound(update: &Value) -> Option<InboundMessage> {
    let message = update
        .get("message")
        .or_else(|| update.get("edited_message"))?;
    let media = telegram_message_media(message);
    let content = message
        .get("text")
        .or_else(|| message.get("caption"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if content.trim().is_empty() && media.is_empty() {
        return None;
    }
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
            media,
        }
        .into_message(),
    )
}

fn telegram_update_to_inbound_with_download(
    agent: &ureq::Agent,
    token: &str,
    update: &Value,
) -> Option<InboundMessage> {
    let message = update
        .get("message")
        .or_else(|| update.get("edited_message"))?;
    let mut inbound = telegram_update_to_inbound(update)?;
    inbound.media = telegram_message_media_data_urls(agent, token, message);
    Some(inbound)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelegramAttachmentRef {
    kind: &'static str,
    file_id: String,
    mime: Option<String>,
    filename: Option<String>,
    size: Option<u64>,
}

fn telegram_message_media(message: &Value) -> Vec<String> {
    telegram_message_attachment_refs(message)
        .into_iter()
        .map(|attachment| telegram_attachment_handle(&attachment))
        .collect()
}

fn telegram_message_attachment_refs(message: &Value) -> Vec<TelegramAttachmentRef> {
    let mut attachments = Vec::new();
    if let Some(photo) = message
        .get("photo")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .max_by_key(|item| item.get("file_size").and_then(Value::as_u64).unwrap_or(0))
        })
    {
        if let Some(attachment) = telegram_file_attachment_ref("photo", photo, None) {
            attachments.push(attachment);
        }
    }
    for kind in ["document", "audio", "video"] {
        if let Some(part) = message.get(kind) {
            if let Some(attachment) = telegram_file_attachment_ref(
                kind,
                part,
                part.get("file_name").and_then(Value::as_str),
            ) {
                attachments.push(attachment);
            }
        }
    }
    attachments
}

fn telegram_file_attachment_ref(
    kind: &'static str,
    part: &Value,
    filename: Option<&str>,
) -> Option<TelegramAttachmentRef> {
    Some(TelegramAttachmentRef {
        kind,
        file_id: part.get("file_id").and_then(Value::as_str)?.to_owned(),
        mime: part
            .get("mime_type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        filename: filename.map(str::to_owned),
        size: part.get("file_size").and_then(Value::as_u64),
    })
}

fn telegram_attachment_handle(attachment: &TelegramAttachmentRef) -> String {
    platform_media_handle(
        "telegram",
        attachment.kind,
        Some(&attachment.file_id),
        attachment.mime.as_deref(),
        attachment.filename.as_deref(),
        attachment.size,
    )
}

fn telegram_message_media_data_urls(
    agent: &ureq::Agent,
    token: &str,
    message: &Value,
) -> Vec<String> {
    telegram_message_attachment_refs(message)
        .into_iter()
        .map(|attachment| {
            telegram_attachment_data_url(agent, token, &attachment)
                .unwrap_or_else(|_| telegram_attachment_handle(&attachment))
        })
        .collect()
}

fn telegram_attachment_data_url(
    agent: &ureq::Agent,
    token: &str,
    attachment: &TelegramAttachmentRef,
) -> Result<String, String> {
    if attachment
        .size
        .is_some_and(|size| size > shacs_api::MAX_MEDIA_BYTES as u64)
    {
        return Err("telegram attachment exceeds storage limit".to_owned());
    }
    let file = post_json(
        agent,
        &telegram_url(token, "getFile"),
        None,
        json!({ "file_id": attachment.file_id }),
    )?;
    let file_path = file
        .get("result")
        .and_then(|result| result.get("file_path"))
        .and_then(Value::as_str)
        .filter(|path| telegram_file_path_is_safe(path))
        .ok_or_else(|| "Telegram getFile response missing safe file_path".to_owned())?;
    let url = format!("https://api.telegram.org/file/bot{token}/{file_path}");
    let (bytes, response_mime) = get_binary(agent, &url, None)?;
    let mime = attachment
        .mime
        .as_deref()
        .or(response_mime.as_deref())
        .unwrap_or("application/octet-stream");
    Ok(data_url_with_optional_name(
        mime,
        attachment.filename.as_deref(),
        &bytes,
    ))
}

fn telegram_file_path_is_safe(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains("..") && !path.contains('\\')
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
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    match config.transport {
        DiscordTransportMode::Gateway => {
            run_discord_gateway_transport(config, inbound_bus, outbound_rx, stop, transport_context)
        }
        DiscordTransportMode::RestPolling => run_discord_rest_polling_transport(
            config,
            inbound_bus,
            outbound_rx,
            stop,
            transport_context,
        ),
    }
}

fn run_discord_rest_polling_transport(
    config: DiscordRuntimeConfig,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let agent = runtime_http_agent(Duration::from_secs(30));
    let metadata_path = transport_context.metadata_path("discord-rest");
    let mut last_ids = load_discord_last_ids(&metadata_path);
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(
            &outbound_rx,
            transport_context.send_attempts,
            &stop,
            Some(&metadata_path),
            |message| send_discord_message(&agent, &config, message),
        );
        let DiscordChannelFilter::Only(channel_ids) = &config.channel_filter else {
            eprintln!("discord REST polling requires configured channel ids");
            sleep_with_stop(&stop, Duration::from_secs(config.poll_interval_seconds));
            continue;
        };
        for channel_id in channel_ids {
            match poll_discord_channel(&agent, &config, channel_id, last_ids.get(channel_id)) {
                Ok((messages, newest)) => {
                    if let Some(newest) = newest {
                        last_ids.insert(channel_id.clone(), newest);
                        save_discord_last_ids(&metadata_path, &last_ids);
                    }
                    for inbound in messages {
                        inbound_bus.publish_inbound(inbound);
                    }
                }
                Err(error) => eprintln!("discord polling failed for {channel_id}: {error}"),
            }
        }
        sleep_with_stop(&stop, Duration::from_secs(config.poll_interval_seconds));
    }
}

const DISCORD_GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const DISCORD_GATEWAY_INTENTS: u64 = (1 << 9) | (1 << 12) | (1 << 15);

fn run_discord_gateway_transport(
    config: DiscordRuntimeConfig,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let agent = runtime_http_agent(Duration::from_secs(30));
    let mut backoff = Duration::from_secs(1);
    let metadata_path = transport_context.metadata_path("discord-gateway");
    let mut resume_state = load_discord_gateway_resume_state(&metadata_path, &config.token);
    while !stop.load(Ordering::SeqCst) {
        match run_discord_gateway_session(
            &config,
            &agent,
            &inbound_bus,
            &outbound_rx,
            &stop,
            DiscordGatewaySessionContext {
                send_attempts: transport_context.send_attempts,
                metadata_path: &metadata_path,
                resume_state: &mut resume_state,
            },
        ) {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(error) => {
                if !stop.load(Ordering::SeqCst) {
                    eprintln!("discord gateway worker reconnecting: {error}");
                    sleep_with_stop(&stop, backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    }
}

fn run_discord_gateway_session(
    config: &DiscordRuntimeConfig,
    agent: &ureq::Agent,
    inbound_bus: &MessageBus,
    outbound_rx: &mpsc::Receiver<OutboundMessage>,
    stop: &Arc<AtomicBool>,
    gateway_context: DiscordGatewaySessionContext<'_>,
) -> Result<(), String> {
    let send_attempts = gateway_context.send_attempts;
    let metadata_path = gateway_context.metadata_path;
    let resume_state = gateway_context.resume_state;
    let gateway_url = resume_state
        .resume_gateway_url
        .as_deref()
        .unwrap_or(DISCORD_GATEWAY_URL);
    let (mut socket, _) = websocket_connect(gateway_url).map_err(|error| error.to_string())?;
    set_websocket_timeouts(&mut socket, Duration::from_millis(500));
    let mut heartbeat_interval = Duration::from_secs(30);
    let mut next_heartbeat = Instant::now() + heartbeat_interval;
    let mut last_seq = resume_state.sequence;
    let mut heartbeat_acknowledged = true;
    let mut bot_user_id = None::<String>;
    let mut recent_ids = RecentMessageIds::new(1024);

    while !stop.load(Ordering::SeqCst) {
        drain_outbound(
            outbound_rx,
            send_attempts,
            stop,
            Some(metadata_path),
            |message| send_discord_message(agent, config, message),
        );
        if Instant::now() >= next_heartbeat {
            if !heartbeat_acknowledged {
                return Err("heartbeat ack timed out".to_owned());
            }
            send_websocket_json(&mut socket, json!({"op": 1, "d": last_seq}))?;
            heartbeat_acknowledged = false;
            next_heartbeat = Instant::now() + heartbeat_interval;
        }

        match socket.read() {
            Ok(message) => {
                let Some(text) = websocket_text(message)? else {
                    continue;
                };
                let value =
                    serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())?;
                if let Some(seq) = value.get("s").and_then(Value::as_i64) {
                    last_seq = Some(seq);
                    resume_state.sequence = Some(seq);
                }
                match value.get("op").and_then(Value::as_i64) {
                    Some(10) => {
                        if let Some(interval) = value
                            .get("d")
                            .and_then(|data| data.get("heartbeat_interval"))
                            .and_then(Value::as_u64)
                        {
                            heartbeat_interval = Duration::from_millis(interval.max(1));
                        }
                        if let Some((session_id, sequence)) =
                            discord_gateway_can_resume(resume_state)
                        {
                            send_websocket_json(
                                &mut socket,
                                discord_gateway_resume_payload(&config.token, session_id, sequence),
                            )?;
                        } else {
                            identify_discord_gateway(&mut socket, &config.token)?;
                        }
                        send_websocket_json(&mut socket, json!({"op": 1, "d": last_seq}))?;
                        heartbeat_acknowledged = false;
                        next_heartbeat = Instant::now() + heartbeat_interval;
                    }
                    Some(11) => heartbeat_acknowledged = true,
                    Some(7) => return Err("gateway requested reconnect".to_owned()),
                    Some(9) => {
                        *resume_state = DiscordGatewayResumeState::default();
                        clear_discord_gateway_resume_state(metadata_path);
                        return Err("gateway invalid session".to_owned());
                    }
                    Some(0) => handle_discord_gateway_dispatch(
                        DiscordGatewayDispatchContext {
                            config,
                            agent,
                            inbound_bus,
                            recent_ids: &mut recent_ids,
                            bot_user_id: &mut bot_user_id,
                            resume_state,
                            metadata_path,
                        },
                        &value,
                    ),
                    _ => {}
                }
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => {
                return Err("gateway connection closed".to_owned())
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let _ = socket.close(None);
    Ok(())
}

fn set_websocket_timeouts(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => set_tcp_stream_timeouts(stream, timeout),
        MaybeTlsStream::NativeTls(stream) => set_tcp_stream_timeouts(stream.get_ref(), timeout),
        _ => {}
    }
}

fn set_tcp_stream_timeouts(stream: &TcpStream, timeout: Duration) {
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
}

fn websocket_text(message: WebSocketMessage) -> Result<Option<String>, String> {
    if message.is_text() {
        return message
            .into_text()
            .map(|text| Some(text.to_string()))
            .map_err(|error| error.to_string());
    }
    Ok(None)
}

fn send_websocket_json(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    value: Value,
) -> Result<(), String> {
    socket
        .send(WebSocketMessage::text(value.to_string()))
        .map_err(|error| error.to_string())
}

fn identify_discord_gateway(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    token: &str,
) -> Result<(), String> {
    send_websocket_json(socket, discord_gateway_identify_payload(token))
}

struct DiscordGatewayDispatchContext<'a> {
    config: &'a DiscordRuntimeConfig,
    agent: &'a ureq::Agent,
    inbound_bus: &'a MessageBus,
    recent_ids: &'a mut RecentMessageIds,
    bot_user_id: &'a mut Option<String>,
    resume_state: &'a mut DiscordGatewayResumeState,
    metadata_path: &'a Path,
}

fn handle_discord_gateway_dispatch(context: DiscordGatewayDispatchContext<'_>, value: &Value) {
    match value.get("t").and_then(Value::as_str) {
        Some("READY") => {
            let data = value.get("d");
            *context.bot_user_id = data
                .and_then(|data| data.get("user"))
                .and_then(|user| user.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            context.resume_state.bot_user_id = context.bot_user_id.clone();
            context.resume_state.session_id = data
                .and_then(|data| data.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            context.resume_state.resume_gateway_url = data
                .and_then(|data| data.get("resume_gateway_url"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            save_discord_gateway_resume_state(
                context.metadata_path,
                context.resume_state,
                &context.config.token,
            );
        }
        Some("MESSAGE_CREATE") => {
            let Some(data) = value.get("d") else {
                return;
            };
            if let Some(inbound) = discord_gateway_message_to_inbound_with_download(
                context.config,
                context.agent,
                context.bot_user_id.as_deref(),
                context.recent_ids,
                data,
            ) {
                context.inbound_bus.publish_inbound(inbound);
            }
            save_discord_gateway_resume_state(
                context.metadata_path,
                context.resume_state,
                &context.config.token,
            );
        }
        _ => {}
    }
}

fn discord_gateway_message_to_inbound(
    config: &DiscordRuntimeConfig,
    bot_user_id: Option<&str>,
    recent_ids: &mut RecentMessageIds,
    item: &Value,
) -> Option<InboundMessage> {
    let message_id = item.get("id").and_then(Value::as_str)?.to_owned();
    if !recent_ids.remember(&message_id) {
        return None;
    }
    let channel_id = item.get("channel_id").and_then(Value::as_str)?.to_owned();
    let parent_channel_id = item
        .get("parent_channel_id")
        .or_else(|| item.get("parent_id"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !discord_channel_allowed(
        &config.channel_filter,
        &channel_id,
        parent_channel_id.as_deref(),
    ) {
        return None;
    }
    let author = item.get("author")?;
    if author.get("bot").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let sender_id = author.get("id").and_then(Value::as_str)?.to_owned();
    if bot_user_id.is_some_and(|bot| bot == sender_id) {
        return None;
    }
    if !sender_allowed_for_rest(&config.allowed_senders, &sender_id) {
        return None;
    }
    let guild_id = item
        .get("guild_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mentioned = bot_user_id.is_some_and(|bot| discord_message_mentions_bot(item, bot));
    if guild_id.is_some() && config.group_policy == DiscordGroupPolicy::Mention && !mentioned {
        return None;
    }
    let attachments = discord_attachment_media(item);
    let mut content = item
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if let Some(bot) = bot_user_id {
        content = strip_discord_bot_mention(&content, bot).to_owned();
    }
    if content.trim().is_empty() && attachments.is_empty() {
        return None;
    }
    Some(
        DiscordInbound {
            sender_id,
            channel_id,
            content: content.trim().to_owned(),
            message_id: Some(message_id),
            guild_id,
            parent_channel_id,
            thread_id: item
                .get("thread_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            attachments,
        }
        .into_message(),
    )
}

fn discord_gateway_message_to_inbound_with_download(
    config: &DiscordRuntimeConfig,
    agent: &ureq::Agent,
    bot_user_id: Option<&str>,
    recent_ids: &mut RecentMessageIds,
    item: &Value,
) -> Option<InboundMessage> {
    let mut inbound = discord_gateway_message_to_inbound(config, bot_user_id, recent_ids, item)?;
    inbound.media = discord_attachment_data_urls(agent, item);
    Some(inbound)
}

fn discord_channel_allowed(
    filter: &DiscordChannelFilter,
    channel_id: &str,
    parent_channel_id: Option<&str>,
) -> bool {
    match filter {
        DiscordChannelFilter::AllVisible => true,
        DiscordChannelFilter::Only(channel_ids) => channel_ids.iter().any(|allowed| {
            allowed == "*" || allowed == channel_id || parent_channel_id == Some(allowed.as_str())
        }),
    }
}

fn discord_message_mentions_bot(item: &Value, bot_user_id: &str) -> bool {
    item.get("mentions")
        .and_then(Value::as_array)
        .is_some_and(|mentions| {
            mentions.iter().any(|mention| {
                mention
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == bot_user_id)
            })
        })
}

fn strip_discord_bot_mention<'a>(content: &'a str, bot_user_id: &str) -> &'a str {
    let trimmed = content.trim_start();
    let plain = format!("<@{bot_user_id}>");
    let nickname = format!("<@!{bot_user_id}>");
    trimmed
        .strip_prefix(&plain)
        .or_else(|| trimmed.strip_prefix(&nickname))
        .map(str::trim_start)
        .unwrap_or(trimmed)
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
        if let Some(message) = discord_rest_message_to_inbound(agent, config, channel_id, item) {
            messages.push(message);
        }
    }
    Ok((messages, newest))
}

fn discord_rest_message_to_inbound(
    agent: &ureq::Agent,
    config: &DiscordRuntimeConfig,
    channel_id: &str,
    item: &Value,
) -> Option<InboundMessage> {
    let content = item
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let attachment_media = discord_attachment_media(item);
    if content.trim().is_empty() && attachment_media.is_empty() {
        return None;
    }
    let sender_id = item
        .get("author")
        .and_then(|author| author.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("discord-user")
        .to_owned();
    if !sender_allowed_for_rest(&config.allowed_senders, &sender_id) {
        return None;
    }
    Some(
        DiscordInbound {
            sender_id,
            channel_id: channel_id.to_owned(),
            content: content.trim().to_owned(),
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
            attachments: discord_attachment_data_urls(agent, item),
        }
        .into_message(),
    )
}

fn send_discord_message(
    agent: &ureq::Agent,
    config: &DiscordRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    if message_is_typing_indicator(&message) {
        if let Err(error) = send_discord_typing_indicator(agent, config, &message.chat_id) {
            eprintln!("discord typing indicator failed: {error}");
        }
        return Ok(());
    }
    let url = format!(
        "https://discord.com/api/v10/channels/{}/messages",
        message.chat_id
    );
    if message_is_stream_delta(&message) || message_is_stream_end(&message) {
        return Ok(());
    }
    for (index, chunk) in discord_message_chunks(&message.content)
        .into_iter()
        .enumerate()
    {
        let reply_to = (index == 0)
            .then_some(message.reply_to.as_deref())
            .flatten();
        let body = discord_message_body(&message.chat_id, &chunk, reply_to);
        post_json(agent, &url, Some(discord_auth_header(&config.token)), body)?;
    }
    Ok(())
}

fn discord_message_body(channel_id: &str, content: &str, reply_to: Option<&str>) -> Value {
    let mut body = json!({
        "content": content,
        "allowed_mentions": {
            "parse": [],
            "replied_user": false
        }
    });
    if let Some(reply_to) = reply_to {
        body["message_reference"] = json!({
            "message_id": reply_to,
            "channel_id": channel_id,
            "fail_if_not_exists": false
        });
    }
    body
}

fn send_discord_typing_indicator(
    agent: &ureq::Agent,
    config: &DiscordRuntimeConfig,
    channel_id: &str,
) -> Result<(), String> {
    post_empty(
        agent,
        &discord_typing_url(channel_id),
        Some(discord_auth_header(&config.token)),
    )
}

fn discord_typing_url(channel_id: &str) -> String {
    format!("https://discord.com/api/v10/channels/{channel_id}/typing")
}

fn discord_message_chunks(content: &str) -> Vec<String> {
    external_message_chunks(content, DISCORD_EXTERNAL_MESSAGE_LIMIT)
}

fn discord_auth_header(token: &str) -> String {
    format!("Bot {token}")
}

fn run_slack_transport(
    config: SlackRuntimeConfig,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let agent = runtime_http_agent(Duration::from_secs(30));
    let mut backoff = Duration::from_secs(1);
    let metadata_path = transport_context.metadata_path("slack");
    while !stop.load(Ordering::SeqCst) {
        match run_slack_socket_mode_session(
            &config,
            &agent,
            &inbound_bus,
            &outbound_rx,
            &stop,
            transport_context.send_attempts,
            &metadata_path,
        ) {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(error) => {
                if !stop.load(Ordering::SeqCst) {
                    eprintln!("slack socket mode worker reconnecting: {error}");
                    sleep_with_stop(&stop, backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    }
}

fn run_slack_socket_mode_session(
    config: &SlackRuntimeConfig,
    agent: &ureq::Agent,
    inbound_bus: &MessageBus,
    outbound_rx: &mpsc::Receiver<OutboundMessage>,
    stop: &Arc<AtomicBool>,
    send_attempts: usize,
    metadata_path: &Path,
) -> Result<(), String> {
    let url = open_slack_socket_mode_url(agent, &config.app_token)?;
    let (mut socket, _) = websocket_connect(url.as_str()).map_err(|error| error.to_string())?;
    set_websocket_timeouts(&mut socket, Duration::from_millis(500));
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(
            outbound_rx,
            send_attempts,
            stop,
            Some(metadata_path),
            |message| send_slack_message(agent, config, message),
        );
        match socket.read() {
            Ok(message) => {
                let Some(text) = websocket_text(message)? else {
                    continue;
                };
                let envelope =
                    serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())?;
                if let Some(ack) = slack_socket_ack_frame(&envelope) {
                    send_websocket_json(&mut socket, ack)?;
                }
                if let Some(inbound) =
                    slack_socket_envelope_to_inbound_with_download(config, agent, &envelope)
                {
                    inbound_bus.publish_inbound(inbound);
                }
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => {
                return Err("socket mode connection closed".to_owned())
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let _ = socket.close(None);
    Ok(())
}

fn open_slack_socket_mode_url(agent: &ureq::Agent, app_token: &str) -> Result<String, String> {
    let value = post_json(
        agent,
        "https://slack.com/api/apps.connections.open",
        Some(bearer_header(app_token)),
        json!({}),
    )?;
    if !value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(format!(
            "Slack API error: {}",
            value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
    }
    value
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "Slack apps.connections.open response missing url".to_owned())
}

fn slack_socket_ack_frame(envelope: &Value) -> Option<Value> {
    envelope
        .get("envelope_id")
        .and_then(Value::as_str)
        .map(|envelope_id| json!({"envelope_id": envelope_id}))
}

fn slack_socket_envelope_to_inbound(
    config: &SlackRuntimeConfig,
    envelope: &Value,
) -> Option<InboundMessage> {
    let event = envelope.get("payload")?.get("event")?;
    match event.get("type").and_then(Value::as_str) {
        Some("message") | Some("app_mention") => {}
        _ => return None,
    }
    let subtype = event.get("subtype").and_then(Value::as_str);
    if (subtype.is_some() && subtype != Some("file_share")) || event.get("bot_id").is_some() {
        return None;
    }
    let user_id = event.get("user").and_then(Value::as_str)?.to_owned();
    if slack_envelope_bot_user_ids(envelope)
        .iter()
        .any(|bot_user_id| bot_user_id == &user_id)
    {
        return None;
    }
    if !sender_allowed_for_rest(&config.allowed_senders, &user_id) {
        return None;
    }
    let channel_id = event.get("channel").and_then(Value::as_str)?.to_owned();
    if !slack_channel_allowed(&config.channel_ids, &channel_id) {
        return None;
    }
    let files = slack_file_media(event);
    let content = event
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if content.is_empty() && files.is_empty() {
        return None;
    }
    Some(
        SlackInbound {
            user_id,
            channel_id,
            content,
            event_ts: event.get("ts").and_then(Value::as_str).map(str::to_owned),
            thread_ts: event
                .get("thread_ts")
                .and_then(Value::as_str)
                .map(str::to_owned),
            channel_type: event
                .get("channel_type")
                .and_then(Value::as_str)
                .map(str::to_owned),
            files,
        }
        .into_message(),
    )
}

fn slack_socket_envelope_to_inbound_with_download(
    config: &SlackRuntimeConfig,
    agent: &ureq::Agent,
    envelope: &Value,
) -> Option<InboundMessage> {
    let event = envelope.get("payload")?.get("event")?;
    let mut inbound = slack_socket_envelope_to_inbound(config, envelope)?;
    inbound.media = slack_file_data_urls(agent, &config.bot_token, event);
    Some(inbound)
}

fn discord_attachment_media(item: &Value) -> Vec<String> {
    item.get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attachment| {
            let url = attachment.get("url").and_then(Value::as_str)?;
            Some(platform_media_handle(
                "discord",
                "attachment",
                Some(url),
                attachment.get("content_type").and_then(Value::as_str),
                attachment.get("filename").and_then(Value::as_str),
                attachment.get("size").and_then(Value::as_u64),
            ))
        })
        .collect()
}

fn discord_attachment_data_urls(agent: &ureq::Agent, item: &Value) -> Vec<String> {
    item.get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|attachment| {
            let url = attachment.get("url").and_then(Value::as_str)?;
            let fallback = platform_media_handle(
                "discord",
                "attachment",
                Some(url),
                attachment.get("content_type").and_then(Value::as_str),
                attachment.get("filename").and_then(Value::as_str),
                attachment.get("size").and_then(Value::as_u64),
            );
            Some(discord_attachment_data_url(agent, attachment).unwrap_or(fallback))
        })
        .collect()
}

fn slack_file_media(event: &Value) -> Vec<String> {
    event
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let url = file
                .get("url_private_download")
                .or_else(|| file.get("url_private"))
                .and_then(Value::as_str)?;
            let name = file
                .get("name")
                .or_else(|| file.get("title"))
                .and_then(Value::as_str);
            Some(platform_media_handle(
                "slack",
                "file",
                Some(url),
                file.get("mimetype").and_then(Value::as_str),
                name,
                file.get("size").and_then(Value::as_u64),
            ))
        })
        .collect()
}

fn slack_file_data_urls(agent: &ureq::Agent, bot_token: &str, event: &Value) -> Vec<String> {
    event
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let url = file
                .get("url_private_download")
                .or_else(|| file.get("url_private"))
                .and_then(Value::as_str)?;
            let name = file
                .get("name")
                .or_else(|| file.get("title"))
                .and_then(Value::as_str);
            let fallback = platform_media_handle(
                "slack",
                "file",
                Some(url),
                file.get("mimetype").and_then(Value::as_str),
                name,
                file.get("size").and_then(Value::as_u64),
            );
            Some(slack_file_data_url(agent, bot_token, file).unwrap_or(fallback))
        })
        .collect()
}

fn discord_attachment_data_url(agent: &ureq::Agent, attachment: &Value) -> Result<String, String> {
    let url = attachment
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| {
            platform_attachment_url_allowed(url, &["cdn.discordapp.com", "media.discordapp.net"])
        })
        .ok_or_else(|| "Discord attachment URL is not allowed".to_owned())?;
    let declared_size = attachment.get("size").and_then(Value::as_u64);
    if declared_size.is_some_and(|size| size > shacs_api::MAX_MEDIA_BYTES as u64) {
        return Err("Discord attachment exceeds storage limit".to_owned());
    }
    let (bytes, response_mime) = get_binary(agent, url, None)?;
    let mime = attachment
        .get("content_type")
        .and_then(Value::as_str)
        .or(response_mime.as_deref())
        .unwrap_or("application/octet-stream");
    Ok(data_url_with_optional_name(
        mime,
        attachment.get("filename").and_then(Value::as_str),
        &bytes,
    ))
}

fn slack_file_data_url(
    agent: &ureq::Agent,
    bot_token: &str,
    file: &Value,
) -> Result<String, String> {
    let url = file
        .get("url_private_download")
        .or_else(|| file.get("url_private"))
        .and_then(Value::as_str)
        .filter(|url| platform_attachment_url_allowed(url, &["files.slack.com", "slack-files.com"]))
        .ok_or_else(|| "Slack file URL is not allowed".to_owned())?;
    let declared_size = file.get("size").and_then(Value::as_u64);
    if declared_size.is_some_and(|size| size > shacs_api::MAX_MEDIA_BYTES as u64) {
        return Err("Slack file exceeds storage limit".to_owned());
    }
    let (bytes, response_mime) = get_binary(agent, url, Some(bearer_header(bot_token)))?;
    let name = file
        .get("name")
        .or_else(|| file.get("title"))
        .and_then(Value::as_str);
    let mime = file
        .get("mimetype")
        .and_then(Value::as_str)
        .or(response_mime.as_deref())
        .unwrap_or("application/octet-stream");
    Ok(data_url_with_optional_name(mime, name, &bytes))
}

fn platform_media_handle(
    platform: &str,
    kind: &str,
    opaque_id: Option<&str>,
    mime: Option<&str>,
    filename: Option<&str>,
    size: Option<u64>,
) -> String {
    let id_hash = opaque_id
        .map(short_stable_hash)
        .unwrap_or_else(|| "none".to_owned());
    let mime = mime
        .map(|value| base64_url_no_pad(value.as_bytes()))
        .unwrap_or_default();
    let filename = filename
        .map(|value| base64_url_no_pad(value.as_bytes()))
        .unwrap_or_default();
    let size = size.map(|value| value.to_string()).unwrap_or_default();
    format!("shacs-{platform}-{kind}:{id_hash}:{mime}:{filename}:{size}")
}

fn short_stable_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    base64_url_no_pad(&digest[..12])
}

fn slack_envelope_bot_user_ids(envelope: &Value) -> Vec<String> {
    envelope
        .get("authorizations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|authorization| {
            authorization
                .get("is_bot")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|authorization| authorization.get("user_id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn slack_channel_allowed(channel_ids: &[String], channel_id: &str) -> bool {
    channel_ids.is_empty() || channel_ids.iter().any(|allowed| allowed == channel_id)
}

fn sender_allowed(allowed_senders: &[String], sender_id: &str) -> bool {
    allowed_senders
        .iter()
        .any(|allowed| allowed == "*" || allowed == sender_id)
}

fn sender_allowed_for_rest(allowed_senders: &[String], sender_id: &str) -> bool {
    allowed_senders.is_empty() || sender_allowed(allowed_senders, sender_id)
}

fn send_slack_message(
    agent: &ureq::Agent,
    config: &SlackRuntimeConfig,
    message: OutboundMessage,
) -> Result<(), String> {
    if message_is_typing_indicator(&message) {
        return Ok(());
    }
    let thread_ts = slack_outbound_thread_ts(&message);
    if message_is_stream_delta(&message) || message_is_stream_end(&message) {
        return Ok(());
    }
    send_split_external_messages(&message.content, SLACK_EXTERNAL_MESSAGE_LIMIT, |text| {
        slack_post_message(agent, config, &message.chat_id, text, thread_ts.as_deref())
    })
    .map(|_| ())
}

fn slack_outbound_thread_ts(message: &OutboundMessage) -> Option<String> {
    message
        .metadata
        .get("slack")
        .and_then(|slack| slack.get("thread_ts"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| metadata_string(&message.metadata, "thread_ts"))
}

fn slack_post_message(
    agent: &ureq::Agent,
    config: &SlackRuntimeConfig,
    channel: &str,
    text: &str,
    thread_ts: Option<&str>,
) -> Result<String, String> {
    let body = slack_post_message_body(channel, text, thread_ts);
    let value = post_json(
        agent,
        "https://slack.com/api/chat.postMessage",
        Some(bearer_header(&config.bot_token)),
        body,
    )?;
    if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        value
            .get("ts")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "Slack API response missing ts".to_owned())
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

fn slack_post_message_body(channel: &str, text: &str, thread_ts: Option<&str>) -> Value {
    let mut body = Map::new();
    body.insert("channel".to_owned(), Value::String(channel.to_owned()));
    body.insert("text".to_owned(), Value::String(text.to_owned()));
    if let Some(thread_ts) = thread_ts.filter(|value| !value.is_empty()) {
        body.insert("thread_ts".to_owned(), Value::String(thread_ts.to_owned()));
    }
    Value::Object(body)
}

fn run_email_transport(
    config: EmailRuntimeConfig,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let mut last_poll = Instant::now();
    let metadata_path = transport_context.metadata_path("email-imap");
    let mut seen_state = config
        .imap
        .as_ref()
        .map(|imap| load_email_seen_uid_state(&metadata_path, &email_metadata_key(imap)))
        .unwrap_or_default();
    while !stop.load(Ordering::SeqCst) {
        if let Some(smtp) = config.smtp.as_ref() {
            drain_outbound(
                &outbound_rx,
                transport_context.send_attempts,
                &stop,
                Some(&metadata_path),
                |message| send_email_message(smtp, message),
            );
        } else {
            discard_outbound(&outbound_rx, "email smtp is not configured");
        }
        if let Some(imap) = config.imap.as_ref() {
            if last_poll.elapsed() >= Duration::from_secs(imap.poll_interval_seconds) {
                match poll_email_inbox(&config, imap, &mut seen_state) {
                    Ok(messages) => {
                        for inbound in messages {
                            inbound_bus.publish_inbound(inbound);
                        }
                        save_email_seen_uid_state(
                            &metadata_path,
                            &email_metadata_key(imap),
                            &seen_state,
                        );
                    }
                    Err(error) => eprintln!(
                        "email imap polling failed: {}",
                        redact_email_imap_error(error, imap)
                    ),
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
    if message_is_stream_delta(&message) || message_is_stream_end(&message) {
        return Ok(());
    }
    let from = config
        .from
        .parse::<Mailbox>()
        .map_err(|error| redact_email_smtp_error(error.to_string(), config))?;
    let to = message
        .chat_id
        .parse::<Mailbox>()
        .map_err(|error| redact_email_smtp_error(error.to_string(), config))?;
    let mut email_builder = EmailMessage::builder()
        .from(from)
        .to(to)
        .subject(email_outbound_subject(&message));
    if let Some(reply_to) = message.reply_to.as_ref().filter(|value| !value.is_empty()) {
        email_builder = email_builder
            .in_reply_to(reply_to.clone())
            .references(reply_to.clone());
    }
    let email = email_builder
        .body(message.content)
        .map_err(|error| redact_email_smtp_error(error.to_string(), config))?;
    let mut builder = match config.security {
        EmailSecurity::Plain => SmtpTransport::builder_dangerous(&config.host).port(config.port),
        EmailSecurity::StartTls => SmtpTransport::starttls_relay(&config.host)
            .map_err(|error| redact_email_smtp_error(error.to_string(), config))?
            .port(config.port),
        EmailSecurity::Tls => SmtpTransport::relay(&config.host)
            .map_err(|error| redact_email_smtp_error(error.to_string(), config))?
            .port(config.port),
    };
    builder = builder.timeout(Some(Duration::from_secs(config.timeout_seconds)));
    if let (Some(username), Some(password)) = (&config.username, &config.password) {
        builder = builder.credentials(SmtpCredentials::new(username.clone(), password.clone()));
    }
    builder
        .build()
        .send(&email)
        .map_err(|error| redact_email_smtp_error(error.to_string(), config))?;
    Ok(())
}

fn email_outbound_subject(message: &OutboundMessage) -> String {
    metadata_string(&message.metadata, "subject")
        .map(|subject| {
            if subject.to_ascii_lowercase().starts_with("re:") {
                subject
            } else {
                format!("Re: {subject}")
            }
        })
        .unwrap_or_else(|| "shacs-bot".to_owned())
}

fn redact_email_smtp_error(error: String, config: &EmailSmtpRuntimeConfig) -> String {
    let mut secrets = vec![config.from.as_str()];
    if let Some(username) = config.username.as_deref() {
        secrets.push(username);
    }
    if let Some(password) = config.password.as_deref() {
        secrets.push(password);
    }
    redact_email_error_values(error, &secrets)
}

fn redact_email_imap_error(error: String, config: &EmailImapRuntimeConfig) -> String {
    redact_email_error_values(error, &[config.username.as_str(), config.password.as_str()])
}

fn redact_email_error_values(mut error: String, values: &[&str]) -> String {
    for value in values.iter().copied().filter(|value| !value.is_empty()) {
        error = error.replace(value, "[redacted]");
    }
    error
}

fn poll_email_inbox(
    runtime: &EmailRuntimeConfig,
    config: &EmailImapRuntimeConfig,
    seen_state: &mut EmailSeenUidState,
) -> Result<Vec<InboundMessage>, String> {
    if !matches!(config.security, EmailSecurity::Tls) {
        return Err("only TLS IMAP polling is supported in this runtime".to_owned());
    }
    let client = connect_imap_tls(config)?;
    let mut session = client
        .login(&config.username, &config.password)
        .map_err(|(error, _)| error.to_string())?;
    let mailbox = session
        .select(&config.mailbox)
        .map_err(|error| error.to_string())?;
    apply_email_uid_validity(seen_state, mailbox.uid_validity);
    let uids = session
        .uid_search("UNSEEN")
        .map_err(|error| error.to_string())?;
    let mut messages = Vec::new();
    for uid in uids.iter().take(10) {
        let uid = uid.to_string();
        if seen_state.seen_uids.contains(&uid) {
            continue;
        }
        let fetches = session
            .uid_fetch(uid.clone(), "RFC822")
            .map_err(|error| error.to_string())?;
        for fetch in fetches.iter() {
            let Some(body) = fetch.body() else {
                continue;
            };
            if body.len() > shacs_api::MAX_REQUEST_BODY_BYTES {
                remember_seen_email_uid(
                    &mut seen_state.seen_uids,
                    &mut seen_state.seen_uid_order,
                    uid.clone(),
                );
                continue;
            }
            let mut parsed = parse_email_body(body, uid.clone())?;
            if email_should_skip_inbound(runtime, config, &parsed) {
                remember_seen_email_uid(
                    &mut seen_state.seen_uids,
                    &mut seen_state.seen_uid_order,
                    uid.clone(),
                );
                continue;
            }
            parsed.inbound.attachments = email_attachment_data_urls_from_body(body)?;
            remember_seen_email_uid(
                &mut seen_state.seen_uids,
                &mut seen_state.seen_uid_order,
                uid.clone(),
            );
            messages.push(parsed.inbound.into_message());
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

fn parse_email_body(body: &[u8], uid: String) -> Result<ParsedEmailInbound, String> {
    let parsed = mailparse::parse_mail(body).map_err(|error| error.to_string())?;
    let headers = parsed.get_headers();
    let sender_header = headers
        .get_first_value("From")
        .unwrap_or_else(|| "unknown@example.invalid".to_owned());
    let sender = email_address_from_header(&sender_header);
    let subject = headers
        .get_first_value("Subject")
        .unwrap_or_else(|| "(no subject)".to_owned());
    let date = headers.get_first_value("Date").unwrap_or_default();
    let message_id = headers
        .get_first_value("Message-Id")
        .unwrap_or_else(|| uid.clone());
    let authentication_results = headers.get_first_value("Authentication-Results");
    let body = parsed.get_body().map_err(|error| error.to_string())?;
    Ok(ParsedEmailInbound {
        inbound: EmailInbound {
            sender_email: sender,
            subject,
            date,
            body,
            message_id,
            uid: Some(uid),
            attachments: Vec::new(),
        },
        authentication_results,
    })
}

fn email_attachment_data_urls_from_body(body: &[u8]) -> Result<Vec<String>, String> {
    if body.len() > shacs_api::MAX_REQUEST_BODY_BYTES {
        return Err(format!(
            "email message exceeds {} bytes",
            shacs_api::MAX_REQUEST_BODY_BYTES
        ));
    }
    let parsed = mailparse::parse_mail(body).map_err(|error| error.to_string())?;
    email_attachment_data_urls(&parsed)
}

fn email_attachment_data_urls(parsed: &mailparse::ParsedMail<'_>) -> Result<Vec<String>, String> {
    let mut attachments = Vec::new();
    collect_email_attachment_data_urls(parsed, &mut attachments)?;
    Ok(attachments)
}

fn collect_email_attachment_data_urls(
    parsed: &mailparse::ParsedMail<'_>,
    attachments: &mut Vec<String>,
) -> Result<(), String> {
    for part in &parsed.subparts {
        collect_email_attachment_data_urls(part, attachments)?;
    }
    if parsed.subparts.is_empty() && email_part_is_attachment(parsed) {
        let bytes = parsed.get_body_raw().map_err(|error| error.to_string())?;
        let mime = parsed.ctype.mimetype.as_str();
        let filename = email_part_filename(parsed);
        attachments.push(email_attachment_data_url(
            mime,
            filename.as_deref(),
            &bytes,
        )?);
    }
    Ok(())
}

fn email_attachment_data_url(
    mime: &str,
    filename: Option<&str>,
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.len() > shacs_api::MAX_MEDIA_BYTES {
        return Err(format!(
            "email attachment exceeds {} bytes",
            shacs_api::MAX_MEDIA_BYTES
        ));
    }
    Ok(data_url_with_optional_name(mime, filename, bytes))
}

fn email_part_is_attachment(parsed: &mailparse::ParsedMail<'_>) -> bool {
    let disposition = parsed.get_content_disposition();
    matches!(
        disposition.disposition,
        mailparse::DispositionType::Attachment
    ) || disposition.params.contains_key("filename")
        || parsed.ctype.params.contains_key("name")
}

fn email_part_filename(parsed: &mailparse::ParsedMail<'_>) -> Option<String> {
    let disposition = parsed.get_content_disposition();
    disposition
        .params
        .get("filename")
        .or_else(|| parsed.ctype.params.get("name"))
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_owned())
}

fn data_url_with_optional_name(mime: &str, filename: Option<&str>, bytes: &[u8]) -> String {
    let encoded = base64_standard(bytes);
    match filename {
        Some(filename) => format!(
            "data:{mime};name={};base64,{encoded}",
            base64_url_no_pad(filename.as_bytes())
        ),
        None => format!("data:{mime};base64,{encoded}"),
    }
}

fn email_address_from_header(value: &str) -> String {
    let trimmed = value.trim();
    if let Some((_, rest)) = trimmed.rsplit_once('<') {
        if let Some((address, _)) = rest.split_once('>') {
            let address = address.trim();
            if !address.is_empty() {
                return address.to_owned();
            }
        }
    }
    trimmed.trim_matches('"').to_owned()
}

fn email_should_skip_inbound(
    runtime: &EmailRuntimeConfig,
    config: &EmailImapRuntimeConfig,
    parsed: &ParsedEmailInbound,
) -> bool {
    let sender = parsed.inbound.sender_email.as_str();
    email_is_self_sender(runtime, config, sender)
        || !sender_allowed(&runtime.allowed_senders, sender)
        || !email_authentication_passes(
            parsed.authentication_results.as_deref(),
            runtime.verify_spf,
            runtime.verify_dkim,
        )
}

fn email_is_self_sender(
    runtime: &EmailRuntimeConfig,
    config: &EmailImapRuntimeConfig,
    sender: &str,
) -> bool {
    let mut self_addresses = vec![config.username.as_str()];
    if let Some(smtp) = runtime.smtp.as_ref() {
        self_addresses.push(smtp.from.as_str());
        if let Some(username) = smtp.username.as_deref() {
            self_addresses.push(username);
        }
    }
    self_addresses
        .into_iter()
        .any(|address| address.eq_ignore_ascii_case(sender))
}

fn email_authentication_passes(
    authentication_results: Option<&str>,
    verify_spf: bool,
    verify_dkim: bool,
) -> bool {
    if !verify_spf && !verify_dkim {
        return true;
    }
    let Some(results) = authentication_results.map(str::to_ascii_lowercase) else {
        return false;
    };
    (!verify_spf || results.contains("spf=pass")) && (!verify_dkim || results.contains("dkim=pass"))
}

fn run_whatsapp_transport(
    config: WhatsAppRuntimeConfig,
    inbound_bus: MessageBus,
    outbound_rx: mpsc::Receiver<OutboundMessage>,
    stop: Arc<AtomicBool>,
    transport_context: ExternalTransportRuntimeContext,
) {
    let mut backoff = Duration::from_secs(1);
    let metadata_path = transport_context.metadata_path("whatsapp");
    while !stop.load(Ordering::SeqCst) {
        match run_whatsapp_bridge_session(
            &config,
            &inbound_bus,
            &outbound_rx,
            &stop,
            transport_context.send_attempts,
            &metadata_path,
        ) {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(error) => {
                if !stop.load(Ordering::SeqCst) {
                    eprintln!("whatsapp bridge worker reconnecting: {error}");
                    sleep_with_stop(&stop, backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(30));
                }
            }
        }
    }
}

fn run_whatsapp_bridge_session(
    config: &WhatsAppRuntimeConfig,
    inbound_bus: &MessageBus,
    outbound_rx: &mpsc::Receiver<OutboundMessage>,
    stop: &Arc<AtomicBool>,
    send_attempts: usize,
    metadata_path: &Path,
) -> Result<(), String> {
    let (mut socket, _) =
        websocket_connect(config.bridge_url.as_str()).map_err(|error| error.to_string())?;
    set_websocket_timeouts(&mut socket, Duration::from_millis(500));
    if let Some(token) = config.bridge_token.as_deref() {
        send_whatsapp_frame(&mut socket, &shacs_channels::whatsapp_auth_frame(token))?;
    }
    let mut recent = RecentMessageIds::default();
    let channel_config = WhatsAppChannelConfig {
        bridge_url: config.bridge_url.clone(),
        bridge_token: config.bridge_token.clone(),
        allowlist: config.allowlist.clone(),
        group_policy: config.group_policy.clone(),
    };
    while !stop.load(Ordering::SeqCst) {
        drain_outbound(
            outbound_rx,
            send_attempts,
            stop,
            Some(metadata_path),
            |message| send_whatsapp_message(&mut socket, message),
        );
        match socket.read() {
            Ok(message) => {
                let Some(text) = websocket_text(message)? else {
                    continue;
                };
                let value =
                    serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())?;
                for item in whatsapp_message_items(&value) {
                    match normalize_whatsapp_bridge_value(item, &channel_config, &mut recent) {
                        Ok(Some(inbound)) => inbound_bus.publish_inbound(inbound),
                        Ok(None) => {}
                        Err(error) => eprintln!("whatsapp bridge message failed: {error}"),
                    }
                }
            }
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => {
                return Err("bridge connection closed".to_owned())
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let _ = socket.close(None);
    Ok(())
}

fn whatsapp_message_items(value: &Value) -> Vec<&Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    if value.get("type").is_some() {
        return vec![value];
    }
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn normalize_whatsapp_bridge_value(
    value: &Value,
    config: &WhatsAppChannelConfig,
    recent: &mut RecentMessageIds,
) -> Result<Option<InboundMessage>, String> {
    serde_json::from_value::<WhatsAppBridgeMessage>(value.clone())
        .map_err(|error| error.to_string())
        .and_then(|message| {
            normalize_whatsapp_bridge_message(message, config, recent)
                .map_err(|error| error.to_string())
        })
}

fn send_whatsapp_message(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    message: OutboundMessage,
) -> Result<(), String> {
    if message_is_stream_delta(&message) || message_is_stream_end(&message) {
        return Ok(());
    }
    for frame in whatsapp_outbound_frames(message) {
        send_whatsapp_frame(socket, &frame)?;
    }
    Ok(())
}

fn send_whatsapp_frame(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    frame: &WhatsAppOutboundFrame,
) -> Result<(), String> {
    socket
        .send(WebSocketMessage::text(whatsapp_frame_text(frame)?))
        .map_err(|error| error.to_string())
}

fn whatsapp_frame_text(frame: &WhatsAppOutboundFrame) -> Result<String, String> {
    serde_json::to_string(frame).map_err(|error| error.to_string())
}

fn drain_outbound(
    outbound_rx: &mpsc::Receiver<OutboundMessage>,
    attempts: usize,
    stop: &Arc<AtomicBool>,
    delivery_metadata_path: Option<&Path>,
    mut send: impl FnMut(OutboundMessage) -> Result<(), String>,
) {
    while let Ok(message) = outbound_rx.try_recv() {
        let typing_indicator = message_is_typing_indicator(&message);
        let transport_marker_only = message_is_stream_end(&message);
        if let Some(path) =
            delivery_metadata_path.filter(|_| !transport_marker_only && !typing_indicator)
        {
            record_delivery_state(path, &message, "pending", None);
        }
        match send_with_transport_retries(message.clone(), attempts, stop, &mut send) {
            Ok(()) => {
                if let Some(path) = delivery_metadata_path.filter(|_| !typing_indicator) {
                    let status = if transport_marker_only {
                        "processed"
                    } else {
                        "sent"
                    };
                    record_delivery_state(path, &message, status, None);
                }
            }
            Err(error) => {
                if let Some(path) = delivery_metadata_path.filter(|_| !typing_indicator) {
                    record_delivery_state(path, &message, "failed", Some(&error));
                }
                eprintln!("external channel outbound failed: {error}");
            }
        }
    }
}

fn send_with_transport_retries(
    message: OutboundMessage,
    attempts: usize,
    stop: &Arc<AtomicBool>,
    send: &mut impl FnMut(OutboundMessage) -> Result<(), String>,
) -> Result<(), String> {
    let attempts = attempts.clamp(1, 10);
    let mut last_error = None;
    for attempt in 0..attempts {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match send(message.clone()) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                if attempt + 1 < attempts {
                    sleep_with_stop(
                        stop,
                        Duration::from_millis(100 * (attempt as u64 + 1).min(5)),
                    );
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "transport send stopped before delivery".to_owned()))
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

fn post_empty(agent: &ureq::Agent, url: &str, authorization: Option<String>) -> Result<(), String> {
    let mut request = agent.post(url);
    if let Some(authorization) = authorization {
        request = request.header("Authorization", authorization);
    }
    let response = request.send_empty().map_err(|error| {
        format!(
            "request to {} failed: {}",
            redact_sensitive_url_text(url),
            redact_sensitive_url_text(&error.to_string())
        )
    })?;
    let status = response.status().as_u16();
    if status >= 400 {
        return Err(format!("HTTP {status}"));
    }
    Ok(())
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

fn get_binary(
    agent: &ureq::Agent,
    url: &str,
    authorization: Option<String>,
) -> Result<(Vec<u8>, Option<String>), String> {
    let mut request = agent.get(url).config().max_redirects(0).build();
    if let Some(authorization) = authorization {
        request = request.header("Authorization", authorization);
    }
    let mut response = request.call().map_err(|error| {
        format!(
            "request to {} failed: {}",
            redact_sensitive_url_text(url),
            redact_sensitive_url_text(&error.to_string())
        )
    })?;
    let status = response.status().as_u16();
    if (300..400).contains(&status) {
        return Err(format!(
            "attachment download redirect rejected with HTTP {status}"
        ));
    }
    if status >= 400 {
        return Err(format!("HTTP {status}"));
    }
    if let Some(content_length) = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        if content_length > shacs_api::MAX_MEDIA_BYTES as u64 {
            return Err(format!(
                "downloaded attachment exceeds {} bytes",
                shacs_api::MAX_MEDIA_BYTES
            ));
        }
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let bytes = read_capped_binary_body(response.body_mut().as_reader()).map_err(|error| {
        format!(
            "response from {} could not be read: {}",
            redact_sensitive_url_text(url),
            redact_sensitive_url_text(&error.to_string())
        )
    })?;
    Ok((bytes, content_type))
}

fn read_capped_binary_body(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(shacs_api::MAX_MEDIA_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > shacs_api::MAX_MEDIA_BYTES {
        return Err(format!(
            "downloaded attachment exceeds {} bytes",
            shacs_api::MAX_MEDIA_BYTES
        ));
    }
    Ok(bytes)
}

fn platform_attachment_url_allowed(url: &str, allowed_hosts: &[&str]) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest
        .split_once('/')
        .map(|(authority, _)| authority)
        .unwrap_or(rest);
    if authority.contains('@') {
        return false;
    }
    let host = authority
        .split_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .to_ascii_lowercase();
    allowed_hosts.iter().any(|allowed| host == *allowed)
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
    web_preset_from_bundle(&bundle, &options)
}

fn web_preset_from_bundle(
    bundle: &ConfigBundle,
    options: &WebOptions,
) -> Result<WebPresetReport, CliError> {
    let gateway_options = GatewayOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        port: options.gateway_port,
        verbose: options.verbose,
    };
    let gateway_addr = resolve_gateway_addr(&gateway_options, &bundle.config.gateway)?;
    let websocket = websocket_preset(&bundle.config.channels.plugins, options)?;
    let assets_dir = shacs_web::manifest_dist_dir();
    let assets_populated = shacs_web::dist_is_populated(&assets_dir);
    Ok(WebPresetReport {
        config_path: bundle.context.config_path.clone(),
        workspace: bundle.context.workspace.clone(),
        gateway_addr,
        websocket,
        assets_dir,
        assets_populated,
        verbose: options.verbose,
    })
}

pub fn serve_web_ui(options: WebOptions) -> Result<String, CliError> {
    let bundle = load_runtime_config(RuntimeConfigOptions {
        config_path: options.config_path.clone(),
        workspace_override: options.workspace_override.clone(),
        resolve_env: true,
    })?;
    let report = web_preset_from_bundle(&bundle, &options)?;
    let timeout_seconds = bundle.config.api.timeout.max(0.001);
    let websocket_path = web_ui_websocket_path(&report);
    let adapter = Arc::new(
        AgentLoopChatCompletionAdapter::from_bundle(bundle, false)?
            .with_runtime_verbose(options.verbose),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    eprintln!("{}", format_web_preset_report(report.clone()));
    runtime.block_on(shacs_api::serve_web_ui_with_timeout_and_websocket_path(
        report.gateway_addr,
        adapter,
        Duration::from_secs_f64(timeout_seconds),
        &websocket_path,
        report.assets_dir.clone(),
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    ))?;
    Ok(format!(
        "Web UI server stopped: http://{} (websocket_path={websocket_path})",
        report.gateway_addr
    ))
}

fn web_ui_websocket_path(report: &WebPresetReport) -> String {
    if report.websocket.path == "/" {
        shacs_api::WEBSOCKET_PATH.to_owned()
    } else {
        report.websocket.path.clone()
    }
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
    let websocket_path = web_ui_websocket_path(&report);
    let mut lines = vec![
        "Web UI server ready".to_owned(),
        format!("Config: {}", report.config_path.display()),
        format!("Workspace: {}", report.workspace.display()),
        format!("Web URL: http://{}", report.gateway_addr),
        format!(
            "WebSocket: enabled={} ws://{}{}",
            report.websocket.enabled, report.gateway_addr, websocket_path
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
        "  runtime   Start, stop, restart, inspect, diagnose, update, or recover local runtime state",
        "  session   Manage local session files",
        "  skills    List and inspect local skill registry entries",
        "  apps      Init authoring drafts; install, list, inspect, enable, disable, or uninstall local app bundles",
        "  plugins   List, inspect, doctor, enable, or disable descriptor-only plugins",
        "  hooks     List and inspect descriptor-only plugin hook metadata",
        "  channels  List channel registry/config status",
        "  context   Inspect context files and dry-run inline @ references",
        "  ask       Send one message through the local AgentLoop",
        "  run       Start selected channel runtime workers",
        "  serve     Start the local OpenAI-compatible HTTP API",
        "  api serve Compatibility alias for serve",
        "  gateway   Report gateway preset boundary without starting channels",
        "  web       Start the Web UI, API, and WebSocket on one port",
        "  agent     Alias for one-shot direct AgentLoop messages with -m/--message",
        "  provider  Manage provider auth; generic import-key, Codex login/import-token, and Copilot import-token are available",
        "",
        "Options:",
        "  -c, --config <path>   Use an explicit config file",
        "  -w, --workspace <path> Override workspace for runtime, projection, app, context, channel, ask/agent/serve commands",
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
        "      --verbose         Print runtime preview logs for run/web and serve diagnostics",
        "      --allow-remote    Permit non-loopback API binding",
        "      --allow-api-side-effects  Enable write/edit/exec tools in API turns",
        "      --session <id>    Use a CLI session key for ask/agent",
        "      --session <key>   Select a session for session commands",
        "      --max-messages <n> Limit session history output",
        "      --format <json|jsonl> Select session export format",
        "      --keep-messages <n> Retain this many messages during session compact",
        "      --target-version <v> Record runtime update target version",
        "      --all             Include inactive skill diagnostics in skills list",
        "      --bundle <path>   Local .shacsapp bundle for apps install",
        "      --app-id <id>     Select an app for apps commands",
        "  -y, --yes            Confirm irreversible session delete",
        "      --allow-side-effects  Enable write/edit/exec tools in CLI turns",
        "      --token-stdin     Read provider token from stdin for import-token/import-key commands",
        "      --token-env <var> Read provider token from an environment variable",
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

fn parse_runtime_start(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = RuntimeStartOptions {
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
                    "unknown runtime start argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::RuntimeStart(options))
}

fn parse_runtime_stop(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    restart: bool,
) -> Result<CliCommand, CliError> {
    let mut options = RuntimeStopOptions {
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
                    "unknown runtime stop/restart argument `{other}`"
                )))
            }
        }
    }
    if restart {
        Ok(CliCommand::RuntimeRestart(options))
    } else {
        Ok(CliCommand::RuntimeStop(options))
    }
}

fn parse_runtime_diagnostics(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = RuntimeDiagnosticsOptions {
        config_path: global_config,
        workspace_override: None,
        bundle_path: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--bundle" => options.bundle_path = Some(take_path(&mut parser, &arg)?),
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown runtime diagnostics argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::RuntimeDiagnostics(options))
}

fn parse_runtime_update(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = RuntimeUpdateOptions {
        config_path: global_config,
        workspace_override: None,
        target_version: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--target-version" => options.target_version = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown runtime update argument `{other}`"
                )))
            }
        }
    }
    if options.target_version.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "runtime update requires --target-version".to_owned(),
        ));
    }
    validate_runtime_target_version(&options.target_version)?;
    Ok(CliCommand::RuntimeUpdate(options))
}

fn parse_runtime_recover(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = RuntimeRecoverOptions {
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
                    "unknown runtime recover argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::RuntimeRecover(options))
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

fn parse_apps(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "apps requires `init`, `install`, `list`, `inspect`, `enable`, `disable`, or `uninstall`"
                .to_owned(),
        ));
    };
    match action.as_str() {
        "init" | "new" => parse_apps_init(parser, global_config),
        "install" => parse_apps_install(parser, global_config),
        "list" | "ls" => parse_apps_list(parser, global_config),
        "inspect" | "show" => parse_apps_inspect(parser, global_config),
        "enable" => parse_apps_id_action(parser, global_config, "enable"),
        "disable" => parse_apps_id_action(parser, global_config, "disable"),
        "uninstall" | "remove" => parse_apps_id_action(parser, global_config, "uninstall"),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown apps subcommand `{other}`"
        ))),
    }
}

fn parse_apps_init(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = AppsInitOptions {
        config_path: global_config,
        workspace_override: None,
        app_id: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--app-id" | "--id" => options.app_id = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown apps init argument `{other}`"
                )))
            }
            other => {
                if !options.app_id.is_empty() {
                    return Err(CliError::InvalidArguments(
                        "apps init accepts exactly one app id".to_owned(),
                    ));
                }
                options.app_id = other.to_owned();
            }
        }
    }
    if options.app_id.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "apps init requires an app id".to_owned(),
        ));
    }
    Ok(CliCommand::Apps(AppsCommand::Init(options)))
}

fn parse_apps_install(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = AppsInstallOptions {
        config_path: global_config,
        workspace_override: None,
        bundle_path: PathBuf::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--bundle" => options.bundle_path = take_path(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown apps install argument `{other}`"
                )))
            }
            other => {
                if options.bundle_path.as_os_str().is_empty() {
                    options.bundle_path = PathBuf::from(other);
                } else {
                    return Err(CliError::InvalidArguments(
                        "apps install accepts exactly one bundle path".to_owned(),
                    ));
                }
            }
        }
    }
    if options.bundle_path.as_os_str().is_empty() {
        return Err(CliError::InvalidArguments(
            "apps install requires a bundle path".to_owned(),
        ));
    }
    Ok(CliCommand::Apps(AppsCommand::Install(options)))
}

fn parse_apps_list(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut options = AppsListOptions {
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
                    "unknown apps list argument `{other}`"
                )))
            }
        }
    }
    Ok(CliCommand::Apps(AppsCommand::List(options)))
}

fn parse_apps_inspect(
    parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    match parse_apps_id_options(parser, global_config, "inspect")? {
        Some(options) => Ok(CliCommand::Apps(AppsCommand::Inspect(options))),
        None => Ok(CliCommand::Help),
    }
}

fn parse_apps_id_action(
    parser: ArgParser,
    global_config: Option<PathBuf>,
    action: &str,
) -> Result<CliCommand, CliError> {
    let Some(options) = parse_apps_id_options(parser, global_config, action)? else {
        return Ok(CliCommand::Help);
    };
    let options = AppsIdOptions {
        config_path: options.config_path,
        workspace_override: options.workspace_override,
        app_id: options.app_id,
    };
    match action {
        "enable" => Ok(CliCommand::Apps(AppsCommand::Enable(options))),
        "disable" => Ok(CliCommand::Apps(AppsCommand::Disable(options))),
        _ => Ok(CliCommand::Apps(AppsCommand::Uninstall(options))),
    }
}

fn parse_apps_id_options(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    action: &str,
) -> Result<Option<AppsInspectOptions>, CliError> {
    let mut options = AppsInspectOptions {
        config_path: global_config,
        workspace_override: None,
        app_id: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--app-id" | "--id" => options.app_id = take_value(&mut parser, &arg)?,
            "--help" | "-h" => return Ok(None),
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown apps {action} argument `{other}`"
                )))
            }
            other => {
                if !options.app_id.is_empty() {
                    return Err(CliError::InvalidArguments(format!(
                        "apps {action} accepts exactly one app id"
                    )));
                }
                options.app_id = other.to_owned();
            }
        }
    }
    if options.app_id.trim().is_empty() {
        return Err(CliError::InvalidArguments(format!(
            "apps {action} requires an app id"
        )));
    }
    Ok(Some(options))
}

fn parse_plugins(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "plugins requires `list`, `inspect`, `doctor`, `enable`, or `disable`".to_owned(),
        ));
    };
    match action.as_str() {
        "list" | "ls" => Ok(CliCommand::Plugins(PluginsCommand::List(
            parse_plugins_list_options(parser, global_config, "list")?,
        ))),
        "inspect" | "show" => parse_plugins_inspect(parser, global_config),
        "doctor" | "diagnose" => Ok(CliCommand::Plugins(PluginsCommand::Doctor(
            parse_plugins_list_options(parser, global_config, "doctor")?,
        ))),
        "enable" => parse_plugins_mutate(parser, global_config, "enable"),
        "disable" => parse_plugins_mutate(parser, global_config, "disable"),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown plugins subcommand `{other}`"
        ))),
    }
}

fn parse_plugins_list_options(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    action: &str,
) -> Result<PluginsListOptions, CliError> {
    let mut options = PluginsListOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown plugins {action} argument `{other}`"
                )))
            }
        }
    }
    Ok(options)
}

fn parse_plugins_inspect(
    parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let options = parse_plugins_name_options(parser, global_config, "inspect")?;
    Ok(CliCommand::Plugins(PluginsCommand::Inspect(
        PluginsInspectOptions {
            config_path: options.config_path,
            workspace_override: options.workspace_override,
            name: options.name,
        },
    )))
}

fn parse_plugins_mutate(
    parser: ArgParser,
    global_config: Option<PathBuf>,
    action: &str,
) -> Result<CliCommand, CliError> {
    let options = parse_plugins_name_options(parser, global_config, action)?;
    let options = PluginsMutateOptions {
        config_path: options.config_path,
        workspace_override: options.workspace_override,
        name: options.name,
    };
    match action {
        "enable" => Ok(CliCommand::Plugins(PluginsCommand::Enable(options))),
        _ => Ok(CliCommand::Plugins(PluginsCommand::Disable(options))),
    }
}

fn parse_plugins_name_options(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    action: &str,
) -> Result<PluginsInspectOptions, CliError> {
    let mut options = PluginsInspectOptions {
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
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown plugins {action} argument `{other}`"
                )))
            }
            other => {
                if !options.name.is_empty() {
                    return Err(CliError::InvalidArguments(format!(
                        "plugins {action} accepts exactly one plugin name"
                    )));
                }
                options.name = other.to_owned();
            }
        }
    }
    if options.name.trim().is_empty() {
        return Err(CliError::InvalidArguments(format!(
            "plugins {action} requires a plugin name"
        )));
    }
    Ok(options)
}

fn parse_hooks(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "hooks requires `list` or `inspect <filter>`".to_owned(),
        ));
    };
    match action.as_str() {
        "list" | "ls" => Ok(CliCommand::Hooks(HooksCommand::List(parse_hooks_list(
            parser,
            global_config,
        )?))),
        "inspect" | "show" => Ok(CliCommand::Hooks(HooksCommand::Inspect(
            parse_hooks_inspect(parser, global_config)?,
        ))),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown hooks subcommand `{other}`"
        ))),
    }
}

fn parse_hooks_list(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<HooksListOptions, CliError> {
    let mut options = HooksListOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown hooks list argument `{other}`"
                )))
            }
        }
    }
    Ok(options)
}

fn parse_hooks_inspect(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<HooksInspectOptions, CliError> {
    let mut options = HooksInspectOptions {
        config_path: global_config,
        workspace_override: None,
        filter: String::new(),
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other if other.starts_with('-') => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown hooks inspect argument `{other}`"
                )))
            }
            other => {
                if !options.filter.is_empty() {
                    return Err(CliError::InvalidArguments(
                        "hooks inspect accepts exactly one filter".to_owned(),
                    ));
                }
                options.filter = other.to_owned();
            }
        }
    }
    if options.filter.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "hooks inspect requires a filter".to_owned(),
        ));
    }
    Ok(options)
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

fn parse_context(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(scope) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "context requires `files` or `refs`".to_owned(),
        ));
    };
    match scope.as_str() {
        "files" | "file" => parse_context_files(parser, global_config),
        "refs" | "ref" | "references" => parse_context_refs(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown context subcommand `{other}`"
        ))),
    }
}

fn parse_context_files(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "context files requires `list` or `inspect`".to_owned(),
        ));
    };
    let command = match action.as_str() {
        "list" | "ls" => {
            ContextFilesCommand::List(parse_context_files_options(parser, global_config, "list")?)
        }
        "inspect" => ContextFilesCommand::Inspect(parse_context_files_options(
            parser,
            global_config,
            "inspect",
        )?),
        "--help" | "-h" => return Ok(CliCommand::Help),
        other => {
            return Err(CliError::InvalidArguments(format!(
                "unknown context files action `{other}`"
            )))
        }
    };
    Ok(CliCommand::Context(ContextCommand::Files(command)))
}

fn parse_context_files_options(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    action: &str,
) -> Result<ContextFilesOptions, CliError> {
    let mut options = ContextFilesOptions {
        config_path: global_config,
        workspace_override: None,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown context files {action} argument `{other}`"
                )))
            }
        }
    }
    Ok(options)
}

fn parse_context_refs(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "context refs requires `parse` or `resolve`".to_owned(),
        ));
    };
    let command = match action.as_str() {
        "parse" => ContextRefsCommand::Parse(parse_context_refs_parse(parser)?),
        "resolve" => {
            ContextRefsCommand::Resolve(parse_context_refs_resolve(parser, global_config)?)
        }
        "--help" | "-h" => return Ok(CliCommand::Help),
        other => {
            return Err(CliError::InvalidArguments(format!(
                "unknown context refs action `{other}`"
            )))
        }
    };
    Ok(CliCommand::Context(ContextCommand::Refs(command)))
}

fn parse_context_refs_parse(mut parser: ArgParser) -> Result<ContextRefsParseOptions, CliError> {
    let mut options = ContextRefsParseOptions::default();
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--message" | "-m" => options.message = take_value(&mut parser, &arg)?,
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other if options.message.is_empty() => options.message = other.to_owned(),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown context refs parse argument `{other}`"
                )))
            }
        }
    }
    if options.message.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "context refs parse requires --message <text> or a message argument".to_owned(),
        ));
    }
    Ok(options)
}

fn parse_context_refs_resolve(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<ContextRefsResolveOptions, CliError> {
    let mut options = ContextRefsResolveOptions {
        config_path: global_config,
        workspace_override: None,
        message: String::new(),
        network_enabled: false,
    };
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => options.config_path = Some(take_path(&mut parser, &arg)?),
            "--workspace" | "-w" => {
                options.workspace_override = Some(take_path(&mut parser, &arg)?)
            }
            "--message" | "-m" => options.message = take_value(&mut parser, &arg)?,
            "--network" => options.network_enabled = true,
            "--help" | "-h" => {
                return Err(CliError::InvalidArguments(
                    "help requested after action".to_owned(),
                ))
            }
            other if options.message.is_empty() => options.message = other.to_owned(),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown context refs resolve argument `{other}`"
                )))
            }
        }
    }
    if options.message.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "context refs resolve requires --message <text> or a message argument".to_owned(),
        ));
    }
    Ok(options)
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
        "copilot" | "github-copilot" | "github_copilot" => {
            parse_provider_copilot(parser, global_config)
        }
        "import-key" => parse_provider_import_key(parser, global_config, None),
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
        other => parse_named_provider_action(other.to_owned(), parser, global_config),
    }
}

fn parse_named_provider_action(
    provider: String,
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(format!(
            "provider `{provider}` requires `import-key`"
        )));
    };
    match action.as_str() {
        "import-key" => parse_provider_import_key(parser, global_config, Some(provider)),
        other => Err(CliError::InvalidArguments(format!(
            "unknown provider `{provider}` action `{other}`"
        ))),
    }
}

fn parse_provider_copilot(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let Some(action) = parser.next() else {
        return Err(CliError::InvalidArguments(
            "provider copilot requires `import-token`".to_owned(),
        ));
    };
    match action.as_str() {
        "import-token" => parse_copilot_import_token(parser, global_config),
        "--help" | "-h" => Ok(CliCommand::Help),
        other => Err(CliError::InvalidArguments(format!(
            "unknown provider copilot action `{other}`"
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

fn parse_copilot_import_token(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut token_source = None;
    let mut select = false;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--token-stdin" => token_source = Some(TokenSource::Stdin),
            "--token-env" => token_source = Some(TokenSource::Env(take_value(&mut parser, &arg)?)),
            "--select" => select = true,
            "--no-select" => select = false,
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown provider copilot import-token argument `{other}`"
                )))
            }
        }
    }
    let token_source = token_source.ok_or_else(|| {
        CliError::InvalidArguments(
            "provider copilot import-token requires --token-stdin or --token-env <name>".to_owned(),
        )
    })?;
    Ok(CliCommand::Provider(ProviderCommand::CopilotImportToken(
        CopilotImportTokenOptions {
            config_path,
            token_source,
            select,
        },
    )))
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

fn parse_provider_import_key(
    mut parser: ArgParser,
    global_config: Option<PathBuf>,
    provider_override: Option<String>,
) -> Result<CliCommand, CliError> {
    let mut config_path = global_config;
    let mut provider = provider_override;
    let mut token_source = None;
    while let Some(arg) = parser.next() {
        match arg.as_str() {
            "--config" | "-c" => config_path = Some(take_path(&mut parser, &arg)?),
            "--provider" => provider = Some(take_value(&mut parser, &arg)?),
            "--token-stdin" => token_source = Some(TokenSource::Stdin),
            "--token-env" => token_source = Some(TokenSource::Env(take_value(&mut parser, &arg)?)),
            "--help" | "-h" => return Ok(CliCommand::Help),
            other => {
                return Err(CliError::InvalidArguments(format!(
                    "unknown provider import-key argument `{other}`"
                )))
            }
        }
    }
    let provider = provider.ok_or_else(|| {
        CliError::InvalidArguments("provider import-key requires --provider <id>".to_owned())
    })?;
    if provider.trim().is_empty() {
        return Err(CliError::InvalidArguments(
            "provider import-key requires a non-empty provider id".to_owned(),
        ));
    }
    let token_source = token_source.ok_or_else(|| {
        CliError::InvalidArguments(
            "provider import-key requires --token-stdin or --token-env <name>".to_owned(),
        )
    })?;
    Ok(CliCommand::Provider(ProviderCommand::ImportApiKey(
        ProviderApiKeyImportOptions {
            config_path,
            provider,
            token_source,
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

fn provider_backend_supports_native_image_input(backend: &str) -> bool {
    matches!(backend, "openai_compat" | "azure_openai" | "anthropic")
}

fn provider_model_supports_native_image_input(backend: &str, model: &str) -> bool {
    provider_backend_supports_native_image_input(backend)
        && model_name_has_native_image_evidence(model)
}

fn model_name_has_native_image_evidence(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    [
        "gpt-4o",
        "gpt-4.1",
        "gpt-5",
        "o3",
        "o4",
        "claude-3",
        "claude-4",
        "gemini",
        "gemma",
        "glm-4v",
        "qwen-vl",
        "vision",
        "multimodal",
    ]
    .iter()
    .any(|needle| model.contains(needle))
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
    native_image_input_supported: bool,
    client: Arc<dyn ProviderClient>,
    retry_mode: ProviderRetryMode,
    workspace: PathBuf,
    media_dir: PathBuf,
    tools: ToolRegistry,
    message_tool: Option<MessageTool>,
    _mcp_runtime: Option<McpRuntime>,
    _mcp_reports: Vec<McpServerConnectionReport>,
    allow_side_effect_tools: bool,
    send_progress: bool,
    send_tool_hints: bool,
    send_max_retries: u32,
    runtime_verbose: bool,
    session_turn_lock: SessionTurnLock,
    exec_timeout_seconds: u64,
    exec_sandbox: Option<String>,
    exec_path_append: Option<String>,
    exec_allowed_env_keys: Vec<String>,
    exec_env: BTreeMap<String, String>,
    tool_search: ToolSearchConfig,
    containment_snapshot: Option<ContainmentSnapshotRef>,
    permission_mode_snapshot: PermissionModeSnapshot,
    plugin_runtime_snapshot: PluginRuntimeSnapshot,
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
        let exec_sandbox = non_empty(Some(bundle.config.tools.exec.sandbox.as_str()))
            .then(|| bundle.config.tools.exec.sandbox.clone());
        let exec_path_append = non_empty(Some(bundle.config.tools.exec.path_append.as_str()))
            .then(|| bundle.config.tools.exec.path_append.clone());
        let containment = runtime_containment_inspect(&bundle);
        let permission_config_snapshot =
            agent_loop_permission_config_snapshot(&bundle, &containment);
        let containment_snapshot = Some(runtime_containment_snapshot_ref(&containment));
        let permission_mode_snapshot =
            runtime_permission_mode_snapshot(&permission_config_snapshot);
        let plugin_discovery = discover_plugins(&bundle.config, &bundle.context, &ProcessEnv)?;
        let plugin_runtime_snapshot = build_plugin_runtime_snapshot(&plugin_discovery.plugins);
        let tooling = production_tool_registry(&bundle, allow_side_effect_tools)?;
        let provider_id = resolved.provider_id.clone();
        let native_image_input_supported =
            registry.find_by_name(&provider_id).is_some_and(|spec| {
                provider_model_supports_native_image_input(spec.backend, &resolved.model)
            });
        let resolved_model = resolved.model.clone();
        let client: Arc<dyn ProviderClient> = Arc::from(resolved.client);
        Ok(Self {
            configured_model: defaults.model.clone(),
            provider_id,
            defaults,
            resolved_model,
            native_image_input_supported,
            client,
            retry_mode,
            workspace: bundle.context.workspace,
            media_dir,
            tools: tooling.registry,
            message_tool: tooling.message_tool,
            _mcp_runtime: tooling.mcp_runtime,
            _mcp_reports: tooling.mcp_reports,
            allow_side_effect_tools,
            send_progress: true,
            send_tool_hints: bundle.config.channels.send_tool_hints,
            send_max_retries: bundle.config.channels.send_max_retries,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: u64::from(bundle.config.tools.exec.timeout),
            exec_sandbox,
            exec_path_append,
            exec_allowed_env_keys: bundle.config.tools.exec.allowed_env_keys.clone(),
            exec_env: configured_exec_env(&bundle.config),
            tool_search: runtime_tool_search_config(&bundle.config.tools.tool_search),
            containment_snapshot,
            permission_mode_snapshot,
            plugin_runtime_snapshot,
        })
    }

    pub fn with_runtime_verbose(mut self, runtime_verbose: bool) -> Self {
        self.runtime_verbose = runtime_verbose;
        self
    }

    fn loop_config(&self) -> AgentLoopConfig {
        let mut config = AgentLoopConfig::new(&self.workspace, self.resolved_model.clone());
        if let Some(media_root) = self.media_dir.parent() {
            config.media_roots.push(media_root.to_path_buf());
        }
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
        config.tool_search = self.tool_search;
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
        config.concurrent_tools = true;
        config.containment_snapshot = self.containment_snapshot.clone();
        config.permission_mode_snapshot = self.permission_mode_snapshot.clone();
        config
    }

    fn context_builder(&self) -> ContextBuilder {
        let mut extra_roots = Vec::new();
        let media_root = self.media_dir.parent().map(Path::to_path_buf);
        if let Some(data_dir) = self
            .media_dir
            .parent()
            .and_then(|media_dir| media_dir.parent())
        {
            extra_roots.push(data_dir.join("skills"));
        }
        ContextBuilder::new(&self.workspace)
            .with_timezone(self.defaults.timezone.clone())
            .with_disabled_skills(self.defaults.disabled_skills.clone())
            .with_skill_roots(extra_roots)
            .with_media_roots(media_root)
            .with_native_image_input_supported(self.native_image_input_supported)
            .with_configured_env(self.exec_env.clone())
    }

    fn external_effective_session_key(&self, message: &InboundMessage) -> String {
        effective_external_session_key(&self.loop_config(), message)
    }

    fn external_session_is_active(&self, session_key: &str) -> bool {
        self.session_turn_lock.is_active(session_key)
    }

    fn run_agent_loop(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: Option<ApiProviderEventCallback>,
    ) -> Result<LlmResponse, ApiError> {
        self.run_agent_loop_with_origin(
            invocation,
            on_event,
            "api",
            "user",
            shacs_api::API_CHAT_ID,
            &[],
        )
    }

    fn complete_sdk_run(
        &self,
        invocation: ChatCompletionInvocation,
        observability_hooks: &[ShacsBotObservabilityHook],
    ) -> Result<RunResult, ApiError> {
        let turn = self.run_agent_loop_turn_with_origin(
            invocation,
            None,
            "sdk",
            "user",
            "default",
            observability_hooks,
        )?;
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
        observability_hooks: &[ShacsBotObservabilityHook],
    ) -> Result<LlmResponse, ApiError> {
        let turn = self.run_agent_loop_turn_with_origin(
            invocation,
            on_event,
            channel,
            sender_id,
            chat_id,
            observability_hooks,
        )?;
        Ok(llm_response_from_turn(turn))
    }

    fn run_agent_loop_turn_with_origin(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: Option<ApiProviderEventCallback>,
        channel: &str,
        sender_id: &str,
        chat_id: &str,
        observability_hooks: &[ShacsBotObservabilityHook],
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
        let (result, _) =
            self.process_inbound_with_outbound(message, config, on_event, observability_hooks)?;
        Ok(result)
    }

    pub fn process_websocket_frame(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
    ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
        let mut events = Vec::new();
        self.process_websocket_frame_events(frame, client_id, default_chat_id, &mut |event| {
            events.push(event);
        })?;
        Ok(events)
    }

    fn process_websocket_frame_events(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
        emit: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        let action = match normalize_websocket_frame(frame, client_id, default_chat_id) {
            Ok(action) => action,
            Err(error) => {
                emit(WebSocketServerEvent::Error {
                    chat_id: Some(default_chat_id.to_owned()),
                    detail: Some(error.to_string()),
                });
                return Ok(());
            }
        };
        match action {
            WebSocketInboundAction::NewChat => emit(WebSocketServerEvent::Ready {
                chat_id: default_chat_id.to_owned(),
                client_id: client_id.to_owned(),
            }),
            WebSocketInboundAction::Attach { chat_id } => {
                emit(WebSocketServerEvent::Attached { chat_id })
            }
            WebSocketInboundAction::Message(mut inbound) => {
                let session_key = inbound.session_key();
                let chat_id = inbound.chat_id.clone();
                let media_names = websocket_media_names(&inbound.metadata);
                let media_paths = self.persist_media_data_urls_with_context(
                    &session_key,
                    WEBSOCKET_CHANNEL,
                    &inbound.media,
                    &media_names,
                )?;
                inbound.media = media_paths;
                inbound.session_key_override = Some(session_key);
                let stream_id = format!("{chat_id}:{}", now_millis());
                let (_, outbound) = self.process_websocket_inbound_with_streaming(
                    inbound,
                    self.loop_config(),
                    &chat_id,
                    &stream_id,
                    emit,
                )?;
                let sink = WebSocketEventSink::default();
                let mut manager =
                    websocket_event_channel_manager(self.send_max_retries, sink.clone());
                for message in outbound
                    .into_iter()
                    .filter(|message| message.channel == WEBSOCKET_CHANNEL)
                    .filter(should_dispatch_runtime_outbound)
                {
                    if let Err(error) = manager.dispatch_outbound(message) {
                        emit(WebSocketServerEvent::Error {
                            chat_id: Some(default_chat_id.to_owned()),
                            detail: Some(error.to_string()),
                        });
                        return Ok(());
                    }
                }
                for event in sink.take_events() {
                    emit(event);
                }
            }
        }
        Ok(())
    }

    fn process_websocket_inbound_with_streaming(
        &self,
        inbound: InboundMessage,
        config: AgentLoopConfig,
        chat_id: &str,
        stream_id: &str,
        emit: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(AgentLoopTurnResult, Vec<shacs_channels::OutboundMessage>), ApiError> {
        if !self.send_progress {
            return self.process_websocket_inbound_with_live_notifications(inbound, config, emit);
        }

        let (event_tx, event_rx) = mpsc::channel();
        let callback = Arc::new(move |event: &ProviderEvent| {
            let _ = event_tx.send(event.clone());
        });
        let (notification_tx, notification_rx) = mpsc::channel();
        let live_sink: RuntimeNotificationSink = Arc::new(move |message| {
            let _ = notification_tx.send(message);
        });
        thread::scope(|scope| {
            let handle = scope.spawn(move || {
                self.process_inbound_with_outbound_inner(
                    inbound,
                    config,
                    Some(callback),
                    &[],
                    Some(live_sink),
                    None,
                )
            });
            let stream_result = self.emit_websocket_stream_events(
                event_rx,
                Some(notification_rx),
                chat_id,
                stream_id,
                emit,
            );
            let loop_result = match handle.join() {
                Ok(result) => result,
                Err(_) => Err(ApiError::internal("websocket agent loop task panicked")),
            };
            stream_result?;
            loop_result
        })
    }

    fn process_websocket_inbound_with_live_notifications(
        &self,
        inbound: InboundMessage,
        config: AgentLoopConfig,
        emit: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(AgentLoopTurnResult, Vec<shacs_channels::OutboundMessage>), ApiError> {
        let (notification_tx, notification_rx) = mpsc::channel();
        let live_sink: RuntimeNotificationSink = Arc::new(move |message| {
            let _ = notification_tx.send(message);
        });
        thread::scope(|scope| {
            let handle = scope.spawn(move || {
                self.process_inbound_with_outbound_inner(
                    inbound,
                    config,
                    None,
                    &[],
                    Some(live_sink),
                    None,
                )
            });
            let sink = WebSocketEventSink::default();
            let mut manager = websocket_event_channel_manager(self.send_max_retries, sink.clone());
            while !handle.is_finished() {
                Self::drain_websocket_runtime_notifications(
                    &notification_rx,
                    &mut manager,
                    &sink,
                    emit,
                )?;
                thread::sleep(Duration::from_millis(20));
            }
            Self::drain_websocket_runtime_notifications(
                &notification_rx,
                &mut manager,
                &sink,
                emit,
            )?;
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(ApiError::internal("websocket agent loop task panicked")),
            }
        })
    }

    fn process_external_inbound_with_streaming(
        &self,
        mut inbound: InboundMessage,
        config: AgentLoopConfig,
        runtime_bus: &MessageBus,
    ) -> Result<
        (
            AgentLoopTurnResult,
            Vec<shacs_channels::OutboundMessage>,
            SubagentRuntime,
        ),
        ApiError,
    > {
        inbound = self.normalize_external_inbound_media(inbound, &config)?;
        let channel = inbound.channel.clone();
        let chat_id = inbound.chat_id.clone();
        let session_key = self.external_effective_session_key(&inbound);
        let routing_metadata = stream_routing_metadata_from_inbound(&inbound);
        let reply_to = inbound_reply_to(&inbound);
        let subagent_runtime = SubagentRuntime::with_bus(runtime_bus.clone());
        let live_runtime_bus = runtime_bus.clone();
        let live_sink: RuntimeNotificationSink = Arc::new(move |message| {
            live_runtime_bus.publish_outbound(message);
        });
        if !self.send_progress || !provider_streaming_channel(&channel) {
            let result = self.process_inbound_with_outbound_inner(
                inbound,
                config,
                None,
                &[],
                Some(live_sink),
                Some(subagent_runtime.clone()),
            )?;
            return Ok((result.0, result.1, subagent_runtime));
        }

        let stream_id = format!("{channel}:{chat_id}:{session_key}:{}", now_millis());
        let (event_tx, event_rx) = mpsc::channel();
        let callback = Arc::new(move |event: &ProviderEvent| {
            let _ = event_tx.send(event.clone());
        });
        thread::scope(|scope| {
            let child_runtime = subagent_runtime.clone();
            let handle = scope.spawn(move || {
                self.process_inbound_with_outbound_inner(
                    inbound,
                    config,
                    Some(callback),
                    &[],
                    Some(live_sink),
                    Some(child_runtime),
                )
            });
            let routing = ExternalStreamRouting {
                channel: &channel,
                chat_id: &chat_id,
                stream_id: &stream_id,
                metadata: &routing_metadata,
                reply_to: reply_to.as_deref(),
            };
            self.publish_external_stream_events(event_rx, routing, runtime_bus)?;
            match handle.join() {
                Ok(result) => result.map(|(turn, outbound)| (turn, outbound, subagent_runtime)),
                Err(_) => Err(ApiError::internal(
                    "external channel agent loop task panicked",
                )),
            }
        })
    }

    fn publish_external_stream_events(
        &self,
        event_rx: mpsc::Receiver<ProviderEvent>,
        routing: ExternalStreamRouting<'_>,
        runtime_bus: &MessageBus,
    ) -> Result<(), ApiError> {
        let mut coalescer = StreamDeltaCoalescer::new();
        let mut pending_chars = 0usize;
        let mut emitted_stream_event = false;
        let mut emitted_stream_end = false;
        for event in event_rx {
            if let ProviderEvent::TextDelta { text } = &event {
                pending_chars = pending_chars.saturating_add(text.chars().count());
            }
            let mut batch = coalescer.push(&event);
            if batch.is_none() && pending_chars >= WEBSOCKET_STREAM_FLUSH_CHARS {
                batch = coalescer.flush();
            }
            if let Some(batch) = batch {
                pending_chars = 0;
                if !batch.text.is_empty() {
                    emitted_stream_event = true;
                    runtime_bus.publish_outbound(stream_outbound_message_with_routing(
                        routing.channel,
                        routing.chat_id,
                        routing.stream_id,
                        batch.text,
                        false,
                        routing.metadata,
                        routing.reply_to,
                    ));
                }
            }
            if matches!(event, ProviderEvent::Finish { .. }) {
                emitted_stream_event = true;
                emitted_stream_end = true;
                runtime_bus.publish_outbound(stream_outbound_message_with_routing(
                    routing.channel,
                    routing.chat_id,
                    routing.stream_id,
                    String::new(),
                    true,
                    routing.metadata,
                    routing.reply_to,
                ));
            }
        }
        if let Some(batch) = coalescer.flush() {
            if !batch.text.is_empty() {
                emitted_stream_event = true;
                runtime_bus.publish_outbound(stream_outbound_message_with_routing(
                    routing.channel,
                    routing.chat_id,
                    routing.stream_id,
                    batch.text,
                    false,
                    routing.metadata,
                    routing.reply_to,
                ));
            }
        }
        if emitted_stream_event && !emitted_stream_end {
            runtime_bus.publish_outbound(stream_outbound_message_with_routing(
                routing.channel,
                routing.chat_id,
                routing.stream_id,
                String::new(),
                true,
                routing.metadata,
                routing.reply_to,
            ));
        }
        Ok(())
    }

    fn emit_websocket_stream_events(
        &self,
        event_rx: mpsc::Receiver<ProviderEvent>,
        notification_rx: Option<mpsc::Receiver<OutboundMessage>>,
        chat_id: &str,
        stream_id: &str,
        emit: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        let sink = WebSocketEventSink::default();
        let mut manager = websocket_event_channel_manager(self.send_max_retries, sink.clone());
        let mut coalescer = StreamDeltaCoalescer::new();
        let mut pending_chars = 0usize;
        let mut emitted_stream_event = false;
        let mut emitted_stream_end = false;
        loop {
            if let Some(notification_rx) = notification_rx.as_ref() {
                Self::drain_websocket_runtime_notifications(
                    notification_rx,
                    &mut manager,
                    &sink,
                    emit,
                )?;
            }
            let event = match event_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(event) => event,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            if let ProviderEvent::TextDelta { text } = &event {
                pending_chars = pending_chars.saturating_add(text.chars().count());
            }
            let mut batch = coalescer.push(&event);
            if batch.is_none() && pending_chars >= WEBSOCKET_STREAM_FLUSH_CHARS {
                batch = coalescer.flush();
            }
            if let Some(batch) = batch {
                pending_chars = 0;
                if !batch.text.is_empty() {
                    emitted_stream_event = true;
                    Self::emit_websocket_stream_outbound(
                        &mut manager,
                        &sink,
                        chat_id,
                        stream_id,
                        batch.text,
                        false,
                        emit,
                    )?;
                }
            }
            if matches!(event, ProviderEvent::Finish { .. }) {
                emitted_stream_event = true;
                emitted_stream_end = true;
                Self::emit_websocket_stream_outbound(
                    &mut manager,
                    &sink,
                    chat_id,
                    stream_id,
                    String::new(),
                    true,
                    emit,
                )?;
            }
        }
        if let Some(notification_rx) = notification_rx.as_ref() {
            Self::drain_websocket_runtime_notifications(
                notification_rx,
                &mut manager,
                &sink,
                emit,
            )?;
        }
        if let Some(batch) = coalescer.flush() {
            if !batch.text.is_empty() {
                emitted_stream_event = true;
                Self::emit_websocket_stream_outbound(
                    &mut manager,
                    &sink,
                    chat_id,
                    stream_id,
                    batch.text,
                    false,
                    emit,
                )?;
            }
        }
        if emitted_stream_event && !emitted_stream_end {
            Self::emit_websocket_stream_outbound(
                &mut manager,
                &sink,
                chat_id,
                stream_id,
                String::new(),
                true,
                emit,
            )?;
        }
        Ok(())
    }

    fn drain_websocket_runtime_notifications(
        notification_rx: &mpsc::Receiver<OutboundMessage>,
        manager: &mut ChannelManager,
        sink: &WebSocketEventSink,
        emit: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        while let Ok(message) = notification_rx.try_recv() {
            if message.channel != WEBSOCKET_CHANNEL || !is_visible_runtime_notification(&message) {
                continue;
            }
            manager.dispatch_outbound(message).map_err(|error| {
                ApiError::internal(format!(
                    "websocket runtime notification dispatch failed: {error}"
                ))
            })?;
            for event in sink.take_events() {
                emit(event);
            }
        }
        Ok(())
    }

    fn emit_websocket_stream_outbound(
        manager: &mut ChannelManager,
        sink: &WebSocketEventSink,
        chat_id: &str,
        stream_id: &str,
        content: String,
        end: bool,
        emit: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        manager
            .dispatch_outbound(stream_outbound_message(
                WEBSOCKET_CHANNEL,
                chat_id,
                stream_id,
                content,
                end,
            ))
            .map_err(|error| {
                ApiError::internal(format!("websocket stream dispatch failed: {error}"))
            })?;
        for event in sink.take_events() {
            emit(event);
        }
        Ok(())
    }

    fn process_inbound_with_outbound(
        &self,
        message: InboundMessage,
        config: AgentLoopConfig,
        on_event: Option<ApiProviderEventCallback>,
        observability_hooks: &[ShacsBotObservabilityHook],
    ) -> Result<(AgentLoopTurnResult, Vec<shacs_channels::OutboundMessage>), ApiError> {
        self.process_inbound_with_outbound_inner(
            message,
            config,
            on_event,
            observability_hooks,
            None,
            None,
        )
    }

    fn process_inbound_with_outbound_inner(
        &self,
        message: InboundMessage,
        config: AgentLoopConfig,
        on_event: Option<ApiProviderEventCallback>,
        observability_hooks: &[ShacsBotObservabilityHook],
        live_notification_sink: Option<RuntimeNotificationSink>,
        subagent_runtime: Option<SubagentRuntime>,
    ) -> Result<(AgentLoopTurnResult, Vec<shacs_channels::OutboundMessage>), ApiError> {
        if self.runtime_verbose {
            eprintln!(
                "Processing message from {}:{}: {}",
                message.channel.as_str(),
                message.sender_id.as_str(),
                runtime_message_preview(&message),
            );
        }
        let sessions = SessionManager::new(&self.workspace).map_err(|error| {
            ApiError::internal(format!("session manager could not be initialized: {error}"))
        })?;
        let bus = MessageBus::new();
        let outbound_bus = bus.clone();
        let routing_metadata = stream_routing_metadata_from_inbound(&message);
        let reply_to = inbound_reply_to(&message);
        let context_builder = self.context_builder();
        let skill_callback_context = context_builder.clone();
        let skill_notification_callback = selected_skill_notification_callback(
            skill_callback_context,
            outbound_bus.clone(),
            live_notification_sink.clone(),
            message.channel.clone(),
            message.chat_id.clone(),
            routing_metadata.clone(),
            reply_to.clone(),
        );
        let subagent_runtime =
            subagent_runtime.unwrap_or_else(|| SubagentRuntime::with_bus(bus.clone()));
        let spawn_config = self.subagent_execution_config(&config);
        let subagent_client = self.client.clone();
        let spawner_runtime = subagent_runtime.clone();
        let notification_bus = bus.clone();
        let live_notification_sink = live_notification_sink.clone();
        let notification_channel = message.channel.clone();
        let notification_chat_id = message.chat_id.clone();
        let notification_metadata = routing_metadata.clone();
        let notification_reply_to = reply_to.clone();
        let plugin_notification_sink = plugin_hook_dispatch_notification_sink(
            outbound_bus.clone(),
            live_notification_sink.clone(),
            message.channel.clone(),
            message.chat_id.clone(),
            routing_metadata.clone(),
            reply_to.clone(),
        );
        let spawn_tool = SpawnTool::new(Arc::new(move |request| {
            spawner_runtime
                .spawn_and_run_background(request, subagent_client.clone(), spawn_config.clone())
                .map(|outcome| {
                    let notification = subagent_start_notification_message(
                        &notification_channel,
                        &notification_chat_id,
                        &notification_metadata,
                        notification_reply_to.as_deref(),
                        &outcome,
                    );
                    publish_runtime_notification(
                        &notification_bus,
                        live_notification_sink.as_ref(),
                        notification,
                    );
                    outcome.user_message
                })
        }));
        let mut tools = self.tools.clone();
        tools.register(spawn_tool.clone());
        let mut loop_runtime = AgentLoop::new(
            bus,
            sessions,
            context_builder,
            &tools,
            self.client.as_ref(),
            config,
        )
        .with_context_tools(shacs_core::runtime::RuntimeContextTools::new().with_spawn(spawn_tool))
        .with_session_turn_lock(self.session_turn_lock.clone());
        if let Some(message_tool) = &self.message_tool {
            loop_runtime = loop_runtime.with_message_tool_delivery(message_tool.clone());
        }
        let on_event = observability_provider_callback(observability_hooks, on_event);
        if let Some(callback) = on_event {
            loop_runtime = loop_runtime.with_provider_event_callback(callback);
        }
        let mut agent_hook: Option<Arc<dyn AgentHook>> = self
            .runtime_verbose
            .then(|| Arc::new(RuntimeVerboseLogHook) as Arc<dyn AgentHook>);
        let mut tool_event_callbacks = vec![skill_notification_callback];
        if !observability_hooks.is_empty() {
            let pending = Arc::new(Mutex::new(BTreeMap::new()));
            let observability_hook: Arc<dyn AgentHook> = Arc::new(ObservabilityToolStartHook::new(
                observability_hooks.to_vec(),
                pending.clone(),
            ));
            agent_hook = Some(match agent_hook {
                Some(existing) => Arc::new(CompositeHook::new(vec![existing, observability_hook])),
                None => observability_hook,
            });
            if let Some(callback) = observability_tool_callback(observability_hooks, pending) {
                tool_event_callbacks.push(callback);
            }
        }
        if self
            .plugin_runtime_snapshot
            .plugins
            .iter()
            .any(|plugin| !plugin.hooks.is_empty())
        {
            let plugin_hook: Arc<dyn AgentHook> = Arc::new(
                PluginRuntimeHookAgentHook::new(self.plugin_runtime_snapshot.clone())
                    .with_sink(plugin_notification_sink),
            );
            agent_hook = Some(match agent_hook {
                Some(existing) => Arc::new(CompositeHook::new(vec![existing, plugin_hook])),
                None => plugin_hook,
            });
        }
        if let Some(callback) = combine_tool_event_callbacks(tool_event_callbacks) {
            loop_runtime = loop_runtime.with_tool_event_callback(callback);
        }
        if let Some(hook) = agent_hook {
            loop_runtime = loop_runtime.with_agent_hook(hook);
        }
        let result = loop_runtime
            .process_message(message)
            .map_err(|error| match error {
                shacs_core::runtime::AgentLoopError::DuplicateActiveTurn { session_key } => {
                    ApiError {
                        status: 409,
                        message: format!("session turn already active: {session_key}"),
                        error_type: "session_busy".to_owned(),
                    }
                }
                error => ApiError::internal(format!("agent loop request failed: {error}")),
            })?;
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
        subagent.containment_snapshot = config.containment_snapshot.clone();
        subagent.permission_mode_snapshot = config.permission_mode_snapshot.clone();
        subagent.permission_ceiling_snapshot = config.permission_ceiling_snapshot.clone();
        subagent.max_iterations = config.max_iterations;
        subagent.max_tool_result_chars = config.max_tool_result_chars;
        subagent.fail_on_tool_error = true;
        subagent.allow_side_effect_tools = self.allow_side_effect_tools;
        subagent.enable_exec = self.allow_side_effect_tools;
        subagent.enable_web = true;
        subagent.restrict_to_workspace = true;
        subagent.exec_timeout_seconds = self.exec_timeout_seconds;
        subagent.exec_sandbox = self.exec_sandbox.clone();
        subagent.exec_path_append = self.exec_path_append.clone();
        subagent.exec_allowed_env_keys = self.exec_allowed_env_keys.clone();
        subagent.exec_env = self.exec_env.clone();
        subagent
    }

    fn execute_heartbeat_tasks(&self, tasks: &str) -> Result<String, HeartbeatError> {
        let message = InboundMessage::new("heartbeat", "system", "heartbeat", tasks.to_owned())
            .with_session_key_override("heartbeat");
        let (turn, _) = self
            .process_inbound_with_outbound(message, self.loop_config(), None, &[])
            .map_err(|error| HeartbeatError::Execute(error.to_string()))?;
        Ok(turn.final_content.unwrap_or_default())
    }

    fn attachment_media_root(&self) -> Result<PathBuf, ApiError> {
        self.media_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| ApiError::internal("media directory must have a parent attachment root"))
    }

    fn intake_api_attachments(
        &self,
        requests: Vec<ChannelAttachmentIntakeRequest>,
        source_label: &str,
    ) -> Result<Vec<String>, ApiError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let media_root = self.attachment_media_root()?;
        let policy = AttachmentLimitPolicy {
            max_attachments_per_message: DEFAULT_MAX_ATTACHMENTS_PER_MESSAGE,
            max_bytes_per_file: shacs_api::MAX_MEDIA_BYTES as u64,
            max_bytes_per_turn: DEFAULT_MAX_BYTES_PER_TURN,
        };
        let service = AttachmentIntakeService::new(&media_root, policy);
        let batch = service.intake(requests).map_err(|error| {
            ApiError::internal(format!("{source_label} could not be saved: {error}"))
        })?;
        let mut paths = Vec::with_capacity(batch.items.len());
        for item in batch.items {
            match item.intake_status {
                AttachmentIntakeStatus::Stored => {
                    let relative = item.media_root_relative_path.ok_or_else(|| {
                        ApiError::internal(format!("{source_label} storage path is missing"))
                    })?;
                    let absolute = media_root.join(relative).canonicalize().map_err(|error| {
                        ApiError::internal(format!(
                            "{source_label} stored path could not be resolved: {error}"
                        ))
                    })?;
                    paths.push(absolute.to_string_lossy().to_string());
                }
                AttachmentIntakeStatus::Blocked | AttachmentIntakeStatus::Skipped => {
                    return Err(api_error_from_attachment_intake_status(
                        source_label,
                        item.diagnostic_reason.as_deref(),
                    ));
                }
            }
        }
        Ok(paths)
    }
}

fn api_error_from_attachment_adapter_failure(
    index: usize,
    reason: ChannelAttachmentAdapterFailureReason,
    message: String,
) -> ApiError {
    match reason {
        ChannelAttachmentAdapterFailureReason::PayloadTooLargeBeforeStorage => {
            ApiError::payload_too_large(format!("media data URL {index} exceeds storage limits"))
        }
        ChannelAttachmentAdapterFailureReason::UnsupportedDataUrlMimeType => {
            ApiError::unsupported_media(message)
        }
        ChannelAttachmentAdapterFailureReason::MalformedDataUrl => {
            ApiError::unsupported_media("media URL must be a valid base64 data URL")
        }
        ChannelAttachmentAdapterFailureReason::MissingCredential
        | ChannelAttachmentAdapterFailureReason::PlatformDownloadFailed
        | ChannelAttachmentAdapterFailureReason::MimePartDecodeFailed => {
            ApiError::internal("media attachment could not be normalized")
        }
    }
}

fn api_error_from_attachment_intake_status(source_label: &str, reason: Option<&str>) -> ApiError {
    match reason {
        Some("file_size_exceeded") => {
            ApiError::payload_too_large(format!("{source_label} exceeds storage limits"))
        }
        Some("attachment_count_exceeded") | Some("turn_byte_limit_exceeded") => {
            ApiError::payload_too_large(format!("{source_label} exceeds request storage limits"))
        }
        Some(reason) => ApiError::internal(format!("{source_label} could not be saved: {reason}")),
        None => ApiError::internal(format!("{source_label} could not be saved")),
    }
}

fn append_external_attachment_projections(
    metadata: &mut Map<String, Value>,
    projections: Vec<Value>,
) {
    const KEY: &str = "external_attachment_projections";
    match metadata.get_mut(KEY) {
        Some(Value::Array(existing)) => existing.extend(projections),
        _ => {
            metadata.insert(KEY.to_owned(), Value::Array(projections));
        }
    }
}

fn external_attachment_projection_failure(
    item_index: usize,
    source_kind: &str,
    reason: &str,
    display_name: Option<String>,
) -> Value {
    let mut projection = Map::new();
    projection.insert("item_index".to_owned(), json!(item_index));
    projection.insert("source_kind".to_owned(), json!(source_kind));
    projection.insert("status".to_owned(), json!("failed"));
    projection.insert("reason".to_owned(), json!(reason));
    if let Some(display_name) = display_name {
        projection.insert("display_name".to_owned(), json!(display_name));
    }
    Value::Object(projection)
}

fn external_attachment_failure_reason(
    reason: ChannelAttachmentAdapterFailureReason,
) -> &'static str {
    match reason {
        ChannelAttachmentAdapterFailureReason::MalformedDataUrl => "malformed_data_url",
        ChannelAttachmentAdapterFailureReason::UnsupportedDataUrlMimeType => {
            "unsupported_data_url_mime_type"
        }
        ChannelAttachmentAdapterFailureReason::PayloadTooLargeBeforeStorage => {
            "payload_too_large_before_storage"
        }
        ChannelAttachmentAdapterFailureReason::MissingCredential => "missing_credential",
        ChannelAttachmentAdapterFailureReason::PlatformDownloadFailed => "platform_download_failed",
        ChannelAttachmentAdapterFailureReason::MimePartDecodeFailed => "mime_part_decode_failed",
    }
}

fn split_named_data_url(media: &str) -> (String, Option<String>) {
    let Some((header, payload)) = media.split_once(',') else {
        return (media.to_owned(), None);
    };
    let Some(header_body) = header.strip_prefix("data:") else {
        return (media.to_owned(), None);
    };
    let mut name = None;
    let mut kept_parts = Vec::new();
    for part in header_body.split(';') {
        if let Some(encoded_name) = part.strip_prefix("name=") {
            name = base64_url_decode(encoded_name).and_then(|bytes| String::from_utf8(bytes).ok());
        } else {
            kept_parts.push(part);
        }
    }
    if name.is_none() {
        return (media.to_owned(), None);
    }
    (format!("data:{},{payload}", kept_parts.join(";")), name)
}

fn platform_media_projection(item_index: usize, media: &str) -> Option<Value> {
    let (prefix, rest) = media.split_once(':')?;
    let prefix_body = prefix.strip_prefix("shacs-")?;
    let (platform, kind) = prefix_body.split_once('-')?;
    let mut parts = rest.split(':');
    let handle_hash = parts.next().filter(|value| !value.is_empty())?;
    let declared_mime = parts.next().and_then(decode_optional_base64_url_string);
    let display_name = parts.next().and_then(decode_optional_base64_url_string);
    let declared_byte_length = parts.next().and_then(|value| value.parse::<u64>().ok());

    let mut projection = Map::new();
    projection.insert("item_index".to_owned(), json!(item_index));
    projection.insert("source_kind".to_owned(), json!("platform_download"));
    projection.insert("status".to_owned(), json!("failed"));
    projection.insert("reason".to_owned(), json!("platform_download_failed"));
    projection.insert("platform".to_owned(), json!(platform));
    projection.insert("platform_kind".to_owned(), json!(kind));
    projection.insert("handle_hash".to_owned(), json!(handle_hash));
    if let Some(display_name) = display_name {
        projection.insert(
            "display_name".to_owned(),
            json!(redact_string(&display_name)),
        );
    }
    if let Some(declared_mime) = declared_mime {
        projection.insert("declared_mime".to_owned(), json!(declared_mime));
    }
    if let Some(declared_byte_length) = declared_byte_length {
        projection.insert(
            "declared_byte_length".to_owned(),
            json!(declared_byte_length),
        );
    }
    Some(Value::Object(projection))
}

fn decode_optional_base64_url_string(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    base64_url_decode(value).and_then(|bytes| String::from_utf8(bytes).ok())
}

fn external_media_source_kind(media: &str) -> &'static str {
    let lower = media.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        "platform_download"
    } else if lower.starts_with("file:")
        || Path::new(media).is_absolute()
        || media.contains('/')
        || media.contains('\\')
    {
        "local_multipart"
    } else {
        "bridge_media_handle"
    }
}

fn external_media_display_basename(media: &str) -> Option<String> {
    let without_fragment = media
        .split_once('#')
        .map(|(before, _)| before)
        .unwrap_or(media);
    let without_query = without_fragment
        .split_once('?')
        .map(|(before, _)| before)
        .unwrap_or(without_fragment);
    let trimmed = without_query.trim_end_matches(['/', '\\']);
    if trimmed == media && !media.contains('/') && !media.contains('\\') {
        return None;
    }
    let basename = trimmed.rsplit(['/', '\\']).next()?.trim();
    if basename.is_empty() {
        return None;
    }
    let redacted = redact_string(basename);
    Some(redacted.chars().take(80).collect())
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

    fn diagnostics_snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            generated_at_ms: shacs_utils::diagnostics::current_time_ms(),
            runtime: json!({
                "workspace": { "path": self.workspace, "exists": self.workspace.exists() },
                "media_dir": { "path": self.media_dir, "exists": self.media_dir.exists() },
                "defaults": {
                    "provider": self.provider_id,
                    "model": self.configured_model,
                    "resolved_model": self.resolved_model,
                },
                "capabilities": {
                    "side_effect_tools_allowed": self.allow_side_effect_tools,
                    "send_progress": self.send_progress,
                    "send_tool_hints": self.send_tool_hints,
                    "send_max_retries": self.send_max_retries,
                    "exec_timeout_seconds": self.exec_timeout_seconds,
                    "exec_sandbox": self.exec_sandbox,
                    "exec_allowed_env_keys": self.exec_allowed_env_keys,
                    "exec_env_keys": self.exec_env.keys().collect::<Vec<_>>(),
                }
            }),
            operational_logs: vec![OperationalLogRecord::new(
                DiagnosticsSeverity::Info,
                DiagnosticsKind::Api,
                "API diagnostics inspected adapter state only",
            )],
            traces: Vec::new(),
            diagnostics: vec![DiagnosticsRecord::new(
                DiagnosticsSeverity::Info,
                DiagnosticsKind::Runtime,
                "agent loop adapter diagnostics snapshot generated",
            )],
            crash_evidence: Vec::new(),
            recovery_evidence: Vec::new(),
            provider_progress: Vec::new(),
            tool_progress: Vec::new(),
            subagent_progress: Vec::new(),
        }
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
        self.persist_media_data_urls_with_context("api:default", "api", data_urls, &[])
    }

    fn persist_media_data_urls_for_session(
        &self,
        session_key: &str,
        data_urls: &[String],
    ) -> Result<Vec<String>, ApiError> {
        self.persist_media_data_urls_with_context(session_key, "api", data_urls, &[])
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }

    fn persist_uploaded_file(
        &self,
        filename: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        self.persist_uploaded_file_for_session("api:default", filename, bytes)
    }

    fn persist_uploaded_file_for_session(
        &self,
        session_key: &str,
        filename: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        self.persist_uploaded_file_with_context(session_key, "api", filename, bytes)
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

    fn process_websocket_frame_streaming(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
        on_event: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        self.process_websocket_frame_events(frame, client_id, default_chat_id, on_event)
    }
}

impl AgentLoopChatCompletionAdapter {
    fn normalize_external_inbound_media(
        &self,
        mut inbound: InboundMessage,
        config: &AgentLoopConfig,
    ) -> Result<InboundMessage, ApiError> {
        if inbound.media.is_empty() {
            return Ok(inbound);
        }

        let session_key = effective_external_session_key(config, &inbound);
        let channel = inbound.channel.clone();
        let mut data_urls = Vec::new();
        let mut data_url_names = Vec::new();
        let mut projections = Vec::new();

        for (index, media) in inbound.media.iter().enumerate() {
            if media.starts_with("data:") {
                let (data_url, name) = split_named_data_url(media);
                let normalized = normalize_channel_attachment_data_url(
                    &session_key,
                    &channel,
                    name.clone(),
                    name.clone(),
                    &data_url,
                    DEFAULT_MAX_BYTES as u64,
                );
                if let Some(failure) = normalized.failures.into_iter().next() {
                    projections.push(external_attachment_projection_failure(
                        index,
                        "data_url",
                        external_attachment_failure_reason(failure.diagnostic.reason),
                        None,
                    ));
                } else {
                    data_urls.push(data_url);
                    data_url_names.push(name);
                }
                continue;
            }

            projections.push(platform_media_projection(index, media).unwrap_or_else(|| {
                external_attachment_projection_failure(
                    index,
                    external_media_source_kind(media),
                    "unsupported_external_media",
                    external_media_display_basename(media),
                )
            }));
        }

        let stored_media = self.persist_media_data_urls_with_context(
            &session_key,
            &channel,
            &data_urls,
            &data_url_names,
        )?;
        inbound.media = stored_media;
        inbound.session_key_override = Some(session_key);
        if !projections.is_empty() {
            append_external_attachment_projections(&mut inbound.metadata, projections);
        }
        Ok(inbound)
    }

    fn persist_media_data_urls_with_context(
        &self,
        session_key: &str,
        channel: &str,
        data_urls: &[String],
        names: &[Option<String>],
    ) -> Result<Vec<String>, ApiError> {
        let mut requests = Vec::with_capacity(data_urls.len());
        for (index, data_url) in data_urls.iter().enumerate() {
            let name = names.get(index).cloned().flatten();
            let normalized = normalize_channel_attachment_data_url(
                session_key,
                channel,
                name.clone(),
                name,
                data_url,
                DEFAULT_MAX_BYTES as u64,
            );
            if let Some(failure) = normalized.failures.into_iter().next() {
                return Err(api_error_from_attachment_adapter_failure(
                    index,
                    failure.diagnostic.reason,
                    failure.diagnostic.message,
                ));
            }
            requests.extend(normalized.requests);
        }
        self.intake_api_attachments(requests, "media data URL")
    }

    fn persist_uploaded_file_with_context(
        &self,
        session_key: &str,
        channel: &str,
        filename: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        if bytes.len() > shacs_api::MAX_MEDIA_BYTES {
            return Err(ApiError::payload_too_large(format!(
                "uploaded file exceeds {} bytes",
                shacs_api::MAX_MEDIA_BYTES
            )));
        }
        let request = ChannelAttachmentIntakeRequest::from_bytes(
            session_key,
            channel,
            filename.map(str::to_owned),
            None,
            bytes.to_vec(),
        );
        let mut paths = self.intake_api_attachments(vec![request], "uploaded file")?;
        paths
            .pop()
            .ok_or_else(|| ApiError::internal("uploaded file was not stored"))
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
    message_tool: Option<MessageTool>,
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
    let message_tool = if allow_side_effect_tools {
        let tool = MessageTool::new(workspace).with_media_roots([bundle.context.media_dir(None)]);
        registry.register(tool.clone());
        Some(tool)
    } else {
        None
    };
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
        exec_config.env = configured_exec_env(&bundle.config);
        registry.register(ExecTool::new(exec_config));
    }
    if allow_side_effect_tools && bundle.config.tools.image_generation.enable {
        let image_config = &bundle.config.tools.image_generation;
        let provider_registry = ProviderRegistry::new();
        let resolved = resolve_image_generation_client(
            &provider_registry,
            &image_config.provider,
            &image_config.model,
            &bundle.config.providers,
        )
        .map_err(|error| {
            CliError::Config(ConfigError::Env(format!(
                "image_generate provider could not be configured: {}",
                render_image_generation_provider_error(error)
            )))
        })?;
        let image_media_dir = bundle.context.media_dir(Some("image-generation"));
        fs::create_dir_all(&image_media_dir)?;
        registry.register(ImageGenerateTool::new(
            resolved.client,
            image_media_dir,
            ImageGenerateToolConfig {
                provider_id: resolved.provider_id,
                model_id: resolved.model,
                default_format: image_config.default_format.clone(),
                max_count: image_config.max_count,
                max_bytes: image_config.max_bytes,
            },
        ));
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
        message_tool,
        mcp_runtime,
        mcp_reports,
    })
}

fn configured_exec_env(config: &shacs_config::Config) -> BTreeMap<String, String> {
    let mut env = config.env.clone();
    env.extend(config.tools.exec.env.clone());
    env
}

fn runtime_tool_search_config(config: &shacs_config::ToolSearchConfig) -> ToolSearchConfig {
    ToolSearchConfig {
        enabled: match config.enabled {
            shacs_config::ToolSearchMode::Off => ToolSearchMode::Off,
            shacs_config::ToolSearchMode::On => ToolSearchMode::On,
            shacs_config::ToolSearchMode::Auto => ToolSearchMode::Auto,
        },
        threshold_pct: config.threshold_pct,
        search_default_limit: config.search_default_limit,
        max_search_limit: config.max_search_limit,
    }
}

fn render_image_generation_provider_error(error: ProviderError) -> String {
    match error {
        ProviderError::AuthRequired { provider_id } => {
            format!("provider {provider_id} requires configured authentication")
        }
        ProviderError::UnsupportedCapability {
            provider_id,
            capability,
        } => format!("provider {provider_id} does not support {capability}"),
        ProviderError::ProviderNotFound { provider_id, .. } => {
            format!("provider {provider_id} was not found")
        }
        ProviderError::ModelNotFound {
            provider_id,
            model_id,
            ..
        } => format!("model {provider_id}/{model_id} was not found"),
        ProviderError::Api {
            status,
            message,
            retryable,
            ..
        } => format!(
            "provider API error status={} retryable={retryable}: {message}",
            status
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ),
    }
}

fn mcp_server_specs(bundle: &ConfigBundle) -> Vec<McpServerSpec> {
    let parent_containment_snapshot = Some(runtime_containment_snapshot_ref(
        &runtime_containment_inspect(bundle),
    ));
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
            parent_containment_snapshot: parent_containment_snapshot.clone(),
        })
        .collect()
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
        ProviderError::UnsupportedCapability {
            provider_id,
            capability,
        } => ApiError::invalid_request(format!(
            "provider `{provider_id}` does not support `{capability}`"
        )),
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
        format!("Binary version: {}", report.lifecycle.binary_version),
        format!(
            "Data schema: {} (min {})",
            report.lifecycle.data_schema_version, report.lifecycle.data_schema_min_version
        ),
        format!("Compatibility: {}", report.lifecycle.compatibility.as_str()),
        format!(
            "Ownership: {} ({})",
            report.lifecycle.ownership.state.as_str(),
            report.lifecycle.ownership.reason
        ),
        format!("Sessions: {}", report.sessions.count),
    ];
    if let Some(marker) = &report.lifecycle.ownership.marker {
        lines.push(format!(
            "Owner: pid={} mode={} updated_at_ms={}",
            marker.pid, marker.mode, marker.updated_at_ms
        ));
    }
    if let Some(request) = &report.lifecycle.stop_request {
        lines.push(format!(
            "Stop request: {} requested_at_ms={} owner_pid={}",
            request.request,
            request.requested_at_ms,
            request
                .owner_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        ));
    } else {
        lines.push("Stop request: none".to_owned());
    }
    match report.lifecycle.update_marker {
        Some(marker) => lines.push(format!(
            "Update marker: {} {} -> {} (migration_required={})",
            marker.phase, marker.from_version, marker.target_version, marker.migration_required
        )),
        None => lines.push("Update marker: none".to_owned()),
    }
    if let Some(latest_key) = report.sessions.latest_key {
        let updated = report
            .sessions
            .latest_updated_at
            .unwrap_or_else(|| "unknown".to_owned());
        lines.push(format!("Latest session: {latest_key} ({updated})"));
    }
    lines.push(format!(
        "Runtime containment: contained={} backend={} snapshot_digest={}",
        optional_bool_label(report.containment.contained),
        report.containment.backend.as_deref().unwrap_or("none"),
        report.containment.digest.as_deref().unwrap_or("none")
    ));
    lines.push(format!(
        "Generated image artifacts: {}",
        report.generated_media.len()
    ));
    for artifact in &report.generated_media {
        lines.push(format!(
            "  - {}: {} {} bytes redacted={}",
            artifact.artifact_id, artifact.mime_type, artifact.byte_len, artifact.redacted
        ));
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

fn format_runtime_diagnostics(report: RuntimeDiagnosticsReport) -> String {
    let mut output = match serde_json::to_string_pretty(&report.snapshot.redacted_value()) {
        Ok(value) => value,
        Err(error) => format!(
            "{{\n  \"error\": \"diagnostics snapshot could not be formatted safely: {error}\"\n}}"
        ),
    };
    if let Some(path) = report.bundle_path {
        match report.bundle_error {
            Some(error) => {
                output.push_str(&format!(
                    "\nBundle: failed at {} ({error})",
                    redact_string(&display_path(&path)),
                    error = redact_string(&error)
                ));
            }
            None => output.push_str(&format!(
                "\nBundle: {}",
                redact_string(&display_path(&path))
            )),
        }
    }
    output
}

fn optional_bool_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

fn format_runtime_update(outcome: RuntimeUpdateOutcome) -> String {
    [
        "shacs-bot runtime update".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Workspace: {}", display_path(&outcome.workspace)),
        format!("Data dir: {}", display_path(&outcome.data_dir)),
        format!("Current version: {}", outcome.from_version),
        format!("Target version: {}", outcome.target_version),
        format!("Migration required: {}", outcome.migration_required),
        format!("Phase: {}", outcome.phase),
        format!("Marker: {}", display_path(&outcome.marker_path)),
        "Note: Rust source installs still require replacing/rebuilding the binary separately. This command records the local no-op runtime upgrade marker and compatibility evidence.".to_owned(),
    ]
    .join("\n")
}

fn format_runtime_recover(outcome: RuntimeRecoverOutcome) -> String {
    [
        "shacs-bot runtime recover".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Workspace: {}", display_path(&outcome.workspace)),
        format!("Data dir: {}", display_path(&outcome.data_dir)),
        format!("Marker: {}", display_path(&outcome.marker_path)),
        format!("Recovered: {}", outcome.recovered),
        format!("Detail: {}", outcome.detail),
    ]
    .join("\n")
}

fn format_runtime_stop(outcome: RuntimeStopOutcome) -> String {
    format_runtime_stop_like("shacs-bot runtime stop", outcome)
}

fn format_runtime_restart(outcome: RuntimeStopOutcome) -> String {
    format_runtime_stop_like("shacs-bot runtime restart", outcome)
}

fn format_runtime_stop_like(title: &str, outcome: RuntimeStopOutcome) -> String {
    [
        title.to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Workspace: {}", display_path(&outcome.workspace)),
        format!("Data dir: {}", display_path(&outcome.data_dir)),
        format!("Request marker: {}", display_path(&outcome.request_path)),
        format!("Status: {}", outcome.status.as_str()),
        format!("Detail: {}", outcome.detail),
    ]
    .join("\n")
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

fn format_copilot_import_outcome(outcome: CopilotImportOutcome) -> String {
    let model_line = outcome
        .selected_model
        .as_deref()
        .map(|model| format!("Selected model: {model}"))
        .unwrap_or_else(|| "Selected model: unchanged".to_owned());
    [
        "GitHub Copilot token imported.".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Auth: {}", display_path(&outcome.auth_path)),
        format!("Provider: {}", outcome.provider),
        model_line,
        format!("Selected: {}", configured_label(outcome.selected)),
    ]
    .join("\n")
}

fn format_provider_api_key_import_outcome(outcome: ProviderApiKeyImportOutcome) -> String {
    [
        "Provider API key imported.".to_owned(),
        format!("Config: {}", display_path(&outcome.config_path)),
        format!("Auth: {}", display_path(&outcome.auth_path)),
        format!("Provider: {}", outcome.provider),
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

fn display_path_escaped(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(char::escape_default)
        .collect()
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
    use shacs_config::{
        load_auth_store, save_auth_store_to_path, save_config_to_path, AuthStore, Config,
    };
    use shacs_core::runtime::{
        ChildResultEnvelope, ChildResultStatus, ContainerNetworkMode, ContainerRuntimeKind,
        DockerContainmentSnapshot, PermissionCeilingSnapshot, PermissionMode, PermissionRuleInput,
        PluginExecutableCommand, PluginHookCallbackResult, PluginHookDispatchAttempt,
        PluginHookEvent, PluginManifestSource, PluginRuntimeHook, PluginRuntimePlugin,
        ProcExecSummary, RuntimeBoundaryOrigin, SafetyCapability, Session,
    };
    use shacs_core::tools::{JsonMap, Tool, ToolResult};
    use shacs_providers::{
        GenerationSettings, ProviderClient, ProviderEvent, ProviderRequest, ToolCallRequest,
    };
    use shacs_templates::WorkspaceSyncOutcome;
    use std::collections::{BTreeMap, VecDeque};
    use std::error::Error;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};

    fn read_repo_text(relative_path: &str) -> Result<String, Box<dyn Error>> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(relative_path);
        Ok(fs::read_to_string(path)?)
    }

    #[test]
    fn native_image_input_gate_requires_backend_and_model_evidence() {
        assert!(!provider_model_supports_native_image_input(
            "local_text_backend",
            "gpt-4o"
        ));
        assert!(!provider_model_supports_native_image_input(
            "openai_compat",
            "gpt-3.5-turbo"
        ));
        assert!(!provider_model_supports_native_image_input(
            "azure_openai",
            "text-davinci-003"
        ));
        assert!(!provider_model_supports_native_image_input(
            "anthropic",
            "claude-2"
        ));
        assert!(provider_model_supports_native_image_input(
            "openai_compat",
            "openai/gpt-4o-mini"
        ));
        assert!(provider_model_supports_native_image_input(
            "anthropic",
            "claude-3-5-sonnet-latest"
        ));
        assert!(provider_model_supports_native_image_input(
            "openai_compat",
            "google/gemini-2.5-pro"
        ));
    }

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

    fn spec025_write_plugin_fixture(
        root: &Path,
        dir: &str,
        name: &str,
        requires_env: &[&str],
    ) -> Result<(), Box<dyn Error>> {
        let plugin_dir = root.join("plugins").join(dir);
        fs::create_dir_all(&plugin_dir)?;
        fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_vec_pretty(&json!({
                "schemaVersion": 1,
                "name": name,
                "version": "0.1.0",
                "description": "Spec 025 test plugin",
                "requiresEnv": requires_env,
                "requiresConfig": ["SPEC025_CONFIG_SECRET"],
                "surfaces": {
                    "tools": ["demo_tool"],
                    "hooks": ["llm:before"],
                    "skills": ["demo_skill"],
                    "commands": ["demo_command"],
                    "mcp": ["demo_mcp"]
                },
                "entrypoints": {
                    "tools": {"demo_tool": {"description": "Demo tool"}},
                    "commands": {"demo_command": {"backend": "descriptor"}},
                    "mcp": {"demo_mcp": {"backend": "descriptor"}}
                }
            }))?,
        )?;
        Ok(())
    }

    fn spec025_config_fixture() -> Result<(tempfile::TempDir, PathBuf, PathBuf), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.env.insert(
            "SPEC025_CONFIG_SECRET".to_owned(),
            "RAW_CONFIG_SECRET".to_owned(),
        );
        save_config_to_path(&config, &config_path)?;
        spec025_write_plugin_fixture(root.path(), "demo", "demo-plugin", &[])?;
        Ok((root, config_path, workspace))
    }

    #[test]
    fn spec025_parser_accepts_plugin_and_hook_projection_commands() -> Result<(), Box<dyn Error>> {
        assert!(matches!(
            parse_cli_args(["plugin", "list", "-c", "/tmp/config.json"])?,
            CliCommand::Plugins(PluginsCommand::List(_))
        ));
        assert!(matches!(
            parse_cli_args(["plugins", "inspect", "demo-plugin", "-w", "/tmp/ws"])?,
            CliCommand::Plugins(PluginsCommand::Inspect(_))
        ));
        assert!(matches!(
            parse_cli_args(["plugins", "doctor"])?,
            CliCommand::Plugins(PluginsCommand::Doctor(_))
        ));
        assert!(matches!(
            parse_cli_args(["plugins", "enable", "demo-plugin"])?,
            CliCommand::Plugins(PluginsCommand::Enable(_))
        ));
        assert!(matches!(
            parse_cli_args(["plugins", "disable", "demo-plugin"])?,
            CliCommand::Plugins(PluginsCommand::Disable(_))
        ));
        assert!(matches!(
            parse_cli_args(["hook", "list"])?,
            CliCommand::Hooks(HooksCommand::List(_))
        ));
        assert!(matches!(
            parse_cli_args(["hooks", "inspect", "llm:before"])?,
            CliCommand::Hooks(HooksCommand::Inspect(_))
        ));
        Ok(())
    }

    #[test]
    fn spec025_plugins_list_inspect_doctor_render_state_without_raw_secrets(
    ) -> Result<(), Box<dyn Error>> {
        let (_root, config_path, workspace) = spec025_config_fixture()?;
        let list = run_command(CliCommand::Plugins(PluginsCommand::List(
            PluginsListOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(workspace.clone()),
            },
        )))?;
        assert!(list.contains("descriptor-only projection"));
        assert!(list.contains("demo-plugin [not_enabled]"));
        assert!(!list.contains("RAW_CONFIG_SECRET"));

        let inspect = run_command(CliCommand::Plugins(PluginsCommand::Inspect(
            PluginsInspectOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(workspace.clone()),
                name: "demo-plugin".to_owned(),
            },
        )))?;
        assert!(inspect.contains("no hooks, MCP, tools, commands, or processes were executed"));
        assert!(inspect.contains("SPEC025_CONFIG_SECRET"));
        assert!(!inspect.contains("RAW_CONFIG_SECRET"));

        let doctor = run_command(CliCommand::Plugins(PluginsCommand::Doctor(
            PluginsListOptions {
                config_path: Some(config_path),
                workspace_override: Some(workspace),
            },
        )))?;
        assert!(doctor.contains("no live plugin execution"));
        assert!(!doctor.contains("RAW_CONFIG_SECRET"));
        Ok(())
    }

    #[test]
    fn spec025_enable_disable_mutate_config_only_and_report_next_session(
    ) -> Result<(), Box<dyn Error>> {
        let (_root, config_path, workspace) = spec025_config_fixture()?;
        let enable = run_command(CliCommand::Plugins(PluginsCommand::Enable(
            PluginsMutateOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(workspace.clone()),
                name: "demo-plugin".to_owned(),
            },
        )))?;
        assert!(enable.contains("Action: enabled"));
        assert!(enable.contains("next session or runtime reload"));
        assert!(enable.contains("no plugin code was executed"));
        let saved: Config = serde_json::from_str(&fs::read_to_string(&config_path)?)?;
        assert_eq!(saved.plugins.enabled, ["demo-plugin"]);
        assert!(saved.plugins.disabled.is_empty());

        let disable = run_command(CliCommand::Plugins(PluginsCommand::Disable(
            PluginsMutateOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(workspace),
                name: "demo-plugin".to_owned(),
            },
        )))?;
        assert!(disable.contains("Action: disabled"));
        assert!(disable.contains("next session or runtime reload"));
        let saved: Config = serde_json::from_str(&fs::read_to_string(config_path)?)?;
        assert!(saved.plugins.enabled.is_empty());
        assert_eq!(saved.plugins.disabled, ["demo-plugin"]);
        Ok(())
    }

    #[test]
    fn spec025_plugin_mutation_patches_only_raw_plugin_gates() -> Result<(), Box<dyn Error>> {
        let (root, config_path, workspace) = spec025_config_fixture()?;
        fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "agents": {"defaults": {"workspace": workspace.to_string_lossy()}},
                "env": {"SPEC025_CONFIG_SECRET": "RAW_CONFIG_SECRET"},
                "plugins": {
                    "enabled": ["other-plugin"],
                    "disabled": ["demo-plugin", "stale-plugin"],
                    "trustedWorkspaces": ["/tmp/trusted"],
                    "unknownPluginField": {"kept": true}
                },
                "unknownTopLevel": {"kept": true}
            }))?,
        )?;
        spec025_write_plugin_fixture(root.path(), "other", "other-plugin", &[])?;

        let report = plugins_enable(PluginsMutateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: Some(workspace),
            name: "demo-plugin".to_owned(),
        })?;
        assert!(report.changed);

        let saved: Value = serde_json::from_str(&fs::read_to_string(config_path)?)?;
        assert_eq!(saved["unknownTopLevel"], json!({"kept": true}));
        assert_eq!(
            saved["plugins"]["unknownPluginField"],
            json!({"kept": true})
        );
        assert_eq!(
            saved["plugins"]["trustedWorkspaces"],
            json!(["/tmp/trusted"])
        );
        assert_eq!(
            saved["plugins"]["enabled"],
            json!(["demo-plugin", "other-plugin"])
        );
        assert_eq!(saved["plugins"]["disabled"], json!(["stale-plugin"]));
        Ok(())
    }

    #[test]
    fn spec025_disable_removes_stale_configured_plugin_without_manifest(
    ) -> Result<(), Box<dyn Error>> {
        let (_root, config_path, workspace) = spec025_config_fixture()?;
        fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "agents": {"defaults": {"workspace": workspace.to_string_lossy()}},
                "plugins": {
                    "enabled": ["stale-plugin"],
                    "disabled": ["stale-plugin"],
                    "unknownPluginField": true
                }
            }))?,
        )?;

        let report = plugins_disable(PluginsMutateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: Some(workspace.clone()),
            name: "stale-plugin".to_owned(),
        })?;
        assert!(report.changed);

        let saved: Value = serde_json::from_str(&fs::read_to_string(&config_path)?)?;
        assert!(saved["plugins"].get("enabled").is_none());
        assert!(saved["plugins"].get("disabled").is_none());
        assert_eq!(saved["plugins"]["unknownPluginField"], json!(true));

        let unknown = plugins_disable(PluginsMutateOptions {
            config_path: Some(config_path),
            workspace_override: Some(workspace),
            name: "never-configured".to_owned(),
        });
        assert!(unknown.is_err());
        Ok(())
    }

    #[test]
    fn spec025_enable_rejects_unknown_or_blocked_without_config_write() -> Result<(), Box<dyn Error>>
    {
        let (root, config_path, workspace) = spec025_config_fixture()?;
        spec025_write_plugin_fixture(root.path(), "blocked", "blocked-plugin", &["MISSING_ENV"])?;
        let before = fs::read_to_string(&config_path)?;
        let unknown = run_command(CliCommand::Plugins(PluginsCommand::Enable(
            PluginsMutateOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(workspace.clone()),
                name: "missing-plugin".to_owned(),
            },
        )));
        assert!(unknown.is_err());
        assert_eq!(fs::read_to_string(&config_path)?, before);

        let blocked = run_command(CliCommand::Plugins(PluginsCommand::Enable(
            PluginsMutateOptions {
                config_path: Some(config_path.clone()),
                workspace_override: Some(workspace),
                name: "blocked-plugin".to_owned(),
            },
        )));
        assert!(blocked
            .err()
            .map(|error| error.to_string().contains("blocked"))
            .unwrap_or(false));
        assert_eq!(fs::read_to_string(config_path)?, before);
        Ok(())
    }

    #[test]
    fn spec025_hooks_list_inspect_render_metadata_without_dispatch() -> Result<(), Box<dyn Error>> {
        let (_root, config_path, workspace) = spec025_config_fixture()?;
        let _enabled = plugins_enable(PluginsMutateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: Some(workspace.clone()),
            name: "demo-plugin".to_owned(),
        })?;

        let list = run_command(CliCommand::Hooks(HooksCommand::List(HooksListOptions {
            config_path: Some(config_path.clone()),
            workspace_override: Some(workspace.clone()),
        })))?;
        assert!(list.contains("no hook dispatch or plugin process execution"));
        assert!(list.contains("catalog llm:before"));
        assert!(list.contains("plugin demo-plugin event=llm:before execution=false"));

        let inspect = run_command(CliCommand::Hooks(HooksCommand::Inspect(
            HooksInspectOptions {
                config_path: Some(config_path),
                workspace_override: Some(workspace),
                filter: "llm:before".to_owned(),
            },
        )))?;
        assert!(inspect.contains("Catalog: llm:before"));
        assert!(inspect.contains("Plugin hook: demo-plugin event=llm:before execution=false"));
        Ok(())
    }

    #[test]
    fn spec025_help_lists_plugins_and_hooks_as_implemented_commands() {
        let help = help_text();
        assert!(help.contains("plugins   List, inspect, doctor, enable, or disable"));
        assert!(help.contains("hooks     List and inspect descriptor-only"));
        assert!(!help.contains("Reserved for a later plugin slice"));
    }

    #[test]
    fn dockerfile_runtime_stage_uses_non_root_user() -> Result<(), Box<dyn Error>> {
        let dockerfile = read_repo_text("Dockerfile")?;
        assert!(dockerfile.contains("FROM python:3.14-slim-bookworm AS runtime"));
        assert!(dockerfile.contains("USER shacs"));
        assert!(dockerfile.contains("SHACS_RUNTIME_PACKAGE=shacs-bot-official-container"));
        Ok(())
    }

    #[test]
    fn docker_compose_defaults_do_not_mount_docker_socket() -> Result<(), Box<dyn Error>> {
        let compose = read_repo_text("docker-compose.yml")?;
        assert!(!compose.contains("/var/run/docker.sock"));
        assert!(!compose.contains("docker.sock"));
        Ok(())
    }

    #[test]
    fn docker_compose_defaults_do_not_set_privileged_mode() -> Result<(), Box<dyn Error>> {
        let compose = read_repo_text("docker-compose.yml")?;
        assert!(!compose.contains("privileged: true"));
        Ok(())
    }

    #[test]
    fn docker_compose_defaults_do_not_use_host_network_mode() -> Result<(), Box<dyn Error>> {
        let compose = read_repo_text("docker-compose.yml")?;
        assert!(!compose.contains("network_mode: host"));
        assert!(!compose.contains("network_mode: \"host\""));
        Ok(())
    }

    #[test]
    fn docker_compose_default_mount_remains_scoped_to_shacs_home() -> Result<(), Box<dyn Error>> {
        let compose = read_repo_text("docker-compose.yml")?;
        assert!(compose.contains("~/.shacs-bot:/home/shacs/.shacs-bot"));
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
    fn parser_handles_apps_command_surface() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "apps",
            "install",
            "--bundle",
            "/tmp/demo.shacsapp",
            "--workspace",
            "/tmp/workspace",
            "--config",
            "/tmp/config.json",
        ])?;
        let CliCommand::Apps(AppsCommand::Install(options)) = parsed else {
            return Err("expected apps install command".into());
        };
        assert_eq!(options.bundle_path, PathBuf::from("/tmp/demo.shacsapp"));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/config.json")));

        let parsed = parse_cli_args([
            "apps",
            "init",
            "demo.app",
            "--workspace",
            "/tmp/workspace",
            "--config",
            "/tmp/config.json",
        ])?;
        let CliCommand::Apps(AppsCommand::Init(options)) = parsed else {
            return Err("expected apps init command".into());
        };
        assert_eq!(options.app_id, "demo.app");
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/config.json")));

        let parsed = parse_cli_args(["apps", "new", "--app-id", "demo_app"])?;
        let CliCommand::Apps(AppsCommand::Init(options)) = parsed else {
            return Err("expected apps new command".into());
        };
        assert_eq!(options.app_id, "demo_app");

        let parsed = parse_cli_args(["apps", "init", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));

        let parsed = parse_cli_args(["apps", "list", "-w", "/tmp/workspace"])?;
        let CliCommand::Apps(AppsCommand::List(options)) = parsed else {
            return Err("expected apps list command".into());
        };
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let parsed = parse_cli_args(["apps", "inspect", "demo.app"])?;
        let CliCommand::Apps(AppsCommand::Inspect(options)) = parsed else {
            return Err("expected apps inspect command".into());
        };
        assert_eq!(options.app_id, "demo.app");

        let parsed = parse_cli_args(["apps", "inspect", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));

        let parsed = parse_cli_args(["apps", "show", "--app-id", "demo.app"])?;
        let CliCommand::Apps(AppsCommand::Inspect(options)) = parsed else {
            return Err("expected apps show command".into());
        };
        assert_eq!(options.app_id, "demo.app");

        let parsed = parse_cli_args(["apps", "enable", "demo.app"])?;
        let CliCommand::Apps(AppsCommand::Enable(options)) = parsed else {
            return Err("expected apps enable command".into());
        };
        assert_eq!(options.app_id, "demo.app");

        let parsed = parse_cli_args(["apps", "enable", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));

        let parsed = parse_cli_args(["apps", "disable", "demo.app"])?;
        let CliCommand::Apps(AppsCommand::Disable(options)) = parsed else {
            return Err("expected apps disable command".into());
        };
        assert_eq!(options.app_id, "demo.app");

        let parsed = parse_cli_args(["apps", "disable", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));

        let parsed = parse_cli_args(["apps", "uninstall", "demo.app"])?;
        let CliCommand::Apps(AppsCommand::Uninstall(options)) = parsed else {
            return Err("expected apps uninstall command".into());
        };
        assert_eq!(options.app_id, "demo.app");

        let parsed = parse_cli_args(["apps", "uninstall", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));
        Ok(())
    }

    #[test]
    fn apps_commands_use_configured_workspace() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let configured_workspace = root.path().join("configured-workspace");
        let overridden_workspace = root.path().join("overridden-workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = configured_workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let report = apps_list(AppsListOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;
        assert_eq!(report.workspace, configured_workspace);

        let overridden = apps_list(AppsListOptions {
            config_path: Some(config_path),
            workspace_override: Some(overridden_workspace.clone()),
        })?;
        assert_eq!(overridden.workspace, overridden_workspace);
        Ok(())
    }

    #[test]
    fn apps_init_creates_authoring_draft_without_registry_mutation() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let report = apps_init(AppsInitOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })?;

        assert_eq!(report.config_path, config_path);
        assert_eq!(report.workspace, workspace);
        assert_eq!(report.authoring.app_id, AppId::parse("demo.app")?);
        assert_eq!(report.authoring.draft_id.as_str(), "draft-demo.app");
        assert!(report.authoring.draft_metadata_path.exists());
        assert!(report.authoring.scaffold_plan_path.exists());
        assert!(report.authoring.manifest_candidate_path.exists());
        assert!(report.authoring.readme_candidate_path.exists());
        assert!(!root.path().join("apps/registry.json").exists());
        assert!(!root.path().join("apps/demo.app.shacsapp").exists());
        assert!(!root.path().join("runtime").exists());

        let output = format_apps_init(report);
        assert!(output.contains("App authoring draft: demo.app"));
        assert!(output.contains("Outcome: created"));
        assert!(output.contains("Validation: static scaffold created"));
        assert!(output.contains("Next action: review candidates"));
        assert!(!output.contains("State: installed"));
        assert!(!output.contains("State: enabled"));
        assert!(!output.contains("running"));
        Ok(())
    }

    #[test]
    fn apps_init_escapes_control_chars_in_report_paths() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path().join("line\nbreak");
        let config_path = data_dir.join("config.json");
        let workspace = data_dir.join("workspace");
        fs::create_dir_all(&workspace)?;
        save_config_to_path(&Config::default(), &config_path)?;

        let report = apps_init(AppsInitOptions {
            config_path: Some(config_path.clone()),
            workspace_override: Some(workspace),
            app_id: "demo.app".to_owned(),
        })?;
        let output = format_apps_init(report.clone());
        assert!(output.contains("line\\nbreak"));
        assert!(!output.contains("line\nbreak"));

        fs::write(&report.authoring.readme_candidate_path, "changed")?;
        let error = apps_init(AppsInitOptions {
            config_path: Some(config_path),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })
        .err()
        .ok_or("expected escaped path conflict")?
        .to_string();
        assert!(error.contains("line\\nbreak"));
        assert!(!error.contains("line\nbreak"));
        Ok(())
    }

    #[test]
    fn apps_init_is_idempotent_and_conflicts_on_modified_candidates() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        save_config_to_path(&Config::default(), &config_path)?;

        let first = apps_init(AppsInitOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })?;
        let second = apps_init(AppsInitOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })?;
        assert_eq!(
            second.authoring.outcome,
            AppAuthoringInitOutcome::AlreadyExistsSameContent
        );
        assert_eq!(first.authoring.draft_path, second.authoring.draft_path);

        fs::write(&first.authoring.readme_candidate_path, "changed")?;
        let error = apps_init(AppsInitOptions {
            config_path: Some(config_path),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })
        .err()
        .ok_or("expected apps init conflict")?;
        assert!(matches!(
            error,
            CliError::AppAuthoring(AppAuthoringError::Conflict(_))
        ));
        Ok(())
    }

    #[test]
    fn apps_init_blocks_installed_app_without_registry_mutation() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let bundle = root.path().join("apps/demo.app.shacsapp");
        save_config_to_path(&Config::default(), &config_path)?;
        fs::create_dir_all(&bundle)?;
        fs::write(bundle.join("entry.md"), "# entry")?;
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "id": "demo.app",
                "version": "1.0.0",
                "entry": "entry.md"
            }))?,
        )?;
        let install = apps_install(AppsInstallOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            bundle_path: bundle,
        })?;
        let before = fs::read(&install.registry_path)?;

        let error = apps_init(AppsInitOptions {
            config_path: Some(config_path),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })
        .err()
        .ok_or("expected installed app blocker")?;
        assert!(matches!(
            error,
            CliError::AppAuthoring(AppAuthoringError::InstalledApp(app_id)) if app_id == AppId::parse("demo.app")?
        ));
        assert_eq!(fs::read(&install.registry_path)?, before);
        assert!(!root.path().join("authoring/apps/draft-demo.app").exists());
        Ok(())
    }

    #[test]
    fn apps_init_rejects_invalid_app_ids_without_raw_control_output() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        save_config_to_path(&Config::default(), &config_path)?;
        let error = apps_init(AppsInitOptions {
            config_path: Some(config_path),
            workspace_override: None,
            app_id: "bad\napp".to_owned(),
        })
        .err()
        .ok_or("expected invalid app id")?
        .to_string();
        assert!(error.contains("bad\\napp"));
        assert!(!error.contains("bad\napp"));
        Ok(())
    }

    #[test]
    fn apps_install_rejects_bundle_outside_resolved_data_dir() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let outside = root.path().join("outside");
        let bundle = outside.join("demo.app.shacsapp");

        fs::create_dir_all(&workspace)?;
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        fs::create_dir_all(&bundle)?;
        fs::write(bundle.join("entry.md"), "# entry")?;
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "id": "demo.app",
                "version": "1.0.0",
                "entry": "entry.md"
            }))?,
        )?;

        let error = apps_install(AppsInstallOptions {
            config_path: Some(config_path),
            workspace_override: None,
            bundle_path: bundle,
        })
        .err()
        .ok_or("expected invalid bundle location")?;

        assert!(matches!(
            error,
            CliError::App(AppError::InvalidBundleLocation { .. })
        ));
        Ok(())
    }

    #[test]
    fn apps_install_accepts_relative_bundle_path_inside_data_dir() -> Result<(), Box<dyn Error>> {
        let root = tempfile::Builder::new()
            .prefix("shacs-cli-apps-")
            .tempdir_in(".")?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let bundle = root.path().join("apps/demo.app.shacsapp");

        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(&bundle)?;
        let mut config = Config::default();
        let canonical_workspace = workspace.canonicalize()?;
        config.agents.defaults.workspace = canonical_workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        fs::write(bundle.join("entry.md"), "# entry")?;
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "id": "demo.app",
                "version": "1.0.0",
                "entry": "entry.md"
            }))?,
        )?;

        let report = apps_install(AppsInstallOptions {
            config_path: Some(config_path),
            workspace_override: None,
            bundle_path: bundle.clone(),
        })?;

        assert_eq!(report.entry.app_id, AppId::parse("demo.app")?);
        assert_eq!(report.entry.bundle_path, bundle.canonicalize()?);
        Ok(())
    }

    #[test]
    fn apps_uninstall_rejects_poisoned_registry_bundle_path() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let bundle = root.path().join("apps/demo.app.shacsapp");
        let outside = root.path().join("outside-delete-target");

        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        fs::create_dir_all(&bundle)?;
        fs::write(bundle.join("entry.md"), "# entry")?;
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec_pretty(&json!({
                "id": "demo.app",
                "version": "1.0.0",
                "entry": "entry.md"
            }))?,
        )?;
        fs::create_dir_all(&outside)?;
        fs::write(outside.join("keep.txt"), "keep")?;

        let install = apps_install(AppsInstallOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            bundle_path: bundle.clone(),
        })?;
        let mut registry: serde_json::Value =
            serde_json::from_slice(&fs::read(install.registry_path)?)?;
        registry["entries"]["demo.app"]["bundlePath"] = json!(outside);
        fs::write(
            root.path().join("apps/registry.json"),
            serde_json::to_vec_pretty(&registry)?,
        )?;

        let error = apps_uninstall(AppsIdOptions {
            config_path: Some(config_path),
            workspace_override: None,
            app_id: "demo.app".to_owned(),
        })
        .err()
        .ok_or("expected poisoned bundle path rejection")?;

        assert!(matches!(
            error,
            CliError::App(AppError::InvalidBundleLocation { .. })
        ));
        assert!(outside.join("keep.txt").exists());
        assert!(bundle.exists());
        Ok(())
    }

    #[test]
    fn parser_handles_runtime_lifecycle_commands() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "runtime",
            "start",
            "--workspace",
            "/tmp/workspace",
        ])?;
        let CliCommand::RuntimeStart(options) = parsed else {
            return Err("expected runtime start command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let parsed = parse_cli_args(["runtime", "stop", "--workspace", "/tmp/workspace"])?;
        let CliCommand::RuntimeStop(options) = parsed else {
            return Err("expected runtime stop command".into());
        };
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let parsed =
            parse_cli_args(["runtime", "stop", "--workspace", "/tmp/workspace", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));

        let parsed = parse_cli_args(["runtime", "restart", "--config", "/tmp/b.json", "--help"])?;
        assert!(matches!(parsed, CliCommand::Help));

        let parsed = parse_cli_args(["runtime", "restart", "--config", "/tmp/b.json"])?;
        let CliCommand::RuntimeRestart(options) = parsed else {
            return Err("expected runtime restart command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/b.json")));
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
    fn parser_handles_context_command_surface() -> Result<(), Box<dyn Error>> {
        let parsed = parse_cli_args([
            "--config",
            "/tmp/a.json",
            "context",
            "files",
            "list",
            "--workspace",
            "/tmp/workspace",
        ])?;
        let CliCommand::Context(ContextCommand::Files(ContextFilesCommand::List(options))) = parsed
        else {
            return Err("expected context files list command".into());
        };
        assert_eq!(options.config_path, Some(PathBuf::from("/tmp/a.json")));
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/workspace"))
        );

        let parsed = parse_cli_args(["context", "refs", "parse", "read @src/lib.rs"])?;
        let CliCommand::Context(ContextCommand::Refs(ContextRefsCommand::Parse(options))) = parsed
        else {
            return Err("expected context refs parse command".into());
        };
        assert_eq!(options.message, "read @src/lib.rs");

        let parsed = parse_cli_args([
            "context",
            "refs",
            "resolve",
            "--message",
            "read @src/lib.rs",
            "--network",
        ])?;
        let CliCommand::Context(ContextCommand::Refs(ContextRefsCommand::Resolve(options))) =
            parsed
        else {
            return Err("expected context refs resolve command".into());
        };
        assert_eq!(options.message, "read @src/lib.rs");
        assert!(options.network_enabled);
        Ok(())
    }

    #[test]
    fn context_refs_parse_dry_run_reports_span_kind_target_without_source_read(
    ) -> Result<(), Box<dyn Error>> {
        let report = context_refs_parse(ContextRefsParseOptions {
            message: "read @missing-file.md".to_owned(),
        })?;
        let output = format_context_refs_parse_report(report);

        assert!(output.contains("context refs parse"));
        assert!(output.contains("References: 1"));
        assert!(output.contains("kind=File"));
        assert!(output.contains("target=missing-file.md"));
        Ok(())
    }

    #[test]
    fn context_files_list_inspect_reports_status_without_content() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        fs::write(
            workspace.join("AGENTS.md"),
            "OPENAI_API_KEY=sk-context-file-secret",
        )?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let report = context_files_report(ContextFilesOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })?;
        let output = format_context_files_report("context files inspect", report);

        assert!(output.contains("Context files: 1"));
        assert!(output.contains("status=Included"));
        assert!(output.contains("digest="));
        assert!(!output.contains("sk-context-file-secret"));
        Ok(())
    }

    #[test]
    fn context_refs_resolve_dry_run_reports_permission_redaction_budget_status_without_content(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        fs::write(
            workspace.join("secret.txt"),
            "OPENAI_API_KEY=sk-context-resolve-secret visible text",
        )?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let report = context_refs_resolve(ContextRefsResolveOptions {
            config_path: Some(config_path),
            workspace_override: None,
            message: "read @secret.txt".to_owned(),
            network_enabled: false,
        })?;
        let output = format_context_refs_resolve_report(report);

        assert!(output.contains("context refs resolve"));
        assert!(output.contains("Resolved: 1"));
        assert!(output.contains("Redacted: 1"));
        assert!(output.contains("Budget bytes:"));
        assert!(!output.contains("sk-context-resolve-secret"));
        assert!(!output.contains("visible text"));
        Ok(())
    }

    #[test]
    fn context_docs_describe_reference_syntax_limits_and_safety() -> Result<(), Box<dyn Error>> {
        let usage = read_repo_text("docs/USAGE.md")?;

        for expected in [
            "shacs-bot context files list",
            "shacs-bot context refs parse",
            "@path",
            "@folder/",
            "@diff",
            "@staged",
            "@git:<rev>",
            "@url:https://...",
            "shared context budget",
            "Protected target",
            "external_untrusted",
            "redaction pass",
            "replay",
        ] {
            assert!(usage.contains(expected), "missing docs phrase: {expected}");
        }
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

        let parsed = parse_cli_args(["runtime", "update", "--target-version", VERSION])?;
        let CliCommand::RuntimeUpdate(options) = parsed else {
            return Err("expected runtime update command".into());
        };
        assert_eq!(options.target_version, VERSION);

        let parsed = parse_cli_args(["runtime", "recover", "-w", "/tmp/runtime"])?;
        let CliCommand::RuntimeRecover(options) = parsed else {
            return Err("expected runtime recover command".into());
        };
        assert_eq!(
            options.workspace_override,
            Some(PathBuf::from("/tmp/runtime"))
        );

        let error = parse_cli_args(["runtime"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("runtime requires `start`, `stop`, `restart`, `inspect`, `diagnostics`, `update`, or `recover`"));

        let error = parse_cli_args(["runtime", "update"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("runtime update requires --target-version"));

        let error = parse_cli_args(["runtime", "update", "--target-version", "bad\nversion"])
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("must contain only ASCII"));
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
    fn heartbeat_runtime_requires_enabled_non_empty_workspace_file() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        let context = config_context(Some(config_path), Some(workspace.clone()));
        let mut bundle = ConfigBundle {
            config,
            context,
            migrations: Vec::new(),
        };

        assert!(!heartbeat_runtime_enabled(&bundle));
        fs::write(workspace.join(HEARTBEAT_FILE_NAME), "\n\n")?;
        assert!(!heartbeat_runtime_enabled(&bundle));
        fs::write(
            workspace.join(HEARTBEAT_FILE_NAME),
            "## Active\n- refresh docs",
        )?;
        assert!(heartbeat_runtime_enabled(&bundle));
        bundle.config.gateway.heartbeat.enabled = false;
        assert!(!heartbeat_runtime_enabled(&bundle));
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
            json!({ "enabled": true, "appToken": "slack-app-token", "botToken": "slack-token", "channelIds": ["C123"] }),
        );
        config.channels.plugins.insert(
            "email".to_owned(),
            json!({
                "enabled": true,
                "consentGranted": true,
                "allowFrom": ["sender@example.com"],
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
    fn channel_runtime_plan_starts_original_discord_gateway_config() -> Result<(), Box<dyn Error>> {
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
            "discord".to_owned(),
            json!({
                "enabled": true,
                "token": "discord-token",
                "allowFrom": [],
                "replyInThread": false
            }),
        );
        config.channels.plugins.insert(
            "slack".to_owned(),
            json!({
                "enabled": true,
                "appToken": "slack-app-token",
                "botToken": "slack-bot-token",
                "replyInThread": true
            }),
        );
        save_config_to_path(&config, &config_path)?;

        let report = channel_runtime_plan(RunOptions {
            config_path: Some(config_path),
            ..RunOptions::default()
        })?;
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
        assert_eq!(discord.state, ChannelRuntimeWorkerState::Started);
        assert_eq!(slack.state, ChannelRuntimeWorkerState::Started);
        let formatted = format_channel_runtime_plan(report);
        assert!(formatted.contains("discord / Discord Gateway worker: started"));
        assert!(formatted.contains("slack / Slack Socket Mode worker: started"));
        Ok(())
    }

    #[test]
    fn external_runtime_parsers_accept_original_channel_aliases() -> Result<(), Box<dyn Error>> {
        let mut plugins = BTreeMap::new();
        plugins.insert(
            "discord".to_owned(),
            json!({
                "enabled": true,
                "token": "discord-token",
                "allowChannels": ["123"],
                "allowFrom": ["user-1"]
            }),
        );
        plugins.insert(
            "slack".to_owned(),
            json!({
                "enabled": true,
                "app_token": "slack-app-token",
                "botToken": "slack-token",
                "allowChannels": ["C123"],
                "allowFrom": ["U123"]
            }),
        );
        plugins.insert(
            "email".to_owned(),
            json!({
                "enabled": true,
                "consentGranted": true,
                "allowFrom": ["sender@example.com"],
                "smtpHost": "smtp.example.com",
                "smtpUsername": "smtp-user",
                "smtpPassword": "smtp-password",
                "fromAddress": "bot@example.com",
                "imapHost": "imap.example.com",
                "imapUsername": "imap-user",
                "imapPassword": "imap-password"
            }),
        );

        let discord = discord_runtime_config(&plugins).ok_or("discord runtime config missing")?;
        assert_eq!(
            discord.channel_filter,
            DiscordChannelFilter::Only(vec!["123".to_owned()])
        );
        assert_eq!(discord.allowed_senders, vec!["user-1".to_owned()]);
        assert_eq!(discord.transport, DiscordTransportMode::Gateway);
        assert!(discord_channel_allowed(
            &DiscordChannelFilter::Only(vec!["*".to_owned()]),
            "any-channel",
            None,
        ));
        assert!(discord_channel_allowed(
            &DiscordChannelFilter::Only(vec!["parent-channel".to_owned()]),
            "thread-channel",
            Some("parent-channel"),
        ));
        let slack = slack_runtime_config(&plugins).ok_or("slack runtime config missing")?;
        assert_eq!(slack.app_token, "slack-app-token");
        assert_eq!(slack.channel_ids, vec!["C123".to_owned()]);
        assert_eq!(slack.allowed_senders, vec!["U123".to_owned()]);
        let email = email_runtime_config(&plugins).ok_or("email runtime config missing")?;
        let email_smtp = email.smtp.ok_or("email smtp runtime config missing")?;
        assert_eq!(email_smtp.from, "bot@example.com");
        assert_eq!(email_smtp.username.as_deref(), Some("smtp-user"));
        assert_eq!(email_smtp.password.as_deref(), Some("smtp-password"));
        let email_imap = email.imap.ok_or("email imap runtime config missing")?;
        assert_eq!(email_imap.username, "imap-user");
        assert_eq!(email_imap.password, "imap-password");
        assert!(sender_allowed(&discord.allowed_senders, "user-1"));
        assert!(!sender_allowed(&discord.allowed_senders, "user-2"));
        assert!(!sender_allowed(&[], "user-1"));

        let mut whatsapp_plugins = BTreeMap::new();
        whatsapp_plugins.insert(
            "whatsapp".to_owned(),
            json!({ "enabled": true, "bridgeUrl": "http://127.0.0.1:9001" }),
        );
        let whatsapp =
            whatsapp_runtime_config(&whatsapp_plugins).ok_or("whatsapp runtime config missing")?;
        assert_eq!(whatsapp.bridge_url, "ws://127.0.0.1:9001");
        assert!(
            normalize_whatsapp_bridge_websocket_url("https://bridge.example/ws")
                .is_some_and(|url| url == "wss://bridge.example/ws")
        );
        assert!(normalize_whatsapp_bridge_websocket_url("ftp://bridge.example/ws").is_none());
        Ok(())
    }

    #[test]
    fn slack_socket_mode_ack_and_envelope_normalization() -> Result<(), Box<dyn Error>> {
        let config = SlackRuntimeConfig {
            app_token: "xapp-token".to_owned(),
            bot_token: "xoxb-token".to_owned(),
            channel_ids: vec!["C123".to_owned()],
            allowed_senders: vec!["U123".to_owned()],
        };
        let envelope = json!({
            "envelope_id": "env-1",
            "payload": {
                "event": {
                    "type": "app_mention",
                    "user": "U123",
                    "channel": "C123",
                    "text": "<@BOT> hello",
                    "ts": "1710000000.000100",
                    "thread_ts": "1710000000.000001",
                    "channel_type": "channel"
                }
            },
            "authorizations": [{"user_id": "UBOT", "is_bot": true}]
        });

        assert_eq!(
            slack_socket_ack_frame(&envelope),
            Some(json!({"envelope_id": "env-1"}))
        );
        let inbound = slack_socket_envelope_to_inbound(&config, &envelope)
            .ok_or("slack socket envelope was not normalized")?;
        assert_eq!(inbound.channel, SLACK_CHANNEL);
        assert_eq!(inbound.sender_id, "U123");
        assert_eq!(inbound.chat_id, "C123");
        assert_eq!(inbound.content, "<@BOT> hello");
        assert_eq!(
            inbound.session_key_override.as_deref(),
            Some("slack:C123:1710000000.000001")
        );

        let subtype = json!({
            "payload": {"event": {"type": "message", "subtype": "bot_message", "user": "U123", "channel": "C123", "text": "bot"}}
        });
        assert!(slack_socket_envelope_to_inbound(&config, &subtype).is_none());

        let self_message = json!({
            "payload": {"event": {"type": "message", "user": "UBOT", "channel": "C123", "text": "self"}},
            "authorizations": [{"user_id": "UBOT", "is_bot": true}]
        });
        assert!(slack_socket_envelope_to_inbound(&config, &self_message).is_none());

        let blocked_channel = json!({
            "payload": {"event": {"type": "message", "user": "U123", "channel": "C999", "text": "blocked"}}
        });
        assert!(slack_socket_envelope_to_inbound(&config, &blocked_channel).is_none());
        Ok(())
    }

    #[test]
    fn slack_socket_envelope_to_inbound_extracts_event_files_metadata() -> Result<(), Box<dyn Error>>
    {
        let config = SlackRuntimeConfig {
            app_token: "xapp-token".to_owned(),
            bot_token: "xoxb-token".to_owned(),
            channel_ids: vec!["C123".to_owned()],
            allowed_senders: vec!["U123".to_owned()],
        };
        let envelope = json!({
            "payload": {
                "event": {
                    "type": "message",
                    "user": "U123",
                    "channel": "C123",
                    "text": "see attached https://ignored.example/raw.png",
                    "ts": "1710000000.000100",
                    "files": [{
                        "url_private_download": "https://files.slack.com/files-pri/T1-F1/report.png?pub_secret=secret",
                        "mimetype": "image/png",
                        "name": "report.png",
                        "size": 1234
                    }]
                }
            }
        });

        let inbound = slack_socket_envelope_to_inbound(&config, &envelope)
            .ok_or("slack file event was not normalized")?;

        assert_eq!(inbound.media.len(), 1);
        assert!(inbound.media[0].starts_with("shacs-slack-file:"));
        assert!(!inbound.media[0].contains("pub_secret"));
        assert!(!inbound.media[0].contains("files.slack.com"));
        Ok(())
    }

    #[test]
    fn slack_socket_envelope_preserves_file_share_without_text() -> Result<(), Box<dyn Error>> {
        let config = SlackRuntimeConfig {
            app_token: "xapp-token".to_owned(),
            bot_token: "xoxb-token".to_owned(),
            channel_ids: vec!["C123".to_owned()],
            allowed_senders: vec!["U123".to_owned()],
        };
        let envelope = json!({
            "payload": {
                "event": {
                    "type": "message",
                    "subtype": "file_share",
                    "user": "U123",
                    "channel": "C123",
                    "text": "",
                    "ts": "1710000000.000100",
                    "files": [{
                        "url_private_download": "https://files.slack.com/files-pri/T1-F1/report.png?pub_secret=secret",
                        "mimetype": "image/png",
                        "name": "report.png",
                        "size": 1234
                    }]
                }
            }
        });

        let inbound = slack_socket_envelope_to_inbound(&config, &envelope)
            .ok_or("slack file-only event was not normalized")?;

        assert_eq!(inbound.content, "");
        assert_eq!(inbound.media.len(), 1);
        assert!(inbound.media[0].starts_with("shacs-slack-file:"));
        Ok(())
    }

    #[test]
    fn whatsapp_websocket_helper_serializes_and_normalizes_bridge_values(
    ) -> Result<(), Box<dyn Error>> {
        assert_eq!(
            whatsapp_frame_text(&WhatsAppOutboundFrame::Send {
                to: "15551234567".to_owned(),
                text: "hello".to_owned(),
            })?,
            r#"{"type":"send","to":"15551234567","text":"hello"}"#
        );

        let config = WhatsAppChannelConfig {
            bridge_url: "ws://127.0.0.1:9001".to_owned(),
            bridge_token: Some("bridge-token".to_owned()),
            allowlist: shacs_channels::ChannelAllowlist::allow_all(),
            group_policy: WhatsAppGroupPolicy::Open,
        };
        let value = json!({
            "type": "message",
            "pn": "15551234567@s.whatsapp.net",
            "sender": "15551234567@s.whatsapp.net",
            "content": "hello from bridge",
            "id": "wa-1",
            "isGroup": false,
            "wasMentioned": false,
            "media": [],
            "timestamp": "1710000000"
        });
        let inbound =
            normalize_whatsapp_bridge_value(&value, &config, &mut RecentMessageIds::new(16))?
                .ok_or("whatsapp bridge value did not normalize")?;
        assert_eq!(inbound.channel, WHATSAPP_CHANNEL);
        assert_eq!(inbound.sender_id, "15551234567");
        assert_eq!(inbound.content, "hello from bridge");
        assert_eq!(whatsapp_message_items(&value), vec![&value]);
        assert_eq!(whatsapp_message_items(&json!([value.clone()])).len(), 1);
        assert_eq!(
            whatsapp_message_items(&json!({ "messages": [value] })).len(),
            1
        );
        Ok(())
    }

    #[test]
    fn discord_gateway_message_filter_matches_original_semantics() -> Result<(), Box<dyn Error>> {
        let config = DiscordRuntimeConfig {
            token: "token".to_owned(),
            channel_filter: DiscordChannelFilter::AllVisible,
            allowed_senders: vec!["user-1".to_owned()],
            group_policy: DiscordGroupPolicy::Mention,
            streaming: true,
            poll_interval_seconds: 5,
            transport: DiscordTransportMode::Gateway,
        };
        let mut recent = RecentMessageIds::new(16);
        let event = json!({
            "id": "m1",
            "channel_id": "c1",
            "guild_id": "g1",
            "content": "<@bot-1> hello",
            "author": {"id": "user-1", "bot": false},
            "mentions": [{"id": "bot-1"}]
        });

        let inbound =
            discord_gateway_message_to_inbound(&config, Some("bot-1"), &mut recent, &event)
                .ok_or("accepted mentioned guild message missing")?;
        assert_eq!(inbound.content, "hello");
        assert_eq!(inbound.chat_id, "c1");
        assert!(
            discord_gateway_message_to_inbound(&config, Some("bot-1"), &mut recent, &event)
                .is_none()
        );

        let unmentioned = json!({
            "id": "m2",
            "channel_id": "c1",
            "guild_id": "g1",
            "content": "hello",
            "author": {"id": "user-1", "bot": false},
            "mentions": []
        });
        assert!(discord_gateway_message_to_inbound(
            &config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &unmentioned,
        )
        .is_none());

        let everyone = json!({
            "id": "m4",
            "channel_id": "c1",
            "guild_id": "g1",
            "content": "@everyone hello",
            "author": {"id": "user-1", "bot": false},
            "mention_everyone": true,
            "mentions": []
        });
        assert!(discord_gateway_message_to_inbound(
            &config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &everyone,
        )
        .is_none());

        let thread_config = DiscordRuntimeConfig {
            token: "token".to_owned(),
            channel_filter: DiscordChannelFilter::Only(vec!["parent-1".to_owned()]),
            allowed_senders: vec!["user-1".to_owned()],
            group_policy: DiscordGroupPolicy::Open,
            streaming: true,
            poll_interval_seconds: 5,
            transport: DiscordTransportMode::Gateway,
        };
        let thread_message = json!({
            "id": "m5",
            "channel_id": "thread-1",
            "parent_id": "parent-1",
            "guild_id": "g1",
            "content": "hello thread",
            "author": {"id": "user-1", "bot": false},
            "mentions": []
        });
        assert!(discord_gateway_message_to_inbound(
            &thread_config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &thread_message,
        )
        .is_some());

        let dm = json!({
            "id": "m3",
            "channel_id": "dm1",
            "content": "hello dm",
            "author": {"id": "user-1", "bot": false},
            "mentions": []
        });
        assert!(discord_gateway_message_to_inbound(
            &config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &dm,
        )
        .is_some());

        let open_sender_config = DiscordRuntimeConfig {
            allowed_senders: Vec::new(),
            ..config.clone()
        };
        let open_sender_message = json!({
            "id": "m6",
            "channel_id": "dm1",
            "content": "hello from new sender",
            "author": {"id": "user-2", "bot": false},
            "mentions": []
        });
        assert!(discord_gateway_message_to_inbound(
            &open_sender_config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &open_sender_message,
        )
        .is_some());

        let restricted_sender_config = DiscordRuntimeConfig {
            allowed_senders: vec!["user-1".to_owned()],
            ..open_sender_config
        };
        let restricted_sender_message = json!({
            "id": "m7",
            "channel_id": "dm1",
            "content": "blocked sender",
            "author": {"id": "user-2", "bot": false},
            "mentions": []
        });
        assert!(discord_gateway_message_to_inbound(
            &restricted_sender_config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &restricted_sender_message,
        )
        .is_none());

        let attachment_only_config = DiscordRuntimeConfig {
            group_policy: DiscordGroupPolicy::Open,
            ..restricted_sender_config
        };
        let attachment_only = json!({
            "id": "m8",
            "channel_id": "dm1",
            "content": "",
            "author": {"id": "user-1", "bot": false},
            "mentions": [],
            "attachments": [{
                "url": "https://cdn.discordapp.com/attachments/c1/a1/photo.png?ex=signed",
                "content_type": "image/png",
                "filename": "photo.png",
                "size": 42
            }]
        });
        let inbound = discord_gateway_message_to_inbound(
            &attachment_only_config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &attachment_only,
        )
        .ok_or("discord attachment-only message was not normalized")?;
        assert_eq!(inbound.content, "");
        assert_eq!(inbound.media.len(), 1);
        Ok(())
    }

    #[test]
    fn discord_gateway_and_rest_payload_attachment_extraction_preserves_metadata(
    ) -> Result<(), Box<dyn Error>> {
        let config = DiscordRuntimeConfig {
            token: "token".to_owned(),
            channel_filter: DiscordChannelFilter::AllVisible,
            allowed_senders: vec!["user-1".to_owned()],
            group_policy: DiscordGroupPolicy::Open,
            streaming: true,
            poll_interval_seconds: 5,
            transport: DiscordTransportMode::Gateway,
        };
        let event = json!({
            "id": "m-attach-1",
            "channel_id": "c1",
            "content": "attached https://ignored.example/text-url.png",
            "author": {"id": "user-1", "bot": false},
            "mentions": [],
            "attachments": [{
                "url": "https://cdn.discordapp.com/attachments/c1/a1/photo.png?ex=signed",
                "content_type": "image/png",
                "filename": "photo.png",
                "size": 42
            }]
        });

        let inbound = discord_gateway_message_to_inbound(
            &config,
            Some("bot-1"),
            &mut RecentMessageIds::new(16),
            &event,
        )
        .ok_or("discord gateway message was not normalized")?;
        assert_eq!(inbound.media.len(), 1);
        assert!(inbound.media[0].starts_with("shacs-discord-attachment:"));
        assert!(!inbound.media[0].contains("cdn.discordapp.com"));
        assert!(!inbound.media[0].contains("ex=signed"));

        let rest_media = discord_attachment_media(&event);
        assert_eq!(rest_media, inbound.media);

        let rest_attachment_only = json!({
            "id": "m-rest-attach-only",
            "channel_id": "c1",
            "content": "",
            "author": {"id": "user-1", "bot": false},
            "attachments": [{
                "url": "https://unlisted.example/attachment.png?secret=token",
                "content_type": "image/png",
                "filename": "rest-photo.png",
                "size": 7
            }]
        });
        let agent = runtime_http_agent(Duration::from_secs(1));
        let inbound = discord_rest_message_to_inbound(&agent, &config, "c1", &rest_attachment_only)
            .ok_or("discord REST attachment-only message was not normalized")?;
        assert_eq!(inbound.content, "");
        assert_eq!(inbound.media.len(), 1);
        assert!(inbound.media[0].starts_with("shacs-discord-attachment:"));
        assert!(!inbound.media[0].contains("secret=token"));
        Ok(())
    }

    #[test]
    fn platform_attachment_url_allowlist_rejects_userinfo_and_unlisted_hosts() {
        assert!(platform_attachment_url_allowed(
            "https://cdn.discordapp.com/attachments/c1/a1/photo.png?ex=signed",
            &["cdn.discordapp.com", "media.discordapp.net"],
        ));
        assert!(!platform_attachment_url_allowed(
            "http://cdn.discordapp.com/attachments/c1/a1/photo.png",
            &["cdn.discordapp.com"],
        ));
        assert!(!platform_attachment_url_allowed(
            "https://cdn.discordapp.com@evil.example/attachments/c1/a1/photo.png",
            &["cdn.discordapp.com"],
        ));
        assert!(!platform_attachment_url_allowed(
            "https://evil.example/attachments/c1/a1/photo.png",
            &["cdn.discordapp.com"],
        ));
    }

    #[test]
    fn channels_report_recognizes_send_memory_hints() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        fs::write(
            &config_path,
            json!({
                "agents": {"defaults": {"workspace": workspace}},
                "channels": {
                    "sendMemoryHints": true,
                    "discord": {"enabled": false}
                }
            })
            .to_string(),
        )?;

        let report = load_channels_report(Some(config_path), None)?;
        assert!(report.send_memory_hints);
        assert!(!report
            .unknown_plugins
            .iter()
            .any(|plugin| plugin == "sendMemoryHints"));
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
            json!({ "enabled": true, "appToken": "slack-app-token", "botToken": "slack-token", "channelIds": ["C123"] }),
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
    fn external_outbound_channel_manager_dispatches_via_channel_adapters(
    ) -> Result<(), Box<dyn Error>> {
        let (observed_tx, observed_rx) = mpsc::channel();
        let inbound_bus = MessageBus::new();
        let mut manager = external_transport_channel_manager_with_runner(
            vec![test_telegram_spec(), test_slack_spec()],
            inbound_bus.clone(),
            0,
            test_transport_context()?,
            recording_transport_runner(observed_tx),
        );
        manager.start_all()?;
        manager.dispatch_outbound(OutboundMessage::new(TELEGRAM_CHANNEL, "chat-1", "hello"))?;

        let delivered = observed_rx.recv_timeout(Duration::from_millis(50))?;
        assert_eq!(delivered.channel, TELEGRAM_CHANNEL);
        assert_eq!(delivered.chat_id, "chat-1");
        assert_eq!(delivered.content, "hello");
        assert!(observed_rx.try_recv().is_err());
        manager.stop_all()?;
        manager.start_all()?;
        manager.dispatch_outbound(OutboundMessage::new(SLACK_CHANNEL, "chat-2", "again"))?;
        let restarted = observed_rx.recv_timeout(Duration::from_millis(50))?;
        assert_eq!(restarted.channel, SLACK_CHANNEL);
        assert_eq!(restarted.chat_id, "chat-2");
        assert_eq!(restarted.content, "again");
        manager.stop_all()?;
        assert!(inbound_bus.try_consume_inbound().is_none());
        Ok(())
    }

    #[test]
    fn external_outbound_channel_manager_preserves_streaming_frames() -> Result<(), Box<dyn Error>>
    {
        let (observed_tx, observed_rx) = mpsc::channel();
        let inbound_bus = MessageBus::new();
        let mut manager = external_transport_channel_manager_with_runner(
            vec![test_telegram_spec()],
            inbound_bus,
            0,
            test_transport_context()?,
            recording_transport_runner(observed_tx),
        );
        manager.start_all()?;

        let mut metadata = Map::new();
        metadata.insert("_stream_delta".to_owned(), json!(true));
        metadata.insert("_stream_id".to_owned(), json!("stream-1"));
        manager.dispatch_outbound(
            OutboundMessage::new(TELEGRAM_CHANNEL, "chat-1", "chunk")
                .with_reply_to("message-1")
                .with_metadata(metadata),
        )?;

        let delivered = observed_rx.recv_timeout(Duration::from_millis(50))?;
        assert_eq!(delivered.channel, TELEGRAM_CHANNEL);
        assert_eq!(delivered.chat_id, "chat-1");
        assert_eq!(delivered.content, "chunk");
        assert_eq!(delivered.reply_to.as_deref(), Some("message-1"));
        assert_eq!(delivered.metadata["reply_to"], json!("message-1"));
        assert_eq!(delivered.metadata["stream_id"], json!("stream-1"));
        manager.stop_all()?;
        Ok(())
    }

    #[test]
    fn external_outbound_channel_manager_reports_unknown_channels() -> Result<(), Box<dyn Error>> {
        let inbound_bus = MessageBus::new();
        let mut manager = external_transport_channel_manager_with_runner(
            Vec::new(),
            inbound_bus,
            0,
            test_transport_context()?,
            recording_transport_runner(mpsc::channel().0),
        );
        let error = match manager.dispatch_outbound(OutboundMessage::new(
            TELEGRAM_CHANNEL,
            "chat-1",
            "hello",
        )) {
            Ok(()) => return Err("expected unknown channel error".into()),
            Err(error) => error,
        };
        assert!(
            matches!(error, ChannelError::UnknownChannel(channel) if channel == TELEGRAM_CHANNEL)
        );
        Ok(())
    }

    #[test]
    fn channel_retry_policy_treats_send_max_retries_as_total_attempts() {
        assert_eq!(channel_retry_policy(0).max_attempts, 1);
        assert_eq!(channel_retry_policy(2).max_attempts, 2);
        assert_eq!(channel_retry_policy(11).max_attempts, 10);
    }

    #[test]
    fn external_session_turn_coordinator_queues_same_session_followups() {
        let mut coordinator = ExternalSessionTurnCoordinator::default();
        let first = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "one");
        let followup = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "two");
        let other = InboundMessage::new(TELEGRAM_CHANNEL, "u2", "chat-2", "other");

        assert_eq!(
            coordinator
                .start_or_enqueue("telegram:chat-1".to_owned(), first)
                .map(|message| message.content),
            Some("one".to_owned())
        );
        assert!(coordinator
            .start_or_enqueue("telegram:chat-1".to_owned(), followup)
            .is_none());
        assert_eq!(
            coordinator
                .start_or_enqueue("telegram:chat-2".to_owned(), other)
                .map(|message| message.content),
            Some("other".to_owned())
        );
        assert_eq!(
            coordinator
                .finish_turn("telegram:chat-1")
                .map(|message| message.content),
            Some("two".to_owned())
        );
        assert!(coordinator.finish_turn("telegram:chat-1").is_none());
    }

    #[test]
    fn external_session_turn_coordinator_defers_shared_lock_conflicts() {
        let mut coordinator = ExternalSessionTurnCoordinator::default();
        let first = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "one");
        let retry = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "retry");

        assert!(coordinator
            .start_or_enqueue("telegram:chat-1".to_owned(), first)
            .is_some());
        coordinator.defer_turn("telegram:chat-1".to_owned(), retry);
        assert!(coordinator
            .start_next_ready(|session_key| session_key == "telegram:chat-1")
            .is_none());
        assert_eq!(
            coordinator
                .start_next_ready(|_| false)
                .map(|(_, message)| message.content),
            Some("retry".to_owned())
        );
    }

    #[test]
    fn adapter_reports_duplicate_session_turn_as_retryable_busy_error() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let turn_lock = SessionTurnLock::new();
        let _guard = turn_lock
            .acquire("telegram:chat-1")
            .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: false,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: turn_lock,
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: Some(ContainmentSnapshotRef {
                contained: Some(true),
                digest: Some("containment-digest".to_owned()),
                summary: Some("workspace containment".to_owned()),
            }),
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: shacs_config::PermissionMode::AcceptEdits,
                source: Some("test-source".to_owned()),
                scope_ref: Some("scope:test".to_owned()),
            },
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let error = adapter
            .process_external_inbound_with_streaming(
                InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "hello"),
                adapter.loop_config(),
                &MessageBus::new(),
            )
            .err()
            .ok_or("expected session busy error")?;
        assert_eq!(error.status, 409);
        assert_eq!(error.error_type, "session_busy");
        Ok(())
    }

    #[test]
    fn external_inline_data_url_is_stored_under_channel_attachments() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_root = root.path().join("data").join("media");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = external_media_test_adapter(root.path(), captured.clone())?;
        let inbound = InboundMessage::new(DISCORD_CHANNEL, "u1", "chat-1", "hello").with_media([
            "data:text/plain;base64,aGk=".to_owned(),
            "data:audio/mpeg;base64,SUQz".to_owned(),
        ]);

        adapter.process_external_inbound_with_streaming(
            inbound,
            adapter.loop_config(),
            &MessageBus::new(),
        )?;

        let attachment_dir = media_root.join("attachments").join(DISCORD_CHANNEL);
        let stored_files = fs::read_dir(&attachment_dir)?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(stored_files.len(), 2);
        let stored_contents = stored_files
            .iter()
            .map(|entry| fs::read(entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        assert!(stored_contents.iter().any(|content| content == b"hi"));
        assert!(stored_contents.iter().any(|content| content == b"ID3"));
        let attachment_dir = attachment_dir.canonicalize()?;
        let session = SessionManager::new(&workspace)?
            .load_existing("discord:chat-1")
            .ok_or("discord session missing")?;
        let user_message = session
            .messages
            .iter()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .ok_or("user message missing")?;
        let media = user_message["media"]
            .as_array()
            .ok_or("stored media missing")?;
        assert_eq!(media.len(), 2);
        assert!(media.iter().all(|path| path
            .as_str()
            .is_some_and(|path| Path::new(path).starts_with(&attachment_dir))));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        let request_json = serde_json::to_string(&requests[0].messages)?;
        assert!(!request_json.contains("data:text/plain"));
        assert!(!request_json.contains("data:audio/mpeg"));
        assert!(request_json.contains("[attachment:included_text]"));
        assert!(request_json.contains("[attachment:unsupported]"));
        assert!(request_json.contains("audio analyzer is not configured"));
        assert!(request_json.contains("hi"));
        for stored_file in &stored_files {
            assert!(!request_json.contains(&stored_file.path().to_string_lossy().to_string()));
        }
        Ok(())
    }

    #[test]
    fn adapter_context_builder_routes_stored_images_as_notes_when_native_image_input_is_unsupported(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let media_root = root.path().join("data").join("media");
        let attachments = media_root.join("attachments").join("api");
        fs::create_dir_all(&attachments)?;
        let image = attachments.join("stored.png");
        fs::write(&image, b"\x89PNG\r\n\x1a\nrest")?;
        let mut adapter =
            external_media_test_adapter(root.path(), Arc::new(Mutex::new(Vec::new())))?;
        adapter.native_image_input_supported = false;
        let media = vec![image.to_string_lossy().to_string()];

        let messages =
            adapter
                .context_builder()
                .build_messages(shacs_core::runtime::ContextBuildRequest {
                    history: Vec::new(),
                    current_message: "describe image",
                    media: &media,
                    channel: Some("api"),
                    chat_id: Some("default"),
                    current_role: "user",
                    session_summary: None,
                });
        let request_json = serde_json::to_string(&messages)?;

        assert!(request_json.contains("[attachment:unsupported]"));
        assert!(request_json.contains("native image input is not supported by provider/model"));
        assert!(!request_json.contains("image_url"));
        assert!(!request_json.contains("data:image/png"));
        Ok(())
    }

    #[test]
    fn external_non_data_media_records_safe_projection_failure() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = external_media_test_adapter(root.path(), captured.clone())?;
        let signed_url = "https://files.example.test/private/report.png?token=secret";
        let local_path = "/Users/alice/private/photo.jpg";
        let inbound = InboundMessage::new(SLACK_CHANNEL, "u1", "chat-1", "hello")
            .with_media([signed_url.to_owned(), local_path.to_owned()]);

        adapter.process_external_inbound_with_streaming(
            inbound,
            adapter.loop_config(),
            &MessageBus::new(),
        )?;

        let session = SessionManager::new(&workspace)?
            .load_existing("slack:chat-1")
            .ok_or("slack session missing")?;
        let user_message = session
            .messages
            .iter()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .ok_or("user message missing")?;
        assert!(user_message.get("media").is_none());
        let projections = user_message["metadata"]["external_attachment_projections"]
            .as_array()
            .ok_or("projection metadata missing")?;
        assert_eq!(projections.len(), 2);
        assert_eq!(projections[0]["source_kind"], json!("platform_download"));
        assert_eq!(
            projections[0]["reason"],
            json!("unsupported_external_media")
        );
        assert_eq!(projections[0]["display_name"], json!("report.png"));
        assert_eq!(projections[1]["source_kind"], json!("local_multipart"));
        assert_eq!(projections[1]["display_name"], json!("photo.jpg"));
        let safe_session = serde_json::to_string(&session.messages)?;
        assert!(!safe_session.contains("token=secret"));
        assert!(!safe_session.contains("/Users/alice"));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        let request_text = serde_json::to_string(&requests[0].messages)?;
        assert!(!request_text.contains(signed_url));
        assert!(!request_text.contains(local_path));
        Ok(())
    }

    #[test]
    fn external_platform_handles_record_safe_projection_metadata() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = external_media_test_adapter(root.path(), captured.clone())?;
        let raw_url = "https://cdn.discordapp.com/attachments/c1/a1/photo.png?token=secret";
        let media = platform_media_handle(
            "discord",
            "attachment",
            Some(raw_url),
            Some("image/png"),
            Some("photo.png"),
            Some(42),
        );
        let inbound =
            InboundMessage::new(DISCORD_CHANNEL, "u1", "chat-1", "hello").with_media([media]);

        adapter.process_external_inbound_with_streaming(
            inbound,
            adapter.loop_config(),
            &MessageBus::new(),
        )?;

        let session = SessionManager::new(&workspace)?
            .load_existing("discord:chat-1")
            .ok_or("discord session missing")?;
        let serialized = serde_json::to_string(&session.messages)?;
        assert!(!serialized.contains(raw_url));
        assert!(!serialized.contains("token=secret"));
        assert!(serialized.contains("photo.png"));
        assert!(serialized.contains("image/png"));
        assert!(serialized.contains("declared_byte_length"));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert!(!serde_json::to_string(&requests[0].messages)?.contains(raw_url));
        Ok(())
    }

    #[test]
    fn external_malformed_data_url_records_failure_without_raw_media() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = external_media_test_adapter(root.path(), captured.clone())?;
        let inbound = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "hello")
            .with_media(["data:text/plain;base64,%%%".to_owned()]);

        adapter.process_external_inbound_with_streaming(
            inbound,
            adapter.loop_config(),
            &MessageBus::new(),
        )?;

        let session = SessionManager::new(&workspace)?
            .load_existing("telegram:chat-1")
            .ok_or("telegram session missing")?;
        let user_message = session
            .messages
            .iter()
            .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
            .ok_or("user message missing")?;
        assert!(user_message.get("media").is_none());
        let projections = user_message["metadata"]["external_attachment_projections"]
            .as_array()
            .ok_or("projection metadata missing")?;
        assert_eq!(projections[0]["source_kind"], json!("data_url"));
        assert_eq!(projections[0]["reason"], json!("malformed_data_url"));
        let serialized = serde_json::to_string(&session.messages)?;
        assert!(!serialized.contains("%%%"));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert!(!serde_json::to_string(&requests[0].messages)?.contains("%%%"));
        Ok(())
    }

    #[test]
    fn external_text_only_message_is_unchanged_by_media_normalization() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join("workspace"))?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let adapter = external_media_test_adapter(root.path(), captured)?;
        let mut metadata = Map::new();
        metadata.insert("message_id".to_owned(), json!("m1"));
        let inbound = InboundMessage::new(EMAIL_CHANNEL, "u1", "chat-1", "hello")
            .with_metadata(metadata)
            .with_session_key_override("email:custom");

        let normalized =
            adapter.normalize_external_inbound_media(inbound.clone(), &adapter.loop_config())?;

        assert_eq!(normalized, inbound);
        Ok(())
    }

    #[test]
    fn external_effective_session_key_honors_unified_session() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let mut config = AgentLoopConfig::new(root.path(), "model");
        config.unified_session_key = Some("unified:default".to_owned());
        let message = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "hello");
        assert_eq!(
            effective_external_session_key(&config, &message),
            "unified:default"
        );

        let override_message = InboundMessage::new(TELEGRAM_CHANNEL, "u1", "chat-1", "hello")
            .with_session_key_override("telegram:chat-1");
        assert_eq!(
            effective_external_session_key(&config, &override_message),
            "telegram:chat-1"
        );
        Ok(())
    }

    #[test]
    fn worker_metadata_updates_preserve_delivery_history() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("telegram.json");
        let message = OutboundMessage::new(TELEGRAM_CHANNEL, "chat", "hello");

        record_delivery_state(&path, &message, "pending", None);
        save_telegram_offset(&path, 42);

        let metadata = load_metadata_json(&path);
        assert_eq!(metadata["offset"], json!(42));
        assert!(metadata
            .get("deliveries")
            .and_then(Value::as_array)
            .is_some_and(|deliveries| !deliveries.is_empty()));
        Ok(())
    }

    #[test]
    fn typing_indicator_outbound_is_not_recorded_as_delivery_metadata() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let path = root.path().join("discord.json");
        let mut metadata = Map::new();
        metadata.insert(EXTERNAL_TYPING_INDICATOR_KEY.to_owned(), Value::Bool(true));
        let typing = OutboundMessage::new(DISCORD_CHANNEL, "channel-1", "").with_metadata(metadata);
        let (tx, rx) = mpsc::channel();
        tx.send(typing)?;
        drop(tx);
        let stop = Arc::new(AtomicBool::new(false));
        let mut sent = Vec::new();

        drain_outbound(&rx, 1, &stop, Some(&path), |message| {
            sent.push(message);
            Ok(())
        });

        assert_eq!(sent.len(), 1);
        assert!(message_is_typing_indicator(&sent[0]));
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn platform_outbound_helpers_preserve_reply_context() {
        let mut telegram_metadata = Map::new();
        telegram_metadata.insert("message_thread_id".to_owned(), json!("topic-1"));
        let telegram = OutboundMessage::new(TELEGRAM_CHANNEL, "chat", "hello")
            .with_reply_to("11")
            .with_metadata(telegram_metadata);
        let telegram_body = telegram_message_body(&telegram, "hello", None, true);
        assert_eq!(telegram_body["message_thread_id"], json!("topic-1"));
        assert_eq!(telegram_body["reply_parameters"]["message_id"], json!("11"));

        let mut slack_metadata = Map::new();
        slack_metadata.insert("slack".to_owned(), json!({ "thread_ts": "171.1" }));
        let slack =
            OutboundMessage::new(SLACK_CHANNEL, "C123", "hello").with_metadata(slack_metadata);
        assert_eq!(slack_outbound_thread_ts(&slack).as_deref(), Some("171.1"));
        assert_eq!(
            slack_post_message_body("C123", "hello", slack_outbound_thread_ts(&slack).as_deref())
                ["thread_ts"],
            json!("171.1")
        );

        let mut email_metadata = Map::new();
        email_metadata.insert("subject".to_owned(), json!("Question"));
        let email = OutboundMessage::new(EMAIL_CHANNEL, "user@example.com", "hello")
            .with_metadata(email_metadata);
        assert_eq!(email_outbound_subject(&email), "Re: Question");
    }

    #[test]
    fn stream_outbound_routing_metadata_distinguishes_threads() {
        let mut routing_a = Map::new();
        routing_a.insert("slack".to_owned(), json!({ "thread_ts": "171.1" }));
        let mut routing_b = Map::new();
        routing_b.insert("slack".to_owned(), json!({ "thread_ts": "172.1" }));
        let first = stream_outbound_message_with_routing(
            SLACK_CHANNEL,
            "C123",
            "stream-a",
            "chunk".to_owned(),
            false,
            &routing_a,
            Some("m1"),
        );
        let second = stream_outbound_message_with_routing(
            SLACK_CHANNEL,
            "C123",
            "stream-b",
            "chunk".to_owned(),
            false,
            &routing_b,
            Some("m2"),
        );
        assert_ne!(outbound_route_key(&first), outbound_route_key(&second));
        assert_eq!(first.metadata["slack"]["thread_ts"], json!("171.1"));
        assert_eq!(first.reply_to.as_deref(), Some("m1"));
    }

    #[test]
    fn worker_metadata_store_roundtrips_external_worker_state() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let telegram_path = root.path().join("telegram.json");
        save_telegram_offset(&telegram_path, 42);
        assert_eq!(load_telegram_offset(&telegram_path), 42);

        let discord_path = root.path().join("discord-rest.json");
        let mut last_ids = BTreeMap::new();
        last_ids.insert("channel-1".to_owned(), "message-9".to_owned());
        save_discord_last_ids(&discord_path, &last_ids);
        assert_eq!(load_discord_last_ids(&discord_path), last_ids);

        let email_path = root.path().join("email-imap.json");
        let key = "imap.example.com:993:user:INBOX";
        let order = VecDeque::from(["1".to_owned(), "2".to_owned()]);
        let state = EmailSeenUidState {
            uid_validity: Some(123),
            seen_uids: order.iter().cloned().collect(),
            seen_uid_order: order.clone(),
        };
        save_email_seen_uid_state(&email_path, key, &state);
        let loaded = load_email_seen_uid_state(&email_path, key);
        assert_eq!(loaded.uid_validity, Some(123));
        assert!(loaded.seen_uids.contains("1"));
        assert_eq!(loaded.seen_uid_order, order);
        Ok(())
    }

    #[test]
    fn email_uid_validity_change_clears_seen_uid_cache() {
        let mut state = EmailSeenUidState {
            uid_validity: Some(1),
            seen_uids: BTreeSet::from(["10".to_owned()]),
            seen_uid_order: VecDeque::from(["10".to_owned()]),
        };
        apply_email_uid_validity(&mut state, Some(2));
        assert_eq!(state.uid_validity, Some(2));
        assert!(state.seen_uids.is_empty());
        assert!(state.seen_uid_order.is_empty());

        remember_seen_email_uid(
            &mut state.seen_uids,
            &mut state.seen_uid_order,
            "11".to_owned(),
        );
        apply_email_uid_validity(&mut state, Some(2));
        assert!(state.seen_uids.contains("11"));
    }

    #[test]
    fn email_runtime_requires_consent_and_allow_from_for_imap() -> Result<(), Box<dyn Error>> {
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
        assert!(email_runtime_config(&plugins).is_none());

        let imap_descriptor = builtin_live_worker_descriptors()
            .into_iter()
            .find(|descriptor| descriptor.kind == LiveChannelWorkerKind::EmailImap)
            .ok_or("missing email imap descriptor")?;
        assert!(matches!(
            worker_config_state(&plugins, &imap_descriptor),
            WorkerConfigState::Unsupported(detail) if detail.contains("consentGranted")
        ));

        plugins.insert(
            "email".to_owned(),
            json!({
                "enabled": true,
                "consentGranted": true,
                "imap": {
                    "host": "imap.example.com",
                    "username": "bot@example.com",
                    "password": "email-password"
                }
            }),
        );
        assert!(matches!(
            worker_config_state(&plugins, &imap_descriptor),
            WorkerConfigState::Unsupported(detail) if detail.contains("allowFrom")
        ));
        assert!(email_runtime_config(&plugins).is_some_and(|config| config.imap.is_none()));

        plugins.insert(
            "email".to_owned(),
            json!({
                "enabled": true,
                "consentGranted": true,
                "allowFrom": ["sender@example.com"],
                "imap": {
                    "host": "imap.example.com",
                    "username": "bot@example.com",
                    "password": "email-password",
                    "security": "starttls"
                }
            }),
        );
        assert!(matches!(
            worker_config_state(&plugins, &imap_descriptor),
            WorkerConfigState::Unsupported(detail) if detail.contains("TLS")
        ));
        Ok(())
    }

    #[test]
    fn telegram_update_to_inbound_extracts_photo_document_audio_video_metadata(
    ) -> Result<(), Box<dyn Error>> {
        let update = json!({
            "message": {
                "message_id": 10,
                "chat": {"id": 20},
                "from": {"id": 30, "username": "alice"},
                "caption": "attachments",
                "photo": [
                    {"file_id": "photo-small", "file_unique_id": "p-small", "file_size": 10},
                    {"file_id": "photo-large/opaque", "file_unique_id": "p-large", "file_size": 100}
                ],
                "document": {"file_id": "doc-id", "file_name": "doc.pdf", "mime_type": "application/pdf", "file_size": 200},
                "audio": {"file_id": "audio-id", "file_name": "song.mp3", "mime_type": "audio/mpeg", "file_size": 300},
                "video": {"file_id": "video-id", "file_name": "clip.mp4", "mime_type": "video/mp4", "file_size": 400}
            }
        });

        let inbound =
            telegram_update_to_inbound(&update).ok_or("telegram update was not normalized")?;

        assert_eq!(inbound.media.len(), 4);
        assert!(inbound
            .media
            .iter()
            .any(|media| media.starts_with("shacs-telegram-photo:")));
        assert!(inbound
            .media
            .iter()
            .any(|media| media.starts_with("shacs-telegram-document:")));
        assert!(inbound
            .media
            .iter()
            .any(|media| media.starts_with("shacs-telegram-audio:")));
        assert!(inbound
            .media
            .iter()
            .any(|media| media.starts_with("shacs-telegram-video:")));
        assert!(inbound
            .media
            .iter()
            .all(|media| !media.contains("photo-large/opaque")));

        let attachment_only = json!({
            "message": {
                "message_id": 11,
                "chat": {"id": 20},
                "from": {"id": 30, "username": "alice"},
                "photo": [
                    {"file_id": "photo-only", "file_unique_id": "p-only", "file_size": 10}
                ]
            }
        });
        let inbound = telegram_update_to_inbound(&attachment_only)
            .ok_or("telegram media-only update was not normalized")?;
        assert_eq!(inbound.content, "");
        assert_eq!(inbound.media.len(), 1);
        Ok(())
    }

    #[test]
    fn parse_email_body_extracts_mime_attachment_bytes_as_inline_media(
    ) -> Result<(), Box<dyn Error>> {
        let raw = b"From: Alice <alice@example.com>\r\nSubject: Attachment\r\nContent-Type: multipart/mixed; boundary=frontier\r\n\r\n--frontier\r\nContent-Type: text/plain\r\n\r\nBody text\r\n--frontier\r\nContent-Type: text/plain; name=note.txt\r\nContent-Disposition: attachment; filename=note.txt\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--frontier--\r\n";
        let mut parsed = parse_email_body(raw, "44".to_owned())?;
        assert!(parsed.inbound.attachments.is_empty());
        parsed.inbound.attachments = email_attachment_data_urls_from_body(raw)?;

        assert_eq!(parsed.inbound.attachments.len(), 1);
        assert!(
            parsed.inbound.attachments[0].starts_with("data:text/plain;name=bm90ZS50eHQ;base64,")
        );
        assert!(parsed.inbound.attachments[0].ends_with("aGVsbG8="));

        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join("workspace"))?;
        let adapter = external_media_test_adapter(root.path(), Arc::new(Mutex::new(Vec::new())))?;
        let normalized = adapter.normalize_external_inbound_media(
            parsed.inbound.into_message(),
            &adapter.loop_config(),
        )?;
        assert_eq!(normalized.media.len(), 1);
        assert!(Path::new(&normalized.media[0]).exists());
        assert_eq!(fs::read(&normalized.media[0])?, b"hello");
        Ok(())
    }

    #[test]
    fn attachment_caps_reject_oversized_reads_and_fixed_count_limits() -> Result<(), Box<dyn Error>>
    {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: https://cdn.discordapp.com/attachments/c1/a1/photo.png\r\nContent-Length: 0\r\n\r\n",
                );
            }
        });
        let agent = runtime_http_agent(Duration::from_secs(2));
        let error = get_binary(&agent, &format!("http://{address}/redirect"), None)
            .expect_err("attachment redirects must not be followed");
        handle
            .join()
            .map_err(|_| "redirect server thread panicked")?;
        assert!(error.contains("redirect rejected"));

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let declared_length = shacs_api::MAX_MEDIA_BYTES + 1;
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {declared_length}\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let error = get_binary(&agent, &format!("http://{address}/oversized"), None)
            .expect_err("oversized content-length must fail before body read");
        handle
            .join()
            .map_err(|_| "content-length server thread panicked")?;
        assert!(error.contains("downloaded attachment exceeds"));

        let oversized = std::io::repeat(0).take(shacs_api::MAX_MEDIA_BYTES as u64 + 1);
        let error = read_capped_binary_body(oversized).expect_err("oversized body must fail");
        assert!(error.contains("downloaded attachment exceeds"));

        let bytes = vec![0_u8; shacs_api::MAX_MEDIA_BYTES + 1];
        let error = email_attachment_data_url("application/octet-stream", None, &bytes)
            .expect_err("oversized email attachment must fail");
        assert!(error.contains("email attachment exceeds"));

        let root = tempfile::tempdir()?;
        let adapter = external_media_test_adapter(root.path(), Arc::new(Mutex::new(Vec::new())))?;
        let requests = (0..=DEFAULT_MAX_ATTACHMENTS_PER_MESSAGE)
            .map(|index| {
                ChannelAttachmentIntakeRequest::from_bytes(
                    "session".to_owned(),
                    "api".to_owned(),
                    Some(format!("{index}.txt")),
                    Some("text/plain".to_owned()),
                    b"x".to_vec(),
                )
            })
            .collect();
        let error = adapter
            .intake_api_attachments(requests, "test attachment")
            .expect_err("fixed attachment count cap must reject item 11");
        assert_eq!(error.status, 413);
        Ok(())
    }

    #[test]
    fn parse_email_body_extracts_sender_defaults_and_auth_results() -> Result<(), Box<dyn Error>> {
        let parsed = parse_email_body(
            b"From: Alice <alice@example.com>\r\nSubject: Hello\r\nMessage-Id: <m1@example.com>\r\nAuthentication-Results: mx.example; spf=pass smtp.mailfrom=example.com; dkim=pass header.d=example.com\r\n\r\nBody text",
            "42".to_owned(),
        )?;

        assert_eq!(parsed.inbound.sender_email, "alice@example.com");
        assert_eq!(parsed.inbound.subject, "Hello");
        assert_eq!(parsed.inbound.message_id, "<m1@example.com>");
        assert_eq!(parsed.inbound.body, "Body text");
        assert!(parsed
            .authentication_results
            .as_deref()
            .is_some_and(|value| value.contains("spf=pass") && value.contains("dkim=pass")));

        let defaults = parse_email_body(b"\r\nNo headers", "43".to_owned())?;
        assert_eq!(defaults.inbound.sender_email, "unknown@example.invalid");
        assert_eq!(defaults.inbound.subject, "(no subject)");
        assert_eq!(defaults.inbound.message_id, "43");
        Ok(())
    }

    #[test]
    fn email_inbound_hardening_filters_sender_auth_and_self_loop() -> Result<(), Box<dyn Error>> {
        let runtime = EmailRuntimeConfig {
            smtp: Some(EmailSmtpRuntimeConfig {
                host: "smtp.example.com".to_owned(),
                port: 587,
                username: Some("smtp-user@example.com".to_owned()),
                password: Some("smtp-password".to_owned()),
                from: "bot@example.com".to_owned(),
                security: EmailSecurity::StartTls,
                timeout_seconds: 30,
            }),
            imap: None,
            allowed_senders: vec!["alice@example.com".to_owned()],
            verify_spf: true,
            verify_dkim: true,
        };
        let imap = EmailImapRuntimeConfig {
            host: "imap.example.com".to_owned(),
            port: 993,
            username: "bot@example.com".to_owned(),
            password: "imap-password".to_owned(),
            mailbox: "INBOX".to_owned(),
            security: EmailSecurity::Tls,
            poll_interval_seconds: 60,
            timeout_seconds: 30,
            mark_seen: true,
        };

        let accepted = parse_email_body(
            b"From: Alice <alice@example.com>\r\nAuthentication-Results: mx; spf=pass; dkim=pass\r\n\r\nHi",
            "1".to_owned(),
        )?;
        assert!(!email_should_skip_inbound(&runtime, &imap, &accepted));

        let bad_auth = parse_email_body(
            b"From: Alice <alice@example.com>\r\nAuthentication-Results: mx; spf=pass\r\n\r\nHi",
            "2".to_owned(),
        )?;
        assert!(email_should_skip_inbound(&runtime, &imap, &bad_auth));

        let disallowed = parse_email_body(
            b"From: Eve <eve@example.com>\r\nAuthentication-Results: mx; spf=pass; dkim=pass\r\n\r\nHi",
            "3".to_owned(),
        )?;
        assert!(email_should_skip_inbound(&runtime, &imap, &disallowed));

        let self_loop = parse_email_body(
            b"From: bot@example.com\r\nAuthentication-Results: mx; spf=pass; dkim=pass\r\n\r\nHi",
            "4".to_owned(),
        )?;
        assert!(email_should_skip_inbound(&runtime, &imap, &self_loop));
        Ok(())
    }

    #[test]
    fn email_error_redaction_hides_credentials_and_addresses() {
        let smtp = EmailSmtpRuntimeConfig {
            host: "smtp.example.com".to_owned(),
            port: 587,
            username: Some("smtp-user@example.com".to_owned()),
            password: Some("smtp-password".to_owned()),
            from: "bot@example.com".to_owned(),
            security: EmailSecurity::StartTls,
            timeout_seconds: 30,
        };
        let redacted = redact_email_smtp_error(
            "login failed for smtp-user@example.com using smtp-password from bot@example.com"
                .to_owned(),
            &smtp,
        );
        assert!(!redacted.contains("smtp-user@example.com"));
        assert!(!redacted.contains("smtp-password"));
        assert!(!redacted.contains("bot@example.com"));

        let imap = EmailImapRuntimeConfig {
            host: "imap.example.com".to_owned(),
            port: 993,
            username: "imap-user@example.com".to_owned(),
            password: "imap-password".to_owned(),
            mailbox: "INBOX".to_owned(),
            security: EmailSecurity::Tls,
            poll_interval_seconds: 60,
            timeout_seconds: 30,
            mark_seen: true,
        };
        let redacted = redact_email_imap_error(
            "imap-user@example.com could not login with imap-password".to_owned(),
            &imap,
        );
        assert!(!redacted.contains("imap-user@example.com"));
        assert!(!redacted.contains("imap-password"));
    }

    #[test]
    fn drain_outbound_retries_real_send_attempts() -> Result<(), Box<dyn Error>> {
        let (tx, rx) = mpsc::channel();
        tx.send(OutboundMessage::new(TELEGRAM_CHANNEL, "chat", "hello"))?;
        drop(tx);
        let stop = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(Mutex::new(0usize));
        let attempts_for_send = attempts.clone();

        drain_outbound(&rx, 3, &stop, None, move |_message| {
            let mut attempts = attempts_for_send
                .lock()
                .map_err(|_| "attempt lock poisoned".to_owned())?;
            *attempts += 1;
            if *attempts < 3 {
                Err("temporary send failure".to_owned())
            } else {
                Ok(())
            }
        });

        assert_eq!(*attempts.lock().map_err(|_| "attempt lock poisoned")?, 3);
        Ok(())
    }

    #[test]
    fn drain_outbound_records_best_effort_delivery_metadata() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let metadata_path = root.path().join("telegram.json");
        let (tx, rx) = mpsc::channel();
        tx.send(OutboundMessage::new(TELEGRAM_CHANNEL, "chat", "hello"))?;
        drop(tx);
        let stop = Arc::new(AtomicBool::new(false));

        drain_outbound(&rx, 1, &stop, Some(&metadata_path), |_message| Ok(()));

        let metadata = load_metadata_json(&metadata_path);
        let deliveries = metadata
            .get("deliveries")
            .and_then(Value::as_array)
            .ok_or("missing deliveries")?;
        assert!(deliveries
            .iter()
            .any(|delivery| delivery.get("status").and_then(Value::as_str) == Some("pending")));
        assert!(deliveries
            .iter()
            .any(|delivery| delivery.get("status").and_then(Value::as_str) == Some("sent")));
        assert!(deliveries
            .iter()
            .all(|delivery| delivery.get("content").is_none()));
        Ok(())
    }

    #[test]
    fn external_message_chunks_prefers_paragraph_boundary() {
        assert_eq!(
            external_message_chunks("alpha\n\nbeta gamma", 8),
            vec!["alpha", "beta", "gamma"]
        );
    }

    #[test]
    fn external_message_chunks_prefers_newline_boundary() {
        assert_eq!(
            external_message_chunks("alpha\nbeta gamma", 10),
            vec!["alpha", "beta gamma"]
        );
    }

    #[test]
    fn external_message_chunks_prefers_space_boundary() {
        assert_eq!(
            external_message_chunks("alpha beta gamma", 11),
            vec!["alpha beta", "gamma"]
        );
    }

    #[test]
    fn external_message_chunks_hard_cuts_long_unbroken_text() {
        assert_eq!(
            external_message_chunks("abcdefghijk", 5),
            vec!["abcde", "fghij", "k"]
        );
    }

    #[test]
    fn external_message_chunks_keeps_exact_limit_in_one_chunk() {
        assert_eq!(external_message_chunks("abcde", 5), vec!["abcde"]);
    }

    #[test]
    fn external_message_chunks_splits_long_words_by_character_count() {
        assert_eq!(
            external_message_chunks("aaaaaaaaaaaa", 5),
            vec!["aaaaa", "aaaaa", "aa"]
        );
    }

    #[test]
    fn external_message_chunks_preserves_utf8_boundaries_for_cjk_and_emoji() {
        let chunks = external_message_chunks("가나🙂다라🙂", 3);
        assert_eq!(chunks, vec!["가나🙂", "다라🙂"]);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 3));
    }

    #[test]
    fn discord_message_chunks_use_character_count_boundaries() {
        let content = format!("{} tail", "🙂".repeat(DISCORD_EXTERNAL_MESSAGE_LIMIT));
        let chunks = discord_message_chunks(&content);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), DISCORD_EXTERNAL_MESSAGE_LIMIT);
        assert_eq!(chunks[1], "tail");
    }

    #[test]
    fn external_stream_deltas_are_final_only_for_external_transports() -> Result<(), Box<dyn Error>>
    {
        let agent = runtime_http_agent(Duration::from_millis(10));
        let telegram = TelegramRuntimeConfig {
            token: "telegram-token".to_owned(),
            poll_timeout_seconds: 1,
            poll_limit: 1,
        };
        let discord = DiscordRuntimeConfig {
            token: "discord-token".to_owned(),
            channel_filter: DiscordChannelFilter::AllVisible,
            allowed_senders: Vec::new(),
            group_policy: DiscordGroupPolicy::Open,
            streaming: true,
            poll_interval_seconds: 1,
            transport: DiscordTransportMode::Gateway,
        };
        let slack = SlackRuntimeConfig {
            app_token: "slack-app-token".to_owned(),
            bot_token: "slack-token".to_owned(),
            channel_ids: vec!["C123".to_owned()],
            allowed_senders: Vec::new(),
        };

        send_telegram_message(
            &agent,
            &telegram,
            stream_outbound_message(
                TELEGRAM_CHANNEL,
                "chat-1",
                "stream-1",
                "delta".to_owned(),
                false,
            ),
        )?;
        send_discord_message(
            &agent,
            &discord,
            stream_outbound_message(
                DISCORD_CHANNEL,
                "channel-1",
                "stream-1",
                "delta".to_owned(),
                false,
            ),
        )?;
        send_slack_message(
            &agent,
            &slack,
            stream_outbound_message(SLACK_CHANNEL, "C123", "stream-1", "delta".to_owned(), false),
        )?;
        send_telegram_message(
            &agent,
            &telegram,
            stream_outbound_message(TELEGRAM_CHANNEL, "chat-1", "stream-1", String::new(), true),
        )?;
        send_discord_message(
            &agent,
            &discord,
            stream_outbound_message(
                DISCORD_CHANNEL,
                "channel-1",
                "stream-1",
                String::new(),
                true,
            ),
        )?;
        send_slack_message(
            &agent,
            &slack,
            stream_outbound_message(SLACK_CHANNEL, "C123", "stream-1", String::new(), true),
        )?;
        Ok(())
    }

    #[test]
    fn discord_gateway_resume_payload_and_metadata_roundtrip() -> Result<(), Box<dyn Error>> {
        let identify = discord_gateway_identify_payload("token");
        assert_eq!(identify["op"], json!(2));
        assert_eq!(identify["d"]["intents"], json!(DISCORD_GATEWAY_INTENTS));

        let resume = discord_gateway_resume_payload("token", "session-1", 77);
        assert_eq!(resume["op"], json!(6));
        assert_eq!(resume["d"]["session_id"], json!("session-1"));
        assert_eq!(resume["d"]["seq"], json!(77));

        let root = tempfile::tempdir()?;
        let path = root.path().join("discord-gateway.json");
        let state = DiscordGatewayResumeState {
            session_id: Some("session-1".to_owned()),
            sequence: Some(77),
            resume_gateway_url: Some("wss://resume.example".to_owned()),
            bot_user_id: Some("bot-1".to_owned()),
            token_hash: Some(sha256_hex("token")),
        };
        save_discord_gateway_resume_state(&path, &state, "token");
        assert_eq!(load_discord_gateway_resume_state(&path, "token"), state);
        assert_eq!(
            load_discord_gateway_resume_state(&path, "other-token"),
            DiscordGatewayResumeState::default()
        );
        clear_discord_gateway_resume_state(&path);
        assert_eq!(
            load_discord_gateway_resume_state(&path, "token"),
            DiscordGatewayResumeState::default()
        );
        Ok(())
    }

    #[test]
    fn email_and_whatsapp_external_transports_remain_final_only() {
        assert!(!provider_streaming_channel(EMAIL_CHANNEL));
        assert!(!provider_streaming_channel(WHATSAPP_CHANNEL));
        assert!(!test_email_spec().supports_streaming());
        assert!(!test_whatsapp_spec().supports_streaming());
    }

    #[test]
    fn websocket_events_dispatch_through_channel_manager() -> Result<(), Box<dyn Error>> {
        let sink = WebSocketEventSink::default();
        let mut manager = websocket_event_channel_manager(1, sink.clone());
        manager.dispatch_outbound(OutboundMessage::new(WEBSOCKET_CHANNEL, "chat-1", "hello"))?;

        let mut metadata = Map::new();
        metadata.insert("_stream_delta".to_owned(), json!(true));
        metadata.insert("_stream_id".to_owned(), json!("stream-1"));
        manager.dispatch_outbound(
            OutboundMessage::new(WEBSOCKET_CHANNEL, "chat-1", "chunk").with_metadata(metadata),
        )?;

        let events = sink.take_events();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            WebSocketServerEvent::Message { chat_id, text, .. }
                if chat_id == "chat-1" && text == "hello"
        ));
        assert!(matches!(
            &events[1],
            WebSocketServerEvent::Delta { chat_id, text, stream_id }
                if chat_id == "chat-1" && text == "chunk" && stream_id.as_deref() == Some("stream-1")
        ));
        Ok(())
    }

    #[test]
    fn websocket_runtime_notifications_include_subagent_and_exclude_tool(
    ) -> Result<(), Box<dyn Error>> {
        let sink = WebSocketEventSink::default();
        let mut manager = websocket_event_channel_manager(1, sink.clone());
        let (tx, rx) = mpsc::channel();
        let mut subagent_metadata = Map::new();
        subagent_metadata.insert(
            "runtime_notification".to_owned(),
            json!({"kind": "subagent", "phase": "start"}),
        );
        tx.send(
            OutboundMessage::new(WEBSOCKET_CHANNEL, "chat-1", "Subagent [summary] started")
                .with_metadata(subagent_metadata),
        )?;
        let mut tool_metadata = Map::new();
        tool_metadata.insert(
            "runtime_notification".to_owned(),
            json!({"kind": "tool", "phase": "start"}),
        );
        tx.send(
            OutboundMessage::new(WEBSOCKET_CHANNEL, "chat-1", "Using tool: spawn")
                .with_metadata(tool_metadata),
        )?;
        drop(tx);
        let mut events = Vec::new();

        AgentLoopChatCompletionAdapter::drain_websocket_runtime_notifications(
            &rx,
            &mut manager,
            &sink,
            &mut |event| events.push(event),
        )?;

        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            WebSocketServerEvent::Message { chat_id, text, .. }
                if chat_id == "chat-1" && text == "Subagent [summary] started"
        ));
        Ok(())
    }

    fn recording_transport_runner(
        observed_tx: mpsc::Sender<OutboundMessage>,
    ) -> ExternalTransportRunner {
        Arc::new(move |_spec, _inbound_bus, outbound_rx, stop, _context| {
            while !stop.load(Ordering::SeqCst) {
                match outbound_rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(message) => {
                        let _ = observed_tx.send(message);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
    }

    fn test_transport_context() -> Result<ExternalTransportRuntimeContext, Box<dyn Error>> {
        Ok(ExternalTransportRuntimeContext::new(
            tempfile::tempdir()?.path().join("worker-metadata"),
            1,
        ))
    }

    fn test_telegram_spec() -> ExternalTransportSpec {
        ExternalTransportSpec::Telegram(TelegramRuntimeConfig {
            token: "telegram-token".to_owned(),
            poll_timeout_seconds: 1,
            poll_limit: 1,
        })
    }

    fn test_slack_spec() -> ExternalTransportSpec {
        ExternalTransportSpec::Slack(SlackRuntimeConfig {
            app_token: "slack-app-token".to_owned(),
            bot_token: "slack-token".to_owned(),
            channel_ids: vec!["C123".to_owned()],
            allowed_senders: Vec::new(),
        })
    }

    fn test_email_spec() -> ExternalTransportSpec {
        ExternalTransportSpec::Email(EmailRuntimeConfig {
            smtp: None,
            imap: Some(EmailImapRuntimeConfig {
                host: "imap.example.com".to_owned(),
                port: 993,
                username: "user".to_owned(),
                password: "password".to_owned(),
                mailbox: "INBOX".to_owned(),
                security: EmailSecurity::Tls,
                poll_interval_seconds: 60,
                timeout_seconds: 30,
                mark_seen: true,
            }),
            allowed_senders: vec!["sender@example.com".to_owned()],
            verify_spf: true,
            verify_dkim: true,
        })
    }

    fn test_whatsapp_spec() -> ExternalTransportSpec {
        ExternalTransportSpec::WhatsApp(WhatsAppRuntimeConfig {
            bridge_url: "ws://127.0.0.1:9001".to_owned(),
            bridge_token: None,
            allowlist: shacs_channels::ChannelAllowlist::allow_all(),
            group_policy: WhatsAppGroupPolicy::Open,
        })
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
            json!({ "enabled": true, "appToken": "slack-app-token", "botToken": "slack-token", "channelIds": ["C123"] }),
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
                "consentGranted": true,
                "allowFrom": ["sender@example.com"],
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

        let parsed = parse_cli_args([
            "provider",
            "github-copilot",
            "import-token",
            "--token-env",
            "COPILOT_TOKEN",
            "--select",
        ])?;
        let CliCommand::Provider(ProviderCommand::CopilotImportToken(options)) = parsed else {
            return Err("expected copilot import-token command".into());
        };
        assert_eq!(
            options.token_source,
            TokenSource::Env("COPILOT_TOKEN".to_owned())
        );
        assert!(options.select);

        let parsed = parse_cli_args([
            "provider",
            "import-key",
            "--provider",
            "openrouter",
            "--token-env",
            "OPENROUTER_API_KEY",
        ])?;
        let CliCommand::Provider(ProviderCommand::ImportApiKey(options)) = parsed else {
            return Err("expected generic provider import-key command".into());
        };
        assert_eq!(options.provider, "openrouter");
        assert_eq!(
            options.token_source,
            TokenSource::Env("OPENROUTER_API_KEY".to_owned())
        );

        let parsed = parse_cli_args(["provider", "openrouter", "import-key", "--token-stdin"])?;
        let CliCommand::Provider(ProviderCommand::ImportApiKey(options)) = parsed else {
            return Err("expected named provider import-key command".into());
        };
        assert_eq!(options.provider, "openrouter");
        assert_eq!(options.token_source, TokenSource::Stdin);
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
    fn adapter_wires_exec_env_to_context_and_subagents() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        let skill_dir = workspace.join("skills").join("configured-env");
        fs::create_dir_all(&skill_dir)?;
        fs::create_dir_all(&media_dir)?;
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\ndescription: Configured env skill\nrequires.env: SHACS_CLI_TEST_CONFIGURED_ENV_ONLY\n---\nUse configured env.\n",
        )?;
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace: workspace.clone(),
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: true,
            send_progress: false,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 123,
            exec_sandbox: Some("sandboxed".to_owned()),
            exec_path_append: Some("/configured/bin".to_owned()),
            exec_allowed_env_keys: vec!["HOME".to_owned()],
            exec_env: BTreeMap::from([(
                "SHACS_CLI_TEST_CONFIGURED_ENV_ONLY".to_owned(),
                "configured".to_owned(),
            )]),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let skills = adapter
            .context_builder()
            .build_skills_summary(&BTreeSet::new());
        let configured_line = skills
            .lines()
            .find(|line| line.contains("configured-env"))
            .unwrap_or_default();
        if configured_line.is_empty() || configured_line.contains("unavailable") {
            return Err(
                format!("adapter context builder did not use configured env: {skills}").into(),
            );
        }

        let mut loop_config = adapter.loop_config();
        loop_config.permission_ceiling_snapshot = Some(PermissionCeilingSnapshot {
            parent_mode: PermissionMode::Auto,
            capability_ceiling: vec![SafetyCapability::FsRead],
            approved_scope_refs: vec!["scope:test".to_owned()],
            origin: RuntimeBoundaryOrigin::LocalApi {
                request_id: Some("request-1".to_owned()),
            },
        });

        let subagent = adapter.subagent_execution_config(&loop_config);
        assert_eq!(subagent.exec_timeout_seconds, 123);
        assert_eq!(subagent.exec_sandbox.as_deref(), Some("sandboxed"));
        assert_eq!(
            subagent.exec_path_append.as_deref(),
            Some("/configured/bin")
        );
        assert_eq!(subagent.exec_allowed_env_keys, vec!["HOME".to_owned()]);
        assert_eq!(
            subagent.containment_snapshot,
            adapter.loop_config().containment_snapshot
        );
        assert_eq!(
            subagent.permission_mode_snapshot,
            loop_config.permission_mode_snapshot
        );
        assert_eq!(
            subagent.permission_ceiling_snapshot,
            loop_config.permission_ceiling_snapshot
        );
        assert_eq!(
            subagent
                .exec_env
                .get("SHACS_CLI_TEST_CONFIGURED_ENV_ONLY")
                .map(String::as_str),
            Some("configured")
        );
        Ok(())
    }

    #[test]
    fn production_tool_registry_wires_exec_env() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let mut config = shacs_config::Config {
            env: BTreeMap::from([
                (
                    "SHACS_CLI_EXEC_CONFIG_ONLY".to_owned(),
                    "configured".to_owned(),
                ),
                ("SHACS_CLI_EXEC_OVERRIDE".to_owned(), "global".to_owned()),
            ]),
            ..shacs_config::Config::default()
        };
        config.tools.exec.env =
            BTreeMap::from([("SHACS_CLI_EXEC_OVERRIDE".to_owned(), "exec".to_owned())]);
        let bundle = ConfigBundle {
            config,
            context: shacs_config::ConfigContext {
                config_path: root.path().join("config.json"),
                data_dir: root.path().join("data"),
                workspace,
            },
            migrations: Vec::new(),
        };

        let tooling = production_tool_registry(&bundle, true)?;
        let result = tooling
            .registry
            .execute(
                "exec",
                json!({
                    "command": "printf '%s|%s' \"$SHACS_CLI_EXEC_CONFIG_ONLY\" \"$SHACS_CLI_EXEC_OVERRIDE\"",
                    "timeout": 5
                }),
            )
            .into_text();
        if !result.contains("configured|exec") || !result.contains("Exit code: 0") {
            return Err(format!("production registry exec env was not wired: {result}").into());
        }
        Ok(())
    }

    #[test]
    fn production_tool_registry_gates_image_generate_by_side_effects_and_config(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let mut bundle = image_generation_test_bundle(root.path())?;

        let disabled_side_effects = production_tool_registry(&bundle, false)?;
        assert!(!disabled_side_effects.registry.has("image_generate"));

        bundle.config.tools.image_generation.enable = false;
        let disabled_config = production_tool_registry(&bundle, true)?;
        assert!(!disabled_config.registry.has("image_generate"));

        bundle.config.tools.image_generation.enable = true;
        let enabled = production_tool_registry(&bundle, true)?;
        assert!(enabled.registry.has("image_generate"));
        let schema_text = serde_json::to_string(&enabled.registry.definitions())?;
        for forbidden in ["apiKey", "endpoint", "baseUrl", "providerOptions"] {
            if schema_text.contains(forbidden) {
                return Err(
                    format!("image_generate schema leaked forbidden field: {forbidden}").into(),
                );
            }
        }
        Ok(())
    }

    fn image_generation_test_bundle(root: &Path) -> Result<ConfigBundle, Box<dyn Error>> {
        let workspace = root.join("workspace");
        let mut config = shacs_config::Config::default();
        config.tools.image_generation.enable = true;
        config.tools.image_generation.provider = "openai".to_owned();
        config.providers.insert(
            "openai".to_owned(),
            ProviderConfig {
                api_key: Some("sk-test".to_owned()),
                ..ProviderConfig::default()
            },
        );
        Ok(ConfigBundle {
            config,
            context: shacs_config::ConfigContext {
                config_path: root.join("config.json"),
                data_dir: root.join("data"),
                workspace,
            },
            migrations: Vec::new(),
        })
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
        assert!(formatted.contains("Web UI server ready"));
        assert!(formatted.contains("ws://127.0.0.1:8901/ws"));
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
        assert_eq!(report.send_max_retries, 5);
        assert_eq!(report.unknown_plugins, vec!["custom-local".to_owned()]);
        let formatted = format_channels_status(report.clone());
        assert!(!formatted.contains("Send progress"));
        assert!(!formatted.contains("Send tool hints"));
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
        assert!(status.contains("runtime=disabled"));
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
        assert!(root.path().join("data").join("media").exists());
        assert!(root.path().join("data").join("cron").exists());
        assert!(root.path().join("data").join("logs").exists());
        assert!(root
            .path()
            .join("data")
            .join("channels")
            .join("worker-metadata")
            .exists());
        assert!(root.path().join("data").join("skills").exists());
        assert!(outcome.workspace.join("memory").join("MEMORY.md").exists());
        assert!(outcome.workspace.join("skills").exists());
        assert!(outcome
            .workspace
            .join("builtin_skills")
            .join("skill-creator")
            .join("SKILL.md")
            .exists());
        assert!(outcome
            .workspace
            .join("builtin_skills")
            .join("test-driven-development")
            .join("SKILL.md")
            .exists());
        assert!(!outcome
            .workspace
            .join("builtin_skills")
            .join("hermes-agent")
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
        let saved_json: Value = serde_json::from_str(&saved)?;
        let channels = saved_json["channels"]
            .as_object()
            .ok_or("channels config should be an object")?;
        for channel in [
            WEBSOCKET_CHANNEL,
            TELEGRAM_CHANNEL,
            DISCORD_CHANNEL,
            SLACK_CHANNEL,
            EMAIL_CHANNEL,
            WHATSAPP_CHANNEL,
        ] {
            assert!(
                channels.contains_key(channel),
                "missing channel default: {channel}"
            );
        }
        assert_eq!(channels[WEBSOCKET_CHANNEL]["enabled"], json!(true));
        assert_eq!(channels[EMAIL_CHANNEL]["consentGranted"], json!(false));
        Ok(())
    }

    #[test]
    fn onboard_merges_missing_channel_defaults_without_overwriting_existing_values(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.channels.plugins.insert(
            TELEGRAM_CHANNEL.to_owned(),
            json!({ "enabled": true, "token": "${TELEGRAM_TOKEN}" }),
        );
        config.channels.plugins.insert(
            EMAIL_CHANNEL.to_owned(),
            json!({
                "enabled": true,
                "smtp": { "host": "smtp.example.com", "password": "${SMTP_PASSWORD}" }
            }),
        );
        save_config_to_path(&config, &config_path)?;

        onboard(OnboardOptions {
            config_path: Some(config_path.clone()),
            workspace: None,
            wizard: false,
        })?;

        let saved: Value = serde_json::from_str(&fs::read_to_string(config_path)?)?;
        let channels = saved["channels"]
            .as_object()
            .ok_or("channels config should be an object")?;
        assert_eq!(channels[TELEGRAM_CHANNEL]["enabled"], json!(true));
        assert_eq!(
            channels[TELEGRAM_CHANNEL]["token"],
            json!("${TELEGRAM_TOKEN}")
        );
        assert_eq!(channels[TELEGRAM_CHANNEL]["pollLimit"], json!(20));
        assert_eq!(
            channels[EMAIL_CHANNEL]["smtp"]["host"],
            json!("smtp.example.com")
        );
        assert_eq!(
            channels[EMAIL_CHANNEL]["smtp"]["password"],
            json!("${SMTP_PASSWORD}")
        );
        assert_eq!(channels[EMAIL_CHANNEL]["smtp"]["port"], json!(587));
        assert!(channels.contains_key(WEBSOCKET_CHANNEL));
        assert!(channels.contains_key(WHATSAPP_CHANNEL));
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
    fn runtime_config_migration_writeback_preserves_env_placeholders() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let saved_workspace = root.path().join("saved-workspace");
        fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "agents": {
                    "defaults": {
                        "workspace": saved_workspace.to_string_lossy(),
                        "sessionTtlMinutes": 15
                    }
                },
                "providers": {
                    "openrouter": {
                        "apiKey": "${OPENROUTER_API_KEY}"
                    }
                },
                "tools": {
                    "exec": {
                        "restrictToWorkspace": true
                    }
                }
            }))?,
        )?;

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
        assert!(saved.contains("idleCompactAfterMinutes"));
        assert!(saved.contains("restrictToWorkspace"));
        assert!(!saved.contains("sessionTtlMinutes"));
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
        fs::create_dir_all(workspace.join("builtin_skills/hermes-agent"))?;
        fs::write(
            workspace.join("builtin_skills/hermes-agent/SKILL.md"),
            "---\ndescription: Stale deferred builtin\n---\nStale Hermes body",
        )?;

        let list = skills_list(SkillsListOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            all: false,
        })?;
        assert_eq!(list.workspace, workspace);
        assert!(list.entries.iter().any(|entry| {
            entry.descriptor.name == "clawhub" && entry.status == SkillRegistryStatus::Active
        }));
        assert!(list.entries.iter().any(|entry| {
            entry.descriptor.name == "test-driven-development"
                && entry.status == SkillRegistryStatus::Active
        }));
        let output = format_skills_list(list);
        assert!(output.contains("clawhub"));
        assert!(output.contains("test-driven-development"));
        assert!(!output.contains("hermes-agent"));
        assert!(matches!(
            skills_show(SkillsShowOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                name: "hermes-agent".to_owned(),
            }),
            Err(CliError::InvalidArguments(message)) if message.contains("unknown skill")
        ));

        let show = skills_show(SkillsShowOptions {
            config_path: Some(config_path),
            workspace_override: None,
            name: "test-driven-development".to_owned(),
        })?;
        assert_eq!(show.entry.descriptor.name, "test-driven-development");
        assert!(!show.entry.descriptor.body_hash.is_empty());
        let output = format_skills_show(show);
        assert!(output.contains("Skill: test-driven-development"));
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
    fn apply_api_key_auth_overlay_creates_missing_provider_entry() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        save_config_to_path(&Config::default(), &config_path)?;

        let auth_path = config_context(Some(config_path.clone()), None).auth_path();
        let mut auth = AuthStore::default();
        auth.providers
            .insert("openrouter".to_owned(), ProviderAuth::api_key("sk-or-test"));
        save_auth_store_to_path(&auth, &auth_path)?;

        let mut bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &ProcessEnv,
        )?;
        apply_api_key_auth_overlay(&mut bundle)?;

        let provider = bundle
            .config
            .providers
            .get("openrouter")
            .ok_or("missing openrouter provider")?;
        assert_eq!(provider.api_key.as_deref(), Some("sk-or-test"));
        Ok(())
    }

    #[test]
    fn apply_api_key_auth_overlay_preserves_explicit_keys_and_ignores_blank_or_oauth_auth(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config.providers.insert(
            "openrouter".to_owned(),
            ProviderConfig {
                api_key: Some("sk-config".to_owned()),
                api_base: None,
                extra_headers: None,
                extra_body: None,
            },
        );
        config
            .providers
            .insert("blank".to_owned(), ProviderConfig::default());
        config
            .providers
            .insert("oauth".to_owned(), ProviderConfig::default());
        save_config_to_path(&config, &config_path)?;

        let auth_path = config_context(Some(config_path.clone()), None).auth_path();
        let mut auth = AuthStore::default();
        auth.providers
            .insert("openrouter".to_owned(), ProviderAuth::api_key("sk-or-test"));
        auth.providers
            .insert("blank".to_owned(), ProviderAuth::api_key("   \t  "));
        auth.providers.insert(
            "oauth".to_owned(),
            ProviderAuth::oauth_access("oauth-token", None),
        );
        save_auth_store_to_path(&auth, &auth_path)?;

        let mut bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &ProcessEnv,
        )?;
        apply_api_key_auth_overlay(&mut bundle)?;

        assert_eq!(
            bundle
                .config
                .providers
                .get("openrouter")
                .and_then(|provider| provider.api_key.as_deref()),
            Some("sk-config")
        );
        assert_eq!(
            bundle
                .config
                .providers
                .get("blank")
                .and_then(|provider| provider.api_key.as_deref()),
            None
        );
        assert_eq!(
            bundle
                .config
                .providers
                .get("oauth")
                .and_then(|provider| provider.api_key.as_deref()),
            None
        );
        Ok(())
    }

    #[test]
    fn status_and_runtime_inspect_report_api_key_from_auth_store() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        save_config_to_path(&Config::default(), &config_path)?;

        let auth_path = config_context(Some(config_path.clone()), None).auth_path();
        let mut auth = AuthStore::default();
        auth.providers
            .insert("openrouter".to_owned(), ProviderAuth::api_key("sk-or-test"));
        save_auth_store_to_path(&auth, &auth_path)?;

        let status_report = status(StatusOptions {
            config_path: Some(config_path.clone()),
        })?;
        let status_provider = status_report
            .providers
            .iter()
            .find(|provider| provider.name == "openrouter")
            .ok_or("missing status provider")?;
        assert!(status_provider.has_api_key);

        let inspect_report = runtime_inspect_inner(
            RuntimeInspectOptions {
                config_path: Some(config_path),
                workspace_override: None,
            },
            false,
        )?;
        let inspect_provider = inspect_report
            .providers
            .iter()
            .find(|provider| provider.name == "openrouter")
            .ok_or("missing runtime inspect provider")?;
        assert!(inspect_provider.has_api_key);
        Ok(())
    }

    #[test]
    fn import_provider_api_key_persists_secret_only_in_auth_store() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        save_config_to_path(&Config::default(), &config_path)?;

        let outcome = import_provider_api_key(ProviderApiKeyImportOptions {
            config_path: Some(config_path.clone()),
            provider: "openrouter".to_owned(),
            token_source: TokenSource::Literal("sk-or-test".to_owned()),
        })?;
        assert_eq!(outcome.config_path, config_path);

        let auth = load_auth_store(&outcome.auth_path)?;
        let provider_auth = auth
            .providers
            .get("openrouter")
            .ok_or("missing auth provider")?;
        assert_eq!(provider_auth.kind, "apiKey");
        assert_eq!(provider_auth.access, "sk-or-test");

        let config_json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&outcome.config_path)?)?;
        let provider_json = config_json
            .get("providers")
            .and_then(|providers| providers.get("openrouter"))
            .ok_or("missing config provider")?;
        assert!(provider_json.get("apiKey").is_none());
        assert!(!fs::read_to_string(&outcome.config_path)?.contains("sk-or-test"));
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
        config.tools.exec.sandbox = "bwrap".to_owned();
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
        assert!(report.containment.digest.is_some());
        assert!(report.containment.summary.is_some());
        assert!(!report
            .containment
            .summary
            .as_deref()
            .unwrap_or_default()
            .contains("sandboxed"));
        assert_eq!(report.lifecycle.binary_version, VERSION);
        assert_eq!(
            report.lifecycle.data_schema_version,
            RUNTIME_DATA_SCHEMA_VERSION
        );
        assert!(report.lifecycle.update_marker.is_none());
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
        assert!(output.contains("Binary version:"));
        assert!(output.contains("Update marker: none"));
        assert!(output.contains("Sessions: 1"));
        assert!(output.contains("Runtime containment: contained="));
        assert!(output.contains("snapshot_digest="));
        assert!(output.contains("Generated image artifacts: 0"));
        assert!(!output.contains("hello"));
        Ok(())
    }

    #[test]
    fn runtime_diagnostics_outputs_redacted_snapshot_and_bundle() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let bundle_path = root.path().join("diagnostics.zip");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.agents.defaults.provider = "openai_codex".to_owned();
        config.agents.defaults.model = "gpt-5.4".to_owned();
        config.tools.exec.sandbox = "bwrap sk-raw-secret".to_owned();
        config.providers.insert(
            "openai_codex".to_owned(),
            ProviderConfig {
                api_key: Some("sk-raw-secret".to_owned()),
                api_base: Some("https://chatgpt.com/backend-api".to_owned()),
                extra_headers: None,
                extra_body: None,
            },
        );
        save_config_to_path(&config, &config_path)?;
        let media_dir =
            config_context(Some(config_path.clone()), None).media_dir(Some("image-generation"));
        fs::create_dir_all(&media_dir)?;
        fs::write(media_dir.join("img-test.png"), b"not real png")?;
        fs::write(
            media_dir.join("img-test.json"),
            serde_json::to_vec_pretty(&json!({
                "artifactId": "img-test",
                "mediaRef": "media/image-generation/img-test.png",
                "mimeType": "image/png",
                "byteLen": 12,
                "sha256": "abc123",
                "providerId": "openai",
                "modelId": "gpt-image-2",
                "createdAt": "2026-05-28T00:00:00Z",
                "requestOptionSummary": {"format": "png", "count": 1},
                "revisedPrompt": {"sha256": "prompt-digest", "redacted": true},
                "providerRequestId": "req-1"
            }))?,
        )?;

        let parsed = parse_cli_args([
            "--config",
            config_path.to_string_lossy().as_ref(),
            "runtime",
            "diagnostics",
            "--bundle",
            bundle_path.to_string_lossy().as_ref(),
        ])?;
        let CliCommand::RuntimeDiagnostics(options) = parsed else {
            return Err("expected runtime diagnostics command".into());
        };
        let report = runtime_diagnostics(options)?;
        let output = format_runtime_diagnostics(report);

        assert!(bundle_path.exists());
        assert!(!workspace.exists());
        assert!(output.contains("local runtime diagnostics snapshot generated"));
        assert!(output.contains("generated_media"));
        assert!(output.contains("img-test"));
        assert!(!output.contains("private prompt"));
        assert!(!output.contains("not real png"));
        assert!(output.contains("runtime diagnostics provider snapshot"));
        assert!(output.contains("runtime capability snapshot"));
        assert!(output.contains("containment"));
        assert!(output.contains("summary"));
        assert!(output.contains("[REDACTED]") || !output.contains("api_key"));
        assert!(!output.contains("sk-raw-secret"));
        assert!(!output.contains("raw-token"));
        Ok(())
    }

    #[test]
    fn runtime_containment_classifier_reports_native_unknown() {
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            None,
            false,
            &[],
            &[],
        ));

        assert_eq!(inspect.contained, None);
        assert_eq!(inspect.backend, None);
        assert_eq!(
            inspect.summary.as_deref(),
            Some("native runtime; containment not observed")
        );
        assert!(inspect.digest.is_some());
    }

    #[test]
    fn runtime_containment_snapshot_ref_preserves_unknown_state() {
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            None,
            false,
            &[],
            &[],
        ));
        let snapshot = runtime_containment_snapshot_ref(&inspect);

        assert_eq!(snapshot.contained, None);
        assert_eq!(
            snapshot.summary.as_deref(),
            Some("native runtime; containment not observed")
        );
        assert!(snapshot.digest.is_some());
    }

    #[test]
    fn bypass_permissions_falls_back_for_native_unknown_containment() -> Result<(), Box<dyn Error>>
    {
        let config: shacs_config::PermissionsConfig = serde_json::from_value(json!({
            "mode": "bypass_permissions"
        }))?;
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            None,
            false,
            &[],
            &[],
        ));
        let snapshot = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext {
                user_local_auto_opt_in: false,
                containment_precondition_met: runtime_containment_precondition_met(&inspect),
            },
        );
        let runtime_snapshot = runtime_permission_mode_snapshot(&snapshot);

        assert_eq!(snapshot.mode, shacs_config::PermissionMode::Default);
        assert_eq!(snapshot.source, PermissionModeSource::DefaultFallback);
        assert_eq!(
            snapshot.diagnostics.safe_fallback_reason.as_deref(),
            Some("bypass_permissions_requires_containment")
        );
        assert_eq!(runtime_snapshot.mode, shacs_config::PermissionMode::Default);
        assert_eq!(runtime_snapshot.source.as_deref(), Some("default_fallback"));
        Ok(())
    }

    #[test]
    fn bypass_permissions_falls_back_for_unsafe_privileged() -> Result<(), Box<dyn Error>> {
        let config: shacs_config::PermissionsConfig = serde_json::from_value(json!({
            "mode": "bypass_permissions"
        }))?;
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            None,
            true,
            &["dockerenv"],
            &["docker-socket", "privileged-env"],
        ));
        let snapshot = config.normalized_snapshot(
            PermissionModeSource::UserLocalConfig,
            PermissionActivationContext {
                user_local_auto_opt_in: false,
                containment_precondition_met: runtime_containment_precondition_met(&inspect),
            },
        );
        let runtime_snapshot = runtime_permission_mode_snapshot(&snapshot);

        assert_eq!(inspect.contained, Some(false));
        assert_eq!(inspect.backend.as_deref(), Some("unsafe-privileged"));
        assert_eq!(snapshot.mode, shacs_config::PermissionMode::Default);
        assert_eq!(snapshot.source, PermissionModeSource::DefaultFallback);
        assert_eq!(
            snapshot.diagnostics.safe_fallback_reason.as_deref(),
            Some("bypass_permissions_requires_containment")
        );
        assert_eq!(runtime_snapshot.mode, shacs_config::PermissionMode::Default);
        assert_eq!(runtime_snapshot.source.as_deref(), Some("default_fallback"));
        Ok(())
    }

    #[test]
    fn runtime_containment_classifier_reports_official_container_marker() {
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            None,
            true,
            &["dockerenv"],
            &[],
        ));

        assert_eq!(inspect.contained, Some(true));
        assert_eq!(inspect.backend.as_deref(), Some("official-container"));
        let summary = inspect.summary.as_deref().unwrap_or_default();
        assert!(summary.contains("official package marker"));
        assert!(summary.contains("dockerenv"));
        assert!(!summary.contains("sandboxed"));
    }

    #[test]
    fn runtime_containment_classifier_treats_bwrap_as_optional_hardening() {
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            Some("bwrap"),
            false,
            &[],
            &[],
        ));

        assert_eq!(inspect.contained, None);
        assert_eq!(inspect.backend.as_deref(), Some("bwrap"));
        assert_eq!(
            inspect.summary.as_deref(),
            Some("exec sandbox backend configured as optional hardening; runtime containment not observed")
        );
    }

    #[test]
    fn runtime_containment_classifier_reports_recognized_container_evidence() {
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            Some("bwrap"),
            false,
            &["cgroup", "dockerenv"],
            &[],
        ));

        assert_eq!(inspect.contained, Some(true));
        assert_eq!(inspect.backend.as_deref(), Some("container+bwrap"));
        let summary = inspect.summary.as_deref().unwrap_or_default();
        assert!(summary.contains("recognized container runtime evidence"));
        assert!(summary.contains("cgroup"));
        assert!(summary.contains("dockerenv"));
    }

    #[test]
    fn runtime_containment_classifier_reports_unsafe_privileged_evidence() {
        let inspect = runtime_containment_classify(RuntimeContainmentEvidence::from_parts(
            None,
            true,
            &["dockerenv"],
            &["docker-socket", "privileged-env"],
        ));

        assert_eq!(inspect.contained, Some(false));
        assert_eq!(inspect.backend.as_deref(), Some("unsafe-privileged"));
        let summary = inspect.summary.as_deref().unwrap_or_default();
        assert!(summary.contains("unsafe privileged runtime evidence"));
        assert!(summary.contains("docker-socket"));
        assert!(summary.contains("privileged-env"));
    }

    #[test]
    fn runtime_diagnostics_bundle_generation_failure_is_safe() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let bundle_path = root.path().join("missing").join("diagnostics.zip");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        config.providers.insert(
            "openai".to_owned(),
            ProviderConfig {
                api_key: Some("sk-bundle-secret".to_owned()),
                api_base: None,
                extra_headers: None,
                extra_body: None,
            },
        );
        save_config_to_path(&config, &config_path)?;

        let report = runtime_diagnostics(RuntimeDiagnosticsOptions {
            config_path: Some(config_path),
            workspace_override: None,
            bundle_path: Some(bundle_path),
        })?;
        let output = format_runtime_diagnostics(report);

        assert!(output.contains("Bundle: failed"));
        assert!(output.contains("diagnostics bundle generation failed"));
        assert!(!output.contains("sk-bundle-secret"));
        Ok(())
    }

    #[test]
    fn runtime_update_records_marker_and_recover_clears_it() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let update = runtime_update(RuntimeUpdateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            target_version: VERSION.to_owned(),
        })?;

        assert_eq!(update.from_version, VERSION);
        assert_eq!(update.target_version, VERSION);
        assert_eq!(update.phase, "completed_cleanup");
        assert!(update.marker_path.exists());
        let inspect = runtime_inspect(RuntimeInspectOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;
        let marker = inspect
            .lifecycle
            .update_marker
            .ok_or("missing update marker")?;
        assert_eq!(marker.phase, "completed_cleanup");
        assert_eq!(marker.target_version, VERSION);
        let output = format_runtime_update(update);
        assert!(output.contains(&format!("Target version: {VERSION}")));
        assert!(output.contains("Phase: completed_cleanup"));

        let recover = runtime_recover(RuntimeRecoverOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;
        assert!(recover.recovered);
        assert!(!recover.marker_path.exists());
        let output = format_runtime_recover(recover);
        assert!(output.contains("Recovered: true"));
        Ok(())
    }

    #[test]
    fn runtime_update_requires_running_binary_target_and_marker_guard_blocks_mutation(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;

        let mismatch = runtime_update(RuntimeUpdateOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
            target_version: "999.0.0".to_owned(),
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(mismatch.contains("must match the running binary version"));

        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_update_marker_value(VERSION, "in_progress", None),
        )?;

        let blocked = load_runtime_config(RuntimeConfigOptions {
            config_path: Some(config_path),
            workspace_override: None,
            resolve_env: false,
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(blocked.contains("runtime mutation blocked by in_progress update marker"));
        Ok(())
    }

    #[test]
    fn runtime_update_blocks_existing_interrupted_marker() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_update_marker_value("0.2.0", "in_progress", None),
        )?;

        let error = runtime_update(RuntimeUpdateOptions {
            config_path: Some(config_path),
            workspace_override: None,
            target_version: VERSION.to_owned(),
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();

        assert!(error.contains("blocked by existing in_progress marker"));
        Ok(())
    }

    #[test]
    fn runtime_recover_blocks_partial_migration_marker() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_update_marker_value(VERSION, "partial_migration", None),
        )?;

        let error = runtime_recover(RuntimeRecoverOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();

        assert!(error.contains("partial migration marker requires manual inspection"));
        assert!(marker_path.exists());
        Ok(())
    }

    #[test]
    fn runtime_ownership_classifies_active_and_stale_markers() -> Result<(), Box<dyn Error>> {
        let now = now_millis();
        let active = RuntimeOwnershipMarker {
            pid: std::process::id(),
            started_at_ms: now,
            updated_at_ms: now,
            binary_version: VERSION.to_owned(),
            data_schema_version: RUNTIME_DATA_SCHEMA_VERSION,
            mode: "run".to_owned(),
            config_path: "/tmp/config.json".to_owned(),
            workspace: "/tmp/workspace".to_owned(),
        };
        let status = classify_runtime_ownership_marker(active, now);
        assert_eq!(status.state, RuntimeOwnershipState::Active);

        let stale_pid = RuntimeOwnershipMarker {
            pid: 999_999,
            started_at_ms: now,
            updated_at_ms: now,
            binary_version: VERSION.to_owned(),
            data_schema_version: RUNTIME_DATA_SCHEMA_VERSION,
            mode: "run".to_owned(),
            config_path: "/tmp/config.json".to_owned(),
            workspace: "/tmp/workspace".to_owned(),
        };
        let status = classify_runtime_ownership_marker(stale_pid, now);
        assert_eq!(status.state, RuntimeOwnershipState::Stale);

        let stale_heartbeat = RuntimeOwnershipMarker {
            pid: std::process::id(),
            started_at_ms: now,
            updated_at_ms: now.saturating_sub(RUNTIME_OWNERSHIP_HEARTBEAT_TTL_MS + 1),
            binary_version: VERSION.to_owned(),
            data_schema_version: RUNTIME_DATA_SCHEMA_VERSION,
            mode: "serve".to_owned(),
            config_path: "/tmp/config.json".to_owned(),
            workspace: "/tmp/workspace".to_owned(),
        };
        let status = classify_runtime_ownership_marker(stale_heartbeat, now);
        assert_eq!(status.state, RuntimeOwnershipState::Stale);
        Ok(())
    }

    #[test]
    fn runtime_active_ownership_blocks_start_admission() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_ownership_marker_path(&bundle.context.data_dir);
        let now = now_millis();
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_ownership_marker_value(
                std::process::id(),
                now,
                now,
                "run",
                &config_path,
                &workspace,
            ),
        )?;

        let error = run_runtime(RunOptions {
            config_path: Some(config_path),
            workspace_override: None,
            ..RunOptions::default()
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(error.contains("active runtime ownership"));
        Ok(())
    }

    #[test]
    fn runtime_ownership_marker_write_if_absent_returns_already_exists_and_preserves_existing_content(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let marker_path = root.path().join("runtime-ownership.json");
        let existing = serde_json::json!({"owner": "existing"});
        write_runtime_marker_atomically(&marker_path, &existing)?;
        let before = fs::read_to_string(&marker_path)?;

        let error =
            write_runtime_marker_if_absent(&marker_path, &serde_json::json!({"owner": "new"}))
                .err()
                .ok_or_else(|| "expected already exists error".to_owned())?;
        let io_error = match error {
            CliError::Io(error) => error,
            other => {
                return Err(format!("expected io error, got {other:?}").into());
            }
        };
        assert_eq!(io_error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&marker_path)?, before);
        assert_eq!(fs::read_dir(root.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn runtime_ownership_heartbeat_update_preserves_different_owner_marker(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let marker_path = root.path().join("runtime").join("ownership-marker.json");
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let current_started_at_ms = now_millis();
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_ownership_marker_value(
                std::process::id(),
                current_started_at_ms,
                current_started_at_ms,
                "run",
                &config_path,
                &workspace,
            ),
        )?;
        let before = read_runtime_ownership_marker(&marker_path)?
            .ok_or_else(|| "expected ownership marker".to_owned())?;
        let old_started_at_ms = current_started_at_ms.saturating_sub(1);
        let heartbeat_marker = runtime_ownership_marker_value(
            std::process::id(),
            old_started_at_ms,
            now_millis(),
            "run",
            &config_path,
            &workspace,
        );

        let updated = update_runtime_ownership_heartbeat_if_current(
            &marker_path,
            std::process::id(),
            old_started_at_ms,
            &heartbeat_marker,
        )?;

        assert!(!updated);
        let after = read_runtime_ownership_marker(&marker_path)?
            .ok_or_else(|| "expected ownership marker after heartbeat".to_owned())?;
        assert_eq!(after, before);
        Ok(())
    }

    #[test]
    fn runtime_stale_ownership_cleanup_rechecks_active_marker_before_removing(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let data_dir = root.path();
        let marker_path = runtime_ownership_marker_path(data_dir);
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let stale_started_at_ms = now_millis();
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_ownership_marker_value(
                999_999,
                stale_started_at_ms,
                stale_started_at_ms,
                "run",
                &config_path,
                &workspace,
            ),
        )?;
        let active_started_at_ms = now_millis();
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_ownership_marker_value(
                std::process::id(),
                active_started_at_ms,
                active_started_at_ms,
                "run",
                &config_path,
                &workspace,
            ),
        )?;

        let error = remove_stale_runtime_ownership_marker(data_dir, now_millis())
            .err()
            .ok_or_else(|| "expected active owner recover error".to_owned())?;

        assert!(error
            .to_string()
            .contains("active runtime owner must stop first"));
        let marker = read_runtime_ownership_marker(&marker_path)?
            .ok_or_else(|| "expected active ownership marker".to_owned())?;
        assert_eq!(marker.pid, std::process::id());
        assert_eq!(marker.started_at_ms, active_started_at_ms);
        Ok(())
    }

    #[test]
    fn runtime_ownership_acquire_cleans_marker_when_stop_request_removal_fails(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let ownership_path = runtime_ownership_marker_path(&bundle.context.data_dir);
        let request_path = runtime_stop_request_marker_path(&bundle.context.data_dir);
        fs::create_dir_all(&request_path)?;

        let error = RuntimeOwnershipLease::acquire(&bundle, "run")
            .err()
            .ok_or_else(|| "expected stop request removal failure".to_owned())?;

        assert!(error.to_string().contains("CLI I/O failed"));
        assert!(!ownership_path.exists());
        assert!(request_path.exists());
        Ok(())
    }

    #[test]
    fn runtime_marker_atomic_write_removes_temp_on_rename_failure() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let marker_path = root.path().join("ownership-marker.json");
        fs::create_dir(&marker_path)?;

        let error = write_runtime_marker_atomically(&marker_path, &json!({"owner": "new"}))
            .err()
            .ok_or_else(|| "expected atomic marker rename failure".to_owned())?;

        assert!(error.to_string().contains("CLI I/O failed"));
        let mut entries = Vec::new();
        for entry_result in fs::read_dir(root.path())? {
            let entry = entry_result?;
            entries.push(entry.file_name().to_string_lossy().to_string());
        }
        assert_eq!(entries, vec!["ownership-marker.json".to_owned()]);
        Ok(())
    }

    #[test]
    fn runtime_recover_clears_stale_ownership_marker() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_ownership_marker_path(&bundle.context.data_dir);
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_ownership_marker_value(
                999_999,
                now_millis(),
                now_millis(),
                "run",
                &config_path,
                &workspace,
            ),
        )?;

        let recovered = runtime_recover(RuntimeRecoverOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })?;
        assert!(recovered.recovered);
        assert!(!marker_path.exists());
        assert!(recovered.detail.contains("stale runtime ownership"));
        Ok(())
    }

    #[test]
    fn runtime_stop_and_restart_write_request_for_active_owner() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_ownership_marker_path(&bundle.context.data_dir);
        let now = now_millis();
        write_runtime_marker_atomically(
            &marker_path,
            &runtime_ownership_marker_value(
                std::process::id(),
                now,
                now,
                "runtime-start",
                &config_path,
                &workspace,
            ),
        )?;

        let stopped = runtime_stop(RuntimeStopOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;
        assert_eq!(stopped.status, RuntimeStopOutcomeStatus::RequestWritten);
        assert_eq!(
            runtime_stop_request_observed(&bundle.context.data_dir)?.as_deref(),
            Some("stop")
        );

        let restarted = runtime_restart(RuntimeStopOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })?;
        assert_eq!(restarted.status, RuntimeStopOutcomeStatus::RequestWritten);
        assert_eq!(
            runtime_stop_request_observed(&bundle.context.data_dir)?.as_deref(),
            Some("restart")
        );
        Ok(())
    }

    #[test]
    fn runtime_stop_reports_no_active_or_stale_owner() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let no_owner = runtime_stop(RuntimeStopOptions {
            config_path: Some(config_path.clone()),
            workspace_override: None,
        })?;
        assert_eq!(no_owner.status, RuntimeStopOutcomeStatus::NoActiveOwner);

        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        write_runtime_marker_atomically(
            &runtime_ownership_marker_path(&bundle.context.data_dir),
            &runtime_ownership_marker_value(
                999_999,
                now_millis(),
                now_millis(),
                "run",
                &config_path,
                &workspace,
            ),
        )?;
        let stale = runtime_stop(RuntimeStopOptions {
            config_path: Some(config_path),
            workspace_override: None,
        })?;
        assert_eq!(stale.status, RuntimeStopOutcomeStatus::StaleOwnerOnly);
        Ok(())
    }

    #[test]
    fn runtime_compatibility_classification_and_admission() {
        assert_eq!(
            evaluate_runtime_compatibility(RUNTIME_DATA_SCHEMA_VERSION),
            RuntimeCompatibility::FullyCompatible
        );
        assert_eq!(
            evaluate_runtime_compatibility(RUNTIME_DATA_SCHEMA_VERSION + 1),
            RuntimeCompatibility::InspectOnly
        );
        assert_eq!(
            evaluate_runtime_compatibility_with_bounds(1, 2, 1),
            RuntimeCompatibility::MigrationRequired
        );
        assert_eq!(
            evaluate_runtime_compatibility(0),
            RuntimeCompatibility::Incompatible
        );
        assert!(
            guard_runtime_compatibility_for_admission(RuntimeCompatibility::FullyCompatible)
                .is_ok()
        );
        for compatibility in [
            RuntimeCompatibility::MigrationRequired,
            RuntimeCompatibility::InspectOnly,
            RuntimeCompatibility::Incompatible,
        ] {
            assert!(guard_runtime_compatibility_for_admission(compatibility).is_err());
        }
    }

    #[test]
    fn runtime_migration_required_marker_blocks_admission() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let workspace = root.path().join("workspace");
        let mut config = Config::default();
        config.agents.defaults.workspace = workspace.to_string_lossy().to_string();
        save_config_to_path(&config, &config_path)?;
        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path.clone()),
                workspace_override: None,
                resolve_env: false,
                write_back_migrations: false,
            },
            &BTreeMap::<String, String>::new(),
        )?;
        let marker_path = runtime_update_marker_path(&bundle.context.data_dir);
        let mut marker = runtime_update_marker_value(VERSION, "completed_cleanup", None);
        marker["migrationRequired"] = json!(true);
        write_runtime_marker_atomically(&marker_path, &marker)?;
        let error = load_runtime_config(RuntimeConfigOptions {
            config_path: Some(config_path),
            workspace_override: None,
            resolve_env: false,
        })
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(error.contains("runtime mutation blocked"));
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
    fn runtime_commands_are_implemented_or_explicitly_rejected() -> Result<(), Box<dyn Error>> {
        assert!(matches!(
            parse_cli_args(["channels", "status"]),
            Ok(CliCommand::Channels(ChannelsCommand::Status(_)))
        ));
        assert!(matches!(
            parse_cli_args(["plugins", "list"]),
            Ok(CliCommand::Plugins(PluginsCommand::List(_)))
        ));
        let plugins_error = parse_cli_args(["plugins"]).unwrap_err().to_string();
        assert!(plugins_error.contains("plugins requires"));
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
        assert!(help.contains("web       Start the Web UI"));
        assert!(help.contains("runtime   Start, stop, restart"));
        assert!(help.contains("ask       Send one message"));
        assert!(help.contains("agent     Alias"));
        assert!(help.contains("provider  Manage provider auth"));
        assert!(help.contains("generic import-key"));
        assert!(help.contains("apps      Init authoring drafts"));
        assert!(help.contains("channels  List channel"));
        assert!(!help.contains("channels  Reserved"));
        assert!(help.contains("-m, --message"));
        assert!(help.contains("--temperature"));
        assert!(help.contains("--max-tokens"));
        assert!(help.contains("--gateway-port"));
        assert!(help.contains("--websocket-host"));
        assert!(help.contains("--token-stdin"));
        assert!(help.contains("--token-env <var>"));
        assert!(help.contains(
            "--verbose         Print runtime preview logs for run/web and serve diagnostics"
        ));
    }

    #[test]
    fn runtime_verbose_preview_helpers_truncate_input_response_and_tool_args() {
        let input = InboundMessage::new("websocket", "user-1", "chat-1", "a".repeat(81));
        assert_eq!(
            runtime_message_preview(&input),
            format!("{}…", "a".repeat(80))
        );

        let response = LlmResponse {
            content: Some("b".repeat(121)),
            usage: BTreeMap::from([
                ("prompt_tokens".to_owned(), 11),
                ("completion_tokens".to_owned(), 7),
                ("cached_tokens".to_owned(), 3),
            ]),
            ..LlmResponse::default()
        };
        assert_eq!(
            runtime_response_preview(&response),
            format!("{}…", "b".repeat(120))
        );
        assert_eq!(
            runtime_usage_preview(&response),
            "prompt=11 completion=7 cached=3"
        );

        let tool_args = json!({"text": "c".repeat(250)});
        let preview = runtime_tool_args_preview("message", &tool_args);
        assert!(preview.ends_with('…'));
        assert_eq!(preview.chars().count(), 201);
    }

    #[test]
    fn runtime_verbose_preview_helpers_redact_before_truncating() {
        let input = InboundMessage::new(
            "websocket",
            "user-1",
            "chat-1",
            "OPENAI_API_KEY=sk-secret-token visible text",
        );
        let preview = runtime_message_preview(&input);
        assert!(!preview.contains("sk-secret-token"));
        assert!(preview.contains("[REDACTED]") || preview.contains("OPENAI_API_KEY"));

        let response = LlmResponse {
            content: Some("Authorization: Bearer ghp_secret_token after".to_owned()),
            ..LlmResponse::default()
        };
        let preview = runtime_response_preview(&response);
        assert!(!preview.contains("ghp_secret_token"));

        let tool_args = json!({
            "api_key": "plain-secret",
            "query": "visible"
        });
        let preview = runtime_tool_args_preview("web_search", &tool_args);
        assert!(!preview.contains("plain-secret"));
        assert!(preview.contains("visible"));

        let bridge_args = json!({
            "name": "mcp_demo",
            "arguments": {"query": "RAW_NESTED_ARGUMENT"}
        });
        let preview = runtime_tool_args_preview("tool_call", &bridge_args);
        assert!(preview.contains("mcp_demo"));
        assert!(!preview.contains("RAW_NESTED_ARGUMENT"));
        assert!(!preview.contains("arguments"));
    }

    #[test]
    fn agent_loop_adapter_builder_sets_runtime_verbose_flag() {
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace: PathBuf::from("/tmp/workspace"),
            media_dir: PathBuf::from("/tmp/data/media/api"),
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        assert!(!adapter.runtime_verbose);
        let adapter = adapter.with_runtime_verbose(true);
        assert!(adapter.runtime_verbose);
    }

    #[test]
    fn agent_loop_adapter_loop_config_carries_tool_search_config() {
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace: PathBuf::from("/tmp/workspace"),
            media_dir: PathBuf::from("/tmp/data/media/api"),
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            tool_search: ToolSearchConfig {
                enabled: ToolSearchMode::On,
                threshold_pct: 42,
                search_default_limit: 3,
                max_search_limit: 9,
            },
            containment_snapshot: Some(ContainmentSnapshotRef {
                contained: Some(true),
                digest: Some("contained-digest".to_owned()),
                summary: Some("contained summary".to_owned()),
            }),
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: shacs_config::PermissionMode::Auto,
                source: Some("user_local_config".to_owned()),
                scope_ref: None,
            },
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let config = adapter.loop_config();
        assert_eq!(config.tool_search.enabled, ToolSearchMode::On);
        assert_eq!(config.tool_search.threshold_pct, 42);
        assert_eq!(config.tool_search.search_default_limit, 3);
        assert_eq!(config.tool_search.max_search_limit, 9);
        assert_eq!(
            config
                .containment_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.contained),
            Some(true)
        );
        assert_eq!(
            config.permission_mode_snapshot.mode,
            shacs_config::PermissionMode::Auto
        );
        assert_eq!(
            config.permission_mode_snapshot.source.as_deref(),
            Some("user_local_config")
        );
    }

    #[test]
    fn external_typing_indicator_targets_discord_channel_or_thread_only(
    ) -> Result<(), Box<dyn Error>> {
        let discord = DiscordInbound {
            sender_id: "user-1".to_owned(),
            channel_id: "parent-channel".to_owned(),
            content: "hi".to_owned(),
            message_id: Some("message-1".to_owned()),
            guild_id: Some("guild-1".to_owned()),
            parent_channel_id: Some("parent-channel".to_owned()),
            thread_id: Some("thread-channel".to_owned()),
            attachments: Vec::new(),
        }
        .into_message();

        let typing = external_typing_indicator_message(&discord)
            .ok_or("expected Discord typing indicator")?;
        assert_eq!(typing.channel, DISCORD_CHANNEL);
        assert_eq!(typing.chat_id, "thread-channel");
        assert!(typing.content.is_empty());
        assert!(message_is_typing_indicator(&typing));
        assert_eq!(
            discord_typing_url(&typing.chat_id),
            "https://discord.com/api/v10/channels/thread-channel/typing"
        );

        let slack = SlackInbound {
            user_id: "user-1".to_owned(),
            channel_id: "C123".to_owned(),
            content: "hi".to_owned(),
            event_ts: Some("1710000000.000100".to_owned()),
            thread_ts: Some("1710000000.000001".to_owned()),
            channel_type: Some("channel".to_owned()),
            files: Vec::new(),
        }
        .into_message();
        assert!(external_typing_indicator_message(&slack).is_none());
        Ok(())
    }

    #[test]
    fn external_typing_indicator_refreshes_until_turn_finishes() -> Result<(), Box<dyn Error>> {
        let mut metadata = Map::new();
        metadata.insert(EXTERNAL_TYPING_INDICATOR_KEY.to_owned(), Value::Bool(true));
        let message =
            OutboundMessage::new(DISCORD_CHANNEL, "channel-1", "").with_metadata(metadata);
        let runtime_bus = MessageBus::new();
        let mut active = vec![ExternalTypingIndicator {
            session_key: "discord:channel-1".to_owned(),
            message,
            turn_active: true,
            subagent_runtimes: Vec::new(),
            next_at: Instant::now() - Duration::from_secs(1),
        }];

        assert!(publish_due_external_typing_indicators(
            &mut active,
            &runtime_bus
        ));
        let outbound = runtime_bus
            .try_consume_outbound()
            .ok_or("missing refreshed typing indicator")?;
        assert!(message_is_typing_indicator(&outbound));
        assert!(active[0].next_at > Instant::now());

        finish_external_typing_indicator(&mut active, "discord:channel-1", None);
        assert!(active.is_empty());
        Ok(())
    }

    #[test]
    fn external_typing_indicator_survives_parent_finish_until_subagent_finishes(
    ) -> Result<(), Box<dyn Error>> {
        let mut metadata = Map::new();
        metadata.insert(EXTERNAL_TYPING_INDICATOR_KEY.to_owned(), Value::Bool(true));
        let message =
            OutboundMessage::new(DISCORD_CHANNEL, "channel-1", "").with_metadata(metadata);
        let runtime = SubagentRuntime::new();
        let outcome = runtime.spawn_from_request(shacs_core::tools::SpawnRequest {
            task: "Inspect workspace".to_owned(),
            label: Some("inspect".to_owned()),
            origin_channel: DISCORD_CHANNEL.to_owned(),
            origin_chat_id: "channel-1".to_owned(),
            session_key: "discord:channel-1".to_owned(),
        })?;
        let mut active = vec![ExternalTypingIndicator {
            session_key: "discord:channel-1".to_owned(),
            message,
            turn_active: true,
            subagent_runtimes: Vec::new(),
            next_at: Instant::now() - Duration::from_secs(1),
        }];

        finish_external_typing_indicator(&mut active, "discord:channel-1", Some(&runtime));

        if active.len() != 1 || active[0].turn_active || active[0].subagent_runtimes.len() != 1 {
            return Err(
                format!("typing indicator did not track subagent: {}", active.len()).into(),
            );
        }
        let runtime_bus = MessageBus::new();
        assert!(publish_due_external_typing_indicators(
            &mut active,
            &runtime_bus
        ));
        let outbound = runtime_bus
            .try_consume_outbound()
            .ok_or("missing subagent typing refresh")?;
        assert!(message_is_typing_indicator(&outbound));

        let result = ChildResultEnvelope::from_spawn(
            &outcome.envelope,
            ChildResultStatus::Completed,
            "done",
        );
        runtime.publish_child_result(result);
        active[0].next_at = Instant::now() - Duration::from_secs(1);

        assert!(!publish_due_external_typing_indicators(
            &mut active,
            &runtime_bus
        ));
        assert!(active.is_empty());
        assert!(runtime_bus.try_consume_outbound().is_none());
        Ok(())
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
        let media_root = root.path().join("data").join("media");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir: media_dir.clone(),
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let paths = adapter.persist_media_data_urls(&[
            "data:text/plain;base64,aGk=".to_owned(),
            "data:audio/mpeg;base64,SUQz".to_owned(),
        ])?;

        assert_eq!(paths.len(), 2);
        let attachment_dir = media_root.join("attachments").join("api").canonicalize()?;
        let stored_paths = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert!(stored_paths
            .iter()
            .all(|stored_path| stored_path.is_absolute()
                && stored_path.starts_with(&attachment_dir)
                && !stored_path.starts_with(&media_dir)));
        assert!(stored_paths.iter().any(|stored_path| stored_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("-upload.bin"))));
        let stored_contents = stored_paths
            .iter()
            .map(fs::read)
            .collect::<Result<Vec<_>, _>>()?;
        assert!(stored_contents.iter().any(|content| content == b"hi"));
        assert!(stored_contents.iter().any(|content| content == b"ID3"));

        let upload_path = adapter.persist_uploaded_file(Some("note.txt"), b"uploaded")?;
        let upload_path = PathBuf::from(upload_path);
        assert!(upload_path.is_absolute());
        assert!(upload_path.starts_with(&attachment_dir));
        assert!(upload_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("-note.txt")));
        assert_eq!(fs::read_to_string(upload_path)?, "uploaded");

        let error = adapter
            .persist_media_data_urls(&["data:text/plain;base64,%%%".to_owned()])
            .expect_err("malformed data URL should fail");
        assert_eq!(error.status, 415);
        assert_eq!(error.error_type, "unsupported_media_type");
        assert!(!error.message.contains("%%%"));

        let error = adapter
            .persist_media_data_urls(&["data:application/x-sh;base64,aGk=".to_owned()])
            .expect_err("unsupported media type should fail");
        assert_eq!(error.status, 415);
        assert!(error.message.contains("application/x-sh"));
        Ok(())
    }

    #[test]
    fn websocket_frame_bridge_processes_message_through_agent_loop_without_socket(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_root = root.path().join("data").join("media");
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
            native_image_input_supported: true,
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
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
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
            },]
        );
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        let request_json = serde_json::to_string(&requests[0].messages)?;
        assert!(request_json.contains("Channel: websocket"));
        assert!(request_json.contains("Chat ID: chat-b"));
        assert!(request_json.contains("hello from websocket"));
        assert!(request_json.contains("[attachment:included_text]"));
        assert!(request_json.contains("hi"));
        let attachment_dir = media_root.join("attachments").join("websocket");
        let stored_files = fs::read_dir(&attachment_dir)?.collect::<Result<Vec<_>, _>>()?;
        assert_eq!(stored_files.len(), 1);
        assert!(stored_files[0]
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("-a.txt")));
        assert_eq!(fs::read_to_string(stored_files[0].path())?, "hi");
        assert!(fs::read_dir(&media_dir).is_err());
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
    fn websocket_frame_bridge_rejects_malformed_media_without_agent_loop(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_root = root.path().join("data").join("media");
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
            native_image_input_supported: true,
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
            media_dir: media_dir.clone(),
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let error = adapter
            .process_websocket_frame(
                json!({
                    "type": "message",
                    "chat_id": "chat-b",
                    "text": "hello from websocket",
                    "media": [{ "data_url": "data:text/plain;base64,%%%", "name": "bad.txt" }]
                }),
                "client-1",
                "chat-a",
            )
            .expect_err("malformed websocket media should fail");

        assert_eq!(error.status, 415);
        assert_eq!(error.error_type, "unsupported_media_type");
        assert!(!error.message.contains("%%%"));
        assert!(fs::read_dir(media_root.join("attachments").join("websocket")).is_err());
        assert!(fs::read_dir(&media_dir).is_err());
        assert!(captured
            .lock()
            .map_err(|_| "captured lock poisoned")?
            .is_empty());
        Ok(())
    }

    #[test]
    fn websocket_frame_bridge_emits_coalesced_delta_and_stream_end() -> Result<(), Box<dyn Error>> {
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
            native_image_input_supported: true,
            client: Arc::new(StreamingProviderClient {
                captured: captured.clone(),
                response: LlmResponse {
                    content: Some("hello".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                },
                events: vec![
                    ProviderEvent::TextDelta {
                        text: "hel".to_owned(),
                    },
                    ProviderEvent::TextDelta {
                        text: "lo".to_owned(),
                    },
                    ProviderEvent::Finish {
                        usage: json!({}),
                        reason: "stop".to_owned(),
                    },
                ],
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let events = adapter.process_websocket_frame(
            json!({
                "type": "message",
                "chat_id": "chat-stream",
                "text": "stream please"
            }),
            "client-1",
            "chat-a",
        )?;

        assert!(matches!(
            &events[0],
            WebSocketServerEvent::Delta { chat_id, text, stream_id }
                if chat_id == "chat-stream" && text == "hello" && stream_id.is_some()
        ));
        assert!(matches!(
            &events[1],
            WebSocketServerEvent::StreamEnd { chat_id, stream_id }
                if chat_id == "chat-stream" && stream_id.is_some()
        ));
        assert!(matches!(
            events.last(),
            Some(WebSocketServerEvent::Message { chat_id, text, .. })
                if chat_id == "chat-stream" && text == "hello"
        ));
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 1);
        Ok(())
    }

    #[test]
    fn external_stream_events_publish_coalesced_delta_and_stream_end() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults::default(),
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };
        let (event_tx, event_rx) = mpsc::channel();
        event_tx.send(ProviderEvent::TextDelta {
            text: "he".to_owned(),
        })?;
        event_tx.send(ProviderEvent::TextDelta {
            text: "llo".to_owned(),
        })?;
        event_tx.send(ProviderEvent::Finish {
            usage: json!({}),
            reason: "stop".to_owned(),
        })?;
        drop(event_tx);
        let bus = MessageBus::new();
        let routing_metadata = Map::new();

        adapter.publish_external_stream_events(
            event_rx,
            ExternalStreamRouting {
                channel: TELEGRAM_CHANNEL,
                chat_id: "chat-stream",
                stream_id: "stream-1",
                metadata: &routing_metadata,
                reply_to: None,
            },
            &bus,
        )?;

        let delta = bus.try_consume_outbound().ok_or("missing delta")?;
        assert_eq!(delta.channel, TELEGRAM_CHANNEL);
        assert_eq!(delta.chat_id, "chat-stream");
        assert_eq!(delta.content, "hello");
        assert_eq!(delta.metadata["_stream_delta"], json!(true));
        assert_eq!(delta.metadata["_stream_id"], json!("stream-1"));
        let end = bus.try_consume_outbound().ok_or("missing stream end")?;
        assert_eq!(end.channel, TELEGRAM_CHANNEL);
        assert_eq!(end.chat_id, "chat-stream");
        assert_eq!(end.content, "");
        assert_eq!(end.metadata["_stream_end"], json!(true));
        assert!(bus.try_consume_outbound().is_none());
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
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: captured.clone(),
                response: LlmResponse::default(),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
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
            native_image_input_supported: true,
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
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
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
            native_image_input_supported: true,
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
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
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

    #[cfg(unix)]
    #[test]
    fn spec025_plugin_runtime_hook_executes_during_direct_agent_loop() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        let plugin_root = root.path().join("plugins").join("observer");
        let bin_dir = plugin_root.join("bin");
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(&bin_dir)?;
        let marker_path = root.path().join("hook-marker.json");
        let hook_path = bin_dir.join("observe");
        fs::write(
            &hook_path,
            "#!/bin/sh\nprintf '{\"event\":\"llm:after\",\"plugin_id\":\"observer\"}' > \"$1\"\nprintf '{\"diagnostic\":{\"message\":\"observed\"}}'\n",
        )?;
        let mut permissions = fs::metadata(&hook_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook_path, permissions)?;
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
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: captured.clone(),
                response: LlmResponse {
                    content: Some("direct ok".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,
            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot {
                plugins: vec![PluginRuntimePlugin {
                    id: "observer".to_owned(),
                    root: plugin_root,
                    manifest_digest: Some("sha256:test".to_owned()),
                    source: PluginManifestSource::UserData,
                    hooks: vec![PluginRuntimeHook {
                        plugin_id: "observer".to_owned(),
                        event: PluginHookEvent::LlmAfter,
                        event_name: "llm:after".to_owned(),
                        command: PluginExecutableCommand {
                            command_path: hook_path,
                            args: vec![marker_path.to_string_lossy().to_string()],
                            timeout_ms: 1_000,
                        },
                    }],
                }],
                diagnostics: Vec::new(),
            },
        };

        let inbound = InboundMessage::new("cli", "user", "direct", "hello")
            .with_session_key_override("plugin-hook");
        let (turn, outbound) =
            adapter.process_inbound_with_outbound(inbound, adapter.loop_config(), None, &[])?;
        let output =
            render_direct_turn_content(turn.final_content.unwrap_or_default(), outbound.clone());

        assert_eq!(output, "direct ok");
        let marker = fs::read_to_string(marker_path)?;
        assert!(marker.contains("\"event\":\"llm:after\""), "{marker}");
        assert!(marker.contains("\"plugin_id\":\"observer\""), "{marker}");
        let notification = outbound.iter().find(|message| {
            message
                .metadata
                .get("runtime_notification")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                == Some("plugin_hook")
        });
        let Some(notification) = notification else {
            return Err("missing plugin hook dispatch diagnostic notification".into());
        };
        assert!(!is_visible_runtime_notification(notification));
        assert_eq!(notification.content, "Plugin hook diagnostics recorded");
        let summary = notification
            .metadata
            .get("runtime_notification")
            .and_then(|value| value.get("summary"))
            .ok_or("missing plugin hook summary")?;
        assert_eq!(summary.get("event"), Some(&json!("llm:after")));
        assert_eq!(summary.get("dispatch_count"), Some(&json!(1)));
        assert_eq!(summary.get("success_count"), Some(&json!(1)));
        assert_eq!(summary.get("observed_count"), Some(&json!(1)));
        assert_eq!(
            captured.lock().map_err(|_| "captured lock poisoned")?.len(),
            1
        );
        Ok(())
    }

    #[test]
    fn direct_message_renders_selected_skill_notification_before_final_answer_once(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        fs::create_dir_all(workspace.join("skills/weather"))?;
        fs::write(
            workspace.join("skills/weather/SKILL.md"),
            "---\ndescription: Weather skill\n---\nWeather body",
        )?;
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut arguments = Map::new();
        arguments.insert("path".to_owned(), json!("skills/weather/SKILL.md"));
        let mut tools = ToolRegistry::new();
        tools.register(ReadFileTool::new(PathContext {
            workspace: Some(workspace.clone()),
            allowed_dir: Some(workspace.clone()),
            media_dir: Some(media_dir.clone()),
            extra_allowed_dirs: Vec::new(),
        }));
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 2,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(SequentialProviderClient {
                captured: captured.clone(),
                responses: Mutex::new(VecDeque::from([
                    LlmResponse {
                        tool_calls: vec![ToolCallRequest::new(
                            "call-read-skill",
                            "read_file",
                            arguments,
                        )],
                        finish_reason: "tool_calls".to_owned(),
                        ..LlmResponse::default()
                    },
                    LlmResponse {
                        content: Some("final answer".to_owned()),
                        finish_reason: "stop".to_owned(),
                        ..LlmResponse::default()
                    },
                ])),
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools,
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let output = complete_direct_message(
            &adapter,
            &AskOptions {
                message: "weather in Seoul".to_owned(),
                session: Some("work".to_owned()),
                ..AskOptions::default()
            },
        )?;

        let skill_index = output
            .find("Using skill: weather")
            .ok_or("missing direct selected skill notification")?;
        let final_index = output.find("final answer").ok_or("missing final answer")?;
        assert!(skill_index < final_index, "output: {output}");
        assert_eq!(output.matches("Using skill: weather").count(), 1);
        assert!(!output.contains("Using tool:"));
        assert_eq!(output.matches("final answer").count(), 1);
        let requests = captured.lock().map_err(|_| "captured lock poisoned")?;
        assert_eq!(requests.len(), 2);
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
                native_image_input_supported: true,
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
                message_tool: None,
                _mcp_runtime: None,
                _mcp_reports: Vec::new(),
                allow_side_effect_tools: false,
                send_progress: true,
                send_tool_hints: false,
                send_max_retries: 0,
                session_turn_lock: SessionTurnLock::new(),
                exec_timeout_seconds: 60,
                exec_sandbox: None,
                exec_path_append: None,
                exec_allowed_env_keys: Vec::new(),
                exec_env: BTreeMap::new(),
                runtime_verbose: false,

                tool_search: ToolSearchConfig::default(),
                containment_snapshot: None,
                permission_mode_snapshot: PermissionModeSnapshot::default(),
                plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
            },
            lifecycle_hooks: Vec::new(),
            observability_hooks: Vec::new(),
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
    fn programmatic_facade_emits_lifecycle_hooks_around_sdk_run() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_hook = events.clone();
        let hook: ShacsBotLifecycleHook = Arc::new(move |event| {
            if let Ok(mut events) = events_for_hook.lock() {
                events.push(event.clone());
            }
        });
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
                native_image_input_supported: true,
                client: Arc::new(FakeProviderClient {
                    captured: Arc::new(Mutex::new(Vec::new())),
                    response: LlmResponse {
                        content: Some("sdk ok".to_owned()),
                        finish_reason: "stop".to_owned(),
                        ..LlmResponse::default()
                    },
                }),
                retry_mode: ProviderRetryMode::Standard,
                workspace,
                media_dir,
                tools: ToolRegistry::new(),
                message_tool: None,
                _mcp_runtime: None,
                _mcp_reports: Vec::new(),
                allow_side_effect_tools: false,
                send_progress: true,
                send_tool_hints: false,
                send_max_retries: 0,
                session_turn_lock: SessionTurnLock::new(),
                exec_timeout_seconds: 60,
                exec_sandbox: None,
                exec_path_append: None,
                exec_allowed_env_keys: Vec::new(),
                exec_env: BTreeMap::new(),
                runtime_verbose: false,

                tool_search: ToolSearchConfig::default(),
                containment_snapshot: None,
                permission_mode_snapshot: PermissionModeSnapshot::default(),
                plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
            },
            lifecycle_hooks: vec![hook],
            observability_hooks: Vec::new(),
        };

        let result = bot.run_with_options(ShacsBotRunOptions {
            message: "hello from sdk".to_owned(),
            session_key: "sdk:hooks".to_owned(),
            ..ShacsBotRunOptions::default()
        })?;
        assert_eq!(result.content, "sdk ok");
        drop(bot);

        let events = events.lock().map_err(|_| "events lock poisoned")?;
        assert!(matches!(
            events.first(),
            Some(ShacsBotLifecycleEvent::RunStarted { session_key }) if session_key == "sdk:hooks"
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            ShacsBotLifecycleEvent::RunCompleted { session_key, stop_reason }
                if session_key == "sdk:hooks" && stop_reason == "stop"
        )));
        assert!(matches!(
            events.last(),
            Some(ShacsBotLifecycleEvent::Shutdown)
        ));
        Ok(())
    }

    #[test]
    fn programmatic_facade_lifecycle_hook_panic_does_not_abort_run() -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_hook = events.clone();
        let recording_hook: ShacsBotLifecycleHook = Arc::new(move |event| {
            if let Ok(mut events) = events_for_hook.lock() {
                events.push(event.clone());
            }
        });
        let panic_hook: ShacsBotLifecycleHook =
            Arc::new(|_| panic!("hook panic should be isolated"));
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
                native_image_input_supported: true,
                client: Arc::new(FakeProviderClient {
                    captured: Arc::new(Mutex::new(Vec::new())),
                    response: LlmResponse {
                        content: Some("sdk ok".to_owned()),
                        finish_reason: "stop".to_owned(),
                        ..LlmResponse::default()
                    },
                }),
                retry_mode: ProviderRetryMode::Standard,
                workspace,
                media_dir,
                tools: ToolRegistry::new(),
                message_tool: None,
                _mcp_runtime: None,
                _mcp_reports: Vec::new(),
                allow_side_effect_tools: false,
                send_progress: true,
                send_tool_hints: false,
                send_max_retries: 0,
                session_turn_lock: SessionTurnLock::new(),
                exec_timeout_seconds: 60,
                exec_sandbox: None,
                exec_path_append: None,
                exec_allowed_env_keys: Vec::new(),
                exec_env: BTreeMap::new(),
                runtime_verbose: false,

                tool_search: ToolSearchConfig::default(),
                containment_snapshot: None,
                permission_mode_snapshot: PermissionModeSnapshot::default(),
                plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
            },
            lifecycle_hooks: vec![recording_hook, panic_hook],
            observability_hooks: Vec::new(),
        };

        let result = bot.run("hello from sdk")?;
        assert_eq!(result.content, "sdk ok");
        let error = bot.run_with_options(ShacsBotRunOptions {
            message: "   ".to_owned(),
            session_key: "sdk:bad".to_owned(),
            ..ShacsBotRunOptions::default()
        });
        assert!(error.is_err());

        let events = events.lock().map_err(|_| "events lock poisoned")?;
        assert!(events.iter().any(|event| matches!(
            event,
            ShacsBotLifecycleEvent::RunFailed { session_key, error }
                if session_key == "sdk:bad" && error.contains("non-empty message")
        )));
        Ok(())
    }

    #[test]
    fn programmatic_facade_emits_provider_and_tool_observability_events(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_hook = events.clone();
        let recording_hook: ShacsBotObservabilityHook = Arc::new(move |event| {
            if let Ok(mut events) = events_for_hook.lock() {
                events.push(event.clone());
            }
        });
        let panic_hook: ShacsBotObservabilityHook =
            Arc::new(|_| panic!("observability hook panic should be isolated"));
        let mut arguments = Map::new();
        arguments.insert("path".to_owned(), json!("README.md"));
        let mut tools = ToolRegistry::new();
        tools.register(ErrorArtifactTool);
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
                native_image_input_supported: true,
                client: Arc::new(StreamingProviderClient {
                    captured: Arc::new(Mutex::new(Vec::new())),
                    response: LlmResponse {
                        content: Some("checking file".to_owned()),
                        tool_calls: vec![ToolCallRequest::new(
                            "call-1",
                            "error_artifact",
                            arguments.clone(),
                        )],
                        finish_reason: "tool_calls".to_owned(),
                        ..LlmResponse::default()
                    },
                    events: vec![
                        ProviderEvent::TextDelta {
                            text: "checking".to_owned(),
                        },
                        ProviderEvent::Finish {
                            usage: json!({"prompt_tokens": 1}),
                            reason: "tool_calls".to_owned(),
                        },
                    ],
                }),
                retry_mode: ProviderRetryMode::Standard,
                workspace,
                media_dir,
                tools,
                message_tool: None,
                _mcp_runtime: None,
                _mcp_reports: Vec::new(),
                allow_side_effect_tools: false,
                send_progress: true,
                send_tool_hints: false,
                send_max_retries: 0,
                session_turn_lock: SessionTurnLock::new(),
                exec_timeout_seconds: 60,
                exec_sandbox: None,
                exec_path_append: None,
                exec_allowed_env_keys: Vec::new(),
                exec_env: BTreeMap::new(),
                runtime_verbose: false,

                tool_search: ToolSearchConfig::default(),
                containment_snapshot: None,
                permission_mode_snapshot: PermissionModeSnapshot::default(),
                plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
            },
            lifecycle_hooks: Vec::new(),
            observability_hooks: vec![panic_hook],
        };

        let result = bot.run_with_options_and_observability_hooks(
            ShacsBotRunOptions {
                message: "hello from sdk".to_owned(),
                session_key: "sdk:observability".to_owned(),
                ..ShacsBotRunOptions::default()
            },
            vec![recording_hook],
        )?;

        assert_eq!(result.tools_used, vec!["error_artifact"]);
        let events = events.lock().map_err(|_| "events lock poisoned")?;
        assert!(events.iter().any(|event| match event {
            ShacsBotObservabilityEvent::Provider { event } => matches!(
                event.as_ref(),
                ProviderEvent::TextDelta { text } if text == "checking"
            ),
            ShacsBotObservabilityEvent::Tool { .. } => false,
        }));
        let start = events.iter().find_map(|event| match event {
            ShacsBotObservabilityEvent::Tool {
                event,
                payload: Some(payload),
            } if event.name == "error_artifact" && payload.phase == "start" => Some(payload),
            _ => None,
        });
        let Some(start) = start else {
            return Err("missing tool start observability payload".into());
        };
        assert_eq!(start.call_id, "call-1");
        assert_eq!(start.arguments.get("path"), Some(&json!("README.md")));
        let finish = events.iter().find_map(|event| match event {
            ShacsBotObservabilityEvent::Tool {
                event,
                payload: Some(payload),
            } if event.name == "error_artifact" && payload.phase == "error" => Some(payload),
            _ => None,
        });
        let Some(finish) = finish else {
            return Err("missing tool finish observability payload".into());
        };
        assert_eq!(finish.call_id, "call-1");
        assert!(finish
            .error
            .as_deref()
            .is_some_and(|error| error.contains("intentional artifact failure")));
        Ok(())
    }

    #[test]
    fn tool_observability_projects_bridge_arguments_for_start_and_pending_finish(
    ) -> Result<(), Box<dyn Error>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_hook = events.clone();
        let recording_hook: ShacsBotObservabilityHook = Arc::new(move |event| {
            if let Ok(mut events) = events_for_hook.lock() {
                events.push(event.clone());
            }
        });
        let pending = Arc::new(Mutex::new(BTreeMap::new()));
        let start_hook =
            ObservabilityToolStartHook::new(vec![recording_hook.clone()], pending.clone());
        let context = AgentHookContext {
            iteration: 0,
            messages: Vec::new(),
        };
        start_hook.before_execute_tools(
            &context,
            &[RuntimeToolCall::new(
                "bridge-call-1",
                "tool_call",
                json!({
                    "name": "mcp_parent_only",
                    "arguments": {
                        "token": "RAW_BRIDGE_SECRET",
                        "rawNested": "RAW_NESTED_ARGUMENT"
                    }
                }),
            )],
        );
        let callback = observability_tool_callback(&[recording_hook], pending)
            .ok_or("missing observability callback")?;
        callback(&ToolEvent {
            name: "tool_call".to_owned(),
            status: ToolStatus::Ok,
            detail: "mapped".to_owned(),
            call_id: None,
            arguments: None,
            result: Some(json!({"ok": true})),
        });

        let events = events.lock().map_err(|_| "events lock poisoned")?;
        let serialized = format!("{events:?}");
        if serialized.contains("RAW_BRIDGE_SECRET")
            || serialized.contains("RAW_NESTED_ARGUMENT")
            || serialized.contains("rawNested")
        {
            return Err(format!("bridge observability leaked raw arguments: {serialized}").into());
        }
        let start = events.iter().find_map(|event| match event {
            ShacsBotObservabilityEvent::Tool {
                event,
                payload: Some(payload),
            } if event.name == "tool_call" && payload.phase == "start" => Some(payload),
            _ => None,
        });
        let finish = events.iter().find_map(|event| match event {
            ShacsBotObservabilityEvent::Tool {
                event,
                payload: Some(payload),
            } if event.name == "tool_call" && payload.phase == "end" => Some(payload),
            _ => None,
        });
        let Some(start) = start else {
            return Err("missing bridge start payload".into());
        };
        let Some(finish) = finish else {
            return Err("missing bridge finish payload".into());
        };
        assert_eq!(start.arguments, json!({"name": "mcp_parent_only"}));
        assert_eq!(finish.arguments, json!({"name": "mcp_parent_only"}));
        Ok(())
    }

    #[test]
    fn programmatic_facade_tool_observability_preserves_json_result_extras(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_for_hook = events.clone();
        let recording_hook: ShacsBotObservabilityHook = Arc::new(move |event| {
            if let Ok(mut events) = events_for_hook.lock() {
                events.push(event.clone());
            }
        });
        let mut arguments = Map::new();
        arguments.insert("path".to_owned(), json!("artifact.txt"));
        let mut tools = ToolRegistry::new();
        tools.register(JsonArtifactTool);
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
                native_image_input_supported: true,
                client: Arc::new(StreamingProviderClient {
                    captured: Arc::new(Mutex::new(Vec::new())),
                    response: LlmResponse {
                        content: Some("making artifact".to_owned()),
                        tool_calls: vec![ToolCallRequest::new(
                            "call-json",
                            "json_artifact",
                            arguments,
                        )],
                        finish_reason: "tool_calls".to_owned(),
                        ..LlmResponse::default()
                    },
                    events: vec![ProviderEvent::Finish {
                        usage: json!({}),
                        reason: "tool_calls".to_owned(),
                    }],
                }),
                retry_mode: ProviderRetryMode::Standard,
                workspace,
                media_dir,
                tools,
                message_tool: None,
                _mcp_runtime: None,
                _mcp_reports: Vec::new(),
                allow_side_effect_tools: false,
                send_progress: true,
                send_tool_hints: false,
                send_max_retries: 0,
                session_turn_lock: SessionTurnLock::new(),
                exec_timeout_seconds: 60,
                exec_sandbox: None,
                exec_path_append: None,
                exec_allowed_env_keys: Vec::new(),
                exec_env: BTreeMap::new(),
                runtime_verbose: false,

                tool_search: ToolSearchConfig::default(),
                containment_snapshot: None,
                permission_mode_snapshot: PermissionModeSnapshot::default(),
                plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
            },
            lifecycle_hooks: Vec::new(),
            observability_hooks: vec![recording_hook],
        };

        let result = bot.run("hello from sdk")?;
        assert_eq!(result.tools_used, vec!["json_artifact"]);
        let events = events.lock().map_err(|_| "events lock poisoned")?;
        let finish = events.iter().find_map(|event| match event {
            ShacsBotObservabilityEvent::Tool {
                event,
                payload: Some(payload),
            } if event.name == "json_artifact" && payload.phase == "end" => Some(payload),
            _ => None,
        });
        let Some(finish) = finish else {
            return Err("missing json_artifact finish payload".into());
        };
        assert_eq!(finish.call_id, "call-json");
        assert_eq!(finish.arguments.get("path"), Some(&json!("artifact.txt")));
        assert_eq!(
            finish.result.as_ref().and_then(|value| value.get("ok")),
            Some(&json!(true))
        );
        assert_eq!(finish.files, vec![json!("artifact.txt")]);
        assert_eq!(finish.embeds, vec![json!({"type": "text"})]);
        Ok(())
    }

    #[test]
    fn skill_usage_notification_content_marks_selected_skills() {
        let routing = Map::new();
        let selected = skill_usage_notification_message(
            "direct",
            "chat-1",
            &routing,
            None,
            &["weather".to_owned()],
        )
        .expect("selected skill notification should be present");
        assert_eq!(selected.content, "Using skill: weather");
        assert_eq!(selected.metadata["runtime_notification"]["kind"], "skill");
        assert_eq!(
            selected.metadata["runtime_notification"]["usage"],
            "selected"
        );

        let multiple = skill_usage_notification_message(
            "direct",
            "chat-1",
            &routing,
            None,
            &["weather".to_owned(), "github".to_owned()],
        )
        .expect("multiple selected skill notification should be present");
        assert_eq!(multiple.content, "Using skills: weather, github");
    }

    #[test]
    fn subagent_start_notification_uses_outcome_content_and_metadata() {
        let routing = Map::from_iter([("thread_id".to_owned(), json!("thread-1"))]);
        let outcome = shacs_core::runtime::SubagentSpawnOutcome {
            envelope: shacs_core::runtime::SpawnEnvelope::from_spawn_request(
                shacs_core::tools::SpawnRequest {
                    task: "draft the summary".to_owned(),
                    label: Some("summary".to_owned()),
                    origin_channel: "discord".to_owned(),
                    origin_chat_id: "channel-1".to_owned(),
                    session_key: "discord:channel-1".to_owned(),
                },
                "child-42".to_owned(),
            ),
            user_message:
                "Subagent [summary] started (id: child-42). I'll notify you when it completes."
                    .to_owned(),
        };
        let notification = subagent_start_notification_message(
            "discord",
            "channel-1",
            &routing,
            Some("message-1"),
            &outcome,
        );

        assert_eq!(notification.content, outcome.user_message);
        assert_eq!(notification.reply_to.as_deref(), Some("message-1"));
        assert_eq!(notification.metadata["thread_id"], json!("thread-1"));
        assert_eq!(
            notification.metadata["runtime_notification"]["kind"],
            "subagent"
        );
        assert_eq!(
            notification.metadata["runtime_notification"]["phase"],
            "start"
        );
        assert_eq!(
            notification.metadata["runtime_notification"]["child_task_id"],
            "child-42"
        );
        assert_eq!(
            notification.metadata["runtime_notification"]["label"],
            "summary"
        );
        assert!(notification.metadata.get("session_key").is_none());
        assert!(notification.metadata.get("stop_reason").is_none());
    }

    #[test]
    fn plugin_hook_dispatch_notification_is_retained_but_not_live_visible() {
        let summary = shacs_core::runtime::summarize_plugin_hook_dispatch(
            PluginHookEvent::LlmAfter,
            vec![PluginHookDispatchAttempt {
                plugin_id: "observer".to_owned(),
                event: PluginHookEvent::LlmAfter,
                timeout_ms: 1_000,
                result: PluginHookCallbackResult::Output(json!({
                    "diagnostic": {"message": "observed"}
                })),
            }],
        );
        let notification = plugin_hook_dispatch_notification_message(
            "direct",
            "chat-1",
            &Map::new(),
            None,
            &summary,
        );

        assert_eq!(notification.content, "Plugin hook diagnostics recorded");
        assert_eq!(
            notification.metadata["runtime_notification"]["kind"],
            "plugin_hook"
        );
        assert_eq!(
            notification.metadata["runtime_notification"]["summary"]["event"],
            "llm:after"
        );
        assert!(!is_visible_runtime_notification(&notification));
        assert!(!should_dispatch_runtime_outbound(&notification));
    }

    #[test]
    fn direct_render_includes_subagent_runtime_notifications_but_not_tools() {
        let mut subagent_metadata = Map::new();
        subagent_metadata.insert(
            "runtime_notification".to_owned(),
            json!({"kind": "subagent", "phase": "start"}),
        );
        let mut tool_metadata = Map::new();
        tool_metadata.insert(
            "runtime_notification".to_owned(),
            json!({"kind": "tool", "phase": "start"}),
        );

        let output = render_direct_turn_content(
            "final answer".to_owned(),
            vec![
                OutboundMessage::new("cli", "direct", "Subagent [summary] started")
                    .with_metadata(subagent_metadata),
                OutboundMessage::new("cli", "direct", "Using tool: spawn")
                    .with_metadata(tool_metadata),
            ],
        );

        assert_eq!(output, "Subagent [summary] started\n\nfinal answer");
    }

    #[test]
    fn process_inbound_with_outbound_does_not_publish_runtime_tool_usage_notifications(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let mut arguments = Map::new();
        arguments.insert("path".to_owned(), json!("artifact.txt"));
        let mut tools = ToolRegistry::new();
        tools.register(JsonArtifactTool);
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse {
                    tool_calls: vec![ToolCallRequest::new(
                        "call-json",
                        "json_artifact",
                        arguments,
                    )],
                    finish_reason: "tool_calls".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools,
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let inbound = InboundMessage::new("direct", "user", "chat-1", "make artifact")
            .with_metadata(Map::from_iter([
                ("message_id".to_owned(), json!("message-1")),
                ("thread_id".to_owned(), json!("thread-1")),
                ("slack".to_owned(), json!({"thread_ts": "171.1"})),
            ]));
        let (_turn, outbound) =
            adapter.process_inbound_with_outbound(inbound, adapter.loop_config(), None, &[])?;
        assert!(!outbound
            .iter()
            .any(|message| message.content == "Using tool: json_artifact"));
        assert!(!outbound.iter().any(|message| message
            .metadata
            .get("runtime_notification")
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str)
            == Some("tool")));
        Ok(())
    }

    #[test]
    fn process_inbound_with_outbound_publishes_subagent_start_notification(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(&workspace)?;
        let mut arguments = Map::new();
        arguments.insert("task".to_owned(), json!("draft the summary"));
        arguments.insert("label".to_owned(), json!("summary"));
        let mut tools = ToolRegistry::new();
        tools.register(JsonArtifactTool);
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse {
                    tool_calls: vec![ToolCallRequest::new("call-spawn", "spawn", arguments)],
                    finish_reason: "tool_calls".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools,
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: shacs_config::PermissionMode::BypassPermissions,
                ..PermissionModeSnapshot::default()
            },
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let inbound = InboundMessage::new("direct", "user", "chat-1", "make summary")
            .with_metadata(Map::from_iter([
                ("message_id".to_owned(), json!("message-1")),
                ("thread_id".to_owned(), json!("thread-1")),
                ("slack".to_owned(), json!({"thread_ts": "171.1"})),
            ]));
        let mut loop_config = adapter.loop_config();
        loop_config.permission_rule_input = PermissionRuleInput {
            containment: DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: Vec::new(),
                network_mode: ContainerNetworkMode::None,
                digest: Some("test-contained".to_owned()),
                summary: Some("non-privileged test containment".to_owned()),
            },
            protected_targets: Vec::new(),
            proc_exec_summary: Some(ProcExecSummary {
                command_family: "spawn".to_owned(),
                target_refs: Vec::new(),
                destructive: false,
                network: false,
                secret_exposure: false,
                summary_available: true,
            }),
        };

        let (_turn, outbound) = adapter.process_inbound_with_outbound_inner(
            inbound,
            loop_config,
            None,
            &[],
            None,
            Some(SubagentRuntime::new()),
        )?;

        let notification = outbound.iter().find(|message| {
            message
                .metadata
                .get("runtime_notification")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                == Some("subagent")
                && message
                    .metadata
                    .get("runtime_notification")
                    .and_then(|value| value.get("phase"))
                    .and_then(Value::as_str)
                    == Some("start")
        });
        let Some(notification) = notification else {
            return Err("missing subagent start notification".into());
        };
        let child_task_id = notification
            .metadata
            .get("runtime_notification")
            .and_then(|value| value.get("child_task_id"))
            .and_then(Value::as_str)
            .ok_or("missing child_task_id")?;
        assert_eq!(
            notification.content,
            format!(
                "Subagent [summary] started (id: {child_task_id}). I'll notify you when it completes."
            )
        );
        assert_eq!(
            notification
                .metadata
                .get("runtime_notification")
                .and_then(|value| value.get("label"))
                .and_then(Value::as_str),
            Some("summary")
        );
        Ok(())
    }

    #[test]
    fn external_active_skill_notification_is_not_published_before_tool_finishes(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(workspace.join("skills/always"))?;
        fs::write(
            workspace.join("skills/always/SKILL.md"),
            "---\ndescription: Always\nalways: true\n---\nAlways body",
        )?;
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let mut tools = ToolRegistry::new();
        tools.register(BlockingArtifactTool { gate: gate.clone() });
        let adapter = Arc::new(AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse {
                    tool_calls: vec![ToolCallRequest::new(
                        "call-blocking",
                        "blocking_artifact",
                        Map::new(),
                    )],
                    finish_reason: "tool_calls".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools,
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        });
        let runtime_bus = MessageBus::new();
        let turn_adapter = adapter.clone();
        let turn_bus = runtime_bus.clone();
        let handle = std::thread::spawn(move || {
            turn_adapter.process_external_inbound_with_streaming(
                InboundMessage::new(TELEGRAM_CHANNEL, "user", "chat-1", "make artifact"),
                turn_adapter.loop_config(),
                &turn_bus,
            )
        });

        let deadline = Instant::now() + Duration::from_millis(300);
        let mut saw_skill_notification = false;
        while Instant::now() < deadline {
            if let Some(message) = runtime_bus.try_consume_outbound() {
                if message.content.contains("always")
                    && message.metadata["runtime_notification"]["kind"] == "skill"
                {
                    saw_skill_notification = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!saw_skill_notification);
        assert!(!handle.is_finished());

        let (lock, cvar) = &*gate;
        *lock.lock().map_err(|_| "gate lock poisoned")? = true;
        cvar.notify_all();
        let result = handle.join().map_err(|_| "external turn thread panicked")?;
        let (_turn, outbound, _subagent_runtime) = result?;
        assert!(!outbound
            .iter()
            .any(|message| message.content == "Using tool: blocking_artifact"));
        Ok(())
    }

    #[test]
    fn process_inbound_with_outbound_does_not_publish_active_skill_notification(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let media_dir = root.path().join("data").join("media").join("api");
        fs::create_dir_all(workspace.join("skills/always"))?;
        fs::write(
            workspace.join("skills/always/SKILL.md"),
            "---\nname: always\nalways: true\ndescription: Always on\n---\nAlways body",
        )?;
        let adapter = AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured: Arc::new(Mutex::new(Vec::new())),
                response: LlmResponse {
                    content: Some("ok".to_owned()),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir,
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: true,
            send_tool_hints: false,
            send_max_retries: 0,
            runtime_verbose: false,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),

            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        };

        let (_first_turn, first_outbound) = adapter.process_inbound_with_outbound(
            InboundMessage::new("direct", "user", "chat-1", "hello"),
            adapter.loop_config(),
            None,
            &[],
        )?;
        assert!(!first_outbound
            .iter()
            .any(|message| message.content.contains("always")
                && message.metadata["runtime_notification"]["kind"] == "skill"));

        let (_second_turn, second_outbound) = adapter.process_inbound_with_outbound(
            InboundMessage::new("direct", "user", "chat-1", "again"),
            adapter.loop_config(),
            None,
            &[],
        )?;
        assert!(!second_outbound
            .iter()
            .any(|message| message.content.contains("always")
                && message.metadata["runtime_notification"]["kind"] == "skill"));

        let (_status_turn, status_outbound) = adapter.process_inbound_with_outbound(
            InboundMessage::new("direct", "user", "chat-2", "/status"),
            adapter.loop_config(),
            None,
            &[],
        )?;
        assert!(!status_outbound
            .iter()
            .any(|message| message.content.contains("always")
                && message.metadata["runtime_notification"]["kind"] == "skill"));

        let (_unknown_slash_turn, unknown_slash_outbound) = adapter.process_inbound_with_outbound(
            InboundMessage::new("direct", "user", "chat-3", "/status now"),
            adapter.loop_config(),
            None,
            &[],
        )?;
        assert!(!unknown_slash_outbound
            .iter()
            .any(|message| message.content.contains("always")
                && message.metadata["runtime_notification"]["kind"] == "skill"));
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
    fn copilot_import_token_writes_auth_without_leaking_token_to_config_or_output(
    ) -> Result<(), Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let outcome = import_copilot_token(CopilotImportTokenOptions {
            config_path: Some(config_path.clone()),
            token_source: TokenSource::Literal("secret-copilot-token".to_owned()),
            select: true,
        })?;

        assert_eq!(outcome.config_path, config_path);
        assert_eq!(
            outcome.selected_model.as_deref(),
            Some(GITHUB_COPILOT_DEFAULT_MODEL)
        );
        let config_text = fs::read_to_string(&outcome.config_path)?;
        assert!(config_text.contains(GITHUB_COPILOT_PROVIDER_ID));
        assert!(config_text.contains(GITHUB_COPILOT_DEFAULT_MODEL));
        assert!(!config_text.contains("secret-copilot-token"));
        let output = format_copilot_import_outcome(outcome.clone());
        assert!(!output.contains("secret-copilot-token"));

        let auth = load_auth_store(&outcome.auth_path)?;
        let copilot = auth
            .providers
            .get(GITHUB_COPILOT_PROVIDER_ID)
            .ok_or("missing copilot auth")?;
        assert_eq!(copilot.access, "secret-copilot-token");
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
    fn runtime_config_overlays_copilot_auth_without_persisting_secret() -> Result<(), Box<dyn Error>>
    {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        let mut config = Config::default();
        config.providers.insert(
            GITHUB_COPILOT_PROVIDER_ID.to_owned(),
            copilot_provider_config(),
        );
        save_config_to_path(&config, &config_path)?;
        let auth_path = config_context(Some(config_path.clone()), None).auth_path();
        let mut auth = AuthStore::default();
        auth.providers.insert(
            GITHUB_COPILOT_PROVIDER_ID.to_owned(),
            ProviderAuth::oauth_access("copilot-runtime-token", None),
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
            .get(GITHUB_COPILOT_PROVIDER_ID)
            .ok_or("missing copilot provider")?;
        assert_eq!(provider.api_key.as_deref(), Some("copilot-runtime-token"));
        assert_eq!(
            provider.api_base.as_deref(),
            Some("https://api.githubcopilot.com")
        );
        assert!(!fs::read_to_string(config_path)?.contains("copilot-runtime-token"));
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

    fn external_media_test_adapter(
        root: &Path,
        captured: Arc<Mutex<Vec<ProviderRequest>>>,
    ) -> Result<AgentLoopChatCompletionAdapter, Box<dyn Error>> {
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace)?;
        Ok(AgentLoopChatCompletionAdapter {
            configured_model: "openai/gpt-5".to_owned(),
            provider_id: "openai".to_owned(),
            defaults: AgentDefaults {
                model: "openai/gpt-5".to_owned(),
                max_tool_iterations: 1,
                ..AgentDefaults::default()
            },
            resolved_model: "gpt-5".to_owned(),
            native_image_input_supported: true,
            client: Arc::new(FakeProviderClient {
                captured,
                response: LlmResponse {
                    content: Some("ok".to_owned()),
                    finish_reason: "stop".to_owned(),
                    ..LlmResponse::default()
                },
            }),
            retry_mode: ProviderRetryMode::Standard,
            workspace,
            media_dir: root.join("data").join("media").join("api"),
            tools: ToolRegistry::new(),
            message_tool: None,
            _mcp_runtime: None,
            _mcp_reports: Vec::new(),
            allow_side_effect_tools: false,
            send_progress: false,
            send_tool_hints: false,
            send_max_retries: 0,
            session_turn_lock: SessionTurnLock::new(),
            exec_timeout_seconds: 60,
            exec_sandbox: None,
            exec_path_append: None,
            exec_allowed_env_keys: Vec::new(),
            exec_env: BTreeMap::new(),
            runtime_verbose: false,
            tool_search: ToolSearchConfig::default(),
            containment_snapshot: None,
            permission_mode_snapshot: PermissionModeSnapshot::default(),
            plugin_runtime_snapshot: PluginRuntimeSnapshot::default(),
        })
    }

    struct FakeProviderClient {
        captured: std::sync::Arc<Mutex<Vec<ProviderRequest>>>,
        response: LlmResponse,
    }

    struct StreamingProviderClient {
        captured: std::sync::Arc<Mutex<Vec<ProviderRequest>>>,
        response: LlmResponse,
        events: Vec<ProviderEvent>,
    }

    struct SequentialProviderClient {
        captured: std::sync::Arc<Mutex<Vec<ProviderRequest>>>,
        responses: Mutex<VecDeque<LlmResponse>>,
    }

    struct JsonArtifactTool;

    struct ErrorArtifactTool;

    impl Tool for JsonArtifactTool {
        fn name(&self) -> &str {
            "json_artifact"
        }

        fn description(&self) -> &str {
            "Return a structured artifact payload"
        }

        fn parameters(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            })
        }

        fn read_only(&self) -> bool {
            true
        }

        fn execute(&self, _params: JsonMap) -> ToolResult {
            ToolResult::Json(json!({
                "ok": true,
                "files": ["artifact.txt"],
                "embeds": [{"type": "text"}]
            }))
        }
    }

    impl Tool for ErrorArtifactTool {
        fn name(&self) -> &str {
            "error_artifact"
        }

        fn description(&self) -> &str {
            "Return an error artifact payload"
        }

        fn parameters(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"]
            })
        }

        fn read_only(&self) -> bool {
            true
        }

        fn execute(&self, _params: JsonMap) -> ToolResult {
            ToolResult::Text("Error: intentional artifact failure".to_owned())
        }
    }

    struct BlockingArtifactTool {
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }

    impl Tool for BlockingArtifactTool {
        fn name(&self) -> &str {
            "blocking_artifact"
        }

        fn description(&self) -> &str {
            "Block until the test releases execution"
        }

        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {}})
        }

        fn read_only(&self) -> bool {
            true
        }

        fn execute(&self, _params: JsonMap) -> ToolResult {
            let (lock, cvar) = &*self.gate;
            let mut released = match lock.lock() {
                Ok(guard) => guard,
                Err(_) => return ToolResult::Text("Error: gate lock poisoned".to_owned()),
            };
            while !*released {
                released = match cvar.wait(released) {
                    Ok(guard) => guard,
                    Err(_) => return ToolResult::Text("Error: gate wait poisoned".to_owned()),
                };
            }
            ToolResult::Text("blocked artifact complete".to_owned())
        }
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

    impl ProviderClient for SequentialProviderClient {
        fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
            self.captured
                .lock()
                .map_err(|_| ProviderError::Api {
                    status: Some(500),
                    message: "sequential fake capture lock failed".to_owned(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            self.responses
                .lock()
                .map_err(|_| ProviderError::Api {
                    status: Some(500),
                    message: "sequential fake response lock failed".to_owned(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .pop_front()
                .ok_or_else(|| ProviderError::Api {
                    status: Some(500),
                    message: "sequential fake response exhausted".to_owned(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })
        }

        fn chat_stream(
            &self,
            request: ProviderRequest,
            _on_event: &mut dyn FnMut(ProviderEvent),
        ) -> Result<LlmResponse, ProviderError> {
            self.chat(request)
        }
    }

    impl ProviderClient for StreamingProviderClient {
        fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
            self.captured
                .lock()
                .map_err(|_| ProviderError::Api {
                    status: Some(500),
                    message: "streaming fake lock failed".to_owned(),
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
            on_event: &mut dyn FnMut(ProviderEvent),
        ) -> Result<LlmResponse, ProviderError> {
            self.captured
                .lock()
                .map_err(|_| ProviderError::Api {
                    status: Some(500),
                    message: "streaming fake lock failed".to_owned(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            for event in &self.events {
                on_event(event.clone());
            }
            Ok(self.response.clone())
        }
    }
}
