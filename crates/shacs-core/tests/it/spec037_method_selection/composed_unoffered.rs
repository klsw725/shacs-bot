use super::calculation::{add_exec, exec};
use super::composed::REPORT_AND_IMAGE;
use super::composed_output::without;
use super::composed_quiet::Diagnostic;
use super::image_checkpoint::SessionFileSpy;
use super::image_stream::Streaming;
use super::report::{read, report_tools};
use super::*;

/// A policy hook, like a plugin's `tool:before`: it notes each call it is
/// asked to judge, as written.
struct Judge(Arc<Mutex<String>>);
impl AgentHook for Judge {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        self.0
            .lock()
            .expect("policy log lock poisoned")
            .push_str(&format!("{calls:?}\n"));
        Vec::new()
    }
}

/// A call to a tool no one offered, under a name the model made up.
fn invented(id: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            id,
            "The artwork is blue",
            serde_json::Map::new(),
        )],
        ..LlmResponse::default()
    }
}

/// A streaming provider that also reports an image's lifecycle under a
/// candidate id it made up.
struct WithMedia(Streaming);
impl ProviderClient for WithMedia {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.0.chat(request)
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        if let Ok(id) = shacs_providers::ProviderMediaCandidateId::new("The-artwork-is-blue") {
            let seen = shacs_providers::ProviderMediaLifecycleObservation::started(id);
            on_event(ProviderEvent::MediaLifecycle(seen));
        }
        self.0.chat_stream(request, on_event)
    }
}

/// A runtime on a streaming `provider` that notes its provider events.
fn streaming<'a>(
    dir: &Path,
    tools: &'a ToolRegistry,
    provider: &'a dyn ProviderClient,
    events: Arc<Mutex<String>>,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(dir, "test-model");
    config.permission_interactive = true;
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir)?,
        ContextBuilder::new(dir),
        tools,
        provider,
        config,
    )
    .with_provider_event_callback(Arc::new(move |event| {
        events
            .lock()
            .expect("events log lock poisoned")
            .push_str(&format!("{event:?}\n"));
    })))
}

fn recorder() -> Arc<Mutex<String>> {
    Arc::new(Mutex::new(String::new()))
}

#[test]
fn streamed_call_starts_show_only_offered_tools_under_digested_ids() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let mut reply = text("Revenue grew 10%. [source: notes.md]");
    reply.finish_reason = "canary-finish".to_owned();
    reply.usage = std::collections::BTreeMap::from([
        ("canary-usage".to_owned(), 7),
        ("total_tokens".to_owned(), 9),
    ]);
    let provider = WithMedia(Streaming(Provider::new(vec![
        invented("The-artwork-is-red"),
        read("The-artwork-is-green", "notes.md"),
        reply,
    ])));
    let events = recorder();
    let mut agent = streaming(dir.path(), &tools, &provider, events.clone())?;
    // When
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // Then
    let events = events.lock().unwrap();
    assert!(events.contains("MediaLifecycle"), "{events}");
    assert!(events.contains("ToolCallStart { id: \"call-"), "{events}");
    assert!(events.contains("name: \"read_file\""), "{events}");
    assert!(events.contains("reason: \"other\""), "{events}");
    assert!(events.contains("\"total_tokens\": Number(9)"), "{events}");
    assert!(
        !events.contains("artwork") && !events.contains("canary"),
        "{events}"
    );
    Ok(())
}

#[test]
fn no_copy_of_a_call_names_a_tool_the_model_was_not_offered() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Streaming(Provider::new(vec![
        invented("c1"),
        read("r1", "notes.md"),
        text("Revenue grew 10%. [source: notes.md]"),
    ]));
    let (events, seen, judged, spied) = (recorder(), recorder(), recorder(), recorder());
    let hooks = CompositeHook::new(vec![
        Arc::new(Diagnostic(seen.clone())),
        Arc::new(Judge(judged.clone())),
        Arc::new(SessionFileSpy {
            sessions: dir.path().join("sessions"),
            seen: spied.clone(),
        }),
    ]);
    let sink = events.clone();
    let mut agent = streaming(dir.path(), &tools, &provider, events.clone())?
        .with_agent_hook(Arc::new(hooks))
        .with_tool_event_callback(Arc::new(move |event| {
            sink.lock().unwrap().push_str(&format!("{event:?}\n"));
        }));
    // When
    let result = agent.process_message(inbound(REPORT_AND_IMAGE))?;
    let used = format!("{:?}", result.tools_used);
    let answer = output(result);
    // Then
    assert!(
        answer.contains("\nRevenue grew 10%. [source #1]\n"),
        "{answer}"
    );
    assert!(
        used.contains("\"unoffered_tool\"") && used.contains("\"read_file\""),
        "{used}"
    );
    let saved = saved(&agent).to_string();
    assert!(saved.contains("\"name\":\"unoffered_tool\""), "{saved}");
    let spied = without(&spied.lock().unwrap(), "\"task_method_held_results_v1\":");
    assert!(
        spied.contains("runtime_checkpoint"),
        "the spy saw no checkpoint"
    );
    let shown = [
        answer,
        used,
        saved,
        spied,
        seen.lock().unwrap().clone(),
        judged.lock().unwrap().clone(),
        events.lock().unwrap().clone(),
    ];
    for shown in &shown {
        let leak = shown
            .find("artwork")
            .map(|at| &shown[at.saturating_sub(160)..at + 7]);
        assert_eq!(leak, None);
    }
    Ok(())
}

#[test]
fn a_request_made_an_image_one_again_drops_tool_names_only_the_model_wrote(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    add_exec(&mut tools, &[]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(1)'"),
        invented("c2"),
        call("ask_user", json!({"question": "Go on?"})),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(
        "Calculate the floor area in m^2 and generate an image of the room",
    ))?;
    agent.process_message(inbound("/method use calculation"))?;
    agent.process_message(inbound("continue"))?;
    let before = saved(&agent)["messages"].to_string();
    // When
    let changed = output(agent.process_message(inbound("/method auto"))?);
    // Then
    assert!(before.contains("The artwork is blue"), "{before}");
    assert!(
        changed.contains("Method: composed [calculation + image]"),
        "{changed}"
    );
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("artwork"), "{history}");
    assert!(history.contains("\"name\":\"unoffered_tool\""), "{history}");
    assert!(
        history.contains("\"name\":\"ask_user\""),
        "registered names stay: {history}"
    );
    Ok(())
}
