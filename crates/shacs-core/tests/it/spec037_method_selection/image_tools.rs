use super::image_support::{image_runtime, image_tools, ok_images};
use super::*;

/// Tools that would carry the model's own words to the user, each called with
/// an image claim nothing checked.
fn claims() -> Vec<LlmResponse> {
    let claim = "The blue cat is ready.";
    vec![
        call(
            "ask_user",
            json!({"question": format!("{claim} Want changes?")}),
        ),
        call("message", json!({"content": claim})),
        call("spawn", json!({"task": claim})),
        call("propose_entrusted_work", json!({"goal": claim})),
        text(claim),
    ]
}

#[test]
fn an_image_request_offers_the_model_only_image_generate() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let mut tools = image_tools(&media, ok_images());
    let counter = Arc::new(AtomicUsize::new(0));
    for name in ["message", "spawn", "propose_entrusted_work"] {
        tools.register(RegisteredTool {
            name,
            counter: counter.clone(),
            read_only: false,
        });
    }
    let provider = Provider::new(claims());
    let events = Arc::new(Mutex::new(String::new()));
    let sink = events.clone();
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?
        .with_tool_event_callback(Arc::new(move |event| {
            sink.lock().unwrap().push_str(&format!("{event:?}"));
        }));
    // When
    let answer = output(agent.process_message(inbound("Generate an image of a red square"))?);
    // Then
    assert!(
        answer.starts_with("No image was generated for this request (image: not generated)"),
        "{answer}"
    );
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &history, &*events.lock().unwrap()] {
        assert!(!shown.contains("blue cat"), "{shown}");
    }
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    let offered: Vec<String> = provider.requests.lock().unwrap()[0]
        .tools
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(offered, ["image_generate"]);
    Ok(())
}
