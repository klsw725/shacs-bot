use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use crate::ProviderMediaCandidate;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationSettings {
    pub temperature: f64,
    pub max_tokens: u32,
    pub reasoning_effort: Option<String>,
}

impl Default for GenerationSettings {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 4096,
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_content: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_specific_fields: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_provider_specific_fields: Option<Map<String, Value>>,
}

impl ToolCallRequest {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: Map<String, Value>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
            extra_content: None,
            provider_specific_fields: None,
            function_provider_specific_fields: None,
        }
    }

    pub fn to_openai_tool_call(&self) -> Value {
        let mut function = Map::new();
        function.insert("name".to_owned(), Value::String(self.name.clone()));
        function.insert(
            "arguments".to_owned(),
            Value::String(Value::Object(self.arguments.clone()).to_string()),
        );
        if let Some(fields) = &self.function_provider_specific_fields {
            function.insert(
                "provider_specific_fields".to_owned(),
                Value::Object(fields.clone()),
            );
        }

        let mut call = Map::new();
        call.insert("id".to_owned(), Value::String(self.id.clone()));
        call.insert("type".to_owned(), Value::String("function".to_owned()));
        call.insert("function".to_owned(), Value::Object(function));
        if let Some(extra_content) = &self.extra_content {
            call.insert(
                "extra_content".to_owned(),
                Value::Object(extra_content.clone()),
            );
        }
        if let Some(fields) = &self.provider_specific_fields {
            call.insert(
                "provider_specific_fields".to_owned(),
                Value::Object(fields.clone()),
            );
        }
        Value::Object(call)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRequest>,
    #[serde(default = "default_finish_reason")]
    pub finish_reason: String,
    #[serde(default)]
    pub usage: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_status_code: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_retry_after_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_should_retry: Option<bool>,
    #[serde(skip)]
    pub media_candidates: Vec<ProviderMediaCandidate>,
}

impl Default for LlmResponse {
    fn default() -> Self {
        Self {
            content: None,
            tool_calls: Vec::new(),
            finish_reason: default_finish_reason(),
            usage: BTreeMap::new(),
            retry_after: None,
            reasoning_content: None,
            thinking_blocks: None,
            error_status_code: None,
            error_kind: None,
            error_type: None,
            error_code: None,
            error_retry_after_s: None,
            error_should_retry: None,
            media_candidates: Vec::new(),
        }
    }
}

impl LlmResponse {
    pub fn should_execute_tools(&self) -> bool {
        !self.tool_calls.is_empty() && matches!(self.finish_reason.as_str(), "tool_calls" | "stop")
    }

    pub fn final_text(&self) -> Option<&str> {
        self.content.as_deref()
    }
}

pub fn finish_reason_from_openai_responses(status: Option<&str>) -> &'static str {
    match status.unwrap_or("completed") {
        "completed" => "stop",
        "incomplete" => "length",
        "failed" | "cancelled" => "error",
        _ => "stop",
    }
}

fn default_finish_reason() -> String {
    "stop".to_owned()
}

pub fn object(entries: impl IntoIterator<Item = (impl Into<String>, Value)>) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}

pub fn tool_arguments(
    entries: impl IntoIterator<Item = (impl Into<String>, Value)>,
) -> Map<String, Value> {
    object(entries)
}

pub fn openai_function_tool(name: &str, arguments: Map<String, Value>) -> Value {
    ToolCallRequest::new("call", name, arguments).to_openai_tool_call()
}

pub fn text_response(content: impl Into<String>) -> LlmResponse {
    LlmResponse {
        content: Some(content.into()),
        ..LlmResponse::default()
    }
}

pub fn tool_call_response(tool_call: ToolCallRequest) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![tool_call],
        ..LlmResponse::default()
    }
}

pub fn usage(
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
) -> BTreeMap<String, u64> {
    BTreeMap::from([
        ("prompt_tokens".to_owned(), prompt_tokens),
        ("completion_tokens".to_owned(), completion_tokens),
        ("total_tokens".to_owned(), total_tokens),
    ])
}

pub fn json_string_arguments(arguments: &Map<String, Value>) -> Value {
    json!(Value::Object(arguments.clone()).to_string())
}
