use super::image_support::{generate, image_runtime, image_tools, ok_images, Images};
use super::*;

/// Streams each reply like a live provider before returning it: its text as
/// reasoning and text deltas, and each tool call with its input; everything
/// else comes from the scripted provider.
pub(super) struct Streaming(pub(super) Provider);
impl ProviderClient for Streaming {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.0.chat(request)
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        let response = self.0.chat(request)?;
        if let Some(text) = &response.content {
            on_event(ProviderEvent::ReasoningDelta { text: text.clone() });
            on_event(ProviderEvent::TextDelta { text: text.clone() });
        }
        for call in &response.tool_calls {
            on_event(ProviderEvent::ToolCallStart {
                id: call.id.clone(),
                name: call.name.clone(),
            });
            on_event(ProviderEvent::ToolCallReady {
                id: call.id.clone(),
                name: call.name.clone(),
                input: Value::Object(call.arguments.clone()),
            });
        }
        on_event(ProviderEvent::Finish {
            usage: json!(response.usage),
            reason: response.finish_reason.clone(),
        });
        Ok(response)
    }
}

#[test]
fn a_failed_image_reply_is_neither_streamed_nor_kept_in_history() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let api = || ProviderError::Api {
        status: Some(500),
        message: "boom".to_owned(),
        retryable: false,
        headers: Default::default(),
        body: None,
    };
    let images = Images {
        error: Some(api),
        ..ok_images()
    };
    let tools = image_tools(&media, images);
    let provider = Streaming(Provider::new(vec![
        generate("64x48"),
        text("Here is your image."),
    ]));
    let streamed = Arc::new(Mutex::new(String::new()));
    let sink = streamed.clone();
    let mut config = AgentLoopConfig::new(dir.path(), "test-model");
    config.permission_interactive = true;
    config.media_roots = vec![media];
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        config,
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let ProviderEvent::TextDelta { text } = event {
            sink.lock().unwrap().push_str(text);
        }
    }));
    agent.process_message(inbound("Generate an image of a red square, 64x48 png"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.starts_with("No new image was saved for this request"),
        "{answer}"
    );
    assert!(!streamed.lock().unwrap().contains("Here is your image."));
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("Here is your image."), "{history}");
    assert!(history.contains("No new image was saved for this request"));
    Ok(())
}

#[test]
fn other_requests_keep_streaming_their_reply() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Streaming(Provider::new(vec![text("Hello there.")]));
    let streamed = Arc::new(Mutex::new(String::new()));
    let sink = streamed.clone();
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        AgentLoopConfig::new(dir.path(), "test-model"),
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let ProviderEvent::TextDelta { text } = event {
            sink.lock().unwrap().push_str(text);
        }
    }));
    // When
    let answer = output(agent.process_message(inbound("hello"))?);
    // Then
    assert_eq!(answer, "Hello there.");
    assert_eq!(streamed.lock().unwrap().as_str(), "Hello there.");
    Ok(())
}

#[test]
fn text_sent_with_an_image_call_is_never_kept() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let mut first = generate("64x48");
    first.content = Some("Here is your blue cat!".to_owned());
    let provider = Provider::new(vec![first, text("The blue cat is perfect.")]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    // When
    let prompt =
        output(agent.process_message(inbound("Generate an image of a red square, 64x48 png"))?);
    let pending = saved(&agent)["messages"].to_string();
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(prompt.contains("Permission approval required"), "{prompt}");
    assert!(!prompt.contains("blue cat"), "{prompt}");
    assert!(!pending.contains("blue cat"), "{pending}");
    assert!(pending.contains("image_generate"), "{pending}");
    assert!(!answer.contains("blue cat"), "{answer}");
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("blue cat"), "{history}");
    assert!(history.contains("image_generate"));
    Ok(())
}

#[test]
fn other_requests_keep_text_sent_with_a_tool_call() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["read_file"], &Arc::new(AtomicUsize::new(0)));
    let mut first = call("read_file", json!({"path": "notes.txt"}));
    first.content = Some("Let me look that up.".to_owned());
    let provider = Provider::new(vec![first, text("Found it.")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound("hello"))?);
    // Then
    assert!(answer.starts_with("Found it."), "{answer}");
    let history = saved(&agent)["messages"].to_string();
    assert!(history.contains("Let me look that up."), "{history}");
    assert!(history.contains("Found it."));
    Ok(())
}

#[test]
fn an_image_turn_streams_none_of_the_models_words() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Streaming(Provider::new(vec![
        call("ask_user", json!({"question": "The blue cat is ready?"})),
        text("The blue cat is ready."),
    ]));
    let events = Arc::new(Mutex::new(String::new()));
    let sink = events.clone();
    let mut config = AgentLoopConfig::new(dir.path(), "test-model");
    config.media_roots = vec![media];
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        config,
    )
    .with_provider_event_callback(Arc::new(move |event| {
        sink.lock().unwrap().push_str(&format!("{event:?}"));
    }));
    // When
    let answer = output(agent.process_message(inbound("Generate an image of a red square"))?);
    // Then
    assert!(
        answer.starts_with("No image was generated for this request"),
        "{answer}"
    );
    let streamed = events.lock().unwrap();
    assert!(!streamed.contains("blue cat"), "{streamed}");
    Ok(())
}
