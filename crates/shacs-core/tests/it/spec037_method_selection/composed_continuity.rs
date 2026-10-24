use super::composed::{REPORT_AND_CALCULATION, REPORT_AND_IMAGE};
use super::image_support::{add_image, ok_images};
use super::report::report_tools;
use super::*;
use shacs_providers::{build_anthropic_messages_request, ProviderConfig};

/// How a provider reports the error it gives once its scripted replies run
/// out, quoting the last tool call the model made, as some providers do.
#[derive(Clone, Copy, Debug)]
enum Fails {
    Api,
    Response,
    Retried,
}

struct Quoting(Provider, Fails);
impl ProviderClient for Quoting {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        if !self.0.responses.lock().unwrap().is_empty() {
            return self.0.chat(request);
        }
        let quoted = request
            .messages
            .iter()
            .rev()
            .find_map(|message| message["tool_calls"][0]["function"]["name"].as_str())
            .unwrap_or_default();
        let message = format!("tool_use '{quoted}' is not a valid tool");
        if let Fails::Api = self.1 {
            return Err(ProviderError::Api {
                status: Some(400),
                message,
                retryable: false,
                headers: Default::default(),
                body: None,
            });
        }
        Ok(LlmResponse {
            content: Some(format!("Error: {message}")),
            finish_reason: "error".to_owned(),
            error_status_code: Some(400),
            error_should_retry: Some(matches!(self.1, Fails::Retried)),
            error_retry_after_s: Some(0.001),
            ..LlmResponse::default()
        })
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn calls(calls: Vec<ToolCallRequest>) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: calls,
        ..LlmResponse::default()
    }
}

fn invented(id: &str) -> ToolCallRequest {
    ToolCallRequest::new(id, "The artwork is red", serde_json::Map::new())
}

fn thinking(text: &str, signature: &str) -> Vec<Value> {
    vec![json!({"type": "thinking", "thinking": text, "signature": signature})]
}

/// A runtime whose configuration asks the provider for thinking.
fn thinking_runtime<'a>(
    dir: &Path,
    tools: &'a ToolRegistry,
    provider: &'a Provider,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(dir, "test-model");
    config.permission_interactive = true;
    config.media_roots = vec![dir.join("media")];
    config.settings.reasoning_effort = Some("high".to_owned());
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir)?,
        ContextBuilder::new(dir),
        tools,
        provider,
        config,
    ))
}

#[test]
fn no_thinking_is_asked_for_kept_or_sent_back_in_an_image_request() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &dir.path().join("media"), ok_images());
    let mut general = text("Hello.");
    general.thinking_blocks = Some(thinking("An earlier musing.", "sig-0"));
    let image = serde_json::Map::from_iter([
        ("prompt".to_owned(), json!("a chart")),
        ("size".to_owned(), json!("64x48")),
    ]);
    let mut first = calls(vec![
        invented("c0"),
        ToolCallRequest::new("i1", "image_generate", image),
    ]);
    first.content = Some("Here is the artwork.".to_owned());
    first.thinking_blocks = Some(thinking("The artwork is blue.", "sig-1"));
    let provider = Provider::new(vec![general, first, text("Done.")]);
    let mut agent = thinking_runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Hello"))?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    let waiting = saved(&agent);
    // When
    agent.process_message(inbound("approve"))?;
    // Then
    let requests = provider.requests.lock().unwrap().clone();
    assert_eq!(
        requests[0].settings.reasoning_effort.as_deref(),
        Some("high")
    );
    for request in &requests[1..] {
        assert_eq!(request.settings.reasoning_effort, None);
        let body =
            build_anthropic_messages_request(request, &ProviderConfig::default(), false).body;
        assert!(body.get("thinking").is_none(), "{body}");
        assert!(!body.to_string().contains("sig-"), "{body}");
    }
    let resumed = &requests[2];
    let turn = resumed
        .messages
        .iter()
        .find(|message| message["tool_calls"].is_array())
        .ok_or("no tool call turn")?;
    assert_eq!(turn["content"], "", "the model's text stays out");
    assert_eq!(turn["tool_calls"][0]["function"]["name"], "unoffered_tool");
    assert_eq!(turn["tool_calls"][1]["function"]["name"], "image_generate");
    let messages = waiting["messages"].to_string();
    assert!(!messages.contains("artwork"), "{messages}");
    let after = saved(&agent).to_string();
    for session in [&waiting.to_string(), &after] {
        assert!(!session.contains("sig-1"), "{session}");
    }
    assert!(!after.contains("artwork"), "{after}");
    Ok(())
}

#[test]
fn a_provider_error_quoting_the_model_shows_only_its_digest() -> Result<(), Box<dyn Error>> {
    for (fails, request, streamed) in [
        (Fails::Api, REPORT_AND_IMAGE, false),
        (Fails::Response, REPORT_AND_CALCULATION, false),
        (Fails::Response, REPORT_AND_CALCULATION, true),
        (Fails::Retried, REPORT_AND_IMAGE, false),
    ] {
        // Given
        let dir = tempfile::tempdir()?;
        let tools = report_tools(dir.path());
        let provider = Quoting(Provider::new(vec![calls(vec![invented("c1")])]), fails);
        let mut agent = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(dir.path())?,
            ContextBuilder::new(dir.path()),
            &tools,
            &provider,
            AgentLoopConfig::new(dir.path(), "test-model"),
        );
        if streamed {
            agent = agent.with_provider_event_callback(Arc::new(|_| {}));
        }
        // When
        let answer = output(agent.process_message(inbound(request))?);
        // Then
        assert!(
            answer.contains("Sorry, I encountered an error calling the AI model: provider API error (HTTP 400); its message is withheld (<withheld: "),
            "{fails:?} {answer}"
        );
        let session = saved(&agent).to_string();
        assert!(session.contains("runtime_execution"), "{session}");
        for shown in [&answer, &session] {
            assert!(!shown.contains("artwork"), "{fails:?} {shown}");
        }
    }
    Ok(())
}
