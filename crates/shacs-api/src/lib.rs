use axum::body::{to_bytes, Body, Bytes};
use axum::extract::ws::{Message as AxumWebSocketMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, FromRequest, Multipart, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use shacs_channels::WebSocketServerEvent;
use shacs_providers::{GenerationSettings, LlmResponse, ProviderEvent, ProviderRequest};
use shacs_session::{
    SessionManager, SessionProjectionOptions, SessionRuntimeExecutionProjection,
    SessionRuntimeWorkflowProjection, SessionUxDiagnostics,
};
use shacs_utils::diagnostics::{
    DiagnosticsKind, DiagnosticsRecord, DiagnosticsSeverity, DiagnosticsSnapshot,
};
pub use shacs_utils::media_decode::{save_base64_data_url, MediaDecodeError, MAX_FILE_SIZE};
pub use shacs_utils::runtime::EMPTY_FINAL_RESPONSE_MESSAGE;
use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::{timeout, Duration};

pub const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";
pub const MODELS_PATH: &str = "/v1/models";
pub const SESSIONS_PATH: &str = "/v1/sessions";
pub const DIAGNOSTICS_PATH: &str = "/v1/diagnostics";
pub const WORKFLOW_RECIPES_PATH: &str = "/v1/workflows/recipes";
pub const HEALTH_PATH: &str = "/health";
pub const WEBSOCKET_PATH: &str = "/ws";
pub const JSON_CONTENT_TYPE: &str = "application/json";
pub const SSE_CONTENT_TYPE: &str = "text/event-stream";
pub const MULTIPART_DEFAULT_MESSAGE: &str = "请分析上传的文件";
pub const API_SESSION_KEY: &str = "api:default";
pub const API_CHAT_ID: &str = "default";
pub const MAX_MEDIA_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_REQUEST_BODY_BYTES: usize = 20 * 1024 * 1024;
const DEFAULT_API_TIMEOUT_SECONDS: f64 = 120.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub messages: Vec<ApiChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiChatMessage {
    pub role: String,
    pub content: ApiMessageContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApiMessageContent {
    Text(String),
    Parts(Vec<ApiContentPart>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApiContentPart {
    Text { text: String },
    ImageUrl { image_url: ApiImageUrl },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedChatRequest {
    pub requested_model: Option<String>,
    pub model: String,
    pub session_key: String,
    pub content: String,
    pub media_data_urls: Vec<String>,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatCompletionInvocation {
    pub provider_request: ProviderRequest,
    pub requested_model: Option<String>,
    pub session_key: String,
    pub media_data_urls: Vec<String>,
    pub media_paths: Vec<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiModel {
    pub id: String,
    pub owned_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    pub error_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMethod {
    Get,
    Post,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiHttpRequest {
    pub method: ApiMethod,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
}

impl ApiHttpRequest {
    pub fn get(path: impl Into<String>) -> Self {
        Self {
            method: ApiMethod::Get,
            path: path.into(),
            headers: BTreeMap::new(),
            body: None,
        }
    }

    pub fn post_json(path: impl Into<String>, body: Value) -> Self {
        Self {
            method: ApiMethod::Post,
            path: path.into(),
            headers: BTreeMap::from([("content-type".to_owned(), JSON_CONTENT_TYPE.to_owned())]),
            body: Some(body),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    pub fn content_type(&self) -> Option<&str> {
        self.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiHttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartChatRequest {
    pub message: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub files: Vec<MultipartFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartFile {
    pub filename: Option<String>,
    pub bytes: Vec<u8>,
}

pub trait ChatCompletionAdapter {
    fn configured_model(&self) -> &str;

    fn models(&self) -> Vec<ApiModel> {
        vec![ApiModel {
            id: self.configured_model().to_owned(),
            owned_by: "shacs-bot".to_owned(),
        }]
    }

    fn complete_chat(&self, invocation: ChatCompletionInvocation) -> Result<LlmResponse, ApiError>;

    fn stream_chat(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ApiError> {
        let response = self.complete_chat(invocation)?;
        if let Some(content) = response
            .content
            .as_ref()
            .filter(|content| !content.is_empty())
        {
            on_event(ProviderEvent::TextDelta {
                text: content.clone(),
            });
        }
        on_event(ProviderEvent::Finish {
            usage: json!(response.usage),
            reason: response.finish_reason.clone(),
        });
        Ok(response)
    }

    fn persist_media_data_urls(&self, data_urls: &[String]) -> Result<Vec<String>, ApiError> {
        Ok(data_urls.to_vec())
    }

    fn persist_media_data_urls_for_session(
        &self,
        _session_key: &str,
        data_urls: &[String],
    ) -> Result<Vec<String>, ApiError> {
        self.persist_media_data_urls(data_urls)
    }

    fn persist_uploaded_file(
        &self,
        _filename: Option<&str>,
        _bytes: &[u8],
    ) -> Result<String, ApiError> {
        Err(ApiError::unsupported_media(
            "multipart file persistence is not configured for this adapter",
        ))
    }

    fn persist_uploaded_file_for_session(
        &self,
        _session_key: &str,
        filename: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ApiError> {
        self.persist_uploaded_file(filename, bytes)
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        None
    }

    fn diagnostics_snapshot(&self) -> DiagnosticsSnapshot {
        let mut snapshot = DiagnosticsSnapshot::unavailable(
            "runtime diagnostics are not configured for this adapter",
        );
        snapshot.diagnostics.push(DiagnosticsRecord::new(
            DiagnosticsSeverity::Info,
            DiagnosticsKind::Api,
            "diagnostics request was read-only",
        ));
        snapshot
    }

    fn workflow_recipes_projection(&self) -> Option<Value> {
        None
    }

    fn process_websocket_frame(
        &self,
        _frame: Value,
        _client_id: &str,
        _default_chat_id: &str,
    ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
        Err(ApiError::not_implemented(
            "websocket frame handling is not configured for this adapter",
        ))
    }

    fn process_websocket_frame_streaming(
        &self,
        frame: Value,
        client_id: &str,
        default_chat_id: &str,
        on_event: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        for event in self.process_websocket_frame(frame, client_id, default_chat_id)? {
            on_event(event);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ApiRouterState {
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    session_locks: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
}

#[derive(Clone)]
pub struct WebUiRouterState {
    api: ApiRouterState,
    assets_dir: PathBuf,
}

impl ApiRouterState {
    pub fn new(adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>) -> Self {
        Self::with_timeout(
            adapter,
            Duration::from_secs_f64(DEFAULT_API_TIMEOUT_SECONDS),
        )
    }

    pub fn with_timeout(
        adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
        timeout: Duration,
    ) -> Self {
        Self {
            adapter,
            timeout,
            session_locks: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }
}

pub fn api_router(adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>) -> Router {
    api_router_with_timeout(
        adapter,
        Duration::from_secs_f64(DEFAULT_API_TIMEOUT_SECONDS),
    )
}

pub fn api_router_with_timeout(
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
) -> Router {
    api_router_with_timeout_and_websocket_path(adapter, timeout, WEBSOCKET_PATH)
}

pub fn api_router_with_timeout_and_websocket_path(
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
) -> Router {
    Router::new()
        .route(HEALTH_PATH, any(axum_dispatch))
        .route(MODELS_PATH, any(axum_dispatch))
        .route(DIAGNOSTICS_PATH, any(axum_dispatch))
        .route(WORKFLOW_RECIPES_PATH, any(axum_dispatch))
        .route(CHAT_COMPLETIONS_PATH, any(axum_dispatch))
        .route(websocket_path, any(websocket_upgrade_axum))
        .fallback(axum_dispatch)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ApiRouterState::with_timeout(adapter, timeout))
}

pub fn create_app(
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    request_timeout: Duration,
) -> Router {
    api_router_with_timeout(adapter, request_timeout)
}

pub fn websocket_router_with_timeout_and_path(
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
) -> Router {
    Router::new()
        .route(HEALTH_PATH, any(axum_dispatch))
        .route(websocket_path, any(websocket_upgrade_axum))
        .fallback(axum_not_found)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(ApiRouterState::with_timeout(adapter, timeout))
}

pub fn web_ui_router_with_timeout_and_websocket_path(
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    assets_dir: impl Into<PathBuf>,
) -> Router {
    Router::new()
        .route(HEALTH_PATH, any(webui_axum_dispatch))
        .route(MODELS_PATH, any(webui_axum_dispatch))
        .route(DIAGNOSTICS_PATH, any(webui_axum_dispatch))
        .route(WORKFLOW_RECIPES_PATH, any(webui_axum_dispatch))
        .route(CHAT_COMPLETIONS_PATH, any(webui_axum_dispatch))
        .route(websocket_path, any(webui_websocket_upgrade_axum))
        .fallback(webui_static_or_api_fallback)
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(WebUiRouterState {
            api: ApiRouterState::with_timeout(adapter, timeout),
            assets_dir: assets_dir.into(),
        })
}

pub async fn serve_api_listener(
    listener: TcpListener,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_api_listener_with_timeout(
        listener,
        adapter,
        Duration::from_secs_f64(DEFAULT_API_TIMEOUT_SECONDS),
        shutdown,
    )
    .await
}

pub async fn serve_api_listener_with_timeout(
    listener: TcpListener,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    serve_api_listener_with_timeout_and_websocket_path(
        listener,
        adapter,
        timeout,
        WEBSOCKET_PATH,
        shutdown,
    )
    .await
}

pub async fn serve_api_listener_with_timeout_and_websocket_path(
    listener: TcpListener,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        api_router_with_timeout_and_websocket_path(adapter, timeout, websocket_path),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

pub async fn serve_api(
    addr: SocketAddr,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_api_listener(listener, adapter, shutdown).await
}

pub async fn serve_api_with_timeout(
    addr: SocketAddr,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_api_listener_with_timeout(listener, adapter, timeout, shutdown).await
}

pub async fn serve_api_with_timeout_and_websocket_path(
    addr: SocketAddr,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_api_listener_with_timeout_and_websocket_path(
        listener,
        adapter,
        timeout,
        websocket_path,
        shutdown,
    )
    .await
}

pub async fn serve_websocket_listener_with_timeout_and_path(
    listener: TcpListener,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        websocket_router_with_timeout_and_path(adapter, timeout, websocket_path),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

pub async fn serve_websocket_with_timeout_and_path(
    addr: SocketAddr,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_websocket_listener_with_timeout_and_path(
        listener,
        adapter,
        timeout,
        websocket_path,
        shutdown,
    )
    .await
}

pub async fn serve_web_ui_listener_with_timeout_and_websocket_path(
    listener: TcpListener,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    assets_dir: impl Into<PathBuf>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        web_ui_router_with_timeout_and_websocket_path(adapter, timeout, websocket_path, assets_dir),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

pub async fn serve_web_ui_with_timeout_and_websocket_path(
    addr: SocketAddr,
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    timeout: Duration,
    websocket_path: &str,
    assets_dir: impl Into<PathBuf>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    serve_web_ui_listener_with_timeout_and_websocket_path(
        listener,
        adapter,
        timeout,
        websocket_path,
        assets_dir,
        shutdown,
    )
    .await
}

impl ApiError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
            error_type: "invalid_request_error".to_owned(),
        }
    }

    pub fn unsupported_media(message: impl Into<String>) -> Self {
        Self {
            status: 415,
            message: message.into(),
            error_type: "unsupported_media_type".to_owned(),
        }
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            status: 504,
            message: message.into(),
            error_type: "timeout_error".to_owned(),
        }
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self {
            status: 413,
            message: message.into(),
            error_type: "payload_too_large".to_owned(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            message: message.into(),
            error_type: "server_error".to_owned(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            message: message.into(),
            error_type: "not_found".to_owned(),
        }
    }

    pub fn method_not_allowed(message: impl Into<String>) -> Self {
        Self {
            status: 405,
            message: message.into(),
            error_type: "method_not_allowed".to_owned(),
        }
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self {
            status: 501,
            message: message.into(),
            error_type: "not_implemented".to_owned(),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "API error {} ({}): {}",
            self.status, self.error_type, self.message
        )
    }
}

impl std::error::Error for ApiError {}

pub fn health_response() -> Value {
    json!({"status": "ok"})
}

pub fn models_response(model: &str) -> Value {
    models_response_with_owned_by(&[ApiModel {
        id: model.to_owned(),
        owned_by: "shacs-bot".to_owned(),
    }])
}

pub fn models_response_with_owned_by(models: &[ApiModel]) -> Value {
    json!({
        "object": "list",
        "data": models.iter().map(|model| {
            json!({
                "id": model.id,
                "object": "model",
                "created": 0,
                "owned_by": model.owned_by,
            })
        }).collect::<Vec<_>>()
    })
}

pub fn api_error_response(error: &ApiError) -> Value {
    json!({
        "error": {
            "message": error.message,
            "type": error.error_type,
            "code": error.status,
        }
    })
}

pub fn handle_api_request(
    request: ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    match (request.method, request.path.as_str()) {
        (ApiMethod::Get, HEALTH_PATH) => json_response(200, health_response()),
        (ApiMethod::Get, MODELS_PATH) => {
            json_response(200, models_response_with_owned_by(&adapter.models()))
        }
        (ApiMethod::Get, DIAGNOSTICS_PATH) => {
            json_response(200, adapter.diagnostics_snapshot().redacted_value())
        }
        (ApiMethod::Get, WORKFLOW_RECIPES_PATH) => match adapter.workflow_recipes_projection() {
            Some(projection) => json_response(200, projection),
            None => error_response(ApiError::not_found(
                "workflow recipe projection is not configured",
            )),
        },
        (ApiMethod::Get, path) if path == SESSIONS_PATH || path.starts_with("/v1/sessions/") => {
            handle_session_query_request(path, adapter)
        }
        (ApiMethod::Post, CHAT_COMPLETIONS_PATH) => {
            handle_chat_completion_request(request, adapter)
        }
        (_, HEALTH_PATH)
        | (_, MODELS_PATH)
        | (_, DIAGNOSTICS_PATH)
        | (_, WORKFLOW_RECIPES_PATH)
        | (_, CHAT_COMPLETIONS_PATH)
        | (_, SESSIONS_PATH) => error_response(ApiError::method_not_allowed(
            "method is not supported for this endpoint",
        )),
        (_, path) if path.starts_with("/v1/sessions/") => error_response(
            ApiError::method_not_allowed("method is not supported for this endpoint"),
        ),
        _ => error_response(ApiError::not_found("API route not found")),
    }
}

fn handle_session_query_request(
    path: &str,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    let Some(workspace) = adapter.session_workspace() else {
        return error_response(ApiError::not_found(
            "session query surface is not configured",
        ));
    };
    let Some(route) = session_query_route(path) else {
        return error_response(ApiError::not_found("session API route not found"));
    };
    let manager = match SessionManager::open_existing(&workspace) {
        Ok(manager) => manager,
        Err(error) => {
            return error_response(ApiError::internal(format!(
                "session store could not be opened: {error}"
            )))
        }
    };
    match route {
        SessionQueryRoute::List => handle_session_list_query(manager.as_ref()),
        SessionQueryRoute::Detail(key) => handle_session_detail_query(manager.as_ref(), &key),
        SessionQueryRoute::History(key) => handle_session_history_query(manager.as_ref(), &key),
        SessionQueryRoute::Diagnostics(key) => {
            handle_session_diagnostics_query(manager.as_ref(), &workspace, &key)
        }
    }
}

fn handle_session_list_query(manager: Option<&SessionManager>) -> ApiHttpResponse {
    let Some(manager) = manager else {
        return json_response(200, json!({ "object": "list", "data": [] }));
    };
    match manager.list_session_ux() {
        Ok(sessions) => json_response(200, json!({ "object": "list", "data": sessions })),
        Err(error) => error_response(ApiError::internal(format!(
            "session list could not be read: {error}"
        ))),
    }
}

fn handle_session_detail_query(manager: Option<&SessionManager>, key: &str) -> ApiHttpResponse {
    let Some(manager) = manager else {
        return error_response(ApiError::not_found(format!(
            "session `{key}` was not found"
        )));
    };
    match manager.session_ux_detail(key) {
        Some(detail) => json_response(200, json!(detail)),
        None => error_response(ApiError::not_found(format!(
            "session `{key}` was not found"
        ))),
    }
}

fn handle_session_history_query(manager: Option<&SessionManager>, key: &str) -> ApiHttpResponse {
    let Some(manager) = manager else {
        return error_response(ApiError::not_found(format!(
            "session `{key}` was not found"
        )));
    };
    match manager.session_ux_history(key, SessionProjectionOptions::default()) {
        Some(history) => json_response(200, json!(history)),
        None => error_response(ApiError::not_found(format!(
            "session `{key}` was not found"
        ))),
    }
}

fn handle_session_diagnostics_query(
    manager: Option<&SessionManager>,
    workspace: &std::path::Path,
    key: &str,
) -> ApiHttpResponse {
    if let Some(manager) = manager {
        return json_response(200, json!(manager.session_ux_diagnostics(key)));
    }
    json_response(
        200,
        json!(SessionUxDiagnostics {
            key: key.to_owned(),
            path: workspace
                .join("sessions")
                .join(format!("{}.jsonl", SessionManager::safe_key(key))),
            exists: false,
            message_count: 0,
            last_consolidated: 0,
            metadata_keys: Vec::new(),
            recovery_markers: Vec::new(),
            checkpoint_phase: None,
            diagnostics_refs: Vec::new(),
            runtime_workflow: None::<SessionRuntimeWorkflowProjection>,
            runtime_execution: None::<SessionRuntimeExecutionProjection>,
            legal_start: 0,
        }),
    )
}

enum SessionQueryRoute {
    List,
    Detail(String),
    History(String),
    Diagnostics(String),
}

fn session_query_route(path: &str) -> Option<SessionQueryRoute> {
    if path == SESSIONS_PATH {
        return Some(SessionQueryRoute::List);
    }
    let suffix = path.strip_prefix("/v1/sessions/")?;
    let mut segments = suffix.split('/');
    let key = decode_path_segment(segments.next()?)?;
    if key.is_empty() {
        return None;
    }
    match (segments.next(), segments.next()) {
        (None, None) => Some(SessionQueryRoute::Detail(key)),
        (Some("history"), None) => Some(SessionQueryRoute::History(key)),
        (Some("diagnostics"), None) => Some(SessionQueryRoute::Diagnostics(key)),
        _ => None,
    }
}

fn decode_path_segment(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return None;
                }
                let high = hex_value(bytes[index + 1])?;
                let low = hex_value(bytes[index + 2])?;
                output.push(high << 4 | low);
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

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub fn handle_chat_completions(
    request: ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    handle_chat_completion_request(request, adapter)
}

async fn axum_dispatch(State(state): State<ApiRouterState>, request: Request) -> Response {
    let method = api_method_from_axum(request.method());
    let path = request.uri().path().to_owned();
    if method == ApiMethod::Post
        && path == CHAT_COMPLETIONS_PATH
        && is_multipart_header_map(request.headers())
    {
        return handle_multipart_chat_axum(state, request).await;
    }

    let api_request = match api_request_from_axum(request).await {
        Ok(request) => request,
        Err(error) => return axum_response_from_api(error_response(error)),
    };
    if api_request.method == ApiMethod::Post && api_request.path == CHAT_COMPLETIONS_PATH {
        return handle_json_chat_axum(state, api_request).await;
    }
    let adapter = state.adapter.clone();
    let response =
        tokio::task::spawn_blocking(move || handle_api_request(api_request, adapter.as_ref()))
            .await
            .unwrap_or_else(|_| error_response(ApiError::internal("API request task failed")));
    axum_response_from_api(response)
}

async fn webui_axum_dispatch(State(state): State<WebUiRouterState>, request: Request) -> Response {
    axum_dispatch(State(state.api), request).await
}

async fn webui_websocket_upgrade_axum(
    State(state): State<WebUiRouterState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    websocket_upgrade_axum(State(state.api), headers, ws).await
}

async fn webui_static_or_api_fallback(
    State(state): State<WebUiRouterState>,
    request: Request,
) -> Response {
    let path = request.uri().path().to_owned();
    if is_api_path(&path) {
        return axum_dispatch(State(state.api), request).await;
    }
    match shacs_web::static_files::serve_static(&state.assets_dir, &path) {
        Ok(Some(static_response)) => axum_static_response(static_response),
        Ok(None) => axum_not_found().await,
        Err(shacs_web::static_files::StaticFileError::Forbidden) => {
            StatusCode::FORBIDDEN.into_response()
        }
        Err(shacs_web::static_files::StaticFileError::ReadError) => {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn is_api_path(path: &str) -> bool {
    path == HEALTH_PATH
        || path == MODELS_PATH
        || path == CHAT_COMPLETIONS_PATH
        || path == "/v1"
        || path.starts_with("/v1/")
}

fn axum_static_response(response: shacs_web::static_files::StaticFileResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    let content_type = HeaderValue::from_str(&response.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let cache_control = HeaderValue::from_str(&response.cache_control)
        .unwrap_or_else(|_| HeaderValue::from_static("no-cache"));
    (
        status,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, cache_control),
        ],
        Body::from(response.body),
    )
        .into_response()
}

async fn axum_not_found() -> Response {
    axum_response_from_api(error_response(ApiError::not_found("API route not found")))
}

async fn websocket_upgrade_axum(
    State(state): State<ApiRouterState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(error) = validate_websocket_origin(&headers) {
        return axum_response_from_api(error_response(error));
    }
    let client_id = "websocket-client".to_owned();
    let chat_id = "default".to_owned();
    ws.on_upgrade(move |socket| handle_websocket_connection(state, socket, client_id, chat_id))
        .into_response()
}

async fn handle_websocket_connection(
    state: ApiRouterState,
    mut socket: WebSocket,
    client_id: String,
    default_chat_id: String,
) {
    while let Some(message) = socket.recv().await {
        let result = match websocket_frame_from_axum(message) {
            Ok(Some(frame)) => {
                dispatch_websocket_frame(
                    state.adapter.clone(),
                    frame,
                    client_id.clone(),
                    default_chat_id.clone(),
                    &mut socket,
                )
                .await
            }
            Ok(None) => continue,
            Err(error) => {
                send_websocket_event(
                    &mut socket,
                    WebSocketServerEvent::Error {
                        chat_id: Some(default_chat_id.clone()),
                        detail: Some(error.message),
                    },
                    &default_chat_id,
                )
                .await
            }
        };
        if let Err(error) = result {
            let fallback = WebSocketServerEvent::Error {
                chat_id: Some(default_chat_id.clone()),
                detail: Some(error.message),
            };
            if send_websocket_event(&mut socket, fallback, &default_chat_id)
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

async fn dispatch_websocket_frame(
    adapter: Arc<dyn ChatCompletionAdapter + Send + Sync>,
    frame: Value,
    client_id: String,
    default_chat_id: String,
    socket: &mut WebSocket,
) -> Result<(), ApiError> {
    let fallback_chat_id = default_chat_id.clone();
    let (event_tx, mut event_rx) = mpsc::channel::<WebSocketServerEvent>(64);
    let task = tokio::task::spawn_blocking(move || {
        let mut emit = move |event| {
            let _ = event_tx.blocking_send(event);
        };
        adapter.process_websocket_frame_streaming(frame, &client_id, &default_chat_id, &mut emit)
    });

    while let Some(event) = event_rx.recv().await {
        send_websocket_event(socket, event, &fallback_chat_id).await?;
    }

    task.await
        .unwrap_or_else(|_| Err(ApiError::internal("websocket frame task failed")))
}

async fn send_websocket_event(
    socket: &mut WebSocket,
    event: WebSocketServerEvent,
    fallback_chat_id: &str,
) -> Result<(), ApiError> {
    let payload = match serde_json::to_string(&event) {
        Ok(payload) => payload,
        Err(error) => {
            let fallback = WebSocketServerEvent::Error {
                chat_id: Some(fallback_chat_id.to_owned()),
                detail: Some(format!("websocket event could not be serialized: {error}")),
            };
            fallback_payload(&fallback)
        }
    };
    socket
        .send(AxumWebSocketMessage::Text(payload.into()))
        .await
        .map_err(|_| ApiError::internal("websocket client disconnected"))
}

fn websocket_frame_from_axum(
    message: Result<AxumWebSocketMessage, axum::Error>,
) -> Result<Option<Value>, ApiError> {
    match message.map_err(|_| ApiError::invalid_request("websocket frame could not be read"))? {
        AxumWebSocketMessage::Text(text) => websocket_json_from_bytes(text.as_str().as_bytes()),
        AxumWebSocketMessage::Binary(bytes) => websocket_json_from_bytes(bytes.as_ref()),
        AxumWebSocketMessage::Ping(_) | AxumWebSocketMessage::Pong(_) => Ok(None),
        AxumWebSocketMessage::Close(_) => Ok(None),
    }
}

fn websocket_json_from_bytes(bytes: &[u8]) -> Result<Option<Value>, ApiError> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > MAX_REQUEST_BODY_BYTES {
        return Err(ApiError::payload_too_large(format!(
            "websocket frame exceeds {MAX_REQUEST_BODY_BYTES} bytes"
        )));
    }
    serde_json::from_slice(bytes)
        .map(Some)
        .map_err(|_| ApiError::invalid_request("websocket frame must be valid JSON"))
}

fn validate_websocket_origin(headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(origin) = header_str(headers, header::ORIGIN) else {
        return Ok(());
    };
    let Some(host) = header_str(headers, header::HOST) else {
        return Err(ApiError::invalid_request(
            "websocket origin requires a host header",
        ));
    };
    if websocket_origin_matches_host(origin, host) {
        Ok(())
    } else {
        Err(ApiError::invalid_request("websocket origin is not allowed"))
    }
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn websocket_origin_matches_host(origin: &str, host: &str) -> bool {
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    authority
        .split('/')
        .next()
        .is_some_and(|origin_host| origin_host.eq_ignore_ascii_case(host))
}

fn fallback_payload(event: &WebSocketServerEvent) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| {
        r#"{"event":"error","detail":"websocket event could not be serialized"}"#.to_owned()
    })
}

async fn api_request_from_axum(request: Request) -> Result<ApiHttpRequest, ApiError> {
    let (parts, body) = request.into_parts();
    let headers = headers_to_map(&parts.headers);
    let method = api_method_from_axum(&parts.method);
    let path = parts.uri.path().to_owned();
    let body = if should_read_axum_body(method, &path, &headers) {
        let body_bytes = to_bytes(body, MAX_REQUEST_BODY_BYTES)
            .await
            .map_err(|error| {
                ApiError::invalid_request(format!("request body could not be read: {error}"))
            })?;
        body_value_from_bytes(body_bytes)?
    } else {
        None
    };

    Ok(ApiHttpRequest {
        method,
        path,
        headers,
        body,
    })
}

fn should_read_axum_body(
    method: ApiMethod,
    path: &str,
    headers: &BTreeMap<String, String>,
) -> bool {
    method == ApiMethod::Post && path == CHAT_COMPLETIONS_PATH && !is_multipart_headers(headers)
}

fn body_value_from_bytes(body_bytes: Bytes) -> Result<Option<Value>, ApiError> {
    if body_bytes.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&body_bytes)
        .map(Some)
        .map_err(|error| {
            ApiError::invalid_request(format!("request body must be valid JSON: {error}"))
        })
}

fn is_multipart_headers(headers: &BTreeMap<String, String>) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .and_then(|(_, value)| reject_multipart_request(value))
        .is_some()
}

fn is_multipart_header_map(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(is_multipart_content_type)
        .unwrap_or(false)
}

fn headers_to_map(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
        })
        .collect()
}

fn api_method_from_axum(method: &Method) -> ApiMethod {
    match *method {
        Method::GET => ApiMethod::Get,
        Method::POST => ApiMethod::Post,
        _ => ApiMethod::Other,
    }
}

fn axum_response_from_api(response: ApiHttpResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut axum_response = (status, Json(response.body)).into_response();
    let content_type = HeaderValue::from_str(&response.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static(JSON_CONTENT_TYPE));
    axum_response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    axum_response
}

fn axum_sse_stream_response(rx: mpsc::Receiver<String>) -> Response {
    let body_stream = stream::unfold(rx, |mut rx| async {
        rx.recv()
            .await
            .map(|frame| (Ok::<Bytes, Infallible>(Bytes::from(frame)), rx))
    });
    let mut response = (StatusCode::OK, Body::from_stream(body_stream)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(SSE_CONTENT_TYPE),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    response
}

async fn handle_json_chat_axum(state: ApiRouterState, request: ApiHttpRequest) -> Response {
    let Some(body) = request.body.as_ref() else {
        return axum_response_from_api(error_response(ApiError::invalid_request(
            "request body is required",
        )));
    };
    let chat_request = match parse_chat_completion_request(body) {
        Ok(request) => request,
        Err(error) => return axum_response_from_api(error_response(error)),
    };
    handle_chat_request_axum(state, chat_request, Vec::new()).await
}

async fn handle_multipart_chat_axum(state: ApiRouterState, request: Request) -> Response {
    let mut multipart = match Multipart::from_request(request, &state).await {
        Ok(multipart) => multipart,
        Err(error) => {
            return axum_response_from_api(error_response(ApiError::invalid_request(format!(
                "multipart request could not be parsed: {error}"
            ))))
        }
    };
    let multipart_request = match multipart_chat_request_from_axum(&mut multipart).await {
        Ok(request) => request,
        Err(error) => return axum_response_from_api(error_response(error)),
    };
    let chat_request = chat_request_from_multipart(&multipart_request);
    handle_chat_request_axum(state, chat_request, multipart_request.files).await
}

async fn handle_chat_request_axum(
    state: ApiRouterState,
    chat_request: ChatCompletionRequest,
    uploaded_files: Vec<MultipartFile>,
) -> Response {
    let validated =
        match validate_chat_completion_request(&chat_request, state.adapter.configured_model()) {
            Ok(validated) => validated,
            Err(error) => return axum_response_from_api(error_response(error)),
        };
    let session_lock = session_lock_for(&state, &validated.session_key).await;
    if chat_request.stream {
        return stream_chat_request_axum(
            state,
            session_lock,
            chat_request,
            uploaded_files,
            validated.session_key,
        );
    }
    let session_guard = session_lock.lock_owned().await;
    let timeout_duration = state.timeout;
    let adapter = state.adapter.clone();
    let request_id = chat_completion_id(&format!(
        "{}:{}:{}",
        adapter.configured_model(),
        validated.session_key,
        current_unix_timestamp()
    ));
    let created = current_unix_timestamp();
    let operation = tokio::task::spawn_blocking(move || {
        let _session_guard = session_guard;
        let invocation = chat_completion_invocation_with_uploads(
            &chat_request,
            adapter.configured_model(),
            uploaded_files,
            adapter.as_ref(),
        )?;
        adapter
            .complete_chat(invocation)
            .map(ChatOperationResult::Completion)
    });

    let result = match timeout(timeout_duration, operation).await {
        Ok(Ok(result)) => result,
        Ok(Err(_join_error)) => {
            return axum_response_from_api(error_response(ApiError::internal(
                "API request task failed",
            )))
        }
        Err(_elapsed) => {
            return axum_response_from_api(error_response(ApiError::timeout(
                "chat completion timed out",
            )))
        }
    };

    match result {
        Ok(ChatOperationResult::Completion(response)) => axum_response_from_api(json_response(
            200,
            chat_completion_response(
                &response,
                state.adapter.configured_model(),
                &request_id,
                created,
            ),
        )),
        Err(error) => axum_response_from_api(error_response(error)),
    }
}

enum ChatOperationResult {
    Completion(LlmResponse),
}

fn stream_chat_request_axum(
    state: ApiRouterState,
    session_lock: Arc<AsyncMutex<()>>,
    chat_request: ChatCompletionRequest,
    uploaded_files: Vec<MultipartFile>,
    session_key: String,
) -> Response {
    let adapter = state.adapter.clone();
    let timeout_duration = state.timeout;
    let request_id = chat_completion_id(&format!(
        "{}:{}:{}",
        adapter.configured_model(),
        session_key,
        current_unix_timestamp()
    ));
    let created = current_unix_timestamp();
    let model = adapter.configured_model().to_owned();
    let (tx, rx) = mpsc::channel::<String>(16);
    let cancelled = Arc::new(AtomicBool::new(false));
    tokio::spawn({
        let cancelled = cancelled.clone();
        async move {
            let session_guard = session_lock.lock_owned().await;
            let worker_cancelled = cancelled.clone();
            let operation = tokio::task::spawn_blocking(move || {
                let _session_guard = session_guard;
                let invocation = chat_completion_invocation_with_uploads(
                    &chat_request,
                    adapter.configured_model(),
                    uploaded_files,
                    adapter.as_ref(),
                )?;
                let mut saw_finish = false;
                let response = adapter.stream_chat(invocation, &mut |event| {
                    if worker_cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    if matches!(event, ProviderEvent::Finish { .. }) {
                        saw_finish = true;
                    }
                    let frame = stream_event_frame(&event, &model, &request_id, created);
                    let _ = tx.blocking_send(frame);
                })?;
                if !worker_cancelled.load(Ordering::SeqCst) {
                    if !saw_finish {
                        let _ = tx.blocking_send(finish_stream_frame(
                            &model,
                            &request_id,
                            created,
                            &response.finish_reason,
                        ));
                    }
                    let _ = tx.blocking_send(done_stream_frame());
                }
                Ok::<(), ApiError>(())
            });
            match timeout(timeout_duration, operation).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(_))) | Ok(Err(_)) | Err(_) => {
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
        }
    });
    axum_sse_stream_response(rx)
}

async fn session_lock_for(state: &ApiRouterState, session_key: &str) -> Arc<AsyncMutex<()>> {
    let mut locks = state.session_locks.lock().await;
    locks
        .entry(session_key.to_owned())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

pub fn parse_chat_completion_request(body: &Value) -> Result<ChatCompletionRequest, ApiError> {
    if !body.is_object() {
        return Err(ApiError::invalid_request(
            "request body must be a JSON object",
        ));
    }
    serde_json::from_value(body.clone()).map_err(|error| {
        ApiError::invalid_request(format!("invalid chat completion request: {error}"))
    })
}

pub fn validate_chat_completion_request(
    request: &ChatCompletionRequest,
    configured_model: &str,
) -> Result<ValidatedChatRequest, ApiError> {
    if let Some(model) = request.model.as_deref() {
        if model != configured_model {
            return Err(ApiError::invalid_request(format!(
                "model `{model}` is not available; only configured model `{configured_model}` is available"
            )));
        }
    }
    if request.messages.len() != 1 {
        return Err(ApiError::invalid_request(
            "exactly one user message is supported",
        ));
    }
    let message = &request.messages[0];
    if message.role != "user" {
        return Err(ApiError::invalid_request(
            "only a single user message is supported",
        ));
    }
    let (content, media_data_urls) = flatten_content(&message.content)?;
    Ok(ValidatedChatRequest {
        requested_model: request.model.clone(),
        model: configured_model.to_owned(),
        session_key: session_key(request.session_id.as_deref()),
        content,
        media_data_urls,
        stream: request.stream,
    })
}

pub fn provider_request_from_chat_request(
    request: &ChatCompletionRequest,
    configured_model: &str,
) -> Result<ProviderRequest, ApiError> {
    let validated = validate_chat_completion_request(request, configured_model)?;
    provider_request_from_validated(request, &validated)
}

pub fn chat_completion_invocation(
    request: &ChatCompletionRequest,
    configured_model: &str,
) -> Result<ChatCompletionInvocation, ApiError> {
    chat_completion_invocation_with_uploads(
        request,
        configured_model,
        Vec::new(),
        &NoopMediaAdapter,
    )
}

fn chat_completion_invocation_with_uploads(
    request: &ChatCompletionRequest,
    configured_model: &str,
    uploaded_files: Vec<MultipartFile>,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> Result<ChatCompletionInvocation, ApiError> {
    let validated = validate_chat_completion_request(request, configured_model)?;
    let provider_request = provider_request_from_validated(request, &validated)?;
    let mut media_paths = adapter
        .persist_media_data_urls_for_session(&validated.session_key, &validated.media_data_urls)?;
    for file in uploaded_files {
        if file.bytes.len() > MAX_MEDIA_BYTES {
            return Err(ApiError::payload_too_large(format!(
                "uploaded file exceeds {} bytes",
                MAX_MEDIA_BYTES
            )));
        }
        media_paths.push(adapter.persist_uploaded_file_for_session(
            &validated.session_key,
            file.filename.as_deref(),
            &file.bytes,
        )?);
    }
    Ok(ChatCompletionInvocation {
        provider_request,
        requested_model: validated.requested_model,
        session_key: validated.session_key,
        media_data_urls: validated.media_data_urls,
        media_paths,
        temperature: request.temperature,
        max_tokens: request.max_tokens,
    })
}

struct NoopMediaAdapter;

impl ChatCompletionAdapter for NoopMediaAdapter {
    fn configured_model(&self) -> &str {
        ""
    }

    fn complete_chat(
        &self,
        _invocation: ChatCompletionInvocation,
    ) -> Result<LlmResponse, ApiError> {
        Err(ApiError::internal("noop adapter cannot complete chat"))
    }
}

fn provider_request_from_validated(
    request: &ChatCompletionRequest,
    validated: &ValidatedChatRequest,
) -> Result<ProviderRequest, ApiError> {
    let mut message = Map::new();
    message.insert("role".to_owned(), Value::String("user".to_owned()));
    message.insert("content".to_owned(), provider_message_content(validated));
    let default_settings = GenerationSettings::default();
    Ok(ProviderRequest {
        messages: vec![Value::Object(message)],
        tools: request.tools.clone(),
        model: validated.model.clone(),
        settings: GenerationSettings {
            temperature: request.temperature.unwrap_or(default_settings.temperature),
            max_tokens: request.max_tokens.unwrap_or(default_settings.max_tokens),
            reasoning_effort: default_settings.reasoning_effort,
        },
        tool_choice: request.tool_choice.clone(),
    })
}

pub fn json_response(status: u16, body: Value) -> ApiHttpResponse {
    ApiHttpResponse {
        status,
        content_type: JSON_CONTENT_TYPE.to_owned(),
        body,
    }
}

pub fn error_response(error: ApiError) -> ApiHttpResponse {
    json_response(error.status, api_error_response(&error))
}

pub fn chat_completion_response(
    response: &LlmResponse,
    model: &str,
    request_id: &str,
    created: u64,
) -> Value {
    let mut message = Map::new();
    message.insert("role".to_owned(), Value::String("assistant".to_owned()));
    message.insert(
        "content".to_owned(),
        Value::String(response.content.clone().unwrap_or_default()),
    );
    if !response.tool_calls.is_empty() {
        message.insert(
            "tool_calls".to_owned(),
            Value::Array(
                response
                    .tool_calls
                    .iter()
                    .map(|tool_call| tool_call.to_openai_tool_call())
                    .collect(),
            ),
        );
    }

    json!({
        "id": request_id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": response.finish_reason,
        }],
        "usage": usage_json(&response.usage),
    })
}

pub fn stream_event_frame(
    event: &ProviderEvent,
    model: &str,
    request_id: &str,
    created: u64,
) -> String {
    let chunk = match event {
        ProviderEvent::TextDelta { text } => json!({
            "id": request_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"content": text},
                "finish_reason": Value::Null,
            }],
        }),
        ProviderEvent::ReasoningDelta { text } => json!({
            "id": request_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {"reasoning_content": text},
                "finish_reason": Value::Null,
            }],
        }),
        ProviderEvent::Finish { reason, .. } => {
            finish_stream_chunk(model, request_id, created, reason)
        }
        ProviderEvent::ToolCallStart { .. }
        | ProviderEvent::ToolCallDelta { .. }
        | ProviderEvent::ToolCallReady { .. } => json!({
            "id": request_id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": Value::Null,
            }],
        }),
    };
    sse_data_frame(&chunk)
}

pub fn finish_stream_frame(model: &str, request_id: &str, created: u64, reason: &str) -> String {
    sse_data_frame(&finish_stream_chunk(model, request_id, created, reason))
}

pub fn done_stream_frame() -> String {
    "data: [DONE]\n\n".to_owned()
}

pub fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn chat_completion_id(seed: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in seed.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("chatcmpl-{hash:012x}")
}

pub fn reject_multipart_request(content_type: &str) -> Option<ApiError> {
    content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
        .then(|| {
            ApiError::unsupported_media(
                "multipart chat completion requests require the axum API runtime",
            )
        })
}

fn handle_chat_completion_request(
    request: ApiHttpRequest,
    adapter: &(impl ChatCompletionAdapter + ?Sized),
) -> ApiHttpResponse {
    if let Some(content_type) = request.content_type() {
        if let Some(error) = reject_multipart_request(content_type) {
            return error_response(error);
        }
    }
    let Some(body) = request.body.as_ref() else {
        return error_response(ApiError::invalid_request("request body is required"));
    };
    let chat_request = match parse_chat_completion_request(body) {
        Ok(request) => request,
        Err(error) => return error_response(error),
    };
    if chat_request.stream {
        let invocation = match chat_completion_invocation(&chat_request, adapter.configured_model())
        {
            Ok(invocation) => invocation,
            Err(error) => return error_response(error),
        };
        let mut events = Vec::new();
        let response = match adapter.stream_chat(invocation, &mut |event| events.push(event)) {
            Ok(response) => response,
            Err(error) => return error_response(error),
        };
        return ApiHttpResponse {
            status: 200,
            content_type: SSE_CONTENT_TYPE.to_owned(),
            body: Value::String(stream_response_body(
                &events,
                &response,
                adapter.configured_model(),
                &chat_completion_id(&format!(
                    "{}:{}",
                    adapter.configured_model(),
                    current_unix_timestamp()
                )),
                current_unix_timestamp(),
            )),
        };
    }
    let invocation = match chat_completion_invocation(&chat_request, adapter.configured_model()) {
        Ok(invocation) => invocation,
        Err(error) => return error_response(error),
    };
    let response = match adapter.complete_chat(invocation) {
        Ok(response) => response,
        Err(error) => return error_response(error),
    };
    json_response(
        200,
        chat_completion_response(
            &response,
            adapter.configured_model(),
            &chat_completion_id(&format!(
                "{}:{}",
                adapter.configured_model(),
                current_unix_timestamp()
            )),
            current_unix_timestamp(),
        ),
    )
}

fn provider_message_content(validated: &ValidatedChatRequest) -> Value {
    let image_data_urls = validated
        .media_data_urls
        .iter()
        .filter(|url| is_image_data_url(url))
        .collect::<Vec<_>>();
    if image_data_urls.is_empty() {
        return Value::String(validated.content.clone());
    }

    let mut parts = Vec::new();
    if !validated.content.is_empty() {
        parts.push(json!({"type": "text", "text": validated.content}));
    }
    for url in image_data_urls {
        parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }
    Value::Array(parts)
}

fn is_image_data_url(url: &str) -> bool {
    let Some((header, _)) = url.split_once(',') else {
        return false;
    };
    let Some(media_type) = header.strip_prefix("data:") else {
        return false;
    };
    media_type
        .split(';')
        .next()
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("image/"))
}

fn flatten_content(content: &ApiMessageContent) -> Result<(String, Vec<String>), ApiError> {
    match content {
        ApiMessageContent::Text(text) => Ok((text.clone(), Vec::new())),
        ApiMessageContent::Parts(parts) => {
            let mut text_parts = Vec::new();
            let mut media = Vec::new();
            for part in parts {
                match part {
                    ApiContentPart::Text { text } => {
                        if !text.is_empty() {
                            text_parts.push(text.clone());
                        }
                    }
                    ApiContentPart::ImageUrl { image_url } => {
                        if image_url.url.starts_with("data:") {
                            media.push(image_url.url.clone());
                        } else {
                            return Err(ApiError::invalid_request(
                                "remote image URLs are not supported; use a data URL",
                            ));
                        }
                    }
                }
            }
            Ok((text_parts.join(" "), media))
        }
    }
}

async fn multipart_chat_request_from_axum(
    multipart: &mut Multipart,
) -> Result<MultipartChatRequest, ApiError> {
    let mut message = String::new();
    let mut session_id = None;
    let mut model = None;
    let mut files = Vec::new();
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        ApiError::invalid_request(format!("multipart field could not be read: {error}"))
    })? {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "message" => message = read_multipart_text(field).await?,
            "session_id" => {
                let value = read_multipart_text(field).await?;
                session_id = non_empty_trimmed(value);
            }
            "model" => {
                let value = read_multipart_text(field).await?;
                model = non_empty_trimmed(value);
            }
            "files" => {
                let filename = field.file_name().map(str::to_owned);
                let bytes = read_multipart_bytes(field).await?;
                files.push(MultipartFile { filename, bytes });
            }
            _ => {}
        }
    }
    if message.trim().is_empty() {
        message = MULTIPART_DEFAULT_MESSAGE.to_owned();
    }
    Ok(MultipartChatRequest {
        message,
        session_id,
        model,
        files,
    })
}

async fn read_multipart_text(
    field: axum::extract::multipart::Field<'_>,
) -> Result<String, ApiError> {
    let bytes = read_multipart_bytes(field).await?;
    String::from_utf8(bytes).map_err(|error| {
        ApiError::invalid_request(format!("multipart text field must be UTF-8: {error}"))
    })
}

async fn read_multipart_bytes(
    field: axum::extract::multipart::Field<'_>,
) -> Result<Vec<u8>, ApiError> {
    let bytes = field.bytes().await.map_err(|error| {
        ApiError::invalid_request(format!("multipart field bytes could not be read: {error}"))
    })?;
    if bytes.len() > MAX_MEDIA_BYTES {
        return Err(ApiError::payload_too_large(format!(
            "multipart field exceeds {} bytes",
            MAX_MEDIA_BYTES
        )));
    }
    Ok(bytes.to_vec())
}

fn non_empty_trimmed(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn chat_request_from_multipart(request: &MultipartChatRequest) -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: request.model.clone(),
        messages: vec![ApiChatMessage {
            role: "user".to_owned(),
            content: ApiMessageContent::Text(request.message.clone()),
        }],
        stream: false,
        session_id: request.session_id.clone(),
        temperature: None,
        max_tokens: None,
        tools: Vec::new(),
        tool_choice: None,
    }
}

fn session_key(session_id: Option<&str>) -> String {
    match session_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(session_id) => format!("api:{session_id}"),
        None => API_SESSION_KEY.to_owned(),
    }
}

fn stream_response_body(
    events: &[ProviderEvent],
    response: &LlmResponse,
    model: &str,
    request_id: &str,
    created: u64,
) -> String {
    let mut body = String::new();
    let mut saw_finish = false;
    for event in events {
        if matches!(event, ProviderEvent::Finish { .. }) {
            saw_finish = true;
        }
        body.push_str(&stream_event_frame(event, model, request_id, created));
    }
    if !saw_finish {
        body.push_str(&finish_stream_frame(
            model,
            request_id,
            created,
            &response.finish_reason,
        ));
    }
    body.push_str(&done_stream_frame());
    body
}

fn is_multipart_content_type(content_type: &str) -> bool {
    content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
}

fn usage_json(usage: &BTreeMap<String, u64>) -> Value {
    json!({
        "prompt_tokens": usage.get("prompt_tokens").copied().unwrap_or(0),
        "completion_tokens": usage.get("completion_tokens").copied().unwrap_or(0),
        "total_tokens": usage.get("total_tokens").copied().unwrap_or(0),
    })
}

fn finish_stream_chunk(model: &str, request_id: &str, created: u64, reason: &str) -> Value {
    json!({
        "id": request_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {},
            "finish_reason": reason,
        }],
    })
}

fn sse_data_frame(value: &Value) -> String {
    format!("data: {value}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use shacs_providers::types::{text_response, usage};
    use shacs_session::{Session, SessionManager};
    use std::error::Error;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration as StdDuration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::oneshot;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
    use tower::ServiceExt;

    struct FakeAdapter {
        model: String,
        response: LlmResponse,
        stream_events: Vec<ProviderEvent>,
        captured: Mutex<Vec<ChatCompletionInvocation>>,
        websocket_frames: Mutex<Vec<Value>>,
        session_workspace: Option<PathBuf>,
        workflow_recipes_projection: Option<Value>,
    }

    struct SlowAdapter {
        model: String,
        delay: StdDuration,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    impl SlowAdapter {
        fn new(delay: StdDuration) -> Self {
            Self {
                model: "gpt-5".to_owned(),
                delay,
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl ChatCompletionAdapter for SlowAdapter {
        fn configured_model(&self) -> &str {
            &self.model
        }

        fn complete_chat(
            &self,
            _invocation: ChatCompletionInvocation,
        ) -> Result<LlmResponse, ApiError> {
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            thread::sleep(self.delay);
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(text_response("slow ok"))
        }
    }

    impl FakeAdapter {
        fn new(model: &str, response: LlmResponse) -> Self {
            Self {
                model: model.to_owned(),
                response,
                stream_events: Vec::new(),
                captured: Mutex::new(Vec::new()),
                websocket_frames: Mutex::new(Vec::new()),
                session_workspace: None,
                workflow_recipes_projection: None,
            }
        }

        fn with_session_workspace(mut self, workspace: PathBuf) -> Self {
            self.session_workspace = Some(workspace);
            self
        }

        fn with_stream_events(mut self, events: Vec<ProviderEvent>) -> Self {
            self.stream_events = events;
            self
        }

        fn with_workflow_recipes_projection(mut self, projection: Value) -> Self {
            self.workflow_recipes_projection = Some(projection);
            self
        }

        fn call_count(&self) -> usize {
            self.captured
                .lock()
                .map(|requests| requests.len())
                .unwrap_or(0)
        }

        fn captured_invocation(&self) -> Option<ChatCompletionInvocation> {
            self.captured
                .lock()
                .ok()
                .and_then(|requests| requests.first().cloned())
        }

        fn websocket_frame_count(&self) -> usize {
            self.websocket_frames
                .lock()
                .map(|frames| frames.len())
                .unwrap_or(0)
        }
    }

    impl ChatCompletionAdapter for FakeAdapter {
        fn configured_model(&self) -> &str {
            &self.model
        }

        fn complete_chat(
            &self,
            invocation: ChatCompletionInvocation,
        ) -> Result<LlmResponse, ApiError> {
            self.captured
                .lock()
                .map_err(|_| ApiError::internal("fake adapter lock failed"))?
                .push(invocation);
            Ok(self.response.clone())
        }

        fn stream_chat(
            &self,
            invocation: ChatCompletionInvocation,
            on_event: &mut dyn FnMut(ProviderEvent),
        ) -> Result<LlmResponse, ApiError> {
            self.captured
                .lock()
                .map_err(|_| ApiError::internal("fake adapter lock failed"))?
                .push(invocation);
            for event in &self.stream_events {
                on_event(event.clone());
            }
            Ok(self.response.clone())
        }

        fn persist_media_data_urls(&self, data_urls: &[String]) -> Result<Vec<String>, ApiError> {
            Ok(data_urls
                .iter()
                .filter(|url| url.starts_with("data:"))
                .map(|url| format!("/fake/{}.bin", url.len()))
                .collect())
        }

        fn persist_uploaded_file(
            &self,
            filename: Option<&str>,
            bytes: &[u8],
        ) -> Result<String, ApiError> {
            Ok(format!(
                "/fake/{}-{}",
                bytes.len(),
                filename.unwrap_or("upload.bin")
            ))
        }

        fn session_workspace(&self) -> Option<PathBuf> {
            self.session_workspace.clone()
        }

        fn diagnostics_snapshot(&self) -> DiagnosticsSnapshot {
            let mut snapshot = DiagnosticsSnapshot::unavailable("fake diagnostics configured");
            snapshot.generated_at_ms = 1;
            for diagnostic in &mut snapshot.diagnostics {
                diagnostic.timestamp_ms = 1;
            }
            snapshot.runtime = json!({
                "provider": "fake",
                "api_key": "sk-raw-secret",
                "config_path": "/tmp/shacs/config.json",
            });
            snapshot
        }

        fn workflow_recipes_projection(&self) -> Option<Value> {
            self.workflow_recipes_projection.clone()
        }

        fn process_websocket_frame(
            &self,
            frame: Value,
            client_id: &str,
            default_chat_id: &str,
        ) -> Result<Vec<WebSocketServerEvent>, ApiError> {
            self.websocket_frames
                .lock()
                .map_err(|_| ApiError::internal("fake websocket frame lock failed"))?
                .push(frame);
            Ok(vec![WebSocketServerEvent::Ready {
                chat_id: default_chat_id.to_owned(),
                client_id: client_id.to_owned(),
            }])
        }
    }

    #[test]
    fn validates_minimal_chat_completion_request() -> Result<(), Box<dyn Error>> {
        let request = parse_chat_completion_request(&json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "hi"}],
            "stream": false,
            "session_id": "work"
        }))?;

        let validated = validate_chat_completion_request(&request, "gpt-5")?;
        assert_eq!(validated.model, "gpt-5");
        assert_eq!(validated.session_key, "api:work");
        assert_eq!(validated.content, "hi");
        assert!(!validated.stream);

        let provider_request = provider_request_from_chat_request(&request, "gpt-5")?;
        assert_eq!(provider_request.model, "gpt-5");
        assert_eq!(provider_request.settings.temperature, 0.7);
        assert_eq!(provider_request.settings.max_tokens, 4096);
        assert_eq!(provider_request.messages[0]["role"], "user");
        assert_eq!(provider_request.messages[0]["content"], "hi");

        let invocation = chat_completion_invocation(&request, "gpt-5")?;
        assert_eq!(invocation.session_key, "api:work");
        assert_eq!(invocation.requested_model.as_deref(), Some("gpt-5"));
        Ok(())
    }

    #[test]
    fn validates_text_and_data_url_parts_but_rejects_remote_images() -> Result<(), Box<dyn Error>> {
        let request = parse_chat_completion_request(&json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
                {"type": "image_url", "image_url": {"url": "data:video/mp4;base64,BBBB"}}
            ]}]
        }))?;
        let validated = validate_chat_completion_request(&request, "model")?;
        assert_eq!(validated.content, "describe");
        assert_eq!(
            validated.media_data_urls,
            ["data:image/png;base64,AAAA", "data:video/mp4;base64,BBBB"]
        );
        let provider_request = provider_request_from_chat_request(&request, "model")?;
        assert_eq!(provider_request.messages[0]["content"][0]["type"], "text");
        assert_eq!(
            provider_request.messages[0]["content"][0]["text"],
            "describe"
        );
        assert_eq!(
            provider_request.messages[0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
        let provider_content = provider_request.messages[0]["content"].to_string();
        assert!(!provider_content.contains("data:video/mp4"));

        let request = parse_chat_completion_request(&json!({
            "messages": [{"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "https://example.com/a.png"}}
            ]}]
        }))?;
        let error = validate_chat_completion_request(&request, "model").unwrap_err();
        assert_eq!(error.status, 400);
        assert!(error.message.contains("remote image"));
        Ok(())
    }

    #[test]
    fn preserves_non_image_data_urls_without_forwarding_them_as_images(
    ) -> Result<(), Box<dyn Error>> {
        let request = parse_chat_completion_request(&json!({
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "analyze"},
                {"type": "image_url", "image_url": {"url": "data:audio/mpeg;base64,AAAA"}},
                {"type": "image_url", "image_url": {"url": "data:video/mp4;base64,BBBB"}}
            ]}]
        }))?;

        let validated = validate_chat_completion_request(&request, "model")?;
        assert_eq!(
            validated.media_data_urls,
            ["data:audio/mpeg;base64,AAAA", "data:video/mp4;base64,BBBB"]
        );
        let provider_request = provider_request_from_chat_request(&request, "model")?;
        assert_eq!(provider_request.messages[0]["content"], "analyze");

        Ok(())
    }

    #[test]
    fn rejects_invalid_chat_completion_requests_without_runtime() {
        let error = parse_chat_completion_request(&json!("bad")).unwrap_err();
        assert_eq!(error.status, 400);

        let request = parse_chat_completion_request(&json!({
            "messages": []
        }))
        .expect("empty messages still deserialize");
        let error = validate_chat_completion_request(&request, "model").unwrap_err();
        assert_eq!(error.status, 400);
        assert!(error.message.contains("exactly one"));

        let request = parse_chat_completion_request(&json!({
            "messages": [{"role": "assistant", "content": "hi"}]
        }))
        .expect("assistant message still deserializes");
        let error = validate_chat_completion_request(&request, "model").unwrap_err();
        assert_eq!(error.status, 400);
        assert!(error.message.contains("user"));
    }

    #[test]
    fn rejects_request_model_mismatch_before_provider_invocation() -> Result<(), Box<dyn Error>> {
        let request = parse_chat_completion_request(&json!({
            "model": "gpt-4.1",
            "messages": [{"role": "user", "content": "hi"}]
        }))?;
        let error = validate_chat_completion_request(&request, "gpt-5").unwrap_err();
        assert_eq!(error.status, 400);
        assert!(error.message.contains("gpt-4.1"));
        assert!(error.message.contains("gpt-5"));
        Ok(())
    }

    #[test]
    fn formats_health_models_error_and_non_stream_response_envelopes() {
        assert_eq!(API_SESSION_KEY, "api:default");
        assert_eq!(API_CHAT_ID, "default");
        assert_eq!(MAX_FILE_SIZE, MAX_MEDIA_BYTES);
        assert!(EMPTY_FINAL_RESPONSE_MESSAGE.contains("couldn't produce a final answer"));
        assert_eq!(health_response(), json!({"status": "ok"}));
        assert_eq!(models_response("gpt-5")["data"][0]["id"], "gpt-5");
        assert_eq!(models_response("gpt-5")["data"][0]["owned_by"], "shacs-bot");

        let error = ApiError::invalid_request("bad input");
        assert_eq!(api_error_response(&error)["error"]["code"], 400);

        let mut response = text_response("hello");
        response.usage = usage(3, 2, 5);
        let envelope = chat_completion_response(&response, "gpt-5", "chatcmpl-test", 123);
        assert_eq!(envelope["object"], "chat.completion");
        assert_eq!(envelope["choices"][0]["message"]["role"], "assistant");
        assert_eq!(envelope["choices"][0]["message"]["content"], "hello");
        assert_eq!(envelope["choices"][0]["finish_reason"], "stop");
        assert_eq!(envelope["usage"]["total_tokens"], 5);
    }

    #[test]
    fn http_boundary_routes_health_and_models_without_runtime() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let health = handle_api_request(ApiHttpRequest::get(HEALTH_PATH), &adapter);
        assert_eq!(health.status, 200);
        assert_eq!(health.content_type, JSON_CONTENT_TYPE);
        assert_eq!(health.body, json!({"status": "ok"}));

        let models = handle_api_request(ApiHttpRequest::get(MODELS_PATH), &adapter);
        assert_eq!(models.status, 200);
        assert_eq!(models.body["object"], "list");
        assert_eq!(models.body["data"][0]["id"], "gpt-5");
        assert_eq!(models.body["data"][0]["owned_by"], "shacs-bot");
        assert_eq!(adapter.call_count(), 0);
    }

    #[test]
    fn api_diagnostics_inspect_is_read_only_redacted_and_matches_cli_projection() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let response = handle_api_request(ApiHttpRequest::get(DIAGNOSTICS_PATH), &adapter);

        assert_eq!(response.status, 200);
        assert_eq!(
            response.body,
            adapter.diagnostics_snapshot().redacted_value()
        );
        assert_eq!(response.body["runtime"]["api_key"], "[REDACTED]");
        let serialized = serde_json::to_string(&response.body).unwrap_or_default();
        assert!(!serialized.contains("sk-raw-secret"));
        assert_eq!(adapter.call_count(), 0);
        assert_eq!(adapter.websocket_frame_count(), 0);
    }

    #[test]
    fn workflow_recipe_projection_route_uses_adapter_projection() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"))
            .with_workflow_recipes_projection(json!({
                "schema_label": "024WorkflowRecipeProjection",
                "schema_version": "024WorkflowRecipeProjection.v1",
                "object": "list",
                "data": [{
                    "recipe_id": "review-loop",
                    "readiness": "ready",
                    "pattern": "loop_until_done"
                }]
            }));

        let response = handle_api_request(ApiHttpRequest::get(WORKFLOW_RECIPES_PATH), &adapter);

        assert_eq!(response.status, 200);
        assert_eq!(
            response.body["schema_version"],
            "024WorkflowRecipeProjection.v1"
        );
        assert_eq!(response.body["data"][0]["recipe_id"], "review-loop");
        assert_eq!(adapter.call_count(), 0);
        assert_eq!(adapter.websocket_frame_count(), 0);
    }

    #[test]
    fn http_boundary_json_chat_non_stream_invokes_adapter_once() -> Result<(), Box<dyn Error>> {
        let mut llm = text_response("hello from adapter");
        llm.usage = usage(10, 4, 14);
        let adapter = FakeAdapter::new("gpt-5", llm);

        let response = handle_api_request(
            ApiHttpRequest::post_json(
                CHAT_COMPLETIONS_PATH,
                json!({
                    "model": "gpt-5",
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            &adapter,
        );

        assert_eq!(response.status, 200);
        assert_eq!(response.body["object"], "chat.completion");
        assert_eq!(
            response.body["choices"][0]["message"]["content"],
            "hello from adapter"
        );
        assert_eq!(response.body["usage"]["total_tokens"], 14);
        assert_eq!(adapter.call_count(), 1);
        let captured = adapter
            .captured_invocation()
            .ok_or("adapter should capture provider invocation")?;
        assert_eq!(captured.session_key, "api:default");
        assert_eq!(captured.requested_model.as_deref(), Some("gpt-5"));
        assert!(captured.media_data_urls.is_empty());
        assert_eq!(captured.provider_request.model, "gpt-5");
        assert_eq!(captured.provider_request.messages[0]["role"], "user");
        assert_eq!(captured.provider_request.messages[0]["content"], "hello");
        Ok(())
    }

    #[test]
    fn compatibility_handle_chat_completions_wrapper_matches_http_boundary(
    ) -> Result<(), Box<dyn Error>> {
        let adapter = FakeAdapter::new("gpt-5", text_response("hello from wrapper"));

        let response = handle_chat_completions(
            ApiHttpRequest::post_json(
                CHAT_COMPLETIONS_PATH,
                json!({
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            &adapter,
        );

        assert_eq!(response.status, 200);
        assert_eq!(
            response.body["choices"][0]["message"]["content"],
            "hello from wrapper"
        );
        assert_eq!(adapter.call_count(), 1);
        Ok(())
    }

    #[test]
    fn http_boundary_preserves_session_key_and_data_urls_for_adapter() -> Result<(), Box<dyn Error>>
    {
        let adapter = FakeAdapter::new("gpt-5", text_response("ok"));

        let response = handle_api_request(
            ApiHttpRequest::post_json(
                CHAT_COMPLETIONS_PATH,
                json!({
                    "session_id": "images",
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": "describe"},
                        {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}
                    ]}]
                }),
            ),
            &adapter,
        );

        assert_eq!(response.status, 200);
        let captured = adapter
            .captured_invocation()
            .ok_or("adapter should capture provider invocation")?;
        assert_eq!(captured.session_key, "api:images");
        assert_eq!(captured.media_data_urls, ["data:image/png;base64,AAAA"]);
        assert_eq!(
            captured.provider_request.messages[0]["content"][0]["text"],
            "describe"
        );
        assert_eq!(
            captured.provider_request.messages[0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAA"
        );
        Ok(())
    }

    #[test]
    fn http_boundary_rejects_model_mismatch_before_adapter_invocation() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let response = handle_api_request(
            ApiHttpRequest::post_json(
                CHAT_COMPLETIONS_PATH,
                json!({
                    "model": "gpt-4.1",
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            &adapter,
        );

        assert_eq!(response.status, 400);
        assert_eq!(response.body["error"]["type"], "invalid_request_error");
        assert_eq!(adapter.call_count(), 0);
    }

    #[test]
    fn http_boundary_defers_multipart_but_streams_without_axum() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let multipart = handle_api_request(
            ApiHttpRequest::post_json(CHAT_COMPLETIONS_PATH, json!({}))
                .with_header("content-type", "multipart/form-data; boundary=x"),
            &adapter,
        );
        assert_eq!(multipart.status, 415);
        assert_eq!(multipart.body["error"]["type"], "unsupported_media_type");

        let stream = handle_api_request(
            ApiHttpRequest::post_json(
                CHAT_COMPLETIONS_PATH,
                json!({
                    "stream": true,
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            ),
            &adapter,
        );
        assert_eq!(stream.status, 200);
        assert_eq!(stream.content_type, SSE_CONTENT_TYPE);
        assert!(stream
            .body
            .as_str()
            .unwrap_or_default()
            .contains("data: [DONE]"));
        assert_eq!(adapter.call_count(), 1);
    }

    #[test]
    fn http_boundary_rejects_multipart_with_mixed_case_content_type() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let mut request = ApiHttpRequest::post_json(CHAT_COMPLETIONS_PATH, json!({}));
        request.headers.clear();
        request.headers.insert(
            "Content-Type".to_owned(),
            "multipart/form-data; boundary=x".to_owned(),
        );
        let response = handle_api_request(request, &adapter);

        assert_eq!(response.status, 415);
        assert_eq!(response.body["error"]["type"], "unsupported_media_type");
        assert_eq!(adapter.call_count(), 0);
    }

    #[test]
    fn http_boundary_returns_explicit_route_errors() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let not_found = handle_api_request(ApiHttpRequest::get("/missing"), &adapter);
        assert_eq!(not_found.status, 404);
        assert_eq!(not_found.body["error"]["type"], "not_found");

        let method = handle_api_request(ApiHttpRequest::get(CHAT_COMPLETIONS_PATH), &adapter);
        assert_eq!(method.status, 405);
        assert_eq!(method.body["error"]["type"], "method_not_allowed");
    }

    #[test]
    fn session_query_surface_fails_closed_without_workspace() {
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"));

        let list = handle_api_request(ApiHttpRequest::get(SESSIONS_PATH), &adapter);
        assert_eq!(list.status, 404);
        assert_eq!(list.body["error"]["type"], "not_found");
        assert!(list.body["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("session query surface is not configured"));

        let history = handle_api_request(
            ApiHttpRequest::get("/v1/sessions/api:work/history"),
            &adapter,
        );
        assert_eq!(history.status, 404);
        assert_eq!(adapter.call_count(), 0);
    }

    #[test]
    fn formats_chat_completion_stream_chunks_as_sse_data_frames() {
        let frame = stream_event_frame(
            &ProviderEvent::TextDelta {
                text: "hel".to_owned(),
            },
            "gpt-5",
            "chatcmpl-test",
            123,
        );
        assert!(frame.starts_with("data: "));
        assert!(frame.ends_with("\n\n"));
        assert!(frame.contains("chat.completion.chunk"));
        assert!(frame.contains("\"content\":\"hel\""));

        let finish = finish_stream_frame("gpt-5", "chatcmpl-test", 123, "stop");
        assert!(finish.contains("\"finish_reason\":\"stop\""));
        assert_eq!(done_stream_frame(), "data: [DONE]\n\n");
    }

    #[test]
    fn defers_multipart_chat_completion_requests() {
        let error = reject_multipart_request("multipart/form-data; boundary=x")
            .expect("multipart should be explicitly deferred");
        assert_eq!(error.status, 415);
        assert!(error.message.contains("axum API runtime"));
        assert!(reject_multipart_request("application/json").is_none());
    }

    #[test]
    fn chat_completion_ids_are_stable_and_prefixed() {
        assert_eq!(chat_completion_id("same"), chat_completion_id("same"));
        assert!(chat_completion_id("same").starts_with("chatcmpl-"));
    }

    #[tokio::test]
    async fn axum_router_routes_health_and_models_without_port() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let app = api_router(adapter.clone());

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(HEALTH_PATH)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(response_json(health).await?, json!({"status": "ok"}));

        let models = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(MODELS_PATH)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(models.status(), StatusCode::OK);
        let body = response_json(models).await?;
        assert_eq!(body["object"], "list");
        assert_eq!(body["data"][0]["id"], "gpt-5");
        assert_eq!(body["data"][0]["owned_by"], "shacs-bot");
        assert_eq!(adapter.call_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_exposes_session_query_projection_without_raw_detail(
    ) -> Result<(), Box<dyn Error>> {
        let workspace = unique_test_dir("api-session-query")?;
        let mut manager = SessionManager::new(&workspace)?;
        let mut session = Session::new("api:work");
        session
            .metadata
            .insert("api_token".to_owned(), json!("secret-value"));
        session.metadata.insert(
            "runtime_checkpoint".to_owned(),
            json!({ "phase": "awaiting_tools", "raw": "hidden" }),
        );
        session.metadata.insert(
            "runtime_workflow".to_owned(),
            json!({
                "raw_prompt": "hidden workflow prompt",
                "projection": {
                    "schema_label": "024WorkflowProjection",
                    "schema_version": "024WorkflowProjection.v1",
                    "workflow_id": "wf-api",
                    "objective_summary": "hidden objective",
                    "pattern": "fan_out_and_synthesize",
                    "state": "Succeeded",
                    "progress_count": 4,
                    "active_child_count": 0,
                    "pending_barrier_count": 0,
                    "verifier_status": "passed",
                    "budget_usage": {
                        "known_tokens": 10,
                        "estimated_tokens": 20,
                        "child_runs": 4,
                        "verifier_runs": 1,
                        "heavy_commands": 0
                    },
                    "resume_available": false,
                    "worktree_refs": ["secret diff"],
                    "evidence_refs": [{"id": "secret evidence"}]
                }
            }),
        );
        session.add_message("user", "hello", Map::new());
        let mut assistant_extra = Map::new();
        assistant_extra.insert(
            "tool_calls".to_owned(),
            json!([{"id": "call-1", "type": "function", "function": {"name": "raw_tool", "arguments": "hidden args"}}]),
        );
        assistant_extra.insert("reasoning_content".to_owned(), json!("hidden reasoning"));
        session.add_message("assistant", "world", assistant_extra);
        manager.save(&session)?;

        let adapter = Arc::new(
            FakeAdapter::new("gpt-5", text_response("unused")).with_session_workspace(workspace),
        );
        let app = api_router(adapter.clone());

        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(SESSIONS_PATH)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(list.status(), StatusCode::OK);
        let list_body = response_json(list).await?;
        assert_eq!(list_body["data"][0]["key"], "api:work");
        assert!(!list_body.to_string().contains("secret-value"));

        let detail = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/sessions/api:work")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(detail.status(), StatusCode::OK);
        let detail_body = response_json(detail).await?;
        assert_eq!(
            detail_body["metadata_keys"],
            json!(["api_token", "runtime_checkpoint", "runtime_workflow"])
        );
        assert_eq!(detail_body["checkpoint_phase"], "awaiting_tools");
        assert_eq!(detail_body["runtime_workflow"]["workflow_id"], "wf-api");
        assert_eq!(
            detail_body["runtime_workflow"]["pattern"],
            "fan_out_and_synthesize"
        );
        assert_eq!(detail_body["runtime_workflow"]["progress_count"], 4);
        assert_eq!(
            detail_body["runtime_workflow"]["budget_usage"]["child_runs"],
            4
        );
        assert_eq!(detail_body["runtime_workflow"]["worktree_ref_count"], 1);
        assert_eq!(detail_body["runtime_workflow"]["evidence_ref_count"], 1);
        assert!(detail_body.get("messages").is_none());
        let detail_text = detail_body.to_string();
        assert!(!detail_text.contains("secret-value"));
        assert!(!detail_text.contains("hidden"));
        assert!(!detail_text.contains("secret diff"));
        assert!(!detail_text.contains("secret evidence"));

        let history = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/sessions/api:work/history")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(history.status(), StatusCode::OK);
        let history_body = response_json(history).await?;
        assert_eq!(history_body["history"][0]["content"], "hello");
        assert_eq!(history_body["history"][1]["content"], "world");
        assert!(!history_body.to_string().contains("tool_calls"));
        assert!(!history_body.to_string().contains("raw_tool"));
        assert!(!history_body.to_string().contains("hidden reasoning"));

        let diagnostics = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/sessions/api:work/diagnostics")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(diagnostics.status(), StatusCode::OK);
        let diagnostics_body = response_json(diagnostics).await?;
        assert_eq!(diagnostics_body["exists"], true);
        assert_eq!(diagnostics_body["checkpoint_phase"], "awaiting_tools");
        assert_eq!(
            diagnostics_body["runtime_workflow"]["verifier_status"],
            "passed"
        );
        assert!(!diagnostics_body.to_string().contains("secret-value"));
        assert!(!diagnostics_body.to_string().contains("hidden"));
        assert!(!diagnostics_body.to_string().contains("secret diff"));
        assert_eq!(adapter.call_count(), 0);
        Ok(())
    }

    #[test]
    fn session_query_routes_are_read_only_and_validate_segments() -> Result<(), Box<dyn Error>> {
        let workspace = unique_test_dir("api-session-read-only")?;
        let adapter = FakeAdapter::new("gpt-5", text_response("unused"))
            .with_session_workspace(workspace.clone());

        let list = handle_api_request(ApiHttpRequest::get(SESSIONS_PATH), &adapter);
        assert_eq!(list.status, 200);
        assert_eq!(list.body["data"], json!([]));

        let missing = handle_api_request(ApiHttpRequest::get("/v1/sessions/missing"), &adapter);
        assert_eq!(missing.status, 404);

        let missing_history = handle_api_request(
            ApiHttpRequest::get("/v1/sessions/missing/history"),
            &adapter,
        );
        assert_eq!(missing_history.status, 404);

        let missing_diagnostics = handle_api_request(
            ApiHttpRequest::get("/v1/sessions/missing/diagnostics"),
            &adapter,
        );
        assert_eq!(missing_diagnostics.status, 200);
        assert_eq!(missing_diagnostics.body["exists"], false);

        let invalid_percent = handle_api_request(ApiHttpRequest::get("/v1/sessions/%zz"), &adapter);
        assert_eq!(invalid_percent.status, 404);

        let extra_segment = handle_api_request(
            ApiHttpRequest::get("/v1/sessions/missing/history/extra"),
            &adapter,
        );
        assert_eq!(extra_segment.status, 404);

        let wrong_method = handle_api_request(
            ApiHttpRequest {
                method: ApiMethod::Post,
                path: SESSIONS_PATH.to_owned(),
                headers: BTreeMap::new(),
                body: None,
            },
            &adapter,
        );
        assert_eq!(wrong_method.status, 405);
        assert!(!workspace.join("sessions").exists());
        Ok(())
    }

    #[test]
    fn session_query_routes_decode_encoded_session_keys() -> Result<(), Box<dyn Error>> {
        let workspace = unique_test_dir("api-session-encoded")?;
        let mut manager = SessionManager::new(&workspace)?;
        let mut session = Session::new("api:encoded/key");
        session.add_message("user", "hello", Map::new());
        manager.save(&session)?;
        let adapter =
            FakeAdapter::new("gpt-5", text_response("unused")).with_session_workspace(workspace);

        let detail = handle_api_request(
            ApiHttpRequest::get("/v1/sessions/api%3Aencoded%2Fkey"),
            &adapter,
        );
        assert_eq!(detail.status, 200);
        assert_eq!(detail.body["key"], "api:encoded/key");

        let history = handle_api_request(
            ApiHttpRequest::get("/v1/sessions/api%3Aencoded%2Fkey/history"),
            &adapter,
        );
        assert_eq!(history.status, 200);
        assert_eq!(history.body["history"][0]["content"], "hello");
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_exposes_websocket_upgrade_boundary() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let app = api_router(adapter.clone());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!("{WEBSOCKET_PATH}?client_id=browser&chat_id=work"))
                    .body(Body::empty())?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(adapter.call_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn websocket_only_router_does_not_expose_chat_completions() -> Result<(), Box<dyn Error>>
    {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let app = websocket_router_with_timeout_and_path(
            adapter.clone(),
            Duration::from_secs(30),
            WEBSOCKET_PATH,
        );

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(HEALTH_PATH)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(health.status(), StatusCode::OK);

        let chat = app
            .oneshot(json_request(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                json!({"messages": [{"role": "user", "content": "hidden?"}]}),
            )?)
            .await?;
        assert_eq!(chat.status(), StatusCode::NOT_FOUND);
        assert_eq!(adapter.call_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn web_ui_router_serves_static_api_and_websocket_on_one_surface(
    ) -> Result<(), Box<dyn Error>> {
        let assets_dir = unique_test_dir("web-ui-router")?;
        fs::write(assets_dir.join("index.html"), "<html>app</html>")?;
        fs::write(assets_dir.join("app.js"), "console.log('app')")?;
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let app = web_ui_router_with_timeout_and_websocket_path(
            adapter.clone(),
            Duration::from_secs(30),
            WEBSOCKET_PATH,
            assets_dir.clone(),
        );

        let root = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(root.status(), StatusCode::OK);
        assert_eq!(response_text(root).await?, "<html>app</html>");

        let asset = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/app.js")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(asset.status(), StatusCode::OK);
        assert_eq!(response_text(asset).await?, "console.log('app')");

        let api_404 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/unknown")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(api_404.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_json(api_404).await?["error"]["type"], "not_found");

        let ws = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(WEBSOCKET_PATH)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(ws.status(), StatusCode::BAD_REQUEST);
        assert_eq!(adapter.call_count(), 0);
        let _ = fs::remove_dir_all(assets_dir);
        Ok(())
    }

    #[tokio::test]
    async fn serve_websocket_listener_round_trips_json_frames() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(serve_websocket_listener_with_timeout_and_path(
            listener,
            adapter.clone(),
            Duration::from_secs(30),
            WEBSOCKET_PATH,
            async {
                let _ = shutdown_rx.await;
            },
        ));

        let url = format!("ws://{addr}{WEBSOCKET_PATH}");
        let (mut websocket, _) = connect_async(url).await?;
        websocket
            .send(TungsteniteMessage::Text(
                json!({"type": "new_chat"}).to_string().into(),
            ))
            .await?;
        let Some(message) = websocket.next().await else {
            return Err("expected websocket response".into());
        };
        let text = message?.into_text()?;
        let event: WebSocketServerEvent = serde_json::from_str(&text)?;
        assert_eq!(
            event,
            WebSocketServerEvent::Ready {
                chat_id: "default".to_owned(),
                client_id: "websocket-client".to_owned(),
            }
        );
        assert_eq!(adapter.websocket_frame_count(), 1);
        let _ = websocket.close(None).await;
        let _ = shutdown_tx.send(());
        server.await??;
        Ok(())
    }

    #[test]
    fn websocket_json_from_bytes_rejects_oversized_frames_before_parse() {
        let bytes = vec![b' '; MAX_REQUEST_BODY_BYTES + 1];

        let error = websocket_json_from_bytes(&bytes).expect_err("oversized frame should fail");

        assert_eq!(error.status, 413);
        assert_eq!(error.error_type, "payload_too_large");
        assert!(error.message.contains("websocket frame exceeds"));
    }

    #[test]
    fn websocket_json_from_bytes_hides_parser_details() {
        let error = websocket_json_from_bytes(b"{").expect_err("invalid JSON should fail");

        assert_eq!(error.status, 400);
        assert_eq!(error.message, "websocket frame must be valid JSON");
    }

    #[test]
    fn websocket_origin_must_match_host_when_present() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8900"));
        assert!(validate_websocket_origin(&headers).is_ok());

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:8900"),
        );
        assert!(validate_websocket_origin(&headers).is_ok());

        headers.insert(header::ORIGIN, HeaderValue::from_static("http://evil.test"));
        let error = validate_websocket_origin(&headers).expect_err("origin mismatch should fail");
        assert_eq!(error.status, 400);
        assert_eq!(error.message, "websocket origin is not allowed");
    }

    #[tokio::test]
    async fn axum_router_invokes_chat_adapter_once() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("hello via axum")));
        let app = api_router(adapter.clone());

        let response = app
            .oneshot(json_request(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                json!({
                    "model": "gpt-5",
                    "session_id": "axum",
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            )?)
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await?;
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(body["choices"][0]["message"]["content"], "hello via axum");
        assert_eq!(adapter.call_count(), 1);
        let captured = adapter
            .captured_invocation()
            .ok_or("adapter should capture provider invocation")?;
        assert_eq!(captured.session_key, "api:axum");
        assert_eq!(captured.provider_request.messages[0]["content"], "hello");
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_accepts_json_payloads_up_to_request_body_limit(
    ) -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("large ok")));
        let app = api_router(adapter.clone());
        let data_url = format!("data:text/plain;base64,{}", "A".repeat(1024 * 1024 + 32));

        let response = app
            .oneshot(json_request(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                json!({
                    "messages": [{"role": "user", "content": [
                        {"type": "text", "text": "large"},
                        {"type": "image_url", "image_url": {"url": data_url}}
                    ]}]
                }),
            )?)
            .await?;

        let status = response.status();
        let body = response_json(response).await?;
        assert_eq!(status, StatusCode::OK, "unexpected response body: {body}");
        assert_eq!(body["choices"][0]["message"]["content"], "large ok");
        let captured = adapter
            .captured_invocation()
            .ok_or("adapter should capture large JSON invocation")?;
        assert_eq!(captured.media_data_urls.len(), 1);
        assert!(captured.media_data_urls[0].len() > 1024 * 1024);
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_streams_sse_frames_and_done() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(
            FakeAdapter::new("gpt-5", text_response("hello")).with_stream_events(vec![
                ProviderEvent::TextDelta {
                    text: "hel".to_owned(),
                },
            ]),
        );
        let app = api_router(adapter.clone());

        let response = app
            .oneshot(json_request(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                json!({
                    "stream": true,
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            )?)
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static(SSE_CONTENT_TYPE))
        );
        let body = response_text(response).await?;
        assert!(body.contains("chat.completion.chunk"));
        assert!(body.contains("\"content\":\"hel\""));
        assert!(body.contains("data: [DONE]"));
        assert_eq!(adapter.call_count(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_persists_multipart_file_and_default_message() -> Result<(), Box<dyn Error>>
    {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("ok")));
        let app = api_router(adapter.clone());
        let boundary = "x-shacs-boundary";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
        );

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(CHAT_COMPLETIONS_PATH)
                    .header(
                        header::CONTENT_TYPE,
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))?,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let captured = adapter
            .captured_invocation()
            .ok_or("adapter should capture multipart invocation")?;
        assert_eq!(
            captured.provider_request.messages[0]["content"],
            MULTIPART_DEFAULT_MESSAGE
        );
        assert_eq!(captured.media_paths, ["/fake/5-a.txt"]);
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_timeout_returns_504() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(SlowAdapter::new(StdDuration::from_millis(100)));
        let app = api_router_with_timeout(adapter, Duration::from_millis(10));

        let response = app
            .oneshot(json_request(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                json!({"messages": [{"role": "user", "content": "hello"}]}),
            )?)
            .await?;

        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(
            response_json(response).await?["error"]["type"],
            "timeout_error"
        );
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_serializes_same_session() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(SlowAdapter::new(StdDuration::from_millis(25)));
        let max_active = adapter.max_active.clone();
        let app = api_router(adapter);
        let first = app.clone().oneshot(json_request(
            Method::POST,
            CHAT_COMPLETIONS_PATH,
            json!({"session_id": "same", "messages": [{"role": "user", "content": "one"}]}),
        )?);
        let second = app.oneshot(json_request(
            Method::POST,
            CHAT_COMPLETIONS_PATH,
            json!({"session_id": "same", "messages": [{"role": "user", "content": "two"}]}),
        )?);

        let (first, second) = tokio::join!(first, second);
        assert_eq!(first?.status(), StatusCode::OK);
        assert_eq!(second?.status(), StatusCode::OK);
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_rejects_errors_before_adapter_invocation() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let app = api_router(adapter.clone());

        let mismatch = app
            .clone()
            .oneshot(json_request(
                Method::POST,
                CHAT_COMPLETIONS_PATH,
                json!({
                    "model": "gpt-4.1",
                    "messages": [{"role": "user", "content": "hello"}]
                }),
            )?)
            .await?;
        assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(mismatch).await?["error"]["type"],
            "invalid_request_error"
        );

        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/missing")
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_json(missing).await?["error"]["type"], "not_found");

        let wrong_method = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(CHAT_COMPLETIONS_PATH)
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            response_json(wrong_method).await?["error"]["type"],
            "method_not_allowed"
        );
        assert_eq!(adapter.call_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn axum_router_preserves_route_status_before_body_parsing() -> Result<(), Box<dyn Error>>
    {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let app = api_router(adapter.clone());

        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(HEALTH_PATH)
                    .header(header::CONTENT_TYPE, JSON_CONTENT_TYPE)
                    .body(Body::from("not-json"))?,
            )
            .await?;
        assert_eq!(health.status(), StatusCode::OK);

        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/missing")
                    .header(header::CONTENT_TYPE, JSON_CONTENT_TYPE)
                    .body(Body::from("not-json"))?,
            )
            .await?;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(response_json(missing).await?["error"]["type"], "not_found");

        let wrong_method = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(CHAT_COMPLETIONS_PATH)
                    .header(header::CONTENT_TYPE, JSON_CONTENT_TYPE)
                    .body(Body::from("not-json"))?,
            )
            .await?;
        assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            response_json(wrong_method).await?["error"]["type"],
            "method_not_allowed"
        );

        let invalid_multipart = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(CHAT_COMPLETIONS_PATH)
                    .header(header::CONTENT_TYPE, "multipart/form-data; boundary=x")
                    .body(Body::from("not-multipart"))?,
            )
            .await?;
        assert_eq!(invalid_multipart.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(invalid_multipart).await?["error"]["type"],
            "invalid_request_error"
        );
        assert_eq!(adapter.call_count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn serve_api_listener_handles_health_over_tcp() -> Result<(), Box<dyn Error>> {
        let adapter = Arc::new(FakeAdapter::new("gpt-5", text_response("unused")));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(serve_api_listener(listener, adapter, async {
            let _ = shutdown_rx.await;
        }));

        let mut stream = TcpStream::connect(addr).await?;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains(r#"{"status":"ok"}"#));
        let _ = shutdown_tx.send(());
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn serve_api_listener_returns_sanitized_session_history_over_tcp(
    ) -> Result<(), Box<dyn Error>> {
        let workspace = unique_test_dir("api-session-tcp-history")?;
        let mut manager = SessionManager::new(&workspace)?;
        let mut session = Session::new("api:tcp");
        session.add_message("user", "visible question", Map::new());
        let mut assistant_extra = Map::new();
        assistant_extra.insert(
            "tool_calls".to_owned(),
            json!([{"id": "call-1", "type": "function", "function": {"name": "hidden_tool", "arguments": "hidden args"}}]),
        );
        assistant_extra.insert("reasoning_content".to_owned(), json!("hidden reasoning"));
        session.add_message("assistant", "visible answer", assistant_extra);
        let mut tool_extra = Map::new();
        tool_extra.insert("tool_call_id".to_owned(), json!("call-1"));
        tool_extra.insert("name".to_owned(), json!("hidden_tool"));
        session.add_message("tool", "hidden tool result", tool_extra);
        manager.save(&session)?;

        let adapter = Arc::new(
            FakeAdapter::new("gpt-5", text_response("unused")).with_session_workspace(workspace),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(serve_api_listener(listener, adapter, async {
            let _ = shutdown_rx.await;
        }));

        let mut stream = TcpStream::connect(addr).await?;
        stream
            .write_all(
                b"GET /v1/sessions/api%3Atcp/history HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await?;
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;

        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.contains("visible question"), "{response}");
        assert!(response.contains("visible answer"), "{response}");
        assert!(!response.contains("tool_calls"), "{response}");
        assert!(!response.contains("hidden_tool"), "{response}");
        assert!(!response.contains("hidden reasoning"), "{response}");
        assert!(!response.contains("hidden tool result"), "{response}");
        let _ = shutdown_tx.send(());
        server.await??;
        Ok(())
    }

    fn json_request(method: Method, uri: &str, body: Value) -> Result<Request, Box<dyn Error>> {
        Ok(Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, JSON_CONTENT_TYPE)
            .body(Body::from(body.to_string()))?)
    }

    async fn response_json(response: Response) -> Result<Value, Box<dyn Error>> {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn response_text(response: Response) -> Result<String, Box<dyn Error>> {
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await?;
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    fn unique_test_dir(name: &str) -> Result<PathBuf, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "shacs-api-{name}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}
