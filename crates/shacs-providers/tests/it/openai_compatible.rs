use serde_json::{json, Map, Value};
use shacs_providers::{
    build_chat_completions_request, build_chat_completions_stream_request, build_headers,
    build_responses_request, chat_completions_tool, find_by_name, merge_json_objects,
    normalize_chat_finish_reason, openai_compatible_client_from_config,
    parse_chat_completions_response, parse_chat_completions_stream,
    parse_openai_responses_response, parse_openai_responses_stream,
    resolve_openai_compatible_api_base, GenerationSettings, OpenAiCompatibleClient,
    OpenAiCompatibleRequestParts, OpenAiHttpResponse, OpenAiHttpStreamResponse,
    OpenAiHttpTransport, ProviderClient, ProviderConfig, ProviderEvent, ProviderInvocation,
    ProviderRequest, UreqOpenAiHttpTransport,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

#[test]
fn invocation_deadline_bounds_supported_openai_http_operation() -> Result<(), Box<dyn Error>> {
    // Given
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || -> Result<(), String> {
        let (_stream, _) = listener.accept().map_err(|error| error.to_string())?;
        thread::sleep(Duration::from_millis(250));
        Ok(())
    });
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        UreqOpenAiHttpTransport::with_timeout(format!("http://{address}"), Duration::from_secs(5)),
    );
    let invocation =
        ProviderInvocation::default().with_deadline(Instant::now() + Duration::from_millis(40));
    let started = Instant::now();

    // When
    let result = client.chat_with_invocation(
        ProviderRequest {
            model: "test-model".to_owned(),
            messages: vec![json!({"role": "user", "content": "hello"})],
            tools: Vec::new(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &invocation,
    );

    // Then
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_millis(200));
    server.join().map_err(|_| "server thread panicked")??;
    Ok(())
}

type RequestCaptureHandle = thread::JoinHandle<Result<String, String>>;

#[test]
fn chat_completions_builder_keeps_body_and_headers_separate() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: vec![json!({ "role": "user", "content": "hi" })],
        tools: vec![chat_completions_tool(
            "search",
            "Search docs",
            json!({ "type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"] }),
        )],
        settings: GenerationSettings {
            temperature: 0.2,
            max_tokens: 0,
            reasoning_effort: Some("medium".to_owned()),
        },
        tool_choice: None,
    };
    let config = ProviderConfig {
        api_key: Some("sk-test".to_owned()),
        api_key_ref: None,
        extra_headers: Some(BTreeMap::from([("X-Test".to_owned(), "yes".to_owned())])),
        ..ProviderConfig::default()
    };

    let parts = build_chat_completions_request(&request, &config);
    if parts.path != "/chat/completions"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer sk-test")
        || parts.headers.get("X-Test").map(String::as_str) != Some("yes")
        || parts.body["model"] != "gpt-4.1"
        || parts.body["messages"] != json!([{ "role": "user", "content": "hi" }])
        || parts.body["max_tokens"] != 1
        || parts.body["reasoning_effort"] != "medium"
        || parts.body["tool_choice"] != "auto"
        || parts.body["tools"][0]["function"]["name"] != "search"
        || parts.body.get("headers").is_some()
        || parts.body.get("temperature").is_some()
    {
        return Err(format!("unexpected OpenAI-compatible request parts: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn chat_completions_builder_omits_tools_and_reasoning_none() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: vec![json!({ "role": "user", "content": "hi" })],
        tools: Vec::new(),
        settings: GenerationSettings {
            temperature: 0.7,
            max_tokens: 4096,
            reasoning_effort: Some("none".to_owned()),
        },
        tool_choice: Some(json!({"type":"function","function":{"name":"search"}})),
    };
    let parts = build_chat_completions_request(&request, &ProviderConfig::default());
    if parts.body.get("tools").is_some()
        || parts.body.get("tool_choice").is_some()
        || parts.body.get("reasoning_effort").is_some()
    {
        return Err(format!("empty tools/reasoning none should be omitted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn chat_completions_stream_builder_enables_sse_and_usage() -> Result<(), Box<dyn Error>> {
    let parts = build_chat_completions_stream_request(
        &ProviderRequest {
            model: "gpt-4.1".to_owned(),
            messages: vec![json!({"role": "user", "content": "hi"})],
            tools: Vec::new(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &ProviderConfig::default(),
    );
    if parts.path != "/chat/completions"
        || parts.body["stream"] != true
        || parts.body["stream_options"]["include_usage"] != true
    {
        return Err(format!("stream request did not enable SSE usage: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn responses_builder_converts_messages_tools_and_reasoning() -> Result<(), Box<dyn Error>> {
    let parts = build_responses_request(
        &ProviderRequest {
            model: "gpt-5".to_owned(),
            messages: vec![
                json!({"role": "system", "content": "be terse"}),
                json!({"role": "user", "content": [{"type": "text", "text": "look"}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,aaa"}}]}),
                json!({"role": "tool", "tool_call_id": "call_1|fc_1", "content": {"ok": true}}),
            ],
            tools: vec![chat_completions_tool(
                "search",
                "Search",
                json!({"type": "object"}),
            )],
            settings: GenerationSettings {
                temperature: 0.3,
                max_tokens: 12,
                reasoning_effort: Some("medium".to_owned()),
            },
            tool_choice: None,
        },
        &ProviderConfig::default(),
        true,
    );
    if parts.path != "/responses"
        || parts.body["instructions"] != "be terse"
        || parts.body["max_output_tokens"] != 12
        || parts.body["stream"] != true
        || parts.body["reasoning"]["effort"] != "medium"
        || parts.body["input"][0]["content"][1]["type"] != "input_image"
        || parts.body["input"][1]["call_id"] != "call_1"
        || parts.body["tools"][0]["name"] != "search"
    {
        return Err(format!("responses request conversion drifted: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn responses_builder_skips_empty_function_call_names_and_matching_outputs(
) -> Result<(), Box<dyn Error>> {
    let parts = build_responses_request(
        &ProviderRequest {
            model: "gpt-5".to_owned(),
            messages: vec![
                json!({"role": "user", "content": "install curl"}),
                json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "call_empty|fc_empty", "type": "function", "function": {"name": "", "arguments": "{}"}},
                        {"id": "call_missing|fc_missing", "type": "function", "function": {"arguments": "{}"}},
                        {"id": "call_good|fc_good", "type": "function", "function": {"name": "search", "arguments": "{\"query\":\"curl\"}"}}
                    ]
                }),
                json!({"role": "tool", "tool_call_id": "call_empty|fc_empty", "content": "bad"}),
                json!({"role": "tool", "tool_call_id": "call_missing|fc_missing", "content": "bad"}),
                json!({"role": "tool", "tool_call_id": "call_good|fc_good", "content": "ok"}),
            ],
            tools: vec![chat_completions_tool(
                "search",
                "Search",
                json!({"type": "object"}),
            )],
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &ProviderConfig::default(),
        false,
    );
    let input = parts.body["input"]
        .as_array()
        .ok_or("responses input should be an array")?;
    if input.iter().any(|item| {
        item["type"] == "function_call" && item["name"].as_str().unwrap_or_default().is_empty()
    }) {
        return Err(format!("responses input retained empty function name: {input:?}").into());
    }
    if input.iter().any(|item| {
        item["type"] == "function_call_output"
            && matches!(
                item["call_id"].as_str(),
                Some("call_empty" | "call_missing")
            )
    }) {
        return Err(format!("responses input retained orphan tool output: {input:?}").into());
    }
    let function_call = input
        .iter()
        .find(|item| item["type"] == "function_call")
        .ok_or("valid function call should remain")?;
    if function_call["call_id"] != "call_good"
        || function_call["id"] != "fc_good"
        || function_call["name"] != "search"
        || function_call["arguments"] != "{\"query\":\"curl\"}"
    {
        return Err(format!("valid function call changed: {function_call:?}").into());
    }
    if !input.iter().any(|item| {
        item["type"] == "function_call_output"
            && item["call_id"] == "call_good"
            && item["output"] == "ok"
    }) {
        return Err(format!("valid function output missing: {input:?}").into());
    }
    Ok(())
}

#[test]
fn responses_builder_omits_temperature_for_reasoning_models() -> Result<(), Box<dyn Error>> {
    let reasoning = build_responses_request(
        &ProviderRequest {
            model: "gpt-4.1".to_owned(),
            messages: Vec::new(),
            tools: Vec::new(),
            settings: GenerationSettings {
                temperature: 0.9,
                max_tokens: 8,
                reasoning_effort: Some("medium".to_owned()),
            },
            tool_choice: None,
        },
        &ProviderConfig::default(),
        false,
    );
    let gpt5 = build_responses_request(
        &ProviderRequest {
            model: "gpt-5".to_owned(),
            messages: Vec::new(),
            tools: Vec::new(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &ProviderConfig::default(),
        false,
    );
    if reasoning.body.get("temperature").is_some() || gpt5.body.get("temperature").is_some() {
        return Err(format!(
            "Responses reasoning/GPT-5 requests must omit temperature: reasoning={reasoning:?} gpt5={gpt5:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn extra_body_is_recursively_merged_and_can_override_defaults() -> Result<(), Box<dyn Error>> {
    let request = ProviderRequest {
        model: "deepseek-reasoner".to_owned(),
        messages: vec![json!({ "role": "user", "content": "hi" })],
        tools: Vec::new(),
        settings: GenerationSettings {
            temperature: 0.7,
            max_tokens: 4096,
            reasoning_effort: Some("high".to_owned()),
        },
        tool_choice: None,
    };
    let config = ProviderConfig {
        extra_body: Some(Map::from_iter([
            ("temperature".to_owned(), json!(1.0)),
            (
                "metadata".to_owned(),
                json!({ "tags": ["a"], "nested": {"right": true} }),
            ),
        ])),
        ..ProviderConfig::default()
    };
    let parts = build_chat_completions_request(&request, &config);
    if parts.body["temperature"] != 1.0
        || parts.body["metadata"] != json!({ "tags": ["a"], "nested": {"right": true} })
    {
        return Err(format!("extra_body did not merge/override: {parts:?}").into());
    }
    Ok(())
}

#[test]
fn merge_json_objects_recursively_preserves_unmentioned_nested_keys() -> Result<(), Box<dyn Error>>
{
    let mut target = Map::from_iter([(
        "metadata".to_owned(),
        json!({ "nested": { "left": true, "replace": false }, "keep": 1 }),
    )]);
    let source = Map::from_iter([(
        "metadata".to_owned(),
        json!({ "nested": { "right": true, "replace": true } }),
    )]);
    merge_json_objects(&mut target, &source);
    if Value::Object(target)
        != json!({ "metadata": { "nested": { "left": true, "right": true, "replace": true }, "keep": 1 } })
    {
        return Err("recursive merge did not preserve nested keys".into());
    }
    Ok(())
}

#[test]
fn build_headers_user_headers_override_authorization() -> Result<(), Box<dyn Error>> {
    let config = ProviderConfig {
        api_key: Some("from-api-key".to_owned()),
        api_key_ref: None,
        extra_headers: Some(BTreeMap::from([(
            "Authorization".to_owned(),
            "Bearer override".to_owned(),
        )])),
        ..ProviderConfig::default()
    };
    let headers = build_headers(&config);
    if headers.get("Authorization").map(String::as_str) != Some("Bearer override") {
        return Err(format!("extra_headers should override defaults: {headers:?}").into());
    }
    Ok(())
}

#[test]
fn parse_chat_completions_extracts_content_reasoning_usage_and_tool_calls(
) -> Result<(), Box<dyn Error>> {
    let response = json!({
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {
                "content": null,
                "reasoning_content": "thinking",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "index": 0,
                    "function": {
                        "name": "search",
                        "arguments": "{\"query\":\"rust\"}",
                        "strict": true
                    },
                    "extra_content": {"vendor": "x"}
                }]
            }
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
            "prompt_tokens_details": {"cached_tokens": 7}
        }
    });

    let parsed = parse_chat_completions_response(&response)?;
    if parsed.finish_reason != "tool_calls"
        || parsed.reasoning_content.as_deref() != Some("thinking")
        || parsed.usage.get("prompt_tokens") != Some(&10)
        || parsed.usage.get("cached_tokens") != Some(&7)
        || parsed.tool_calls.len() != 1
    {
        return Err(format!("unexpected parsed response: {parsed:?}").into());
    }
    let call = &parsed.tool_calls[0];
    if call.id != "call_1"
        || call.name != "search"
        || call.arguments["query"] != "rust"
        || call
            .extra_content
            .as_ref()
            .and_then(|value| value.get("vendor"))
            != Some(&json!("x"))
        || call
            .provider_specific_fields
            .as_ref()
            .and_then(|value| value.get("index"))
            != Some(&json!(0))
        || call
            .function_provider_specific_fields
            .as_ref()
            .and_then(|value| value.get("strict"))
            != Some(&json!(true))
    {
        return Err(format!("unexpected parsed tool call: {call:?}").into());
    }
    Ok(())
}

#[test]
fn parse_chat_completions_falls_back_from_empty_content_and_reasoning() -> Result<(), Box<dyn Error>>
{
    let response = json!({
        "choices": [
            {"finish_reason": "stop", "message": {"content": "", "reasoning_content": "", "reasoning": "fallback reasoning"}},
            {"finish_reason": "stop", "message": {"content": "second content"}}
        ],
        "usage": {}
    });
    let parsed = parse_chat_completions_response(&response)?;
    if parsed.content.as_deref() != Some("second content")
        || parsed.reasoning_content.as_deref() != Some("fallback reasoning")
        || parsed.usage.get("prompt_tokens") != Some(&0)
        || parsed.usage.get("completion_tokens") != Some(&0)
        || parsed.usage.get("total_tokens") != Some(&0)
    {
        return Err(format!("empty content/reasoning fallback drifted: {parsed:?}").into());
    }
    Ok(())
}

#[test]
fn parse_chat_completions_handles_string_top_level_and_empty_choices() -> Result<(), Box<dyn Error>>
{
    let text = parse_chat_completions_response(&json!("plain response"))?;
    if text.content.as_deref() != Some("plain response") || text.finish_reason != "stop" {
        return Err(format!("plain string response drifted: {text:?}").into());
    }

    let top_level = parse_chat_completions_response(&json!({"output_text": "fallback text"}))?;
    if top_level.content.as_deref() != Some("fallback text") || top_level.finish_reason != "stop" {
        return Err(format!("top-level output_text response drifted: {top_level:?}").into());
    }

    let empty = parse_chat_completions_response(&json!({"choices": []}))?;
    if empty.finish_reason != "error"
        || empty.content.as_deref() != Some("Error: API returned empty choices.")
    {
        return Err(format!("empty choices response drifted: {empty:?}").into());
    }
    Ok(())
}

#[test]
fn parse_chat_completions_maps_error_metadata() -> Result<(), Box<dyn Error>> {
    let parsed = parse_chat_completions_response(&json!({
        "status": 429,
        "retry_after": 2.5,
        "should_retry": true,
        "error": {"message": "rate limited", "type": "rate_limit", "code": "too_many_requests"}
    }))?;
    if parsed.finish_reason != "error"
        || parsed.content.as_deref() != Some("Error: rate limited")
        || parsed.error_status_code != Some(429)
        || parsed.error_type.as_deref() != Some("rate_limit")
        || parsed.error_code.as_deref() != Some("too_many_requests")
        || parsed.error_retry_after_s != Some(2.5)
        || parsed.error_should_retry != Some(true)
    {
        return Err(format!("error metadata drifted: {parsed:?}").into());
    }
    Ok(())
}

#[test]
fn parse_chat_completions_stream_maps_deltas_tools_usage_and_finish() -> Result<(), Box<dyn Error>>
{
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hel\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\",\"reasoning_content\":\"think\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"search\",\"arguments\":\"{\\\"q\\\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"rust\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    let mut events = Vec::new();
    let response = parse_chat_completions_stream(body, &mut |event| events.push(event))?;
    if response.content.as_deref() != Some("hello")
        || response.reasoning_content.as_deref() != Some("think")
        || response.finish_reason != "tool_calls"
        || response.usage.get("total_tokens") != Some(&3)
        || response.tool_calls.len() != 1
        || response.tool_calls[0].arguments["q"] != "rust"
        || !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::ToolCallReady { .. }))
    {
        return Err(
            format!("chat stream parser drifted: response={response:?} events={events:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn parse_openai_responses_response_maps_output_reasoning_tools_and_usage(
) -> Result<(), Box<dyn Error>> {
    let response = parse_openai_responses_response(&json!({
        "status": "completed",
        "output": [
            {"type": "message", "content": [{"type": "output_text", "text": "hello"}]},
            {"type": "reasoning", "summary": [{"type": "summary_text", "text": "because"}]},
            {"type": "function_call", "call_id": "call_1", "id": "fc_1", "name": "search", "arguments": "{\"q\":\"rust\"}"}
        ],
        "usage": {"input_tokens": 4, "output_tokens": 5, "total_tokens": 9}
    }))?;
    if response.content.as_deref() != Some("hello")
        || response.reasoning_content.as_deref() != Some("because")
        || response.finish_reason != "stop"
        || response.usage.get("prompt_tokens") != Some(&4)
        || response.tool_calls.len() != 1
        || response.tool_calls[0].id != "call_1|fc_1"
    {
        return Err(format!("responses parser drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn parse_openai_responses_stream_maps_events_to_response_and_provider_events(
) -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"id\":\"fc_1\",\"name\":\"search\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
        "data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"why\"}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_1\",\"delta\":\"{\\\"q\\\":\\\"rust\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"id\":\"fc_1\",\"name\":\"search\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n",
    );
    let mut events = Vec::new();
    let response = parse_openai_responses_stream(body, &mut |event| events.push(event))?;
    if response.content.as_deref() != Some("hi")
        || response.reasoning_content.as_deref() != Some("why")
        || response.usage.get("total_tokens") != Some(&3)
        || response.tool_calls.len() != 1
        || response.tool_calls[0].arguments["q"] != "rust"
        || !events.iter().any(
            |event| matches!(event, ProviderEvent::ToolCallDelta { id, .. } if id == "call_1|fc_1"),
        )
        || !events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Finish { .. }))
    {
        return Err(format!(
            "responses stream parser drifted: response={response:?} events={events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn parse_openai_responses_stream_uses_completed_output_reasoning_summary(
) -> Result<(), Box<dyn Error>> {
    let body = concat!(
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"final why\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n",
    );
    let response = parse_openai_responses_stream(body, &mut |_| {})?;
    if response.reasoning_content.as_deref() != Some("final why") {
        return Err(
            format!("completed output reasoning summary was not collected: {response:?}").into(),
        );
    }
    Ok(())
}

#[test]
fn parse_chat_completions_stream_maps_sse_error_frame() -> Result<(), Box<dyn Error>> {
    let response = parse_chat_completions_stream(
        "event: error\ndata: {\"error\":{\"message\":\"boom\",\"type\":\"server_error\"}}\n\n",
        &mut |_| {},
    )?;
    if response.finish_reason != "error"
        || response.content.as_deref() != Some("Error: boom")
        || response.error_type.as_deref() != Some("server_error")
    {
        return Err(format!("SSE error frame drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn parse_chat_completions_rejects_malformed_tool_arguments() -> Result<(), Box<dyn Error>> {
    let response = json!({
        "choices": [{
            "finish_reason": "tool_calls",
            "message": {"tool_calls": [{"id": "call", "function": {"name": "bad", "arguments": "{"}}]}
        }]
    });
    let error = parse_chat_completions_response(&response).expect_err("malformed args should fail");
    if !error.to_string().contains("invalid tool arguments JSON") {
        return Err(format!("unexpected malformed arguments error: {error}").into());
    }
    Ok(())
}

#[test]
fn normalize_chat_finish_reason_uses_tool_calls_when_missing_with_tools(
) -> Result<(), Box<dyn Error>> {
    if normalize_chat_finish_reason(None, true) != "tool_calls"
        || normalize_chat_finish_reason(Some("function_call"), false) != "tool_calls"
        || normalize_chat_finish_reason(Some("content_filter"), false) != "content_filter"
    {
        return Err("finish reason normalization drifted".into());
    }
    Ok(())
}

#[test]
fn openai_client_posts_built_request_and_parses_success_response() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = OpenAiCompatibleClient::new(
        ProviderConfig {
            api_key: Some("sk-test".to_owned()),
            api_key_ref: None,
            extra_headers: Some(BTreeMap::from([("X-Test".to_owned(), "yes".to_owned())])),
            ..ProviderConfig::default()
        },
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({
                    "choices": [{"finish_reason": "stop", "message": {"content": "hello"}}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}
                }),
            })
        },
    );

    let response = client.chat(ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    if response.content.as_deref() != Some("hello")
        || response.usage.get("total_tokens") != Some(&3)
    {
        return Err(format!("client did not parse success response: {response:?}").into());
    }
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let Some(request) = captured.first() else {
        return Err("transport did not receive request".into());
    };
    if request.path != "/chat/completions"
        || request.headers.get("Authorization").map(String::as_str) != Some("Bearer sk-test")
        || request.headers.get("X-Test").map(String::as_str) != Some("yes")
        || request.body["model"] != "gpt-4.1"
    {
        return Err(format!("client did not pass request parts correctly: {request:?}").into());
    }
    Ok(())
}

#[test]
fn openai_client_maps_non_success_response_to_error_metadata() -> Result<(), Box<dyn Error>> {
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        |_request: OpenAiCompatibleRequestParts| {
            Ok(OpenAiHttpResponse {
                status: 429,
                headers: BTreeMap::from([
                    ("retry-after-ms".to_owned(), "1500".to_owned()),
                    ("x-should-retry".to_owned(), "true".to_owned()),
                ]),
                body: json!({"error": {"message": "slow down", "type": "rate_limit", "code": "429"}}),
            })
        },
    );
    let response = client.chat(ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: Vec::new(),
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    if response.finish_reason != "error"
        || response.content.as_deref() != Some("Error: slow down")
        || response.error_status_code != Some(429)
        || response.error_retry_after_s != Some(1.5)
        || response.error_should_retry != Some(true)
    {
        return Err(format!("client error metadata drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn openai_client_preserves_missing_should_retry_as_unknown() -> Result<(), Box<dyn Error>> {
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        |_request: OpenAiCompatibleRequestParts| {
            Ok(OpenAiHttpResponse {
                status: 429,
                headers: BTreeMap::new(),
                body: json!({"error": {"message": "rate limited"}}),
            })
        },
    );
    let response = client.chat(ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: Vec::new(),
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    if response.error_status_code != Some(429) || response.error_should_retry.is_some() {
        return Err(format!("missing should_retry should stay unknown: {response:?}").into());
    }
    Ok(())
}

#[test]
fn openai_client_forces_non_success_body_into_error_envelope() -> Result<(), Box<dyn Error>> {
    let retry_after = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(120));
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        move |_request: OpenAiCompatibleRequestParts| {
            Ok(OpenAiHttpResponse {
                status: 503,
                headers: BTreeMap::from([
                    ("Retry-After".to_owned(), retry_after.clone()),
                    ("X-Should-Retry".to_owned(), " true ".to_owned()),
                ]),
                body: json!({
                    "content": "this must not be treated as success",
                    "choices": [{"finish_reason": "stop", "message": {"content": "nor this"}}]
                }),
            })
        },
    );
    let response = client.chat(ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: Vec::new(),
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    if response.finish_reason != "error"
        || response.content.as_deref() != Some("Error: this must not be treated as success")
        || response.error_status_code != Some(503)
        || response.error_retry_after_s.unwrap_or_default() <= 0.0
        || response.error_should_retry != Some(true)
    {
        return Err(format!("non-success response was not forced to error: {response:?}").into());
    }
    Ok(())
}

#[test]
fn openai_client_chat_stream_uses_native_sse_when_transport_supports_it(
) -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        StaticStreamTransport {
            captured: captured.clone(),
            response: OpenAiHttpStreamResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: concat!(
                    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n",
                )
                .to_owned(),
            },
        },
    );
    let mut events = Vec::new();
    let response = client.chat_stream(
        ProviderRequest {
            model: "gpt-4.1".to_owned(),
            messages: Vec::new(),
            tools: Vec::new(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &mut |event| events.push(event),
    )?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    if response.content.as_deref() != Some("hi")
        || captured
            .first()
            .and_then(|request| request.body.get("stream"))
            != Some(&json!(true))
        || events
            != vec![
                ProviderEvent::TextDelta {
                    text: "hi".to_owned(),
                },
                ProviderEvent::Finish {
                    usage: json!({}),
                    reason: "stop".to_owned(),
                },
            ]
    {
        return Err(format!(
            "native SSE stream path drifted: response={response:?} captured={captured:?} events={events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn openai_client_routes_reasoning_openai_models_to_responses_api() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = OpenAiCompatibleClient::with_provider_context(
        ProviderConfig::default(),
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({
                    "status": "completed",
                    "output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}],
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}
                }),
            })
        },
        "openai",
        "https://api.openai.com/v1",
    );
    let response = client.chat(ProviderRequest {
        model: "gpt-5".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    if response.content.as_deref() != Some("ok")
        || captured.first().map(|request| request.path.as_str()) != Some("/responses")
    {
        return Err(format!(
            "responses routing drifted: response={response:?} captured={captured:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn openai_client_falls_back_from_responses_compatibility_error() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = OpenAiCompatibleClient::with_provider_context(
        ProviderConfig::default(),
        move |request: OpenAiCompatibleRequestParts| {
            let path = request.path.clone();
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            if path == "/responses" {
                Ok(OpenAiHttpResponse {
                    status: 400,
                    headers: BTreeMap::new(),
                    body: json!({"error": {"message": "unsupported parameter max_output_tokens for responses"}}),
                })
            } else {
                Ok(OpenAiHttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "fallback ok"}}]}),
                })
            }
        },
        "openai",
        "https://api.openai.com/v1",
    );
    let response = client.chat(ProviderRequest {
        model: "gpt-5".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    let paths = captured
        .lock()
        .map_err(|error| error.to_string())?
        .iter()
        .map(|request| request.path.clone())
        .collect::<Vec<_>>();
    if response.content.as_deref() != Some("fallback ok")
        || paths != vec!["/responses".to_owned(), "/chat/completions".to_owned()]
    {
        return Err(format!(
            "responses compatibility fallback drifted: response={response:?} paths={paths:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn provider_spec_metadata_changes_chat_completions_wire_body() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let spec = *find_by_name("openai").ok_or("missing openai spec")?;
    let client = OpenAiCompatibleClient::with_provider_spec(
        ProviderConfig::default(),
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]}),
            })
        },
        spec,
        "https://api.openai.com/v1",
    );
    let response = client.chat(ProviderRequest {
        model: "gpt-4.1".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings {
            temperature: 0.4,
            max_tokens: 123,
            reasoning_effort: None,
        },
        tool_choice: None,
    })?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let request = captured.first().ok_or("missing captured request")?;
    if response.content.as_deref() != Some("ok")
        || request.body.get("max_tokens").is_some()
        || request.body["max_completion_tokens"] != 123
        || !request.headers.contains_key("x-session-affinity")
    {
        return Err(format!("provider metadata was not applied: {request:?}").into());
    }
    Ok(())
}

#[test]
fn provider_spec_applies_model_overrides_thinking_and_openrouter_headers(
) -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let spec = *find_by_name("moonshot").ok_or("missing moonshot spec")?;
    let client = OpenAiCompatibleClient::with_provider_spec(
        ProviderConfig::default(),
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]}),
            })
        },
        spec,
        "https://api.moonshot.ai/v1",
    );
    client.chat(ProviderRequest {
        model: "moonshotai/kimi-k2.5".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings {
            temperature: 0.4,
            max_tokens: 123,
            reasoning_effort: Some("none".to_owned()),
        },
        tool_choice: None,
    })?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let request = captured.first().ok_or("missing captured request")?;
    if request.body["temperature"] != 1.0 || request.body["thinking"]["type"] != "disabled" {
        return Err(format!("model override/thinking parity drifted: {request:?}").into());
    }

    let openrouter_captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let openrouter_transport = openrouter_captured.clone();
    let openrouter_spec = *find_by_name("openrouter").ok_or("missing openrouter spec")?;
    let openrouter_client = OpenAiCompatibleClient::with_provider_spec(
        ProviderConfig {
            api_key: Some("sk-or-test".to_owned()),
            api_key_ref: None,
            extra_headers: Some(BTreeMap::from([(
                "X-OpenRouter-Title".to_owned(),
                "override".to_owned(),
            )])),
            ..ProviderConfig::default()
        },
        move |request: OpenAiCompatibleRequestParts| {
            openrouter_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]}),
            })
        },
        openrouter_spec,
        "https://openrouter.ai/api/v1",
    );
    openrouter_client.chat(ProviderRequest {
        model: "anthropic/claude-opus-4-6".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    let openrouter_captured = openrouter_captured
        .lock()
        .map_err(|error| error.to_string())?;
    let openrouter_request = openrouter_captured
        .first()
        .ok_or("missing openrouter request")?;
    if openrouter_request
        .headers
        .get("HTTP-Referer")
        .map(String::as_str)
        != Some("https://github.com/HKUDS/shacs-bot")
        || openrouter_request
            .headers
            .get("X-OpenRouter-Title")
            .map(String::as_str)
            != Some("override")
        || openrouter_request
            .headers
            .get("X-OpenRouter-Categories")
            .map(String::as_str)
            != Some("cli-agent,personal-agent")
    {
        return Err(format!("openrouter headers drifted: {openrouter_request:?}").into());
    }
    Ok(())
}

#[test]
fn provider_spec_sanitizes_openai_compatible_history_and_tool_ids() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let spec = *find_by_name("moonshot").ok_or("missing moonshot spec")?;
    let client = OpenAiCompatibleClient::with_provider_spec(
        ProviderConfig::default(),
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]}),
            })
        },
        spec,
        "https://api.moonshot.ai/v1",
    );
    client.chat(ProviderRequest {
        model: "moonshotai/kimi-k2.5".to_owned(),
        messages: vec![
            json!({"role": "system", "content": "rules"}),
            json!({"role": "assistant", "content": "", "tool_calls": [{"id": "unsafe-tool-call-id", "function": {"name": "search", "arguments": ""}}]}),
            json!({"role": "tool", "tool_call_id": "unsafe-tool-call-id", "content": {"ok": true}, "_ignored": true}),
            json!({"role": "user", "content": [{"type": "text", "text": ""}, {"type": "text", "text": "hello", "_meta": {"path": "x"}}]}),
            json!({"role": "assistant", "content": "trailing prefill"}),
        ],
        tools: Vec::new(),
        settings: GenerationSettings {
            temperature: 0.7,
            max_tokens: 4096,
            reasoning_effort: Some("high".to_owned()),
        },
        tool_choice: None,
    })?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let request = captured.first().ok_or("missing captured request")?;
    let messages = request.body["messages"]
        .as_array()
        .ok_or("messages should be an array")?;
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .ok_or("assistant tool message missing")?;
    let tool = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .ok_or("tool message missing")?;
    let tool_call_id = assistant["tool_calls"][0]["id"]
        .as_str()
        .ok_or("tool id should be string")?;
    if messages.last().and_then(|message| message["role"].as_str()) == Some("assistant")
        || assistant["content"] != Value::Null
        || assistant["reasoning_content"] != ""
        || assistant["tool_calls"][0]["function"]["arguments"] != "{}"
        || tool["tool_call_id"] != tool_call_id
        || tool_call_id.len() != 9
        || !tool_call_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
        || messages
            .iter()
            .any(|message| message.get("_ignored").is_some())
    {
        return Err(format!("history sanitization drifted: {request:?}").into());
    }
    Ok(())
}

#[test]
fn provider_spec_applies_openrouter_prompt_cache_markers() -> Result<(), Box<dyn Error>> {
    let captured = Arc::new(Mutex::new(Vec::<OpenAiCompatibleRequestParts>::new()));
    let captured_transport = captured.clone();
    let spec = *find_by_name("openrouter").ok_or("missing openrouter spec")?;
    let client = OpenAiCompatibleClient::with_provider_spec(
        ProviderConfig::default(),
        move |request: OpenAiCompatibleRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| shacs_providers::ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "ok"}}]}),
            })
        },
        spec,
        "https://openrouter.ai/api/v1",
    );
    client.chat(ProviderRequest {
        model: "anthropic/claude-opus-4-6".to_owned(),
        messages: vec![
            json!({"role": "system", "content": "rules"}),
            json!({"role": "user", "content": "first"}),
            json!({"role": "assistant", "content": "second"}),
            json!({"role": "user", "content": "third"}),
        ],
        tools: vec![
            chat_completions_tool("read", "Read", json!({"type": "object"})),
            chat_completions_tool("mcp_lookup", "Lookup", json!({"type": "object"})),
        ],
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let request = captured.first().ok_or("missing captured request")?;
    if request.body["messages"][0]["content"][0]["cache_control"]["type"] != "ephemeral"
        || request.body["messages"][2]["content"][0]["cache_control"]["type"] != "ephemeral"
        || request.body["tools"][0]["cache_control"]["type"] != "ephemeral"
        || request.body["tools"][1]["cache_control"]["type"] != "ephemeral"
    {
        return Err(format!("prompt cache markers drifted: {request:?}").into());
    }
    Ok(())
}

#[test]
fn provider_spec_maps_reasoning_to_content_for_stepfun() -> Result<(), Box<dyn Error>> {
    let spec = *find_by_name("stepfun").ok_or("missing stepfun spec")?;
    let client = OpenAiCompatibleClient::with_provider_spec(
        ProviderConfig::default(),
        |_request: OpenAiCompatibleRequestParts| {
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({
                    "choices": [{"finish_reason": "stop", "message": {"content": "", "reasoning": "actual answer"}}]
                }),
            })
        },
        spec,
        "https://api.stepfun.com/v1",
    );
    let response = client.chat(ProviderRequest {
        model: "step-2".to_owned(),
        messages: vec![json!({"role": "user", "content": "hi"})],
        tools: Vec::new(),
        settings: GenerationSettings::default(),
        tool_choice: None,
    })?;
    if response.content.as_deref() != Some("actual answer")
        || response.reasoning_content.as_deref() != Some("actual answer")
    {
        return Err(format!("StepFun reasoning content drifted: {response:?}").into());
    }
    Ok(())
}

#[test]
fn ureq_transport_posts_json_and_preserves_http_response() -> Result<(), Box<dyn Error>> {
    let (base_url, request_handle) = serve_one_response(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nRetry-After: 2\r\nContent-Length: 44\r\n\r\n{\"error\":{\"message\":\"slow down\",\"code\":429}}",
    )?;
    let transport = UreqOpenAiHttpTransport::with_timeout(base_url, Duration::from_secs(5));
    let response = transport.post_json(OpenAiCompatibleRequestParts {
        path: "/chat/completions".to_owned(),
        headers: BTreeMap::from([("Authorization".to_owned(), "Bearer sk-test".to_owned())]),
        body: json!({"model": "gpt-4.1", "messages": []}),
    })?;
    let raw_request = request_handle
        .join()
        .map_err(|_| "request capture thread panicked")??;
    let lower_request = raw_request.to_ascii_lowercase();
    if !raw_request.starts_with("POST /v1/chat/completions HTTP/1.1")
        || !lower_request.contains("authorization: bearer sk-test")
        || !lower_request.contains("content-type: application/json")
        || !raw_request.contains("{\"messages\":[],\"model\":\"gpt-4.1\"}")
        || response.status != 429
        || response.headers.get("retry-after").map(String::as_str) != Some("2")
        || response.body["error"]["message"] != "slow down"
        || response.body["error"]["code"] != 429
    {
        return Err(format!(
            "ureq transport drifted: request={raw_request:?} response={response:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn ureq_transport_posts_stream_request_and_preserves_sse_body() -> Result<(), Box<dyn Error>> {
    let (base_url, request_handle) = serve_one_response(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 31\r\n\r\ndata: {\"ok\":true}\n\ndata: [DONE]",
    )?;
    let transport = UreqOpenAiHttpTransport::with_timeout(base_url, Duration::from_secs(5));
    let response = transport.post_json_stream(OpenAiCompatibleRequestParts {
        path: "/chat/completions".to_owned(),
        headers: BTreeMap::new(),
        body: json!({"stream": true}),
    })?;
    let raw_request = request_handle
        .join()
        .map_err(|_| "request capture thread panicked")??;
    let lower_request = raw_request.to_ascii_lowercase();
    if !raw_request.starts_with("POST /v1/chat/completions HTTP/1.1")
        || !lower_request.contains("accept: text/event-stream")
        || response.status != 200
        || response.body != "data: {\"ok\":true}\n\ndata: [DONE]"
    {
        return Err(format!(
            "stream transport drifted: request={raw_request:?} response={response:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn ureq_stream_transport_uses_idle_timeout_not_global_wall_clock() -> Result<(), Box<dyn Error>> {
    let (base_url, request_handle) = serve_slow_sse_response(
        vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"he\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"llo\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(110),
    )?;
    let transport = UreqOpenAiHttpTransport::with_timeout(base_url, Duration::from_millis(250));
    let response = transport.post_json_stream(OpenAiCompatibleRequestParts {
        path: "/chat/completions".to_owned(),
        headers: BTreeMap::new(),
        body: json!({"stream": true}),
    })?;
    let raw_request = request_handle
        .join()
        .map_err(|_| "request capture thread panicked")??;
    if !raw_request.starts_with("POST /v1/chat/completions HTTP/1.1")
        || response.status != 200
        || !response.body.contains("\"content\":\"he\"")
        || !response.body.contains("\"content\":\"llo\"")
        || !response.body.contains("data: [DONE]")
    {
        return Err(format!(
            "stream transport should survive total duration beyond timeout when chunks keep arriving: request={raw_request:?} response={response:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn openai_client_emits_stream_delta_before_response_eof() -> Result<(), Box<dyn Error>> {
    let (base_url, request_handle) = serve_slow_sse_response(
        vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"early\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" done\"},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::from_millis(500),
    )?;
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        UreqOpenAiHttpTransport::with_timeout(base_url, Duration::from_secs(2)),
    );
    let (event_tx, event_rx) = mpsc::channel();
    let client_handle = thread::spawn(move || {
        client
            .chat_stream(
                ProviderRequest {
                    model: "gpt-4.1".to_owned(),
                    messages: vec![json!({"role": "user", "content": "hi"})],
                    tools: Vec::new(),
                    settings: GenerationSettings::default(),
                    tool_choice: None,
                },
                &mut |event| {
                    let _ = event_tx.send(event);
                },
            )
            .map_err(|error| error.to_string())
    });
    let first_event = event_rx.recv_timeout(Duration::from_millis(250))?;
    if first_event
        != (ProviderEvent::TextDelta {
            text: "early".to_owned(),
        })
    {
        return Err(format!("first event should be early delta: {first_event:?}").into());
    }
    let response = client_handle
        .join()
        .map_err(|_| "client stream thread panicked")??;
    let raw_request = request_handle
        .join()
        .map_err(|_| "request capture thread panicked")??;
    if response.content.as_deref() != Some("early done")
        || !raw_request.starts_with("POST /v1/chat/completions HTTP/1.1")
    {
        return Err(format!(
            "client stream result drifted: request={raw_request:?} response={response:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn openai_compatible_factory_prefers_config_api_base_over_spec_default(
) -> Result<(), Box<dyn Error>> {
    let spec = find_by_name("openai").ok_or("openai spec missing")?;
    let config = ProviderConfig {
        api_base: Some(" https://proxy.example.test/v1/ ".to_owned()),
        ..ProviderConfig::default()
    };
    let resolved = resolve_openai_compatible_api_base(&config, spec)?;
    let client = openai_compatible_client_from_config(config, spec)?;
    if resolved != "https://proxy.example.test/v1/"
        || client.transport().base_url() != "https://proxy.example.test/v1/"
    {
        return Err(format!(
            "config api_base should override default: resolved={resolved:?} transport={:?}",
            client.transport().base_url()
        )
        .into());
    }
    Ok(())
}

#[test]
fn openai_compatible_factory_uses_spec_and_openai_defaults() -> Result<(), Box<dyn Error>> {
    let openrouter = find_by_name("openrouter").ok_or("openrouter spec missing")?;
    let openai = find_by_name("openai").ok_or("openai spec missing")?;
    let config = ProviderConfig::default();
    if resolve_openai_compatible_api_base(&config, openrouter)? != "https://openrouter.ai/api/v1"
        || openai_compatible_client_from_config(config.clone(), openrouter)?
            .transport()
            .base_url()
            != "https://openrouter.ai/api/v1"
        || resolve_openai_compatible_api_base(&config, openai)? != "https://api.openai.com/v1"
        || openai_compatible_client_from_config(config, openai)?
            .transport()
            .base_url()
            != "https://api.openai.com/v1"
    {
        return Err("factory did not use spec/OpenAI default api_base".into());
    }
    Ok(())
}

#[test]
fn openai_compatible_factory_treats_blank_config_base_as_missing_or_default(
) -> Result<(), Box<dyn Error>> {
    let openai = find_by_name("openai").ok_or("openai spec missing")?;
    let custom = find_by_name("custom").ok_or("custom spec missing")?;
    let config = ProviderConfig {
        api_base: Some("  ".to_owned()),
        ..ProviderConfig::default()
    };
    if resolve_openai_compatible_api_base(&config, openai)? != "https://api.openai.com/v1" {
        return Err("blank config api_base should fall back to OpenAI default".into());
    }
    let error = match openai_compatible_client_from_config(config, custom) {
        Ok(_) => return Err("custom provider without base should fail".into()),
        Err(error) => error,
    };
    if !error
        .to_string()
        .contains("missing OpenAI-compatible base URL for provider 'custom'")
    {
        return Err(format!("unexpected missing base error: {error}").into());
    }
    Ok(())
}

#[test]
fn openai_compatible_factory_rejects_non_compatible_backend() -> Result<(), Box<dyn Error>> {
    let anthropic = find_by_name("anthropic").ok_or("anthropic spec missing")?;
    let config = ProviderConfig {
        api_base: Some("https://api.anthropic.com".to_owned()),
        ..ProviderConfig::default()
    };
    let error = match openai_compatible_client_from_config(config, anthropic) {
        Ok(_) => return Err("non-openai-compatible backend should fail".into()),
        Err(error) => error,
    };
    if !error
        .to_string()
        .contains("provider 'anthropic' does not use OpenAI-compatible backend")
    {
        return Err(format!("unexpected backend guard error: {error}").into());
    }
    Ok(())
}

fn serve_one_response(
    response: &'static str,
) -> Result<(String, RequestCaptureHandle), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let request = read_http_request(&mut stream)?;
        stream
            .write_all(response.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(request)
    });
    Ok((format!("http://{address}/v1/"), handle))
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
    Ok((format!("http://{address}/v1/"), handle))
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

struct StaticStreamTransport {
    captured: Arc<Mutex<Vec<OpenAiCompatibleRequestParts>>>,
    response: OpenAiHttpStreamResponse,
}

impl OpenAiHttpTransport for StaticStreamTransport {
    fn post_json(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpResponse, shacs_providers::ProviderError> {
        self.captured
            .lock()
            .map_err(|error| shacs_providers::ProviderError::Api {
                status: None,
                message: error.to_string(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            })?
            .push(request);
        Ok(OpenAiHttpResponse {
            status: 200,
            headers: BTreeMap::new(),
            body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "fallback"}}]}),
        })
    }

    fn post_json_stream(
        &self,
        request: OpenAiCompatibleRequestParts,
    ) -> Result<OpenAiHttpStreamResponse, shacs_providers::ProviderError> {
        self.captured
            .lock()
            .map_err(|error| shacs_providers::ProviderError::Api {
                status: None,
                message: error.to_string(),
                retryable: false,
                headers: BTreeMap::new(),
                body: None,
            })?
            .push(request);
        Ok(self.response.clone())
    }
}

#[test]
fn openai_client_stream_falls_back_to_single_text_delta() -> Result<(), Box<dyn Error>> {
    let client = OpenAiCompatibleClient::new(
        ProviderConfig::default(),
        |_request: OpenAiCompatibleRequestParts| {
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "hello"}}]}),
            })
        },
    );
    let mut events = Vec::new();
    let response = client.chat_stream(
        ProviderRequest {
            model: "gpt-4.1".to_owned(),
            messages: Vec::new(),
            tools: Vec::new(),
            settings: GenerationSettings::default(),
            tool_choice: None,
        },
        &mut |event| events.push(event),
    )?;
    if response.content.as_deref() != Some("hello")
        || events
            != vec![
                ProviderEvent::TextDelta {
                    text: "hello".to_owned(),
                },
                ProviderEvent::Finish {
                    usage: json!({}),
                    reason: "stop".to_owned(),
                },
            ]
    {
        return Err(
            format!("stream fallback drifted: response={response:?} events={events:?}").into(),
        );
    }
    Ok(())
}
