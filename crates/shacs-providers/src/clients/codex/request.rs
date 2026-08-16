use super::{
    CodexClient, CodexRequestParts, UreqCodexHttpTransport, DEFAULT_CODEX_API_BASE,
    DEFAULT_ORIGINATOR,
};
use crate::clients::openai_compatible::build_responses_request;
use crate::config::ProviderConfig;
use crate::error::ProviderError;
use crate::provider::ProviderRequest;
use crate::registry::ProviderSpec;
use serde_json::{json, Map, Number, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn codex_client_from_config(
    config: ProviderConfig,
    spec: &ProviderSpec,
) -> Result<CodexClient<UreqCodexHttpTransport>, ProviderError> {
    ensure_codex_backend(spec)?;
    let base_url = config
        .api_base
        .as_deref()
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .or(spec.default_api_base)
        .unwrap_or(DEFAULT_CODEX_API_BASE)
        .to_owned();
    Ok(CodexClient::new(
        config,
        UreqCodexHttpTransport::new(base_url),
    ))
}

fn ensure_codex_backend(spec: &ProviderSpec) -> Result<(), ProviderError> {
    if spec.backend == "openai_codex" {
        return Ok(());
    }
    Err(super::api_error(
        None,
        format!("provider '{}' does not use OpenAI Codex backend", spec.name),
    ))
}

pub fn build_codex_responses_request(
    request: &ProviderRequest,
    config: &ProviderConfig,
) -> CodexRequestParts {
    let mut codex_request = request.clone();
    codex_request.model = strip_codex_model_prefix(&codex_request.model);
    let mut parts = build_responses_request(&codex_request, &ProviderConfig::default(), true);
    let Some(body) = parts.body.as_object_mut() else {
        return CodexRequestParts {
            path: "/codex/responses".to_owned(),
            headers: build_codex_headers(config),
            body: parts.body,
        };
    };
    body.remove("max_output_tokens");
    body.remove("temperature");
    body.entry("instructions".to_owned())
        .or_insert_with(|| Value::String(String::new()));
    body.insert("store".to_owned(), Value::Bool(false));
    body.insert("stream".to_owned(), Value::Bool(true));
    body.insert("text".to_owned(), json!({ "verbosity": "medium" }));
    body.insert("include".to_owned(), json!(["reasoning.encrypted_content"]));
    body.insert(
        "prompt_cache_key".to_owned(),
        Value::String(prompt_cache_key(&request.messages)),
    );
    body.insert(
        "tool_choice".to_owned(),
        request
            .tool_choice
            .clone()
            .unwrap_or_else(|| Value::String("auto".to_owned())),
    );
    body.insert("parallel_tool_calls".to_owned(), Value::Bool(true));
    let ordinary_tools = body.get("tools").cloned();
    let ordinary_tool_choice = body.get("tool_choice").cloned();
    if let Some(extra_body) = &config.extra_body {
        merge_json_objects(body, extra_body);
        body.insert("stream".to_owned(), Value::Bool(true));
    }
    match ordinary_tools {
        Some(tools) => {
            body.insert("tools".to_owned(), tools);
        }
        None => {
            body.remove("tools");
        }
    }
    match ordinary_tool_choice {
        Some(tool_choice) => {
            body.insert("tool_choice".to_owned(), tool_choice);
        }
        None => {
            body.remove("tool_choice");
        }
    }
    CodexRequestParts {
        path: "/codex/responses".to_owned(),
        headers: build_codex_headers(config),
        body: parts.body,
    }
}

pub fn build_codex_headers(config: &ProviderConfig) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::from([
        (
            "OpenAI-Beta".to_owned(),
            "responses=experimental".to_owned(),
        ),
        ("originator".to_owned(), DEFAULT_ORIGINATOR.to_owned()),
        ("User-Agent".to_owned(), "shacs-bot (rust)".to_owned()),
        ("accept".to_owned(), "text/event-stream".to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ]);
    if let Some(api_key) = config.api_key.as_deref().filter(|value| !value.is_empty()) {
        headers.insert("Authorization".to_owned(), format!("Bearer {api_key}"));
    }
    if let Some(extra_headers) = &config.extra_headers {
        for (key, value) in extra_headers {
            headers.insert(key.clone(), value.clone());
        }
    }
    headers
}

fn strip_codex_model_prefix(model: &str) -> String {
    for prefix in ["openai-codex/", "openai_codex/", "openai/"] {
        if let Some(stripped) = model.strip_prefix(prefix) {
            return stripped.to_owned();
        }
    }
    model.to_owned()
}

fn prompt_cache_key(messages: &[Value]) -> String {
    let raw = python_json_dumps(&Value::Array(messages.to_vec()));
    let digest = Sha256::digest(raw.as_bytes());
    format!("{digest:x}")
}

fn python_json_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => python_json_number(value),
        Value::String(value) => python_json_string(value),
        Value::Array(items) => {
            let items = items.iter().map(python_json_dumps).collect::<Vec<_>>();
            format!("[{}]", items.join(", "))
        }
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let entries = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}: {}",
                        python_json_string(key),
                        python_json_dumps(&object[key])
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", entries.join(", "))
        }
    }
}

fn python_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' || character == '\u{7f}' => {
                output.push_str(&format!("\\u{:04x}", u32::from(character)));
            }
            character if !character.is_ascii() => {
                for unit in character.encode_utf16(&mut [0; 2]) {
                    output.push_str(&format!("\\u{unit:04x}"));
                }
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn python_json_number(value: &Number) -> String {
    let raw = value.to_string();
    let Some(exponent_index) = raw.find(['e', 'E']) else {
        return raw;
    };
    let mantissa = &raw[..exponent_index];
    let exponent = &raw[exponent_index + 1..];
    let (sign, digits) = match exponent.strip_prefix(['+', '-']) {
        Some(digits) if exponent.starts_with('-') => ("-", digits),
        Some(digits) => ("+", digits),
        None => ("+", exponent),
    };
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    format!("{mantissa}e{sign}{digits:0>2}")
}

fn merge_json_objects(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(target_object)), Value::Object(source_object)) => {
                merge_json_objects(target_object, source_object);
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}
