use serde_json::{json, Value};
use shacs_providers::{
    anthropic_client_from_config, build_anthropic_headers, build_anthropic_messages_request,
    find_by_name, parse_anthropic_response, parse_anthropic_stream, AnthropicClient,
    AnthropicHttpResponse, AnthropicHttpTransport, AnthropicRequestParts, GenerationSettings,
    ProviderClient, ProviderConfig, ProviderEvent, ProviderRequest, UreqAnthropicHttpTransport,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, SystemTime};

type RequestCaptureHandle = thread::JoinHandle<Result<String, String>>;

#[test]
fn anthropic_builder_converts_messages_tools_thinking_and_cache() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "anthropic/claude-opus-4-5".to_owned(),
        messages: vec![
            json!({"role": "system", "content": "be helpful"}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,aaa"}}
            ]}),
            json!({"role": "assistant", "content": "I'll call a tool", "tool_calls": [{"id": "call.1", "function": {"name": "search", "arguments": "{\"query\":\"rust\"}"}}]}),
            json!({"role": "tool", "tool_call_id": "call.1", "content": "result"}),
        ],
        tools: vec![
            json!({"type": "function", "function": {"name": "search", "description": "Search", "parameters": {"type": "object"}}}),
        ],
        settings: GenerationSettings {
            temperature: 0.2,
            max_tokens: 2048,
            reasoning_effort: Some("medium".to_owned()),
        },
        tool_choice: Some(json!({"type": "function", "function": {"name": "search"}})),
    };
    let parts = build_anthropic_messages_request(&request, &ProviderConfig::default(), false);
    if parts.path != "/v1/messages"
        || parts.body["model"] != "claude-opus-4-5"
        || parts.body["system"][0]["cache_control"]["type"] != "ephemeral"
        || parts.body["messages"][0]["content"][1]["source"]["media_type"] != "image/png"
        || parts.body["messages"][1]["content"][1]["id"] != "call_1"
        || parts.body["messages"][2]["content"][0]["tool_use_id"] != "call_1"
        || parts.body["tools"][0]["input_schema"]["type"] != "object"
        || parts.body["tool_choice"] != json!({"type": "auto"})
        || parts.body["thinking"]["budget_tokens"] != 4096
        || parts.body["max_tokens"] != 8192
        || parts.body["temperature"] != 1.0
    {
        return Err(format!("Anthropic request conversion drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_builder_adaptive_thinking_uses_temperature_one_and_auto_tool_choice(
) -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        tools: vec![
            json!({"type": "function", "function": {"name": "search", "parameters": {"type": "object"}}}),
        ],
        settings: GenerationSettings {
            reasoning_effort: Some("adaptive".to_owned()),
            ..GenerationSettings::default()
        },
        tool_choice: Some(json!({"type": "function", "function": {"name": "search"}})),
        ..provider_request()
    };
    let parts = build_anthropic_messages_request(&request, &ProviderConfig::default(), false);
    if parts.body["thinking"] != json!({"type": "adaptive"})
        || parts.body["temperature"] != 1.0
        || parts.body["tool_choice"] != json!({"type": "auto"})
    {
        return Err(format!("Anthropic adaptive thinking parity drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_builder_omits_temperature_for_opus_4_7_even_with_thinking(
) -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "anthropic/claude-opus-4-7".to_owned(),
        settings: GenerationSettings {
            reasoning_effort: Some("medium".to_owned()),
            ..GenerationSettings::default()
        },
        ..provider_request()
    };
    let parts = build_anthropic_messages_request(&request, &ProviderConfig::default(), false);
    if parts.body["thinking"]["budget_tokens"] != 4096 || parts.body.get("temperature").is_some() {
        return Err(format!("Anthropic opus-4-7 temperature omission drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_builder_preserves_large_max_tokens_when_thinking_is_enabled(
) -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        settings: GenerationSettings {
            max_tokens: 12_000,
            reasoning_effort: Some("medium".to_owned()),
            ..GenerationSettings::default()
        },
        ..provider_request()
    };
    let parts = build_anthropic_messages_request(&request, &ProviderConfig::default(), false);
    if parts.body["thinking"]["budget_tokens"] != 4096 || parts.body["max_tokens"] != 12_000 {
        return Err(format!("Anthropic thinking token cap drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_headers_include_version_key_and_allow_overrides() -> Result<(), Box<dyn Error>> {
    let headers = build_anthropic_headers(&ProviderConfig {
        api_key: Some("sk-ant".to_owned()),
        api_key_ref: None,
        extra_headers: Some(BTreeMap::from([(
            "anthropic-version".to_owned(),
            "override".to_owned(),
        )])),
        ..ProviderConfig::default()
    });
    if headers.get("x-api-key").map(String::as_str) != Some("sk-ant")
        || headers.get("anthropic-version").map(String::as_str) != Some("override")
        || !headers.contains_key("anthropic-beta")
    {
        return Err(format!("Anthropic headers drifted: {headers:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_parser_extracts_text_tool_thinking_usage_and_finish() -> Result<(), Box<dyn Error>> {
    let response = parse_anthropic_response(&json!({
        "content": [
            {"type": "thinking", "thinking": "because"},
            {"type": "text", "text": "hello"},
            {"type": "tool_use", "id": "toolu_1", "name": "search", "input": {"query": "rust"}}
        ],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 10,
            "cache_creation_input_tokens": 2,
            "cache_read_input_tokens": 3,
            "output_tokens": 4
        }
    }))?;
    if response.content.as_deref() != Some("hello")
        || response.finish_reason != "tool_calls"
        || response.tool_calls.len() != 1
        || response.tool_calls[0].arguments["query"] != "rust"
        || response.thinking_blocks.as_ref().map(Vec::len) != Some(1)
        || response.usage.get("prompt_tokens") != Some(&15)
        || response.usage.get("cached_tokens") != Some(&3)
        || response.usage.get("completion_tokens") != Some(&4)
    {
        return Err(format!("Anthropic response parser drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_stream_parser_maps_text_thinking_tools_and_finish() -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":2,\"cache_read_input_tokens\":3}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"search\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"rust\\\"}\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"why\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":5}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    let mut events = Vec::new();
    let response = parse_anthropic_stream(body, &mut |event| events.push(event))?;
    if response.content.as_deref() != Some("hello")
        || response.finish_reason != "tool_calls"
        || response.tool_calls.len() != 1
        || response.tool_calls[0].arguments["query"] != "rust"
        || response
            .thinking_blocks
            .as_ref()
            .and_then(|blocks| blocks.first())
            .and_then(|block| block.get("thinking"))
            .and_then(Value::as_str)
            != Some("why")
        || response
            .thinking_blocks
            .as_ref()
            .and_then(|blocks| blocks.first())
            .and_then(|block| block.get("signature"))
            .and_then(Value::as_str)
            != Some("sig")
        || response.usage.get("prompt_tokens") != Some(&15)
        || response.usage.get("cached_tokens") != Some(&3)
        || response.usage.get("completion_tokens") != Some(&5)
        || response.usage.get("total_tokens") != Some(&20)
        || !events.iter().any(
            |event| matches!(event, ProviderEvent::ToolCallReady { id, .. } if id == "toolu_1"),
        )
        || !events.iter().any(
            |event| matches!(event, ProviderEvent::Finish { reason, .. } if reason == "tool_calls"),
        )
    {
        return Err(format!(
            "Anthropic stream parser drifted: response={response:?} events={events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn anthropic_client_maps_http_date_retry_after() -> Result<(), Box<dyn Error>> {
    let retry_at = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60));
    let client = AnthropicClient::new(
        ProviderConfig::default(),
        move |_request: AnthropicRequestParts| {
            Ok(AnthropicHttpResponse {
                status: 429,
                headers: BTreeMap::from([("retry-after".to_owned(), retry_at.clone())]),
                body: json!({"error": {"message": "slow down", "type": "rate_limit_error"}}),
            })
        },
    );
    let response = client.chat(provider_request())?;
    let valid_retry_after = response
        .error_retry_after_s
        .map(|seconds| (0.0..=60.0).contains(&seconds))
        .unwrap_or(false);
    if !valid_retry_after {
        return Err(format!("Anthropic HTTP-date retry-after drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_client_posts_request_and_maps_error_metadata() -> Result<(), Box<dyn Error>> {
    let client = AnthropicClient::new(
        ProviderConfig::default(),
        |_request: AnthropicRequestParts| {
            Ok(AnthropicHttpResponse {
                status: 429,
                headers: BTreeMap::from([
                    ("retry-after-ms".to_owned(), "2500".to_owned()),
                    ("x-should-retry".to_owned(), "true".to_owned()),
                ]),
                body: json!({"error": {"message": "slow down", "type": "rate_limit_error"}}),
            })
        },
    );
    let response = client.chat(provider_request())?;
    if response.finish_reason != "error"
        || response.content.as_deref() != Some("Error: slow down")
        || response.error_status_code != Some(429)
        || response.error_type.as_deref() != Some("rate_limit_error")
        || response.error_retry_after_s != Some(2.5)
        || response.error_should_retry != Some(true)
    {
        return Err(format!("Anthropic error metadata drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn anthropic_ureq_stream_transport_uses_idle_timeout_not_global_wall_clock(
) -> Result<(), Box<dyn Error>> {
    let (base_url, request_handle) = serve_slow_sse_response(
        vec![
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":1}}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"he\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"llo\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ],
        Duration::from_millis(90),
    )?;
    let transport = UreqAnthropicHttpTransport::with_timeout(base_url, Duration::from_millis(250));
    let response = transport.post_json_stream(AnthropicRequestParts {
        path: "/v1/messages".to_owned(),
        headers: BTreeMap::new(),
        body: json!({"stream": true}),
    })?;
    let raw_request = request_handle
        .join()
        .map_err(|_| "request capture thread panicked")??;
    if !raw_request.starts_with("POST /v1/messages HTTP/1.1")
        || response.status != 200
        || !response.body.contains("\"text\":\"he\"")
        || !response.body.contains("\"text\":\"llo\"")
        || !response.body.contains("message_stop")
    {
        return Err(format!(
            "Anthropic stream transport should use idle timeout semantics: request={raw_request:?} response={response:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn anthropic_factory_builds_anthropic_client_and_rejects_wrong_backend(
) -> Result<(), Box<dyn Error>> {
    let anthropic = find_by_name("anthropic").ok_or("anthropic spec missing")?;
    let client = anthropic_client_from_config(ProviderConfig::default(), anthropic)?;
    if client.transport().base_url() != "https://api.anthropic.com" {
        return Err(format!(
            "unexpected Anthropic base URL: {}",
            client.transport().base_url()
        )
        .into());
    }
    let openai = find_by_name("openai").ok_or("openai spec missing")?;
    let error = match anthropic_client_from_config(ProviderConfig::default(), openai) {
        Ok(_) => return Err("non-Anthropic backend should fail".into()),
        Err(error) => error,
    };
    if !error.to_string().contains("does not use Anthropic backend") {
        return Err(format!("unexpected backend guard error: {error}").into());
    }
    Ok(())
}

fn provider_request() -> ProviderRequest {
    ProviderRequest {
        model: "anthropic/claude-opus-4-5".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    }
}

fn serve_slow_sse_response(
    frames: Vec<&'static str>,
    delay: Duration,
) -> Result<(String, RequestCaptureHandle), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let request = read_http_request(&mut stream)?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .map_err(|error| error.to_string())?;
        for frame in frames {
            stream
                .write_all(frame.as_bytes())
                .and_then(|_| stream.flush())
                .map_err(|error| error.to_string())?;
            thread::sleep(delay);
        }
        Ok(request)
    });
    Ok((format!("http://{address}"), handle))
}

fn read_http_request(stream: &mut TcpStream) -> Result<String, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 512];
        let read = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if http_request_complete(&bytes)? {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn http_request_complete(bytes: &[u8]) -> Result<bool, String> {
    let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(false);
    };
    let header_text =
        String::from_utf8(bytes[..header_end].to_vec()).map_err(|error| error.to_string())?;
    let content_length = header_text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    Ok(bytes.len() >= header_end + 4 + content_length)
}
