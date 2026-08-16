// allow: SIZE_OK — preexisting channel catalog; Spec034 diff only registers and re-exports the focused spec035 media adapter
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use shacs_command::{build_help_text, normalize_channel_command};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

mod spec031;
mod spec035;

pub use spec031::{
    channel_delivery_observation_from_metadata, project_spec031_channel_event,
    ChannelDeliveryObservation, ChannelSpec031ProjectionInput, ChannelSpec031ProjectionKind,
};
pub use spec035::{
    project_spec035_media_for_channel, ChannelSpec035MediaDelivery, ChannelSpec035MediaProjection,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
    #[serde(default = "now_iso")]
    pub timestamp: String,
    #[serde(default)]
    pub media: Vec<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub session_key_override: Option<String>,
    #[serde(skip)]
    owner_accepted_automation_result: Option<OwnerAcceptedAutomationResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerAcceptedAutomationResult {
    SubagentTerminal { result_ref: String },
}

impl InboundMessage {
    pub fn new(
        channel: impl Into<String>,
        sender_id: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            sender_id: sender_id.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            timestamp: now_iso(),
            media: Vec::new(),
            metadata: Map::new(),
            session_key_override: None,
            owner_accepted_automation_result: None,
        }
    }

    pub fn session_key(&self) -> String {
        self.session_key_override
            .clone()
            .unwrap_or_else(|| format!("{}:{}", self.channel, self.chat_id))
    }

    pub fn with_media(mut self, media: impl IntoIterator<Item = String>) -> Self {
        self.media = media.into_iter().collect();
        self
    }

    pub fn with_metadata(mut self, metadata: Map<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_session_key_override(mut self, session_key: impl Into<String>) -> Self {
        self.session_key_override = Some(session_key.into());
        self
    }

    pub fn with_owner_accepted_automation_result(
        mut self,
        result: OwnerAcceptedAutomationResult,
    ) -> Self {
        self.owner_accepted_automation_result = Some(result);
        self
    }

    pub fn owner_accepted_automation_result(&self) -> Option<&OwnerAcceptedAutomationResult> {
        self.owner_accepted_automation_result.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub chat_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub media: Vec<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub buttons: Vec<Vec<String>>,
}

impl OutboundMessage {
    pub fn new(
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            reply_to: None,
            media: Vec::new(),
            metadata: Map::new(),
            buttons: Vec::new(),
        }
    }

    pub fn with_reply_to(mut self, reply_to: impl Into<String>) -> Self {
        self.reply_to = Some(reply_to.into());
        self
    }

    pub fn with_metadata(mut self, metadata: Map<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelAllowlist {
    pub allowed_senders: Vec<String>,
}

impl ChannelAllowlist {
    pub fn new(allowed_senders: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_senders: allowed_senders.into_iter().collect(),
        }
    }

    pub fn deny_all() -> Self {
        Self {
            allowed_senders: Vec::new(),
        }
    }

    pub fn allow_all() -> Self {
        Self {
            allowed_senders: vec!["*".to_owned()],
        }
    }

    pub fn is_allowed(&self, sender_id: &str) -> bool {
        self.allowed_senders
            .iter()
            .any(|entry| entry == "*" || entry == sender_id)
    }
}

impl Default for ChannelAllowlist {
    fn default() -> Self {
        Self::deny_all()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    Unsupported(&'static str),
    Delivery(String),
    DuplicateChannel(String),
    UnknownChannel(String),
    Protocol(String),
}

impl fmt::Display for ChannelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(action) => {
                write!(formatter, "channel action is unsupported: {action}")
            }
            Self::Delivery(error) => write!(formatter, "channel delivery failed: {error}"),
            Self::DuplicateChannel(name) => {
                write!(formatter, "channel is already registered: {name}")
            }
            Self::UnknownChannel(name) => write!(formatter, "channel is not registered: {name}"),
            Self::Protocol(error) => write!(formatter, "channel protocol error: {error}"),
        }
    }
}

impl std::error::Error for ChannelError {}

pub trait ChannelAdapter {
    fn name(&self) -> &str;

    fn display_name(&self) -> &str {
        self.name()
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported("start"))
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported("stop"))
    }

    fn send(&self, message: OutboundMessage) -> Result<(), ChannelError>;

    fn send_delta(
        &self,
        _chat_id: &str,
        _delta: &str,
        _metadata: Map<String, Value>,
    ) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported("send_delta"))
    }
}

pub const WEBSOCKET_CHANNEL: &str = "websocket";
pub const DISCORD_CHANNEL: &str = "discord";
pub const TELEGRAM_CHANNEL: &str = "telegram";
pub const EMAIL_CHANNEL: &str = "email";
pub const SLACK_CHANNEL: &str = "slack";
pub const WHATSAPP_CHANNEL: &str = "whatsapp";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelCapabilities {
    pub streaming: bool,
    pub media: bool,
    pub buttons: bool,
    pub external_bridge: bool,
}

impl ChannelCapabilities {
    pub fn plain_text() -> Self {
        Self {
            streaming: false,
            media: false,
            buttons: false,
            external_bridge: false,
        }
    }

    pub fn streaming_media_buttons() -> Self {
        Self {
            streaming: true,
            media: true,
            buttons: true,
            external_bridge: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelDescriptor {
    pub name: String,
    pub display_name: String,
    pub enabled_by_default: bool,
    pub capabilities: ChannelCapabilities,
}

impl ChannelDescriptor {
    pub fn new(name: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            display_name: display_name.into(),
            enabled_by_default: false,
            capabilities: ChannelCapabilities::plain_text(),
        }
    }

    pub fn enabled_by_default(mut self, enabled: bool) -> Self {
        self.enabled_by_default = enabled;
        self
    }

    pub fn with_capabilities(mut self, capabilities: ChannelCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChannelRegistry {
    descriptors: BTreeMap<String, ChannelDescriptor>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtin_channels() -> Self {
        let mut registry = Self::new();
        for descriptor in builtin_channel_descriptors() {
            registry
                .register(descriptor)
                .expect("builtin channel names are unique");
        }
        registry
    }

    pub fn register(&mut self, descriptor: ChannelDescriptor) -> Result<(), ChannelError> {
        if self.descriptors.contains_key(&descriptor.name) {
            return Err(ChannelError::DuplicateChannel(descriptor.name));
        }
        self.descriptors.insert(descriptor.name.clone(), descriptor);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&ChannelDescriptor> {
        self.descriptors.get(name)
    }

    pub fn require(&self, name: &str) -> Result<&ChannelDescriptor, ChannelError> {
        self.get(name)
            .ok_or_else(|| ChannelError::UnknownChannel(name.to_owned()))
    }

    pub fn names(&self) -> Vec<&str> {
        self.descriptors.keys().map(String::as_str).collect()
    }

    pub fn selected(&self, names: &[String]) -> Result<Vec<ChannelDescriptor>, ChannelError> {
        names
            .iter()
            .map(|name| self.require(name).cloned())
            .collect()
    }
}

pub fn builtin_channel_descriptors() -> Vec<ChannelDescriptor> {
    vec![
        ChannelDescriptor::new(WEBSOCKET_CHANNEL, "WebSocket")
            .enabled_by_default(true)
            .with_capabilities(ChannelCapabilities::streaming_media_buttons()),
        ChannelDescriptor::new(DISCORD_CHANNEL, "Discord").with_capabilities(ChannelCapabilities {
            streaming: true,
            media: true,
            buttons: false,
            external_bridge: true,
        }),
        ChannelDescriptor::new(TELEGRAM_CHANNEL, "Telegram")
            .with_capabilities(ChannelCapabilities::streaming_media_buttons()),
        ChannelDescriptor::new(EMAIL_CHANNEL, "Email").with_capabilities(ChannelCapabilities {
            streaming: false,
            media: true,
            buttons: false,
            external_bridge: true,
        }),
        ChannelDescriptor::new(SLACK_CHANNEL, "Slack").with_capabilities(ChannelCapabilities {
            streaming: true,
            media: true,
            buttons: false,
            external_bridge: true,
        }),
        ChannelDescriptor::new(WHATSAPP_CHANNEL, "WhatsApp").with_capabilities(
            ChannelCapabilities {
                streaming: false,
                media: true,
                buttons: false,
                external_bridge: true,
            },
        ),
    ]
}

pub fn builtin_channel_default_configs() -> BTreeMap<String, Value> {
    BTreeMap::from([
        (
            WEBSOCKET_CHANNEL.to_owned(),
            json!({
                "enabled": true,
                "host": "127.0.0.1",
                "port": 8765,
                "path": "/"
            }),
        ),
        (
            TELEGRAM_CHANNEL.to_owned(),
            json!({
                "enabled": false,
                "token": "",
                "pollTimeoutSeconds": 30,
                "pollLimit": 20
            }),
        ),
        (
            DISCORD_CHANNEL.to_owned(),
            json!({
                "enabled": false,
                "token": "",
                "allowFrom": [],
                "allowChannels": [],
                "groupPolicy": "mention",
                "streaming": true
            }),
        ),
        (
            SLACK_CHANNEL.to_owned(),
            json!({
                "enabled": false,
                "appToken": "",
                "botToken": "",
                "channelIds": [],
                "allowFrom": []
            }),
        ),
        (
            EMAIL_CHANNEL.to_owned(),
            json!({
                "enabled": false,
                "consentGranted": false,
                "allowFrom": [],
                "verifySpf": true,
                "verifyDkim": true,
                "smtp": {
                    "host": "",
                    "port": 587,
                    "from": "",
                    "username": "",
                    "password": "",
                    "security": "starttls",
                    "timeoutSeconds": 30
                },
                "imap": {
                    "host": "",
                    "port": 993,
                    "username": "",
                    "password": "",
                    "mailbox": "INBOX",
                    "markSeen": true,
                    "pollIntervalSeconds": 30,
                    "timeoutSeconds": 30,
                    "security": "tls"
                }
            }),
        ),
        (
            WHATSAPP_CHANNEL.to_owned(),
            json!({
                "enabled": false,
                "bridgeUrl": "",
                "bridgeToken": "",
                "groupPolicy": "open",
                "allowlist": {
                    "allowedSenders": []
                }
            }),
        ),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveChannelWorkerKind {
    WebSocketServer,
    DiscordGateway,
    TelegramLongPolling,
    EmailSmtp,
    EmailImap,
    SlackSocketMode,
    WhatsAppBridge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveChannelWorkerDescriptor {
    pub channel: String,
    pub kind: LiveChannelWorkerKind,
    pub label: String,
    pub requires_external_credentials: bool,
    pub ready_for_runtime: bool,
}

impl LiveChannelWorkerDescriptor {
    pub fn new(
        channel: impl Into<String>,
        kind: LiveChannelWorkerKind,
        label: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            kind,
            label: label.into(),
            requires_external_credentials: true,
            ready_for_runtime: false,
        }
    }

    pub fn requires_external_credentials(mut self, required: bool) -> Self {
        self.requires_external_credentials = required;
        self
    }

    pub fn ready_for_runtime(mut self, ready: bool) -> Self {
        self.ready_for_runtime = ready;
        self
    }
}

pub trait LiveChannelWorker {
    fn descriptor(&self) -> &LiveChannelWorkerDescriptor;

    fn start(&mut self) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported("live_worker_start"))
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        Err(ChannelError::Unsupported("live_worker_stop"))
    }
}

pub fn builtin_live_worker_descriptors() -> Vec<LiveChannelWorkerDescriptor> {
    vec![
        LiveChannelWorkerDescriptor::new(
            WEBSOCKET_CHANNEL,
            LiveChannelWorkerKind::WebSocketServer,
            "WebSocket server",
        )
        .requires_external_credentials(false)
        .ready_for_runtime(true),
        LiveChannelWorkerDescriptor::new(
            DISCORD_CHANNEL,
            LiveChannelWorkerKind::DiscordGateway,
            "Discord Gateway worker",
        )
        .ready_for_runtime(true),
        LiveChannelWorkerDescriptor::new(
            TELEGRAM_CHANNEL,
            LiveChannelWorkerKind::TelegramLongPolling,
            "Telegram long-polling worker",
        )
        .ready_for_runtime(true),
        LiveChannelWorkerDescriptor::new(
            EMAIL_CHANNEL,
            LiveChannelWorkerKind::EmailSmtp,
            "Email SMTP outbound worker",
        )
        .ready_for_runtime(true),
        LiveChannelWorkerDescriptor::new(
            EMAIL_CHANNEL,
            LiveChannelWorkerKind::EmailImap,
            "Email IMAP inbound worker",
        )
        .ready_for_runtime(true),
        LiveChannelWorkerDescriptor::new(
            SLACK_CHANNEL,
            LiveChannelWorkerKind::SlackSocketMode,
            "Slack Socket Mode worker",
        )
        .ready_for_runtime(true),
        LiveChannelWorkerDescriptor::new(
            WHATSAPP_CHANNEL,
            LiveChannelWorkerKind::WhatsAppBridge,
            "WhatsApp bridge worker",
        )
        .ready_for_runtime(true),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelStatus {
    pub enabled: bool,
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl ChannelStatus {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            running: false,
            last_error: None,
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            running: false,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRetryPolicy {
    pub max_attempts: usize,
}

impl Default for ChannelRetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 1 }
    }
}

#[derive(Default)]
pub struct ChannelManager {
    adapters: BTreeMap<String, Box<dyn ChannelAdapter>>,
    statuses: BTreeMap<String, ChannelStatus>,
    retry_policy: ChannelRetryPolicy,
}

impl ChannelManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_retry_policy(mut self, retry_policy: ChannelRetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    pub fn register_adapter(
        &mut self,
        adapter: Box<dyn ChannelAdapter>,
        enabled: bool,
    ) -> Result<(), ChannelError> {
        let name = adapter.name().to_owned();
        if self.adapters.contains_key(&name) {
            return Err(ChannelError::DuplicateChannel(name));
        }
        self.statuses.insert(
            name.clone(),
            if enabled {
                ChannelStatus::enabled()
            } else {
                ChannelStatus::disabled()
            },
        );
        self.adapters.insert(name, adapter);
        Ok(())
    }

    pub fn status(&self, name: &str) -> Option<&ChannelStatus> {
        self.statuses.get(name)
    }

    pub fn status_report(&self) -> BTreeMap<String, ChannelStatus> {
        self.statuses.clone()
    }

    pub fn start_all(&mut self) -> Result<(), ChannelError> {
        let names = self.adapters.keys().cloned().collect::<Vec<_>>();
        let mut first_error = None;
        for name in names {
            if !self
                .statuses
                .get(&name)
                .is_some_and(|status| status.enabled)
            {
                continue;
            }
            let result = self
                .adapters
                .get_mut(&name)
                .ok_or_else(|| ChannelError::UnknownChannel(name.clone()))?
                .start();
            if let Err(error) = self.record_lifecycle_result(&name, result, true) {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub fn stop_all(&mut self) -> Result<(), ChannelError> {
        let names = self.adapters.keys().cloned().collect::<Vec<_>>();
        let mut first_error = None;
        for name in names {
            if !self
                .statuses
                .get(&name)
                .is_some_and(|status| status.running)
            {
                continue;
            }
            let result = self
                .adapters
                .get_mut(&name)
                .ok_or_else(|| ChannelError::UnknownChannel(name.clone()))?
                .stop();
            if let Err(error) = self.record_lifecycle_result(&name, result, false) {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    pub fn dispatch_outbound(&mut self, message: OutboundMessage) -> Result<(), ChannelError> {
        let channel = message.channel.clone();
        if !self
            .statuses
            .get(&channel)
            .is_some_and(|status| status.enabled)
        {
            return Err(ChannelError::UnknownChannel(channel));
        }
        let attempts = self.retry_policy.max_attempts.max(1);
        let mut last_error = None;
        for _ in 0..attempts {
            let result = if metadata_truthy(&message.metadata, "_stream_delta") {
                self.dispatch_delta(&message, &message.content)
            } else if metadata_bool(&message.metadata, "_stream_end") {
                self.dispatch_delta(&message, "")
            } else {
                self.adapters
                    .get(&channel)
                    .ok_or_else(|| ChannelError::UnknownChannel(channel.clone()))?
                    .send(message.clone())
            };
            match result {
                Ok(()) => {
                    self.clear_last_error(&channel);
                    return Ok(());
                }
                Err(error) => last_error = Some(error),
            }
        }
        let error =
            last_error.unwrap_or_else(|| ChannelError::Delivery("delivery failed".to_owned()));
        self.record_error(&channel, &error);
        Err(error)
    }

    fn dispatch_delta(&self, message: &OutboundMessage, delta: &str) -> Result<(), ChannelError> {
        let stream_id = metadata_string(&message.metadata, "_stream_id");
        let mut metadata = message.metadata.clone();
        if let Some(stream_id) = stream_id {
            metadata.insert("stream_id".to_owned(), Value::String(stream_id));
        }
        if let Some(reply_to) = message.reply_to.as_ref().filter(|value| !value.is_empty()) {
            metadata.insert("reply_to".to_owned(), Value::String(reply_to.clone()));
        }
        self.adapters
            .get(&message.channel)
            .ok_or_else(|| ChannelError::UnknownChannel(message.channel.clone()))?
            .send_delta(&message.chat_id, delta, metadata)
    }

    fn record_lifecycle_result(
        &mut self,
        name: &str,
        result: Result<(), ChannelError>,
        running_on_success: bool,
    ) -> Result<(), ChannelError> {
        match result {
            Ok(()) => {
                if let Some(status) = self.statuses.get_mut(name) {
                    status.running = running_on_success;
                    status.last_error = None;
                }
                Ok(())
            }
            Err(error) => {
                self.record_error(name, &error);
                Err(error)
            }
        }
    }

    fn record_error(&mut self, name: &str, error: &ChannelError) {
        if let Some(status) = self.statuses.get_mut(name) {
            status.last_error = Some(error.to_string());
        }
    }

    fn clear_last_error(&mut self, name: &str) {
        if let Some(status) = self.statuses.get_mut(name) {
            status.last_error = None;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSocketInboundMedia {
    pub data_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WebSocketInboundAction {
    NewChat,
    Attach { chat_id: String },
    Message(InboundMessage),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WebSocketServerEvent {
    Ready {
        chat_id: String,
        client_id: String,
    },
    Attached {
        chat_id: String,
    },
    Message {
        chat_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        buttons: Vec<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    Delta {
        chat_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<String>,
    },
    StreamEnd {
        chat_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<String>,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chat_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

pub fn normalize_websocket_frame(
    frame: Value,
    client_id: impl Into<String>,
    default_chat_id: impl Into<String>,
) -> Result<WebSocketInboundAction, ChannelError> {
    let client_id = client_id.into();
    let default_chat_id = default_chat_id.into();
    match frame {
        Value::String(content) => Ok(WebSocketInboundAction::Message(websocket_message(
            &client_id,
            &default_chat_id,
            content,
            Vec::new(),
        ))),
        Value::Object(mut object) => {
            normalize_websocket_object(&client_id, &default_chat_id, &mut object)
        }
        _ => Err(ChannelError::Protocol(
            "websocket frame must be a string or object".to_owned(),
        )),
    }
}

pub fn websocket_event_from_outbound(message: OutboundMessage) -> WebSocketServerEvent {
    let stream_id = metadata_string(&message.metadata, "_stream_id");
    if metadata_truthy(&message.metadata, "_stream_delta") {
        return WebSocketServerEvent::Delta {
            chat_id: message.chat_id,
            text: message.content,
            stream_id,
        };
    }
    if metadata_bool(&message.metadata, "_stream_end") {
        return WebSocketServerEvent::StreamEnd {
            chat_id: message.chat_id,
            stream_id,
        };
    }
    WebSocketServerEvent::Message {
        chat_id: message.chat_id,
        text: message.content,
        buttons: message.buttons,
        button_prompt: metadata_string(&message.metadata, "button_prompt"),
        media: message.media,
        reply_to: message.reply_to,
        kind: metadata_string(&message.metadata, "kind"),
    }
}

pub fn workflow_recipe_projection_outbound(
    channel: impl Into<String>,
    chat_id: impl Into<String>,
    projection: &Value,
) -> OutboundMessage {
    let recipes = projection
        .get("data")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let malformed = projection
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("readiness").and_then(Value::as_str) == Some("malformed"))
                .count()
        })
        .unwrap_or_default();
    let mut metadata = Map::new();
    metadata.insert(
        "kind".to_owned(),
        Value::String("workflow_recipes".to_owned()),
    );
    metadata.insert(
        "schema_version".to_owned(),
        projection
            .get("schema_version")
            .cloned()
            .unwrap_or_else(|| Value::String("024WorkflowRecipeProjection.v1".to_owned())),
    );
    metadata.insert("recipe_count".to_owned(), json!(recipes));
    metadata.insert("malformed_recipe_count".to_owned(), json!(malformed));
    OutboundMessage::new(
        channel,
        chat_id,
        format!("Workflow recipes: {recipes} discovered, {malformed} malformed"),
    )
    .with_metadata(metadata)
}

pub fn runtime_workflow_projection_outbound(
    channel: impl Into<String>,
    chat_id: impl Into<String>,
    projection: &Value,
) -> OutboundMessage {
    let state = projection_string(projection, "state").unwrap_or_else(|| "unknown".to_owned());
    let workflow_id = projection_string(projection, "workflow_id");
    let pattern = projection_string(projection, "pattern").unwrap_or_else(|| "unknown".to_owned());
    let progress = projection_u64(projection, "progress_count").unwrap_or_default();
    let active_children = projection_u64(projection, "active_child_count").unwrap_or_default();
    let pending_barriers = projection_u64(projection, "pending_barrier_count").unwrap_or_default();
    let verifier =
        projection_string(projection, "verifier_status").unwrap_or_else(|| "unknown".to_owned());
    let worktree_refs = projection_array_len(projection, "worktree_refs");
    let evidence_refs = projection_array_len(projection, "evidence_refs");
    let mut metadata = Map::new();
    metadata.insert(
        "kind".to_owned(),
        Value::String("runtime_workflow".to_owned()),
    );
    if let Some(schema_label) = projection.get("schema_label").cloned() {
        metadata.insert("schema_label".to_owned(), schema_label);
    }
    if let Some(schema_version) = projection.get("schema_version").cloned() {
        metadata.insert("schema_version".to_owned(), schema_version);
    }
    if let Some(workflow_id) = workflow_id.as_ref() {
        metadata.insert("workflow_id".to_owned(), Value::String(workflow_id.clone()));
    }
    metadata.insert("pattern".to_owned(), Value::String(pattern.clone()));
    metadata.insert("state".to_owned(), Value::String(state.clone()));
    metadata.insert("progress_count".to_owned(), json!(progress));
    metadata.insert("active_child_count".to_owned(), json!(active_children));
    metadata.insert("pending_barrier_count".to_owned(), json!(pending_barriers));
    metadata.insert(
        "verifier_status".to_owned(),
        Value::String(verifier.clone()),
    );
    if let Some(next_action) = projection_string(projection, "next_action") {
        metadata.insert("next_action".to_owned(), Value::String(next_action));
    }
    if let Some(blocked_reason) = projection_string(projection, "blocked_reason") {
        metadata.insert("blocked_reason".to_owned(), Value::String(blocked_reason));
    }
    if let Some(resume_available) = projection.get("resume_available").and_then(Value::as_bool) {
        metadata.insert("resume_available".to_owned(), json!(resume_available));
    }
    if let Some(budget_usage) = bounded_budget_usage(projection) {
        metadata.insert("budget_usage".to_owned(), budget_usage);
    }
    metadata.insert("worktree_ref_count".to_owned(), json!(worktree_refs));
    metadata.insert("evidence_ref_count".to_owned(), json!(evidence_refs));
    let workflow_label = workflow_id.as_deref().unwrap_or("unknown");
    OutboundMessage::new(
        channel,
        chat_id,
        format!(
            "Workflow {workflow_label}: state {state}; pattern {pattern}; progress {progress}; active children {active_children}; pending barriers {pending_barriers}; verifier {verifier}; worktree refs {worktree_refs}; evidence refs {evidence_refs}"
        ),
    )
    .with_metadata(metadata)
}

fn bounded_budget_usage(projection: &Value) -> Option<Value> {
    let budget = projection.get("budget_usage")?.as_object()?;
    Some(json!({
        "known_tokens": budget.get("known_tokens").and_then(Value::as_u64).unwrap_or_default(),
        "estimated_tokens": budget.get("estimated_tokens").and_then(Value::as_u64).unwrap_or_default(),
        "child_runs": budget.get("child_runs").and_then(Value::as_u64).unwrap_or_default(),
        "verifier_runs": budget.get("verifier_runs").and_then(Value::as_u64).unwrap_or_default(),
        "heavy_commands": budget.get("heavy_commands").and_then(Value::as_u64).unwrap_or_default(),
    }))
}

fn projection_string(projection: &Value, key: &str) -> Option<String> {
    projection
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn projection_u64(projection: &Value, key: &str) -> Option<u64> {
    projection.get(key).and_then(Value::as_u64)
}

fn projection_array_len(projection: &Value, key: &str) -> usize {
    projection
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default()
}

fn normalize_websocket_object(
    client_id: &str,
    default_chat_id: &str,
    object: &mut Map<String, Value>,
) -> Result<WebSocketInboundAction, ChannelError> {
    match object.get("type").and_then(Value::as_str) {
        Some("new_chat") => Ok(WebSocketInboundAction::NewChat),
        Some("attach") => {
            let chat_id = take_string(object, "chat_id")
                .ok_or_else(|| ChannelError::Protocol("attach frame needs chat_id".to_owned()))?;
            Ok(WebSocketInboundAction::Attach { chat_id })
        }
        Some("message") | None => {
            let chat_id =
                take_string(object, "chat_id").unwrap_or_else(|| default_chat_id.to_owned());
            let content = take_first_string(object, &["content", "text", "message"])
                .ok_or_else(|| ChannelError::Protocol("message frame needs content".to_owned()))?;
            let media = parse_websocket_media(object.remove("media"))?;
            Ok(WebSocketInboundAction::Message(websocket_message(
                client_id, &chat_id, content, media,
            )))
        }
        Some(kind) => Err(ChannelError::Protocol(format!(
            "unsupported websocket frame type: {kind}"
        ))),
    }
}

fn websocket_message(
    client_id: &str,
    chat_id: &str,
    content: String,
    media: Vec<WebSocketInboundMedia>,
) -> InboundMessage {
    let mut metadata = Map::new();
    metadata.insert("client_id".to_owned(), Value::String(client_id.to_owned()));
    let mut message = InboundMessage::new(WEBSOCKET_CHANNEL, client_id, chat_id, content);
    if !media.is_empty() {
        let names = media
            .iter()
            .filter_map(|item| item.name.clone())
            .map(Value::String)
            .collect::<Vec<_>>();
        if !names.is_empty() {
            metadata.insert("media_names".to_owned(), Value::Array(names));
        }
        message.media = media.into_iter().map(|item| item.data_url).collect();
    }
    message.metadata = metadata;
    message
}

fn parse_websocket_media(value: Option<Value>) -> Result<Vec<WebSocketInboundMedia>, ChannelError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        if value.is_null() {
            return Ok(Vec::new());
        }
        return Err(ChannelError::Protocol(
            "websocket media must be an array".to_owned(),
        ));
    };
    let mut media = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Object(mut object) => {
                let data_url = take_string(&mut object, "data_url").ok_or_else(|| {
                    ChannelError::Protocol("websocket media item needs data_url".to_owned())
                })?;
                let name = match object.remove("name") {
                    Some(Value::String(name)) => Some(name),
                    Some(Value::Null) | None => None,
                    Some(_) => {
                        return Err(ChannelError::Protocol(
                            "websocket media item name must be a string".to_owned(),
                        ))
                    }
                };
                media.push(WebSocketInboundMedia { data_url, name });
            }
            Value::String(data_url) => media.push(WebSocketInboundMedia {
                data_url,
                name: None,
            }),
            _ => {
                return Err(ChannelError::Protocol(
                    "websocket media item must be a string or object".to_owned(),
                ))
            }
        }
    }
    Ok(media)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscordInbound {
    pub sender_id: String,
    pub channel_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
}

impl DiscordInbound {
    pub fn into_message(self) -> InboundMessage {
        let mut metadata = Map::new();
        insert_optional(&mut metadata, "message_id", self.message_id);
        insert_optional(&mut metadata, "guild_id", self.guild_id);
        insert_optional(
            &mut metadata,
            "parent_channel_id",
            self.parent_channel_id.clone(),
        );
        insert_optional(&mut metadata, "thread_id", self.thread_id.clone());
        let mut message = InboundMessage::new(
            DISCORD_CHANNEL,
            self.sender_id,
            self.thread_id.clone().unwrap_or(self.channel_id.clone()),
            self.content,
        )
        .with_media(self.attachments)
        .with_metadata(metadata);
        if let (Some(parent), Some(thread)) = (self.parent_channel_id, self.thread_id) {
            message.session_key_override = Some(discord_thread_session_key(&parent, &thread));
        }
        message
    }
}

pub fn discord_thread_session_key(parent_channel_id: &str, thread_id: &str) -> String {
    format!("{DISCORD_CHANNEL}:{parent_channel_id}:thread:{thread_id}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramInbound {
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<String>,
    #[serde(default)]
    pub media: Vec<String>,
}

impl TelegramInbound {
    pub fn into_message(self) -> InboundMessage {
        let mut metadata = Map::new();
        insert_optional(&mut metadata, "message_id", self.message_id);
        insert_optional(&mut metadata, "username", self.username);
        insert_optional(
            &mut metadata,
            "message_thread_id",
            self.message_thread_id.clone(),
        );
        let mut message = InboundMessage::new(
            TELEGRAM_CHANNEL,
            self.sender_id,
            self.chat_id.clone(),
            self.content,
        )
        .with_media(self.media)
        .with_metadata(metadata);
        if let Some(thread_id) = self.message_thread_id {
            message.session_key_override =
                Some(telegram_topic_session_key(&self.chat_id, &thread_id));
        }
        message
    }
}

pub fn telegram_topic_session_key(chat_id: &str, message_thread_id: &str) -> String {
    format!("{TELEGRAM_CHANNEL}:{chat_id}:topic:{message_thread_id}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailInbound {
    pub sender_email: String,
    pub subject: String,
    pub date: String,
    pub body: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
}

impl EmailInbound {
    pub fn into_message(self) -> InboundMessage {
        let mut metadata = Map::new();
        metadata.insert(
            "message_id".to_owned(),
            Value::String(self.message_id.clone()),
        );
        metadata.insert("subject".to_owned(), Value::String(self.subject.clone()));
        metadata.insert("date".to_owned(), Value::String(self.date.clone()));
        metadata.insert(
            "sender_email".to_owned(),
            Value::String(self.sender_email.clone()),
        );
        insert_optional(&mut metadata, "uid", self.uid);
        InboundMessage::new(
            EMAIL_CHANNEL,
            self.sender_email.clone(),
            self.sender_email.clone(),
            format!(
                "[EMAIL-CONTEXT] Email received.\nFrom: {}\nSubject: {}\nDate: {}\n\n{}",
                self.sender_email, self.subject, self.date, self.body
            ),
        )
        .with_media(self.attachments)
        .with_metadata(metadata)
        .with_session_key_override(email_session_key(&self.sender_email))
    }
}

pub fn email_session_key(sender_email: &str) -> String {
    format!("{EMAIL_CHANNEL}:{sender_email}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackInbound {
    pub user_id: String,
    pub channel_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_type: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

impl SlackInbound {
    pub fn into_message(self) -> InboundMessage {
        let mut event = Map::new();
        event.insert("channel".to_owned(), Value::String(self.channel_id.clone()));
        event.insert("user".to_owned(), Value::String(self.user_id.clone()));
        insert_optional(&mut event, "ts", self.event_ts);

        let mut slack = Map::new();
        slack.insert("event".to_owned(), Value::Object(event));
        insert_optional(&mut slack, "thread_ts", self.thread_ts.clone());
        insert_optional(&mut slack, "channel_type", self.channel_type);
        let mut metadata = Map::new();
        metadata.insert("slack".to_owned(), Value::Object(slack));
        let mut message = InboundMessage::new(
            SLACK_CHANNEL,
            self.user_id,
            self.channel_id.clone(),
            self.content,
        )
        .with_media(self.files)
        .with_metadata(metadata);
        if let Some(thread_ts) = self.thread_ts {
            message.session_key_override =
                Some(slack_thread_session_key(&self.channel_id, &thread_ts));
        }
        message
    }
}

pub fn slack_thread_session_key(channel_id: &str, thread_ts: &str) -> String {
    format!("{SLACK_CHANNEL}:{channel_id}:{thread_ts}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhatsAppGroupPolicy {
    Open,
    Mention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhatsAppChannelConfig {
    pub bridge_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bridge_token: Option<String>,
    pub allowlist: ChannelAllowlist,
    pub group_policy: WhatsAppGroupPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhatsAppBridgeMessage {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, rename = "isGroup")]
    pub is_group: bool,
    #[serde(default, rename = "wasMentioned")]
    pub was_mentioned: bool,
    #[serde(default)]
    pub media: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WhatsAppOutboundFrame {
    Auth {
        token: String,
    },
    Send {
        to: String,
        text: String,
    },
    SendMedia {
        to: String,
        #[serde(rename = "filePath")]
        file_path: String,
        mimetype: String,
        #[serde(rename = "fileName")]
        file_name: String,
    },
}

#[derive(Debug, Clone)]
pub struct RecentMessageIds {
    capacity: usize,
    order: VecDeque<String>,
    seen: HashSet<String>,
    lid_to_phone: BTreeMap<String, String>,
}

impl RecentMessageIds {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            seen: HashSet::new(),
            lid_to_phone: BTreeMap::new(),
        }
    }

    pub fn remember(&mut self, id: &str) -> bool {
        if self.seen.contains(id) {
            return false;
        }
        if self.capacity == 0 {
            return true;
        }
        self.seen.insert(id.to_owned());
        self.order.push_back(id.to_owned());
        while self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        true
    }

    pub fn remember_lid_phone(&mut self, lid: &str, phone: &str) {
        self.lid_to_phone
            .insert(normalize_lid_key(lid), phone.to_owned());
    }

    pub fn phone_for_lid(&self, lid: &str) -> Option<String> {
        self.lid_to_phone.get(&normalize_lid_key(lid)).cloned()
    }
}

impl Default for RecentMessageIds {
    fn default() -> Self {
        Self::new(1000)
    }
}

pub fn normalize_whatsapp_bridge_message(
    message: WhatsAppBridgeMessage,
    config: &WhatsAppChannelConfig,
    recent_ids: &mut RecentMessageIds,
) -> Result<Option<InboundMessage>, ChannelError> {
    if message.kind != "message" {
        return Ok(None);
    }
    if let Some(id) = message.id.as_deref() {
        if !recent_ids.remember(id) {
            return Ok(None);
        }
    }
    if matches!(config.group_policy, WhatsAppGroupPolicy::Mention)
        && message.is_group
        && !message.was_mentioned
    {
        return Ok(None);
    }
    let sender_jid = message.sender.clone();
    let normalized_phone = message.pn.as_deref().and_then(normalize_whatsapp_phone);
    if let (Some(sender), Some(phone)) = (sender_jid.as_deref(), normalized_phone.as_deref()) {
        if is_lid_jid(sender) {
            recent_ids.remember_lid_phone(sender, phone);
        }
    }
    let sender_id = normalized_phone
        .or_else(|| {
            sender_jid.as_deref().and_then(|sender| {
                recent_ids
                    .phone_for_lid(sender)
                    .or_else(|| normalize_whatsapp_phone(sender))
            })
        })
        .ok_or_else(|| ChannelError::Protocol("whatsapp message needs sender".to_owned()))?;
    if !whatsapp_allowlist_is_allowed(&config.allowlist, &sender_id) {
        return Ok(None);
    }
    let chat_id = sender_jid.clone().unwrap_or_else(|| sender_id.clone());
    let mut content = message.content.unwrap_or_default();
    for path in &message.media {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&media_marker(path));
    }
    let mut metadata = Map::new();
    insert_optional(&mut metadata, "message_id", message.id);
    insert_optional(&mut metadata, "sender_jid", sender_jid);
    insert_optional(&mut metadata, "phone", Some(sender_id.clone()));
    metadata.insert("is_group".to_owned(), Value::Bool(message.is_group));
    metadata.insert(
        "was_mentioned".to_owned(),
        Value::Bool(message.was_mentioned),
    );
    insert_optional(&mut metadata, "timestamp", message.timestamp);
    Ok(Some(
        InboundMessage::new(WHATSAPP_CHANNEL, sender_id, chat_id, content)
            .with_media(message.media)
            .with_metadata(metadata),
    ))
}

pub fn whatsapp_auth_frame(token: impl Into<String>) -> WhatsAppOutboundFrame {
    WhatsAppOutboundFrame::Auth {
        token: token.into(),
    }
}

pub fn whatsapp_outbound_frames(message: OutboundMessage) -> Vec<WhatsAppOutboundFrame> {
    let mut frames = Vec::new();
    if !message.content.is_empty() {
        frames.push(WhatsAppOutboundFrame::Send {
            to: message.chat_id.clone(),
            text: message.content,
        });
    }
    for media in message.media {
        frames.push(WhatsAppOutboundFrame::SendMedia {
            to: message.chat_id.clone(),
            mimetype: mimetype_for_path(&media).to_owned(),
            file_name: file_name_for_path(&media),
            file_path: media,
        });
    }
    frames
}

fn metadata_string(metadata: &Map<String, Value>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn metadata_bool(metadata: &Map<String, Value>, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn metadata_truthy(metadata: &Map<String, Value>, key: &str) -> bool {
    match metadata.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => !value.is_empty() && value != "false",
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0) != 0,
        Some(Value::Null) | None => false,
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(items)) => !items.is_empty(),
    }
}

fn take_string(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    object.remove(key).and_then(|value| match value {
        Value::String(value) => Some(value),
        _ => None,
    })
}

fn take_first_string(object: &mut Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| take_string(object, key))
}

fn insert_optional(metadata: &mut Map<String, Value>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        metadata.insert(key.to_owned(), Value::String(value));
    }
}

fn normalize_whatsapp_phone(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || is_lid_jid(trimmed) || trimmed.ends_with("@g.us") {
        return None;
    }
    let without_domain = trimmed
        .strip_suffix("@s.whatsapp.net")
        .or_else(|| trimmed.strip_suffix("@c.us"))
        .unwrap_or(trimmed)
        .trim_start_matches("whatsapp:")
        .trim();
    if without_domain.is_empty() {
        None
    } else {
        Some(without_domain.to_owned())
    }
}

fn normalize_lid_key(lid: &str) -> String {
    lid.trim()
        .strip_suffix("@lid.whatsapp.net")
        .unwrap_or_else(|| lid.trim())
        .to_ascii_lowercase()
}

fn is_lid_jid(value: &str) -> bool {
    value.trim().ends_with("@lid.whatsapp.net")
}

fn whatsapp_allowlist_is_allowed(allowlist: &ChannelAllowlist, sender_id: &str) -> bool {
    if allowlist.is_allowed(sender_id) {
        return true;
    }
    if let Some(no_plus) = sender_id.strip_prefix('+') {
        return allowlist.is_allowed(no_plus);
    }
    allowlist.is_allowed(&format!("+{sender_id}"))
}

fn media_marker(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
    {
        "[image attachment]".to_owned()
    } else {
        "[file attachment]".to_owned()
    }
}

fn mimetype_for_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".mp4") {
        "video/mp4"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else {
        "application/octet-stream"
    }
}

fn file_name_for_path(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChannelCommandAction {
    Forward(InboundMessage),
    DirectHelp(OutboundMessage),
    NotCommand(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelCommandRequest {
    pub channel: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
    pub bot_name: Option<String>,
    pub session_key_override: Option<String>,
    pub metadata: Map<String, Value>,
}

impl ChannelCommandRequest {
    pub fn new(
        channel: impl Into<String>,
        sender_id: impl Into<String>,
        chat_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            channel: channel.into(),
            sender_id: sender_id.into(),
            chat_id: chat_id.into(),
            content: content.into(),
            bot_name: None,
            session_key_override: None,
            metadata: Map::new(),
        }
    }

    pub fn with_bot_name(mut self, bot_name: impl Into<String>) -> Self {
        self.bot_name = Some(bot_name.into());
        self
    }

    pub fn with_session_key_override(mut self, session_key: impl Into<String>) -> Self {
        self.session_key_override = Some(session_key.into());
        self
    }

    pub fn with_metadata(mut self, metadata: Map<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }
}

pub fn route_channel_command(request: ChannelCommandRequest) -> ChannelCommandAction {
    let normalized = normalize_channel_command(&request.content, request.bot_name.as_deref());
    if !normalized.starts_with('/') {
        return ChannelCommandAction::NotCommand(request.content.trim().to_owned());
    }
    if normalized.split_whitespace().next() == Some("/help") {
        return ChannelCommandAction::DirectHelp(OutboundMessage::new(
            request.channel,
            request.chat_id,
            build_help_text(),
        ));
    }
    let inbound = InboundMessage {
        channel: request.channel,
        sender_id: request.sender_id,
        chat_id: request.chat_id,
        content: normalized,
        timestamp: now_iso(),
        media: Vec::new(),
        metadata: request.metadata,
        session_key_override: request.session_key_override,
        owner_accepted_automation_result: None,
    };
    ChannelCommandAction::Forward(inbound)
}

fn now_iso() -> String {
    Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whatsapp_websocket_frames_serialize_with_bridge_types() -> Result<(), String> {
        let auth = whatsapp_auth_frame("bridge-token");
        assert_eq!(
            serde_json::to_value(auth).map_err(|error| error.to_string())?,
            json!({"type": "auth", "token": "bridge-token"})
        );

        let mut outbound = OutboundMessage::new(WHATSAPP_CHANNEL, "15551234567", "hello");
        outbound.media = vec!["/tmp/photo.jpg".to_owned()];
        let frames = whatsapp_outbound_frames(outbound);
        assert_eq!(
            serde_json::to_value(&frames[0]).map_err(|error| error.to_string())?,
            json!({"type": "send", "to": "15551234567", "text": "hello"})
        );
        assert_eq!(
            serde_json::to_value(&frames[1]).map_err(|error| error.to_string())?,
            json!({
                "type": "send_media",
                "to": "15551234567",
                "filePath": "/tmp/photo.jpg",
                "mimetype": "image/jpeg",
                "fileName": "photo.jpg"
            })
        );
        Ok(())
    }

    #[test]
    fn whatsapp_bridge_message_normalizes_websocket_payload() -> Result<(), String> {
        let config = WhatsAppChannelConfig {
            bridge_url: "ws://127.0.0.1:9001".to_owned(),
            bridge_token: None,
            allowlist: ChannelAllowlist::new(vec!["15551234567".to_owned()]),
            group_policy: WhatsAppGroupPolicy::Open,
        };
        let message = WhatsAppBridgeMessage {
            kind: "message".to_owned(),
            pn: Some("15551234567@s.whatsapp.net".to_owned()),
            sender: Some("abc@lid.whatsapp.net".to_owned()),
            content: Some("hello".to_owned()),
            id: Some("msg-1".to_owned()),
            is_group: false,
            was_mentioned: false,
            media: vec!["/tmp/photo.jpg".to_owned()],
            timestamp: Some("1710000000".to_owned()),
        };
        let inbound =
            normalize_whatsapp_bridge_message(message, &config, &mut RecentMessageIds::new(8))
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "message should pass filters".to_owned())?;

        assert_eq!(inbound.channel, WHATSAPP_CHANNEL);
        assert_eq!(inbound.sender_id, "15551234567");
        assert_eq!(inbound.chat_id, "abc@lid.whatsapp.net");
        assert!(inbound.content.contains("hello"));
        assert!(inbound.content.contains("[image attachment]"));
        assert!(!inbound.content.contains("/tmp/photo.jpg"));
        assert_eq!(inbound.media, vec!["/tmp/photo.jpg"]);
        assert_eq!(inbound.metadata["message_id"], json!("msg-1"));
        Ok(())
    }

    #[test]
    fn workflow_recipe_projection_outbound_summarizes_shared_projection() {
        let projection = json!({
            "schema_version": "024WorkflowRecipeProjection.v1",
            "data": [
                {"recipe_id": "ready", "readiness": "ready"},
                {"recipe_id": "bad", "readiness": "malformed"}
            ]
        });

        let outbound = workflow_recipe_projection_outbound(SLACK_CHANNEL, "C1", &projection);
        assert_eq!(outbound.channel, SLACK_CHANNEL);
        assert_eq!(outbound.chat_id, "C1");
        assert!(outbound.content.contains("2 discovered"));
        assert!(outbound.content.contains("1 malformed"));
        assert_eq!(outbound.metadata["kind"], json!("workflow_recipes"));
        assert_eq!(
            outbound.metadata["schema_version"],
            json!("024WorkflowRecipeProjection.v1")
        );
        assert_eq!(outbound.metadata["recipe_count"], json!(2));
    }

    #[test]
    fn runtime_workflow_projection_outbound_summarizes_bounded_milestones() {
        let projection = json!({
            "schema_label": "024WorkflowProjection",
            "schema_version": "024WorkflowProjection.v1",
            "workflow_id": "wf-channel",
            "objective_summary": "secret prompt must not be sent",
            "pattern": "fan_out_and_synthesize",
            "state": "Running",
            "progress_count": 2,
            "active_child_count": 1,
            "pending_barrier_count": 0,
            "verifier_status": "pending",
            "budget_usage": {
                "known_tokens": 10,
                "estimated_tokens": 20,
                "child_runs": 2,
                "verifier_runs": 1,
                "heavy_commands": 0
            },
            "next_action": "wait_for_child",
            "resume_available": true,
            "worktree_refs": ["diff secret"],
            "evidence_refs": [{"id": "raw evidence hidden"}]
        });

        let outbound = runtime_workflow_projection_outbound(WEBSOCKET_CHANNEL, "chat", &projection);
        assert_eq!(outbound.channel, WEBSOCKET_CHANNEL);
        assert_eq!(outbound.chat_id, "chat");
        assert!(outbound.content.contains("Workflow wf-channel"));
        assert!(outbound.content.contains("state Running"));
        assert!(outbound.content.contains("progress 2"));
        assert_eq!(outbound.metadata["kind"], json!("runtime_workflow"));
        assert_eq!(outbound.metadata["workflow_id"], json!("wf-channel"));
        assert_eq!(
            outbound.metadata["pattern"],
            json!("fan_out_and_synthesize")
        );
        assert_eq!(outbound.metadata["progress_count"], json!(2));
        assert_eq!(outbound.metadata["budget_usage"]["child_runs"], json!(2));
        assert_eq!(outbound.metadata["worktree_ref_count"], json!(1));
        assert_eq!(outbound.metadata["evidence_ref_count"], json!(1));
        assert_eq!(outbound.metadata["next_action"], json!("wait_for_child"));
        let serialized = serde_json::to_string(&outbound).unwrap_or_default();
        assert!(!serialized.contains("secret prompt"));
        assert!(!serialized.contains("diff secret"));
        assert!(!serialized.contains("raw evidence"));

        let event = websocket_event_from_outbound(outbound);
        let event_json = serde_json::to_value(event).unwrap_or_default();
        assert_eq!(event_json["kind"], json!("runtime_workflow"));
    }
}
