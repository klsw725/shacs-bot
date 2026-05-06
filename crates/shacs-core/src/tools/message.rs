use crate::tools::{ArraySchema, JsonMap, StringSchema, Tool, ToolParameters};
use crate::tools::{SchemaFragment, ToolResult, ValidationError};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

static NEXT_MESSAGE_TOOL_ID: AtomicUsize = AtomicUsize::new(1);

thread_local! {
    static MESSAGE_CONTEXTS: RefCell<HashMap<usize, MessageContext>> = RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub chat_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub media: Vec<String>,
    pub metadata: Value,
    pub buttons: Vec<Vec<String>>,
}

pub trait MessageSender: Send + Sync {
    fn send(&self, message: OutboundMessage) -> Result<(), String>;
}

impl<F> MessageSender for F
where
    F: Fn(OutboundMessage) -> Result<(), String> + Send + Sync,
{
    fn send(&self, message: OutboundMessage) -> Result<(), String> {
        self(message)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MessageContext {
    channel: String,
    chat_id: String,
    message_id: Option<String>,
    metadata: Value,
    sent_in_turn: bool,
    record_channel_delivery: bool,
}

impl MessageContext {
    fn new(channel: String, chat_id: String, message_id: Option<String>) -> Self {
        Self {
            channel,
            chat_id,
            message_id,
            metadata: Value::Object(Map::new()),
            sent_in_turn: false,
            record_channel_delivery: false,
        }
    }
}

#[derive(Clone)]
pub struct MessageTool {
    id: usize,
    sender: Arc<Mutex<Option<Arc<dyn MessageSender>>>>,
    workspace: PathBuf,
    initial_context: MessageContext,
}

impl MessageTool {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self::with_defaults(workspace, "", "", None)
    }

    pub fn with_defaults(
        workspace: impl Into<PathBuf>,
        default_channel: impl Into<String>,
        default_chat_id: impl Into<String>,
        default_message_id: Option<String>,
    ) -> Self {
        Self {
            id: NEXT_MESSAGE_TOOL_ID.fetch_add(1, Ordering::Relaxed),
            sender: Arc::new(Mutex::new(None)),
            workspace: workspace.into(),
            initial_context: MessageContext::new(
                default_channel.into(),
                default_chat_id.into(),
                default_message_id,
            ),
        }
    }

    pub fn with_sender(
        workspace: impl Into<PathBuf>,
        sender: Arc<dyn MessageSender>,
        default_channel: impl Into<String>,
        default_chat_id: impl Into<String>,
        default_message_id: Option<String>,
    ) -> Self {
        let tool = Self::with_defaults(
            workspace,
            default_channel,
            default_chat_id,
            default_message_id,
        );
        tool.set_sender(sender);
        tool
    }

    pub fn set_context(
        &self,
        channel: impl Into<String>,
        chat_id: impl Into<String>,
        message_id: Option<String>,
        metadata: Option<Value>,
    ) {
        let mut context = self.context();
        context.channel = channel.into();
        context.chat_id = chat_id.into();
        context.message_id = message_id;
        context.metadata = metadata.unwrap_or_else(|| Value::Object(Map::new()));
        self.replace_context(context);
    }

    pub fn set_sender(&self, sender: Arc<dyn MessageSender>) {
        *recover_lock(&self.sender) = Some(sender);
    }

    pub fn start_turn(&self) {
        let mut context = self.context();
        context.sent_in_turn = false;
        self.replace_context(context);
    }

    pub fn sent_in_turn(&self) -> bool {
        self.context().sent_in_turn
    }

    pub fn set_record_channel_delivery(&self, active: bool) -> bool {
        let mut context = self.context();
        let previous = context.record_channel_delivery;
        context.record_channel_delivery = active;
        self.replace_context(context);
        previous
    }

    pub fn reset_record_channel_delivery(&self, previous: bool) {
        let mut context = self.context();
        context.record_channel_delivery = previous;
        self.replace_context(context);
    }

    fn context(&self) -> MessageContext {
        MESSAGE_CONTEXTS.with(|contexts| {
            contexts
                .borrow()
                .get(&self.id)
                .cloned()
                .unwrap_or_else(|| self.initial_context.clone())
        })
    }

    fn replace_context(&self, context: MessageContext) {
        MESSAGE_CONTEXTS.with(|contexts| {
            contexts.borrow_mut().insert(self.id, context);
        });
    }
}

impl Tool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a message to the user, optionally with file attachments. This is the ONLY way to deliver files (images, documents, audio, video) to the user. Use the 'media' parameter with file paths to attach files. Do NOT use read_file to send files — that only reads content for your own analysis."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new()
            .property("content", StringSchema::new("The message content to send"))
            .property(
                "channel",
                StringSchema::new("Optional: target channel (telegram, discord, etc.)"),
            )
            .property("chat_id", StringSchema::new("Optional: target chat/user ID"))
            .property(
                "media",
                ArraySchema::new(StringSchema::new("")).description(
                    "Optional: list of file paths to attach (images, video, audio, documents)",
                ),
            )
            .property(
                "buttons",
                ArraySchema::new(ArraySchema::new(StringSchema::new("Button label"))).description(
                    "Optional: inline keyboard buttons as list of rows, each row is list of button labels.",
                ),
            )
            .required(["content"])
            .to_json_schema()
    }

    fn validate_params(&self, params: &JsonMap) -> Vec<ValidationError> {
        let mut errors = crate::tools::base::validate_json_schema_value(
            &Value::Object(params.clone()),
            &self.parameters(),
            "",
        );
        if let Some(buttons) = params.get("buttons") {
            if parse_buttons(buttons).is_err() {
                errors.push(ValidationError::new(
                    "buttons",
                    "must be a list of list of strings",
                ));
            }
        }
        errors
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.send_message(params).into()
    }
}

impl MessageTool {
    fn send_message(&self, params: JsonMap) -> String {
        let content = strip_think(
            params
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        let buttons = match params.get("buttons") {
            Some(value) => match parse_buttons(value) {
                Ok(buttons) => buttons,
                Err(error) => return error,
            },
            None => Vec::new(),
        };

        let context = self.context();
        let default_channel = context.channel.clone();
        let default_chat_id = context.chat_id.clone();
        let channel = params
            .get("channel")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&default_channel)
            .to_owned();
        let chat_id = params
            .get("chat_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(&default_chat_id)
            .to_owned();
        let same_target = channel == default_channel && chat_id == default_chat_id;
        let message_id = if same_target {
            params
                .get("message_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| context.message_id.clone())
        } else {
            None
        };

        if channel.is_empty() || chat_id.is_empty() {
            return "Error: No target channel/chat specified".to_owned();
        }
        let sender = recover_lock(&self.sender).clone();
        let Some(sender) = sender else {
            return "Error: Message sending not configured".to_owned();
        };

        let media = parse_media(params.get("media"), &self.workspace);
        let mut metadata = if same_target {
            context.metadata.as_object().cloned().unwrap_or_default()
        } else {
            Map::new()
        };
        if let Some(message_id) = message_id {
            metadata.insert("message_id".to_owned(), Value::String(message_id));
        }
        if context.record_channel_delivery {
            metadata.insert("_record_channel_delivery".to_owned(), Value::Bool(true));
        }

        let message = OutboundMessage {
            channel: channel.clone(),
            chat_id: chat_id.clone(),
            content,
            reply_to: None,
            media: media.clone(),
            metadata: Value::Object(metadata),
            buttons: buttons.clone(),
        };

        match sender.send(message) {
            Ok(()) => {
                if same_target {
                    let mut context = self.context();
                    context.sent_in_turn = true;
                    self.replace_context(context);
                }
                let media_info = if media.is_empty() {
                    String::new()
                } else {
                    format!(" with {} attachments", media.len())
                };
                let button_count = buttons.iter().map(Vec::len).sum::<usize>();
                let button_info = if button_count == 0 {
                    String::new()
                } else {
                    format!(" with {button_count} button(s)")
                };
                format!("Message sent to {channel}:{chat_id}{media_info}{button_info}")
            }
            Err(error) => format!("Error sending message: {error}"),
        }
    }
}

fn parse_buttons(value: &Value) -> Result<Vec<Vec<String>>, String> {
    let Some(rows) = value.as_array() else {
        return Err("Error: buttons must be a list of list of strings".to_owned());
    };
    rows.iter()
        .map(|row| {
            let Some(labels) = row.as_array() else {
                return Err("Error: buttons must be a list of list of strings".to_owned());
            };
            labels
                .iter()
                .map(|label| {
                    label.as_str().map(str::to_owned).ok_or_else(|| {
                        "Error: buttons must be a list of list of strings".to_owned()
                    })
                })
                .collect()
        })
        .collect()
}

fn parse_media(value: Option<&Value>, workspace: &Path) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|path| {
            if path.starts_with("http://")
                || path.starts_with("https://")
                || Path::new(path).is_absolute()
            {
                path.to_owned()
            } else {
                workspace.join(path).to_string_lossy().into_owned()
            }
        })
        .collect()
}

fn strip_think(text: &str) -> String {
    let mut stripped = text.to_owned();
    for pattern in [
        r"(?s)<think>.*?</think>",
        r"(?s)^\s*<think>.*$",
        r"(?s)<thought>.*?</thought>",
        r"(?s)^\s*<thought>.*$",
        r"^\s*</think>\s*",
        r"\s*</think>\s*$",
        r"^\s*</thought>\s*",
        r"\s*</thought>\s*$",
        r"^\s*<\|?channel\|?>\s*",
    ] {
        if let Ok(regex) = Regex::new(pattern) {
            stripped = regex.replace_all(&stripped, "").into_owned();
        }
    }
    stripped = strip_malformed_opening_tag(&stripped, "<think");
    stripped = strip_malformed_opening_tag(&stripped, "<thought");
    stripped.trim().to_owned()
}

fn strip_malformed_opening_tag(text: &str, tag: &str) -> String {
    let mut remaining = text;
    let mut output = String::new();
    while let Some(index) = remaining.find(tag) {
        output.push_str(&remaining[..index]);
        let after_tag = &remaining[index + tag.len()..];
        let should_strip = after_tag
            .chars()
            .next()
            .map_or(true, |next| !is_valid_tag_continuation(next));
        if should_strip {
            remaining = after_tag;
        } else {
            output.push_str(tag);
            remaining = after_tag;
        }
    }
    output.push_str(remaining);
    output
}

fn is_valid_tag_continuation(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | ':' | '>' | '/')
}

fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
