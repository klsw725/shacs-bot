use crate::spec048_auto_mode::harness::{
    agent_loop_config, answer, calls, spec, tool_call, RoutingProvider,
};
use crate::spec048_auto_mode::support::{Exec, Options, Rules, Setup, TestResult};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    AgentLoop, AgentRunSpec, AgentRunner, ContextBuilder, MessageBus, PermissionMode,
    RuntimeToolMessage, SessionManager, ToolImageSettings,
};
use shacs_core::tools::{
    JsonMap, McpCallOutcome, McpCapability, McpCapabilityKind, McpClient, McpOperation,
    McpToolWrapper, SchemaFragment, Tool, ToolParameters, ToolResult,
};
use shacs_providers::{
    build_anthropic_messages_request, build_chat_completions_request,
    build_codex_responses_request, build_responses_request, find_by_name, LlmResponse,
    OpenAiCompatibleClient, OpenAiCompatibleRequestParts, OpenAiHttpResponse, ProviderClient,
    ProviderConfig, ProviderError, ProviderEvent, ProviderRequest,
};
use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture";
const INTRO: &str = "Images returned by the tool calls above";

struct ImageTool(Value);

impl Tool for ImageTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Returns image content parts."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, _params: JsonMap) -> ToolResult {
        ToolResult::Json(self.0.clone())
    }
}

fn parts(bytes: &[u8]) -> Value {
    json!([
        {"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))}},
        {"type": "text", "text": "original text"}
    ])
}

fn fetch(ids: &[&str]) -> LlmResponse {
    calls(
        ids.iter()
            .map(|id| {
                tool_call(
                    id,
                    "web_fetch",
                    json!({"url": "https://example.com/image.png"}),
                )
            })
            .collect(),
    )
}

fn settings(setup: &Setup, attach: bool) -> ToolImageSettings {
    ToolImageSettings {
        media_root: setup.media.clone(),
        attach,
    }
}

fn image_messages(messages: &[Value]) -> Vec<&Value> {
    messages
        .iter()
        .filter(|message| {
            message
                .pointer("/content/0/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with(INTRO))
        })
        .collect()
}

#[test]
fn runner_saves_deduplicated_images_without_base64_in_history_or_events() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    tools.register(ImageTool(parts(PNG)));
    let provider = RoutingProvider::new(vec![fetch(&["a", "b"]), answer("done")], Vec::new());
    let mut run = spec(
        &setup,
        &tools,
        &provider,
        &Options::mode(PermissionMode::BypassPermissions, false),
        "fetch",
    );
    run.tool_images = Some(settings(&setup, true));
    let result = AgentRunner::new().run(run)?;
    assert_eq!(result.stop_reason, "completed");
    let tool_messages: Vec<&Value> = result
        .messages
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect();
    assert_eq!(tool_messages.len(), 2);
    let reference = tool_messages[0]["images"][0]
        .as_str()
        .ok_or("no image reference")?;
    assert_eq!(
        reference,
        format!("tool-results/{:x}.png", Sha256::digest(PNG))
    );
    assert_eq!(
        tool_messages[0]["content"],
        format!("[image: {reference}]\noriginal text")
    );
    assert_eq!(tool_messages[0]["images"], tool_messages[1]["images"]);
    assert_eq!(fs::read(setup.media.join(reference))?, PNG);
    assert_eq!(fs::read_dir(setup.media.join("tool-results"))?.count(), 1);
    assert!(image_messages(&result.messages).is_empty());
    assert!(!serde_json::to_string(&result.tool_events)?.contains(&STANDARD.encode(PNG)));
    let mut manager = SessionManager::new(&setup.workspace)?;
    let mut session = manager.get_or_create("test:images");
    session.messages = result.messages.clone();
    manager.save(&session)?;
    let stored = manager
        .read_session_file("test:images")
        .ok_or("no session file")?;
    assert!(!stored.to_string().contains(&STANDARD.encode(PNG)));
    assert!(!stored.to_string().contains(INTRO));
    let requests = provider.assistant_requests();
    let request = requests.get(1).ok_or("no second request")?;
    let attached = image_messages(&request.messages);
    assert_eq!(attached.len(), 1);
    assert_eq!(
        attached[0]["content"][1]["image_url"]["url"],
        format!("data:image/png;base64,{}", STANDARD.encode(PNG))
    );
    assert_eq!(attached[0]["content"][1]["_meta"]["path"], reference);
    let position = request
        .messages
        .iter()
        .position(|message| message == attached[0])
        .ok_or("no attachment")?;
    assert_eq!(request.messages[position - 1]["tool_call_id"], "b");
    assert!(!request
        .messages
        .iter()
        .any(|message| message.get("images").is_some()));

    // Each request path must remove runtime metadata even without governance.
    let mut format_request = request.clone();
    let tool = format_request
        .messages
        .iter_mut()
        .find(|message| message["role"] == "tool")
        .ok_or("no format-test tool")?;
    tool["images"] = json!([reference]);
    let config = ProviderConfig::default();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let capture = captured.clone();
    let client = OpenAiCompatibleClient::with_provider_spec(
        config.clone(),
        move |parts: OpenAiCompatibleRequestParts| {
            assert_eq!(parts.path, "/chat/completions");
            capture
                .lock()
                .expect("test capture lock poisoned")
                .push(parts.body);
            Ok(OpenAiHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"choices": [{"finish_reason": "stop", "message": {"content": "done"}}]}),
            })
        },
        *find_by_name("openai").ok_or("no OpenAI spec")?,
        "http://fixture",
    );
    client.chat(format_request.clone())?;
    let sanitized_body = captured
        .lock()
        .expect("test capture lock poisoned")
        .pop()
        .ok_or("no sanitized request")?;
    let bodies = [
        (
            build_anthropic_messages_request(&format_request, &config, false).body,
            "\"type\":\"image\"",
        ),
        (
            build_chat_completions_request(&format_request, &config).body,
            "\"type\":\"image_url\"",
        ),
        (sanitized_body, "\"type\":\"image_url\""),
        (
            build_responses_request(&format_request, &config, false).body,
            "\"type\":\"input_image\"",
        ),
        (
            build_codex_responses_request(&format_request, &config).body,
            "\"type\":\"input_image\"",
        ),
    ];
    for (body, image_type) in bodies {
        let serialized = body.to_string();
        assert!(serialized.contains(image_type), "{serialized}");
        assert!(!serialized.contains("\"images\""), "{serialized}");
        assert!(!serialized.contains("\"_meta\""), "{serialized}");
    }
    Ok(())
}

#[test]
fn runner_rejects_unkeepable_images_and_preserves_other_results() -> TestResult {
    let setup = Setup::new()?;
    let mut oversized = PNG.to_vec();
    oversized.resize(5 * 1024 * 1024 + 1, 0);
    for (output, configured, reason) in [
        (parts(&oversized), true, "image larger than 5 MiB"),
        (parts(b"unknown"), true, "unsupported image type"),
        (parts(PNG), false, "no media folder"),
        (parts(PNG), true, "write failed"),
    ] {
        let (mut tools, _) = setup.registry(Exec::Gated);
        tools.register(ImageTool(output));
        let provider = RoutingProvider::new(vec![fetch(&["a"]), answer("done")], Vec::new());
        let mut run = spec(
            &setup,
            &tools,
            &provider,
            &Options::mode(PermissionMode::BypassPermissions, false),
            "fetch",
        );
        run.tool_images = configured.then(|| settings(&setup, true));
        if reason == "write failed" {
            let file = setup.media.join("blocked");
            fs::write(&file, "not a directory")?;
            run.tool_images = Some(ToolImageSettings {
                media_root: file,
                attach: true,
            });
        }
        let result = AgentRunner::new().run(run)?;
        let message = result
            .messages
            .iter()
            .find(|message| message["role"] == "tool")
            .ok_or("no tool result")?;
        assert_eq!(
            message["content"],
            format!("[image not kept: {reason}]\noriginal text")
        );
        assert!(message.get("images").is_none());
        assert_eq!(result.stop_reason, "completed");
    }
    let (mut tools, _) = setup.registry(Exec::Gated);
    let ordinary = json!([{"type": "audio", "data": "unchanged"}, {"type": "image_url", "image_url": {"url": "unchanged"}}]);
    tools.register(ImageTool(ordinary.clone()));
    let provider = RoutingProvider::new(vec![fetch(&["a"]), answer("done")], Vec::new());
    let run = spec(
        &setup,
        &tools,
        &provider,
        &Options::mode(PermissionMode::BypassPermissions, false),
        "fetch",
    );
    let result = AgentRunner::new().run(run)?;
    let message = result
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .ok_or("no result")?;
    assert_eq!(message["content"], ordinary.to_string());
    let restored: RuntimeToolMessage = serde_json::from_value(message.clone())?;
    assert!(restored.images.is_empty());
    assert!(restored.to_json().get("images").is_none());
    Ok(())
}

#[test]
fn requests_attach_only_latest_three_and_report_missing_files() -> TestResult {
    for attach in [true, false] {
        let setup = Setup::new()?;
        let (tools, _) = setup.registry(Exec::Gated);
        let provider = RoutingProvider::new(vec![answer("done")], Vec::new());
        let mut run = spec(
            &setup,
            &tools,
            &provider,
            &Options::mode(PermissionMode::BypassPermissions, false),
            "continue",
        );
        run.tool_images = Some(settings(&setup, attach));
        fs::create_dir_all(setup.media.join("tool-results"))?;
        let mut history = Vec::new();
        for index in 0..4 {
            let reference = format!("tool-results/{index:064x}.png");
            if index != 2 {
                fs::write(setup.media.join(&reference), PNG)?;
            }
            history.push(json!({"role": "assistant", "tool_calls": [{"id": format!("c{index}"), "type": "function", "function": {"name": "web_fetch", "arguments": "{}"}}]}));
            history.push(json!({"role": "tool", "tool_call_id": format!("c{index}"), "name": "web_fetch", "content": format!("[image: {reference}]"), "images": [reference]}));
        }
        run.initial_messages.splice(1..1, history);
        let result = AgentRunner::new().run(run)?;
        let requests = provider.assistant_requests();
        let request = requests.first().ok_or("no request")?;
        let attached = image_messages(&request.messages);
        assert_eq!(attached.len(), if attach { 3 } else { 0 });
        if attach {
            assert_eq!(
                attached[0]["content"][1]["_meta"]["path"],
                format!("tool-results/{:064x}.png", 1)
            );
            assert!(attached[1]["content"][0]["text"]
                .as_str()
                .ok_or("no unavailable text")?
                .contains(&format!("[image unavailable: tool-results/{:064x}.png]", 2)));
            assert_eq!(
                attached[2]["content"][1]["_meta"]["path"],
                format!("tool-results/{:064x}.png", 3)
            );
        }
        assert!(!request
            .messages
            .iter()
            .any(|message| message.get("images").is_some()));
        assert!(image_messages(&result.messages).is_empty());
    }
    Ok(())
}

fn assert_unavailable_references(setup: &Setup, references: &[String]) -> TestResult {
    let (tools, _) = setup.registry(Exec::Gated);
    let provider = RoutingProvider::new(vec![answer("done")], Vec::new());
    let mut run = spec(
        setup,
        &tools,
        &provider,
        &Options::mode(PermissionMode::BypassPermissions, false),
        "continue",
    );
    run.tool_images = Some(settings(setup, true));
    let tool_calls: Vec<Value> = references.iter().enumerate().map(|(index, _)| {
        json!({"id": format!("c{index}"), "type": "function", "function": {"name": "web_fetch", "arguments": "{}"}})
    }).collect();
    run.initial_messages
        .push(json!({"role": "assistant", "tool_calls": tool_calls}));
    let indexed_references = references.iter().enumerate();
    for (index, reference) in indexed_references {
        run.initial_messages.push(json!({"role": "tool", "tool_call_id": format!("c{index}"), "content": format!("[image: {reference}]"), "images": [reference]}));
    }
    assert_eq!(AgentRunner::new().run(run)?.stop_reason, "completed");
    let requests = provider.assistant_requests();
    let request = requests.first().ok_or("no request")?;
    let attached = image_messages(&request.messages);
    assert_eq!(attached.len(), 1);
    assert_eq!(
        attached[0]["content"]
            .as_array()
            .ok_or("no attachment blocks")?
            .len(),
        1
    );
    let notice = attached[0]["content"][0]["text"]
        .as_str()
        .ok_or("no unavailable notice")?;
    for reference in references {
        assert!(notice.contains(&format!("[image unavailable: {reference}]")));
    }
    assert!(!serde_json::to_string(&request.messages)?.contains("base64,"));
    Ok(())
}

#[test]
fn history_image_references_cannot_use_absolute_paths_or_parent_traversal() -> TestResult {
    let setup = Setup::new()?;
    fs::create_dir_all(setup.media.join("tool-results"))?;
    let outside = setup.root.join("outside.png");
    fs::write(&outside, PNG)?;
    fs::write(setup.media.join("outside.png"), PNG)?;
    assert_unavailable_references(
        &setup,
        &[
            outside
                .to_str()
                .ok_or("outside path is not UTF-8")?
                .to_owned(),
            "tool-results/../outside.png".to_owned(),
        ],
    )
}

#[cfg(unix)]
#[test]
fn history_image_references_cannot_follow_file_or_store_directory_symlinks() -> TestResult {
    use std::os::unix::fs as unix_fs;

    for kind in ["file", "directory"] {
        let setup = Setup::new()?;
        let filename = format!("{:064x}.png", 1);
        let reference = format!("tool-results/{filename}");
        let store = setup.media.join("tool-results");
        if kind == "file" {
            fs::create_dir_all(&store)?;
            let outside = setup.root.join("outside.png");
            fs::write(&outside, PNG)?;
            unix_fs::symlink(outside, store.join(filename))?;
        } else {
            let outside = setup.root.join("outside-store");
            fs::create_dir_all(&outside)?;
            fs::write(outside.join(filename), PNG)?;
            unix_fs::symlink(outside, store)?;
        }
        assert_unavailable_references(&setup, &[reference])?;
    }
    Ok(())
}

#[test]
fn auto_mode_checks_receive_saved_text_without_image_attachments() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    tools.register(ImageTool(parts(PNG)));
    let provider = RoutingProvider::new(
        vec![
            fetch(&["a"]),
            calls(vec![tool_call(
                "after-image",
                "exec",
                json!({"command": "python3 run.py"}),
            )]),
            answer("done"),
        ],
        vec![Ok("no"), Ok("no")],
    );
    let mut run = spec(&setup, &tools, &provider, &Options::auto(true), "fetch");
    run.tool_images = Some(settings(&setup, true));
    let result = AgentRunner::new().run(run)?;
    let payloads = provider.check_payloads();
    let image_check = payloads
        .iter()
        .find(|payload| payload["tool"] == "web_fetch")
        .ok_or("image result was not checked")?;
    assert!(image_check["result"]
        .as_str()
        .ok_or("no check text")?
        .starts_with("[image: tool-results/"));
    assert!(image_check["result"]
        .as_str()
        .ok_or("no check text")?
        .ends_with("original text"));
    assert!(!image_messages(&provider.assistant_requests()[1].messages).is_empty());
    assert!(
        provider
            .payloads()
            .iter()
            .any(|payload| payload["action"]["name"] == "exec"),
        "the action after the returned image must be classified"
    );
    let checked_requests = provider.check_requests().into_iter().chain(
        provider
            .classifier_requests()
            .into_iter()
            .map(|(request, _)| request),
    );
    for request in checked_requests {
        let serialized = serde_json::to_string(&request.messages)?;
        assert!(!serialized.contains("base64,"));
        assert!(!serialized.contains(INTRO));
    }
    assert!(image_messages(&result.messages).is_empty());
    Ok(())
}

#[test]
fn approved_image_call_is_saved_in_the_session() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    tools.register(ImageTool(parts(PNG)));
    let provider = RoutingProvider::new(vec![fetch(&["a"]), answer("done")], Vec::new());
    let mut config = agent_loop_config(&setup, true);
    config.permission_mode_snapshot.mode = PermissionMode::Default;
    config.permission_environment = Some(setup.environment(&Rules::ask(&["web_fetch"])));
    config.tool_images = Some(settings(&setup, true));
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&setup.workspace)?,
        ContextBuilder::new(&setup.workspace),
        &tools,
        &provider,
        config,
    );
    assert_eq!(
        agent
            .process_direct("fetch the image", Some("test:approved-image"))?
            .stop_reason,
        "ask_user"
    );
    assert_eq!(
        agent
            .process_direct("1", Some("test:approved-image"))?
            .final_content
            .as_deref(),
        Some("done")
    );
    let file = SessionManager::new(&setup.workspace)?
        .read_session_file("test:approved-image")
        .ok_or("no session")?;
    let messages = file["messages"].as_array().ok_or("no messages")?;
    let tool = messages
        .iter()
        .find(|message| message["tool_call_id"] == "a")
        .ok_or("no saved tool result")?;
    assert!(tool["images"][0]
        .as_str()
        .ok_or("no reference")?
        .starts_with("tool-results/"));
    assert!(tool["content"]
        .as_str()
        .ok_or("no content")?
        .starts_with("[image: tool-results/"));
    assert!(!file.to_string().contains("base64,"));
    assert!(image_messages(messages).is_empty());
    let requests = provider.assistant_requests();
    let after_approval = requests.last().ok_or("no request after approval")?;
    let attached = image_messages(&after_approval.messages);
    assert_eq!(attached.len(), 1);
    assert_eq!(
        attached[0]["content"][1]["image_url"]["url"],
        format!("data:image/png;base64,{}", STANDARD.encode(PNG))
    );
    Ok(())
}

#[test]
fn agent_loop_reloads_tool_images_on_the_following_turn() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    tools.register(ImageTool(parts(PNG)));
    let provider = RoutingProvider::new(
        vec![fetch(&["a"]), answer("first done"), answer("second done")],
        vec![Ok("no")],
    );
    let mut config = agent_loop_config(&setup, true);
    config.tool_images = Some(settings(&setup, true));
    {
        let mut agent = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(&setup.workspace)?,
            ContextBuilder::new(&setup.workspace),
            &tools,
            &provider,
            config.clone(),
        );
        assert_eq!(
            agent
                .process_direct("fetch the image", Some("test:reloaded-image"))?
                .final_content
                .as_deref(),
            Some("first done")
        );
    }
    let stored = SessionManager::new(&setup.workspace)?
        .read_session_file("test:reloaded-image")
        .ok_or("no stored session")?;
    assert!(
        stored["messages"]
            .as_array()
            .ok_or("no stored messages")?
            .iter()
            .any(|message| message["role"] == "tool" && message["images"].is_array()),
        "the first turn must execute and save the image tool result"
    );
    let mut reloaded = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&setup.workspace)?,
        ContextBuilder::new(&setup.workspace),
        &tools,
        &provider,
        config,
    );
    assert_eq!(
        reloaded
            .process_direct("describe that image", Some("test:reloaded-image"))?
            .final_content
            .as_deref(),
        Some("second done")
    );
    let requests = provider.assistant_requests();
    let attached = image_messages(&requests.last().ok_or("no following-turn request")?.messages);
    assert_eq!(attached.len(), 1);
    assert_eq!(
        attached[0]["content"][1]["image_url"]["url"],
        format!("data:image/png;base64,{}", STANDARD.encode(PNG))
    );
    Ok(())
}

struct RejectImageProvider {
    inner: RoutingProvider,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl ProviderClient for RejectImageProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .expect("test request lock poisoned")
            .push(request.clone());
        if request.messages.iter().any(|message| {
            message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "image_url"))
        }) {
            return Ok(LlmResponse {
                content: Some("Error: image input unsupported".to_owned()),
                finish_reason: "error".to_owned(),
                error_status_code: Some(400),
                ..LlmResponse::default()
            });
        }
        self.inner.chat(request)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

#[test]
fn rejected_tool_images_retry_as_reference_text() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    tools.register(ImageTool(parts(PNG)));
    let provider = RejectImageProvider {
        inner: RoutingProvider::new(vec![fetch(&["a"]), answer("done")], Vec::new()),
        requests: Mutex::new(Vec::new()),
    };
    let mut run = AgentRunSpec::new(
        vec![json!({"role": "user", "content": "fetch"})],
        &tools,
        &provider,
        "main-model",
    );
    run.tool_context = setup.context(&Options::mode(PermissionMode::BypassPermissions, false));
    run.tool_images = Some(settings(&setup, true));
    assert_eq!(AgentRunner::new().run(run)?.stop_reason, "completed");
    let requests = provider
        .requests
        .lock()
        .expect("test request lock poisoned");
    assert_eq!(requests.len(), 3);
    let attached = image_messages(&requests[2].messages);
    assert_eq!(attached.len(), 1);
    assert!(attached[0]["content"][1]["text"]
        .as_str()
        .ok_or("no fallback text")?
        .starts_with("[image: tool-results/"));
    Ok(())
}

#[test]
fn mcp_image_content_uses_the_same_runner_storage() -> TestResult {
    let setup = Setup::new()?;
    let (mut tools, _) = setup.registry(Exec::Gated);
    let content = parts(PNG).as_array().ok_or("no content parts")?.clone();
    let client: Arc<dyn McpClient> =
        Arc::new(move |_: McpOperation| McpCallOutcome::ImageContent(content.clone()));
    let wrapper = McpToolWrapper::new(
        client,
        McpCapability {
            kind: McpCapabilityKind::Tool,
            server_name: "image".to_owned(),
            name: "capture".to_owned(),
            description: None,
            input_schema: None,
            uri: None,
            arguments: Vec::new(),
            timeout_seconds: 5,
        },
    );
    let name = wrapper.name().to_owned();
    tools.register(wrapper);
    let provider = RoutingProvider::new(
        vec![
            calls(vec![tool_call("m", &name, json!({}))]),
            answer("done"),
        ],
        Vec::new(),
    );
    let mut run = spec(
        &setup,
        &tools,
        &provider,
        &Options::mode(PermissionMode::BypassPermissions, false),
        "capture",
    );
    run.tool_images = Some(settings(&setup, true));
    let result = AgentRunner::new().run(run)?;
    let message = result
        .messages
        .iter()
        .find(|message| message["tool_call_id"] == "m")
        .ok_or("no MCP tool message")?;
    let reference = message["images"][0].as_str().ok_or("no image")?;
    assert_eq!(
        message["content"],
        format!("[image: {reference}]\noriginal text")
    );
    assert_eq!(fs::read(setup.media.join(reference))?, PNG);
    Ok(())
}
