use serde_json::json;
use shacs_providers::{
    build_codex_headers, build_codex_responses_request, chat_completions_tool,
    codex_client_from_config, find_by_name, parse_codex_stream, CodexClient,
    CodexHttpStreamResponse, CodexHttpTransport, CodexRequestParts, GenerationSettings,
    ProviderClient, ProviderConfig, ProviderEvent, ProviderRequest, UreqCodexHttpTransport,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, SystemTime};

type RequestCaptureHandle = thread::JoinHandle<Result<String, String>>;

#[test]
fn codex_builder_converts_responses_body_headers_and_cache_key() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "openai/gpt-5.4".to_owned(),
        messages: vec![
            json!({"role": "system", "content": "be direct"}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "inspect"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,aaa"}}
            ]}),
            json!({"role": "assistant", "content": "calling", "tool_calls": [{"id": "call_1|fc_1", "function": {"name": "search", "arguments": "{\"query\":\"rust\"}"}}]}),
            json!({"role": "tool", "tool_call_id": "call_1|fc_1", "content": {"ok": true}}),
        ],
        tools: vec![chat_completions_tool(
            "search",
            "Search docs",
            json!({"type": "object"}),
        )],
        settings: GenerationSettings {
            temperature: 0.9,
            max_tokens: 1234,
            reasoning_effort: Some("medium".to_owned()),
        },
        tool_choice: None,
    };
    let config = ProviderConfig {
        api_key: Some("codex-token".to_owned()),
        api_key_ref: None,
        extra_headers: Some(BTreeMap::from([(
            "chatgpt-account-id".to_owned(),
            "acct_123".to_owned(),
        )])),
        ..ProviderConfig::default()
    };
    let parts = build_codex_responses_request(&request, &config);
    if parts.path != "/codex/responses"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer codex-token")
        || parts.headers.get("chatgpt-account-id").map(String::as_str) != Some("acct_123")
        || parts.headers.get("OpenAI-Beta").map(String::as_str) != Some("responses=experimental")
        || parts.headers.get("originator").map(String::as_str) != Some("shacs-bot")
        || parts.headers.get("User-Agent").map(String::as_str) != Some("shacs-bot (rust)")
        || parts.body["model"] != "gpt-5.4"
        || parts.body["store"] != false
        || parts.body["stream"] != true
        || parts.body["instructions"] != "be direct"
        || parts.body["text"] != json!({"verbosity": "medium"})
        || parts.body["include"] != json!(["reasoning.encrypted_content"])
        || parts.body["tool_choice"] != "auto"
        || parts.body["parallel_tool_calls"] != true
        || parts.body["reasoning"]["effort"] != "medium"
        || parts.body["input"][0]["content"][1]["type"] != "input_image"
        || parts.body["input"][2]["call_id"] != "call_1"
        || parts.body["tools"][0]["name"] != "search"
        || parts.body.get("max_output_tokens").is_some()
        || parts.body.get("temperature").is_some()
        || parts.body["prompt_cache_key"].as_str().map(str::len) != Some(64)
    {
        return Err(format!("Codex request conversion drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn codex_headers_allow_user_overrides() -> Result<(), Box<dyn Error>> {
    let headers = build_codex_headers(&ProviderConfig {
        api_key: Some("token".to_owned()),
        api_key_ref: None,
        extra_headers: Some(BTreeMap::from([
            ("originator".to_owned(), "custom".to_owned()),
            ("Authorization".to_owned(), "Bearer override".to_owned()),
        ])),
        ..ProviderConfig::default()
    });
    if headers.get("originator").map(String::as_str) != Some("custom")
        || headers.get("Authorization").map(String::as_str) != Some("Bearer override")
        || headers.get("accept").map(String::as_str) != Some("text/event-stream")
    {
        return Err(format!("Codex headers drifted: {headers:?}").into());
    }
    Ok(())
}

#[test]
fn codex_builder_includes_empty_instructions_and_python_cache_hash() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "openai_codex/gpt-5.1-codex".to_owned(),
        messages: vec![json!({"z": "한글", "a": "b"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    };
    let parts = build_codex_responses_request(&request, &ProviderConfig::default());
    if parts.body["instructions"] != ""
        || parts.body["prompt_cache_key"]
            != "6e946c158bad484aacf4326201f2787211d543df55b390ee3c7f1b95f262ab32"
    {
        return Err(format!("Codex empty instructions/cache hash drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn codex_builder_cache_hash_matches_python_for_float_exponent_and_del() -> Result<(), Box<dyn Error>>
{
    let request = ProviderRequest {
        model: "openai_codex/gpt-5.1-codex".to_owned(),
        messages: vec![json!({"c": "\u{7f}", "n": 1e-6})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    };
    let parts = build_codex_responses_request(&request, &ProviderConfig::default());
    if parts.body["prompt_cache_key"]
        != "932e0c749b5dd95ae8b3398f1279008c4450277bc49c3df303d4b0ae9cee883f"
    {
        return Err(format!("Codex Python edge cache hash drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn codex_stream_parser_maps_responses_events() -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"search\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_1\",\"delta\":\"{\\\"query\\\":\"}\n\n",
        "event: response.function_call_arguments.done\ndata: {\"type\":\"response.function_call_arguments.done\",\"call_id\":\"call_1\",\"arguments\":\"{\\\"query\\\":\\\"rust\\\"}\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "event: response.reasoning_summary_text.delta\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"why\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"search\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":5,\"total_tokens\":9}}}\n\n",
    );
    let mut events = Vec::new();
    let response = parse_codex_stream(body, &mut |event| events.push(event))?;
    if response.content.as_deref() != Some("hello")
        || response.reasoning_content.as_deref() != Some("why")
        || response.finish_reason != "stop"
        || response.tool_calls.len() != 1
        || response.tool_calls[0].id != "call_1|fc_1"
        || response.tool_calls[0].arguments["query"] != "rust"
        || response.usage.get("total_tokens") != Some(&9)
        || !events.iter().any(
            |event| matches!(event, ProviderEvent::ToolCallReady { id, .. } if id == "call_1|fc_1"),
        )
        || !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Finish { reason, .. } if reason == "stop"))
    {
        return Err(format!(
            "Codex stream parser drifted: response={response:?} events={events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn codex_stream_parser_preserves_raw_malformed_tool_arguments() -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"search\",\"arguments\":\"{bad\"}}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"search\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
    );
    let response = parse_codex_stream(body, &mut |_| {})?;
    if response.tool_calls.len() != 1 || response.tool_calls[0].arguments["raw"] != "{bad" {
        return Err(format!("Codex malformed tool argument fallback drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn codex_client_posts_stream_and_maps_error_metadata() -> Result<(), Box<dyn Error>> {
    let retry_at = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(60));
    let client = CodexClient::new(
        ProviderConfig::default(),
        move |_request: CodexRequestParts| {
            Ok(CodexHttpStreamResponse {
                status: 429,
                headers: BTreeMap::from([
                    ("retry-after".to_owned(), retry_at.clone()),
                    ("x-should-retry".to_owned(), "true".to_owned()),
                ]),
                body: "quota".to_owned(),
            })
        },
    );
    let response = client.chat(provider_request())?;
    let valid_retry_after = response
        .error_retry_after_s
        .map(|seconds| (0.0..=60.0).contains(&seconds))
        .unwrap_or(false);
    if response.finish_reason != "error"
        || response.error_status_code != Some(429)
        || response.error_should_retry != Some(true)
        || !valid_retry_after
        || !response
            .content
            .as_deref()
            .unwrap_or_default()
            .contains("usage quota")
    {
        return Err(format!("Codex error metadata drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn codex_ureq_stream_transport_uses_idle_timeout_not_global_wall_clock(
) -> Result<(), Box<dyn Error>> {
    let (base_url, request_handle) = serve_slow_sse_response(
        vec![
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"he\"}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"llo\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(90),
    )?;
    let transport = UreqCodexHttpTransport::with_timeout(base_url, Duration::from_millis(250));
    let response = transport.post_json_stream(CodexRequestParts {
        path: "/codex/responses".to_owned(),
        headers: BTreeMap::new(),
        body: json!({"stream": true}),
    })?;
    let raw_request = request_handle
        .join()
        .map_err(|_| "request capture thread panicked")??;
    if !raw_request.starts_with("POST /codex/responses HTTP/1.1")
        || response.status != 200
        || !response.body.contains("\"delta\":\"he\"")
        || !response.body.contains("\"delta\":\"llo\"")
        || !response.body.contains("data: [DONE]")
    {
        return Err(format!(
            "Codex stream transport should use idle timeout semantics: request={raw_request:?} response={response:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn codex_factory_builds_codex_client_and_rejects_wrong_backend() -> Result<(), Box<dyn Error>> {
    let codex = find_by_name("openai_codex").ok_or("openai_codex spec missing")?;
    let client = codex_client_from_config(ProviderConfig::default(), codex)?;
    if client.transport().base_url() != "https://chatgpt.com/backend-api" {
        return Err(format!(
            "unexpected Codex base URL: {}",
            client.transport().base_url()
        )
        .into());
    }
    let openai = find_by_name("openai").ok_or("openai spec missing")?;
    let error = match codex_client_from_config(ProviderConfig::default(), openai) {
        Ok(_) => return Err("non-Codex backend should fail".into()),
        Err(error) => error,
    };
    if !error
        .to_string()
        .contains("does not use OpenAI Codex backend")
    {
        return Err(format!("unexpected backend guard error: {error}").into());
    }
    Ok(())
}

fn provider_request() -> ProviderRequest {
    ProviderRequest {
        model: "openai_codex/gpt-5.1-codex".to_owned(),
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
