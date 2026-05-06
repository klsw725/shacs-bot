use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::provider::{ProviderClient, ProviderEvent, ProviderRequest};
use crate::registry::ProviderSpec;
use crate::types::{LlmResponse, ToolCallRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Number, Value};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicRequestParts {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicHttpStreamResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub trait AnthropicHttpTransport: Send + Sync {
    fn post_json(
        &self,
        request: AnthropicRequestParts,
    ) -> Result<AnthropicHttpResponse, ProviderError>;

    fn post_json_stream(
        &self,
        _request: AnthropicRequestParts,
    ) -> Result<AnthropicHttpStreamResponse, ProviderError> {
        Err(api_error(
            None,
            "Anthropic streaming transport is not implemented",
        ))
    }
}

impl<F> AnthropicHttpTransport for F
where
    F: Fn(AnthropicRequestParts) -> Result<AnthropicHttpResponse, ProviderError> + Send + Sync,
{
    fn post_json(
        &self,
        request: AnthropicRequestParts,
    ) -> Result<AnthropicHttpResponse, ProviderError> {
        self(request)
    }
}

#[derive(Clone)]
pub struct UreqAnthropicHttpTransport {
    base_url: String,
    agent: ureq::Agent,
}

impl UreqAnthropicHttpTransport {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, DEFAULT_HTTP_TIMEOUT)
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            base_url: base_url.into(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .http_status_as_error(false)
                .build()
                .new_agent(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl AnthropicHttpTransport for UreqAnthropicHttpTransport {
    fn post_json(
        &self,
        request: AnthropicRequestParts,
    ) -> Result<AnthropicHttpResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body).map_err(|error| api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body_text =
            response
                .body_mut()
                .read_to_string()
                .map_err(|error| ProviderError::Api {
                    status: Some(status),
                    message: error.to_string(),
                    retryable: false,
                    headers: headers.clone(),
                    body: None,
                })?;
        Ok(AnthropicHttpResponse {
            status,
            headers,
            body: parse_http_body(body_text),
        })
    }

    fn post_json_stream(
        &self,
        request: AnthropicRequestParts,
    ) -> Result<AnthropicHttpStreamResponse, ProviderError> {
        let url = join_base_and_path(&self.base_url, &request.path)?;
        let mut http_request = self
            .agent
            .post(&url)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json");
        for (key, value) in &request.headers {
            http_request = http_request.header(key, value);
        }
        let body = serde_json::to_string(&request.body).map_err(|error| api_error(None, error))?;
        let mut response = http_request.send(body).map_err(map_ureq_error)?;
        let status = response.status().as_u16();
        let headers = response_headers(response.headers());
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| ProviderError::Api {
                status: Some(status),
                message: error.to_string(),
                retryable: false,
                headers: headers.clone(),
                body: None,
            })?;
        Ok(AnthropicHttpStreamResponse {
            status,
            headers,
            body,
        })
    }
}

#[derive(Clone)]
pub struct AnthropicClient<T> {
    config: ProviderConfig,
    transport: T,
}

impl<T> AnthropicClient<T>
where
    T: AnthropicHttpTransport,
{
    pub fn new(config: ProviderConfig, transport: T) -> Self {
        Self { config, transport }
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T> ProviderClient for AnthropicClient<T>
where
    T: AnthropicHttpTransport,
{
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        let parts = build_anthropic_messages_request(&request, &self.config, false);
        let response = self.transport.post_json(parts)?;
        parse_anthropic_http_response(response)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        let parts = build_anthropic_messages_request(&request, &self.config, true);
        match self.transport.post_json_stream(parts) {
            Ok(response) => parse_anthropic_stream_http_response(response, on_event),
            Err(error) if !is_streaming_transport_unsupported(&error) => Err(error),
            Err(_) => {
                let response = self.chat(request)?;
                if let Some(content) = response
                    .content
                    .as_deref()
                    .filter(|content| !content.is_empty())
                {
                    on_event(ProviderEvent::TextDelta {
                        text: content.to_owned(),
                    });
                }
                on_event(ProviderEvent::Finish {
                    usage: serde_json::to_value(&response.usage).unwrap_or(Value::Null),
                    reason: response.finish_reason.clone(),
                });
                Ok(response)
            }
        }
    }
}

pub fn anthropic_client_from_config(
    config: ProviderConfig,
    spec: &ProviderSpec,
) -> Result<AnthropicClient<UreqAnthropicHttpTransport>, ProviderError> {
    ensure_anthropic_backend(spec)?;
    let base_url = config
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .or(spec.default_api_base)
        .unwrap_or(DEFAULT_ANTHROPIC_API_BASE)
        .to_owned();
    Ok(AnthropicClient::new(
        config,
        UreqAnthropicHttpTransport::new(base_url),
    ))
}

fn ensure_anthropic_backend(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.backend == "anthropic" {
        return Ok(());
    }
    Err(api_error(
        None,
        format!("provider '{}' does not use Anthropic backend", spec.name),
    ))
}

pub fn build_anthropic_messages_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
    stream: bool,
) -> AnthropicRequestParts {
    let (system, messages) = convert_messages_to_anthropic(&request.messages);
    let mut body = Map::new();
    body.insert(
        "model".to_owned(),
        Value::String(strip_anthropic_model_prefix(&request.model)),
    );
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert(
        "max_tokens".to_owned(),
        Value::Number(Number::from(request.settings.max_tokens.max(1))),
    );
    if let Some(system) = system {
        body.insert("system".to_owned(), system);
    }
    let thinking_enabled = request
        .settings
        .reasoning_effort
        .as_deref()
        .is_some_and(|effort| !effort.eq_ignore_ascii_case("none"));
    if let Some(thinking) = anthropic_thinking_config(
        request.settings.reasoning_effort.as_deref(),
        request.settings.max_tokens,
    ) {
        if let Some(budget) = thinking.get("budget_tokens").and_then(Value::as_u64) {
            let min_tokens = budget.saturating_add(4096);
            body.insert(
                "max_tokens".to_owned(),
                Value::Number(Number::from(
                    min_tokens.max(u64::from(request.settings.max_tokens)),
                )),
            );
        }
        body.insert("thinking".to_owned(), thinking);
        if should_include_temperature(&request.model) {
            body.insert("temperature".to_owned(), json!(1.0));
        }
    } else if should_include_temperature(&request.model) {
        if let Some(number) = Number::from_f64(request.settings.temperature) {
            body.insert("temperature".to_owned(), Value::Number(number));
        }
    }
    if !request.tools.is_empty() {
        let tools = convert_tools_to_anthropic(&request.tools);
        body.insert("tools".to_owned(), Value::Array(tools));
        if let Some(tool_choice) =
            convert_tool_choice_to_anthropic(request.tool_choice.as_ref(), false, thinking_enabled)
        {
            body.insert("tool_choice".to_owned(), tool_choice);
        }
    } else if let Some(tool_choice) =
        convert_tool_choice_to_anthropic(request.tool_choice.as_ref(), true, thinking_enabled)
    {
        body.insert("tool_choice".to_owned(), tool_choice);
    }
    if stream {
        body.insert("stream".to_owned(), Value::Bool(true));
    }
    apply_anthropic_cache_control(&mut body);

    AnthropicRequestParts {
        path: "/v1/messages".to_owned(),
        headers: build_anthropic_headers(config),
        body: Value::Object(body),
    }
}

pub fn build_anthropic_headers(config: &ProviderConfig) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
        (
            "anthropic-beta".to_owned(),
            "claude-code-20250219,interleaved-thinking-2025-05-14,fine-grained-tool-streaming-2025-05-14".to_owned(),
        ),
    ]);
    if let Some(api_key) = config.api_key.as_deref().filter(|value| !value.is_empty()) {
        headers.insert("x-api-key".to_owned(), api_key.to_owned());
    }
    if let Some(extra_headers) = &config.extra_headers {
        for (key, value) in extra_headers {
            headers.insert(key.clone(), value.clone());
        }
    }
    headers
}

pub fn parse_anthropic_response(response: &Value) -> Result<LlmResponse, ProviderError> {
    let Some(object) = response.as_object() else {
        return Ok(LlmResponse {
            content: Some("Error: API returned an invalid response.".to_owned()),
            finish_reason: "error".to_owned(),
            ..LlmResponse::default()
        });
    };
    if let Some(error) = object.get("error") {
        return Ok(parse_anthropic_error_response(error, object));
    }

    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut thinking_blocks = Vec::new();
    for block in object
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(value) = block.get("text").and_then(Value::as_str) {
                    text.push_str(value);
                }
            }
            Some("tool_use") => tool_calls.push(parse_anthropic_tool_use(block)),
            Some("thinking") => thinking_blocks.push(Value::Object(block.clone())),
            _ => {}
        }
    }
    let stop_reason = object.get("stop_reason").and_then(Value::as_str);
    Ok(LlmResponse {
        content: (!text.is_empty()).then_some(text),
        tool_calls,
        finish_reason: anthropic_finish_reason(stop_reason),
        usage: parse_anthropic_usage(object.get("usage")),
        thinking_blocks: (!thinking_blocks.is_empty()).then_some(thinking_blocks),
        ..LlmResponse::default()
    })
}

fn parse_anthropic_http_response(
    response: AnthropicHttpResponse,
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_anthropic_response(&response.body);
    }
    let body = force_anthropic_error_body(response.status, &response.headers, response.body);
    parse_anthropic_response(&body)
}

fn parse_anthropic_stream_http_response(
    response: AnthropicHttpStreamResponse,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    if (200..300).contains(&response.status) {
        return parse_anthropic_stream(&response.body, on_event);
    }
    let body = force_anthropic_error_body(
        response.status,
        &response.headers,
        parse_http_body(response.body),
    );
    parse_anthropic_response(&body)
}

pub fn parse_anthropic_stream(
    body: &str,
    on_event: &mut dyn FnMut(ProviderEvent),
) -> Result<LlmResponse, ProviderError> {
    let mut text = String::new();
    let mut usage = BTreeMap::new();
    let mut stop_reason = None;
    let mut tools = BTreeMap::<u64, StreamToolBuffer>::new();
    let mut thinking = BTreeMap::<u64, StreamThinkingBuffer>::new();
    for frame in parse_sse_frames(body) {
        let value = parse_sse_json(&frame.data)?;
        let event_type = frame
            .event
            .as_deref()
            .or_else(|| value.get("type").and_then(Value::as_str));
        match event_type {
            Some("error") => {
                return Ok(parse_anthropic_error_response(
                    value.get("error").unwrap_or(&value),
                    value.as_object().unwrap_or(&Map::new()),
                ))
            }
            Some("message_start") => {
                if let Some(frame_usage) = value
                    .get("message")
                    .and_then(|message| message.get("usage"))
                {
                    merge_usage(&mut usage, parse_anthropic_usage(Some(frame_usage)));
                }
            }
            Some("content_block_start") => {
                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
                if let Some(block) = value.get("content_block").and_then(Value::as_object) {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_use") => {
                            let buffer = tools.entry(index).or_default();
                            buffer.id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            buffer.name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            on_event(ProviderEvent::ToolCallStart {
                                id: buffer.id.clone(),
                                name: buffer.name.clone(),
                            });
                        }
                        Some("thinking") => {
                            let buffer = thinking.entry(index).or_default();
                            if let Some(value) = block.get("thinking").and_then(Value::as_str) {
                                buffer.thinking.push_str(value);
                            }
                            if let Some(value) = block.get("signature").and_then(Value::as_str) {
                                buffer.signature = Some(value.to_owned());
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("content_block_delta") => {
                let index = value.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(delta) = value.get("delta").and_then(Value::as_object) else {
                    continue;
                };
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(piece) = delta
                            .get("text")
                            .and_then(Value::as_str)
                            .filter(|piece| !piece.is_empty())
                        {
                            text.push_str(piece);
                            on_event(ProviderEvent::TextDelta {
                                text: piece.to_owned(),
                            });
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(piece) = delta
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|piece| !piece.is_empty())
                        {
                            thinking.entry(index).or_default().thinking.push_str(piece);
                            on_event(ProviderEvent::ReasoningDelta {
                                text: piece.to_owned(),
                            });
                        }
                    }
                    Some("signature_delta") => {
                        if let Some(signature) = delta.get("signature").and_then(Value::as_str) {
                            thinking.entry(index).or_default().signature =
                                Some(signature.to_owned());
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(piece) = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .filter(|piece| !piece.is_empty())
                        {
                            let buffer = tools.entry(index).or_default();
                            buffer.arguments.push_str(piece);
                            on_event(ProviderEvent::ToolCallDelta {
                                id: buffer.id.clone(),
                                delta: piece.to_owned(),
                            });
                        }
                    }
                    _ => {}
                }
            }
            Some("message_delta") => {
                if let Some(delta) = value.get("delta").and_then(Value::as_object) {
                    if let Some(reason) = delta.get("stop_reason").and_then(Value::as_str) {
                        stop_reason = Some(reason.to_owned());
                    }
                }
                if let Some(frame_usage) = value.get("usage") {
                    merge_usage(&mut usage, parse_anthropic_usage(Some(frame_usage)));
                }
            }
            Some("message_stop") => break,
            _ => {}
        }
    }
    let tool_calls = finalize_stream_tools(tools)?;
    for call in &tool_calls {
        on_event(ProviderEvent::ToolCallReady {
            id: call.id.clone(),
            name: call.name.clone(),
            input: Value::Object(call.arguments.clone()),
        });
    }
    let finish_reason = anthropic_finish_reason(stop_reason.as_deref());
    on_event(ProviderEvent::Finish {
        usage: serde_json::to_value(&usage).unwrap_or(Value::Null),
        reason: finish_reason.clone(),
    });
    Ok(LlmResponse {
        content: (!text.is_empty()).then_some(text),
        tool_calls,
        finish_reason,
        usage,
        thinking_blocks: finalize_stream_thinking(thinking),
        ..LlmResponse::default()
    })
}

fn convert_messages_to_anthropic(messages: &[Value]) -> (Option<Value>, Vec<Value>) {
    let mut system = None;
    let mut converted = Vec::new();
    for message in messages.iter().filter_map(Value::as_object) {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match role {
            "system" => system = message.get("content").map(convert_system_content),
            "tool" => converted.push(json!({
                "role": "user",
                "content": [tool_result_block(message)],
            })),
            "assistant" => {
                let blocks = assistant_blocks(message);
                if !blocks.is_empty() {
                    converted.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            "user" => converted.push(json!({
                "role": "user",
                "content": convert_user_content(message.get("content")),
            })),
            _ => {}
        }
    }
    (system, merge_anthropic_messages(converted))
}

fn convert_system_content(content: &Value) -> Value {
    if let Some(text) = content.as_str() {
        return Value::String(text.to_owned());
    }
    convert_content_blocks(content)
}

fn convert_user_content(content: Option<&Value>) -> Value {
    let Some(content) = content else {
        return Value::String("(empty)".to_owned());
    };
    if let Some(text) = content.as_str() {
        return Value::String(if text.is_empty() { "(empty)" } else { text }.to_owned());
    }
    convert_content_blocks(content)
}

fn convert_content_blocks(content: &Value) -> Value {
    let Some(blocks) = content.as_array() else {
        return Value::String("(empty)".to_owned());
    };
    let converted = blocks
        .iter()
        .filter_map(|block| {
            let object = block.as_object()?;
            match object.get("type").and_then(Value::as_str) {
                Some("text") => Some(json!({
                    "type": "text",
                    "text": object.get("text").and_then(Value::as_str).unwrap_or_default(),
                })),
                Some("image_url") => Some(convert_image_block(object)),
                _ => Some(Value::Object(object.clone())),
            }
        })
        .collect::<Vec<_>>();
    Value::Array(if converted.is_empty() {
        vec![json!({"type": "text", "text": "(empty)"})]
    } else {
        converted
    })
}

fn convert_image_block(block: &Map<String, Value>) -> Value {
    let url = block
        .get("image_url")
        .and_then(Value::as_object)
        .and_then(|image| image.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some((prefix, data)) = url.split_once(',') {
        if let Some(media_type) = prefix
            .strip_prefix("data:")
            .and_then(|value| value.split(';').next())
        {
            return json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}});
        }
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

fn assistant_blocks(message: &Map<String, Value>) -> Vec<Value> {
    let mut blocks = Vec::new();
    if let Some(thinking_blocks) = message.get("thinking_blocks").and_then(Value::as_array) {
        blocks.extend(thinking_blocks.iter().cloned());
    }
    if let Some(content) = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
    {
        blocks.push(json!({"type": "text", "text": content}));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls.iter().filter_map(Value::as_object) {
            let function = call.get("function").and_then(Value::as_object);
            blocks.push(json!({
                "type": "tool_use",
                "id": sanitize_anthropic_tool_id(call.get("id").and_then(Value::as_str).unwrap_or("toolu_0")),
                "name": function.and_then(|function| function.get("name")).and_then(Value::as_str).unwrap_or_default(),
                "input": parse_arguments_value(function.and_then(|function| function.get("arguments"))).unwrap_or_default(),
            }));
        }
    }
    blocks
}

fn tool_result_block(message: &Map<String, Value>) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": sanitize_anthropic_tool_id(message.get("tool_call_id").and_then(Value::as_str).unwrap_or("toolu_0")),
        "content": convert_user_content(message.get("content")),
    })
}

fn merge_anthropic_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();
    for message in messages {
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            continue;
        };
        if let Some(last) = merged.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(role) {
                let content = message
                    .get("content")
                    .cloned()
                    .unwrap_or(Value::Array(Vec::new()));
                append_anthropic_content(last, content);
                continue;
            }
        }
        merged.push(message);
    }
    while merged
        .last()
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("assistant")
    {
        merged.pop();
    }
    if merged
        .first()
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("assistant")
    {
        merged.insert(
            0,
            json!({"role": "user", "content": "(conversation continued)"}),
        );
    }
    merged
}

fn append_anthropic_content(target: &mut Value, content: Value) {
    let Some(target_object) = target.as_object_mut() else {
        return;
    };
    let existing = target_object
        .entry("content".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    match (existing, content) {
        (Value::Array(existing), Value::Array(mut next)) => existing.append(&mut next),
        (Value::Array(existing), Value::String(next)) => {
            existing.push(json!({"type": "text", "text": next}))
        }
        (Value::String(existing), Value::String(next)) => existing.push_str(&next),
        (existing, next) => {
            *existing = Value::Array(vec![existing.clone(), next]);
        }
    }
}

fn convert_tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let object = tool.as_object()?;
            let function = if object.get("type").and_then(Value::as_str) == Some("function") {
                object.get("function").and_then(Value::as_object)?
            } else {
                object
            };
            let name = function.get("name").and_then(Value::as_str)?;
            let mut converted = Map::new();
            converted.insert("name".to_owned(), Value::String(name.to_owned()));
            if let Some(description) = function.get("description").and_then(Value::as_str) {
                converted.insert(
                    "description".to_owned(),
                    Value::String(description.to_owned()),
                );
            }
            converted.insert(
                "input_schema".to_owned(),
                function
                    .get("parameters")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
            );
            if let Some(cache_control) = object.get("cache_control") {
                converted.insert("cache_control".to_owned(), cache_control.clone());
            }
            Some(Value::Object(converted))
        })
        .collect()
}

fn convert_tool_choice_to_anthropic(
    tool_choice: Option<&Value>,
    no_tools: bool,
    thinking_enabled: bool,
) -> Option<Value> {
    if no_tools {
        return None;
    }
    if thinking_enabled {
        return Some(json!({"type": "auto"}));
    }
    match tool_choice {
        Some(Value::String(choice)) if choice == "none" => None,
        Some(Value::String(choice)) if choice == "required" => Some(json!({"type": "any"})),
        Some(Value::Object(choice)) => choice
            .get("function")
            .and_then(Value::as_object)
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .map(|name| json!({"type": "tool", "name": name})),
        _ => Some(json!({"type": "auto"})),
    }
}

fn anthropic_thinking_config(reasoning_effort: Option<&str>, max_tokens: u32) -> Option<Value> {
    match reasoning_effort.map(str::to_ascii_lowercase).as_deref() {
        None | Some("none") => None,
        Some("adaptive") => Some(json!({"type": "adaptive"})),
        Some("low") => Some(json!({"type": "enabled", "budget_tokens": 1024})),
        Some("medium") => Some(json!({"type": "enabled", "budget_tokens": 4096})),
        Some("high") => Some(json!({"type": "enabled", "budget_tokens": max_tokens.max(8192)})),
        Some(_) => None,
    }
}

fn apply_anthropic_cache_control(body: &mut Map<String, Value>) {
    let marker = json!({"type": "ephemeral"});
    if let Some(system) = body.get_mut("system") {
        match system {
            Value::String(text) if !text.is_empty() => {
                *system = json!([{"type": "text", "text": text.clone(), "cache_control": marker.clone()}]);
            }
            Value::Array(blocks) => {
                if let Some(Value::Object(block)) = blocks.last_mut() {
                    block.insert("cache_control".to_owned(), marker.clone());
                }
            }
            _ => {}
        }
    }
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        let marker_index = messages.len().checked_sub(2);
        if let Some(index) = marker_index.filter(|_| messages.len() >= 3) {
            if let Some(Value::Object(message)) = messages.get_mut(index) {
                add_cache_control_to_last_content_block(message, marker.clone());
            }
        }
    }
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        if let Some(Value::Object(tool)) = tools.last_mut() {
            tool.entry("cache_control".to_owned()).or_insert(marker);
        }
    }
}

fn add_cache_control_to_last_content_block(message: &mut Map<String, Value>, marker: Value) {
    match message.get_mut("content") {
        Some(Value::Array(blocks)) => {
            if let Some(Value::Object(block)) = blocks.last_mut() {
                block.insert("cache_control".to_owned(), marker);
            }
        }
        Some(Value::String(text)) if !text.is_empty() => {
            let text = text.clone();
            message.insert(
                "content".to_owned(),
                json!([{"type": "text", "text": text, "cache_control": marker}]),
            );
        }
        _ => {}
    }
}

fn parse_anthropic_tool_use(block: &Map<String, Value>) -> ToolCallRequest {
    let arguments = block
        .get("input")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    ToolCallRequest::new(
        block.get("id").and_then(Value::as_str).unwrap_or_default(),
        block
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        arguments,
    )
}

fn parse_arguments_value(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Some(map.clone()),
        Some(Value::String(raw)) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|value| value.as_object().cloned()),
        _ => None,
    }
}

fn parse_anthropic_usage(value: Option<&Value>) -> BTreeMap<String, u64> {
    let Some(object) = value.and_then(Value::as_object) else {
        return BTreeMap::new();
    };
    let input = object
        .get("input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output = object
        .get("output_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_creation = object
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cache_read = object
        .get("cache_read_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let prompt = input + cache_creation + cache_read;
    let mut usage = BTreeMap::from([
        ("prompt_tokens".to_owned(), prompt),
        ("completion_tokens".to_owned(), output),
        ("total_tokens".to_owned(), prompt + output),
    ]);
    if cache_read > 0 {
        usage.insert("cached_tokens".to_owned(), cache_read);
    }
    usage
}

fn merge_usage(target: &mut BTreeMap<String, u64>, source: BTreeMap<String, u64>) {
    for (key, value) in source {
        if key != "total_tokens" && value > 0 {
            target.insert(key, value);
        }
    }
    let prompt = target.get("prompt_tokens").copied().unwrap_or(0);
    let completion = target.get("completion_tokens").copied().unwrap_or(0);
    if prompt > 0 || completion > 0 {
        target.insert("total_tokens".to_owned(), prompt + completion);
    }
}

fn anthropic_finish_reason(stop_reason: Option<&str>) -> String {
    match stop_reason.unwrap_or("end_turn") {
        "tool_use" => "tool_calls".to_owned(),
        "end_turn" => "stop".to_owned(),
        "max_tokens" => "length".to_owned(),
        other => other.to_owned(),
    }
}

fn parse_anthropic_error_response(error: &Value, object: &Map<String, Value>) -> LlmResponse {
    let error_object = error.as_object();
    let message = error_object
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .unwrap_or("provider error");
    LlmResponse {
        content: Some(format!("Error: {message}")),
        finish_reason: "error".to_owned(),
        error_status_code: object
            .get("status")
            .or_else(|| object.get("status_code"))
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok()),
        error_type: error_object
            .and_then(|error| error.get("type"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        error_code: error_object
            .and_then(|error| error.get("code"))
            .and_then(error_code_to_string),
        error_retry_after_s: object.get("retry_after").and_then(Value::as_f64),
        error_should_retry: object.get("should_retry").and_then(Value::as_bool),
        ..LlmResponse::default()
    }
}

fn force_anthropic_error_body(
    status: u16,
    headers: &BTreeMap<String, String>,
    body: Value,
) -> Value {
    let mut object = match body {
        Value::Object(mut object) => {
            let error = object
                .remove("error")
                .unwrap_or_else(|| error_from_non_success_body(&object));
            object.insert("error".to_owned(), error);
            object
        }
        other => Map::from_iter([("error".to_owned(), error_from_non_object_body(&other))]),
    };
    object.insert("status".to_owned(), Value::Number(Number::from(status)));
    if let Some(retry_after) = retry_after_seconds(headers) {
        object.insert(
            "retry_after".to_owned(),
            Number::from_f64(retry_after)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(should_retry) = should_retry(headers) {
        object.insert("should_retry".to_owned(), Value::Bool(should_retry));
    }
    Value::Object(object)
}

fn error_from_non_success_body(body: &Map<String, Value>) -> Value {
    let message = body
        .get("message")
        .or_else(|| body.get("error_description"))
        .and_then(value_to_text)
        .unwrap_or_else(|| "provider error".to_owned());
    json!({"message": message})
}

fn error_from_non_object_body(body: &Value) -> Value {
    match body {
        Value::String(message) => json!({"message": message}),
        Value::Null => json!({"message": "provider error"}),
        other => json!({"message": other.to_string()}),
    }
}

#[derive(Debug, Clone, Default)]
struct StreamToolBuffer {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Default)]
struct StreamThinkingBuffer {
    thinking: String,
    signature: Option<String>,
}

fn finalize_stream_thinking(thinking: BTreeMap<u64, StreamThinkingBuffer>) -> Option<Vec<Value>> {
    let blocks = thinking
        .into_values()
        .filter(|buffer| !buffer.thinking.is_empty())
        .map(|buffer| {
            let mut block = Map::from_iter([
                ("type".to_owned(), Value::String("thinking".to_owned())),
                ("thinking".to_owned(), Value::String(buffer.thinking)),
            ]);
            if let Some(signature) = buffer.signature.filter(|signature| !signature.is_empty()) {
                block.insert("signature".to_owned(), Value::String(signature));
            }
            Value::Object(block)
        })
        .collect::<Vec<_>>();
    (!blocks.is_empty()).then_some(blocks)
}

fn finalize_stream_tools(
    tools: BTreeMap<u64, StreamToolBuffer>,
) -> Result<Vec<ToolCallRequest>, ProviderError> {
    tools
        .into_values()
        .map(|buffer| {
            let arguments = if buffer.arguments.trim().is_empty() {
                Map::new()
            } else {
                match serde_json::from_str::<Value>(&buffer.arguments).map_err(|error| {
                    api_error(None, format!("invalid tool arguments JSON: {error}"))
                })? {
                    Value::Object(arguments) => arguments,
                    _ => Map::new(),
                }
            };
            Ok(ToolCallRequest::new(buffer.id, buffer.name, arguments))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SseFrame {
    event: Option<String>,
    data: String,
}

fn parse_sse_frames(body: &str) -> Vec<SseFrame> {
    let normalized = body.replace("\r\n", "\n");
    let mut frames = Vec::new();
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in normalized.lines() {
        if line.is_empty() {
            if !data_lines.is_empty() {
                frames.push(SseFrame {
                    event: event.take(),
                    data: data_lines.join("\n"),
                });
                data_lines.clear();
            } else {
                event = None;
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim().to_owned());
        }
    }
    if !data_lines.is_empty() {
        frames.push(SseFrame {
            event,
            data: data_lines.join("\n"),
        });
    }
    frames
}

fn parse_sse_json(data: &str) -> Result<Value, ProviderError> {
    serde_json::from_str(data)
        .map_err(|error| api_error(None, format!("invalid SSE JSON: {error}")))
}

fn sanitize_anthropic_tool_id(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn strip_anthropic_model_prefix(model: &str) -> String {
    model.strip_prefix("anthropic/").unwrap_or(model).to_owned()
}

fn should_include_temperature(model: &str) -> bool {
    !model.to_ascii_lowercase().contains("claude-opus-4-7")
}

fn join_base_and_path(base_url: &str, path: &str) -> Result<String, ProviderError> {
    let base_url = base_url.trim();
    if base_url.is_empty() {
        return Err(api_error(None, "missing Anthropic base URL"));
    }
    Ok(format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    ))
}

fn response_headers(headers: &ureq::http::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (key.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_http_body(body: String) -> Value {
    if body.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(&body).unwrap_or(Value::String(body))
}

fn map_ureq_error(error: ureq::Error) -> ProviderError {
    let retryable = matches!(
        error,
        ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Io(_)
    );
    let status = match error {
        ureq::Error::StatusCode(status) => Some(status),
        _ => None,
    };
    ProviderError::Api {
        status,
        message: error.to_string(),
        retryable,
        headers: BTreeMap::new(),
        body: None,
    }
}

fn retry_after_seconds(headers: &BTreeMap<String, String>) -> Option<f64> {
    header_value(headers, "retry-after-ms")
        .and_then(|value| value.parse::<f64>().ok())
        .map(|milliseconds| milliseconds / 1000.0)
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| {
            header_value(headers, "retry-after")
                .and_then(|value| parse_retry_after_header(value.trim()))
        })
}

fn parse_retry_after_header(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|seconds| *seconds > 0.0)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .and_then(|time| time.duration_since(SystemTime::now()).ok())
                .map(|duration| duration.as_secs_f64())
                .filter(|seconds| *seconds > 0.0)
        })
}

fn should_retry(headers: &BTreeMap<String, String>) -> Option<bool> {
    header_value(headers, "x-should-retry").and_then(|value| {
        match value.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

fn header_value<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn error_code_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(code) => Some(code.clone()),
        Value::Number(code) => Some(code.to_string()),
        _ => None,
    }
}

fn value_to_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    item.as_object()
                        .and_then(|object| object.get("text"))
                        .and_then(Value::as_str)
                })
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn is_streaming_transport_unsupported(error: &ProviderError) -> bool {
    error
        .to_string()
        .contains("Anthropic streaming transport is not implemented")
}

fn api_error(status: Option<u16>, error: impl ToString) -> ProviderError {
    ProviderError::Api {
        status,
        message: error.to_string(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
