use super::image_support::{image_runtime, image_tools, ok_images};
use super::*;

/// Notes everything a runtime hook (verbose log, observability, plugin) is
/// handed.
pub(super) struct Recorder(pub(super) Arc<Mutex<String>>);
impl Recorder {
    fn note(&self, seen: impl std::fmt::Debug) {
        self.0
            .lock()
            .expect("recorder log lock poisoned")
            .push_str(&format!("{seen:?}\n"));
    }
}
impl AgentHook for Recorder {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn wants_streaming(&self) -> bool {
        true
    }
    fn before_iteration(&self, context: &AgentHookContext) {
        self.note(&context.messages);
    }
    fn on_stream(&self, _: &AgentHookContext, text: &str) {
        self.note(text);
    }
    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        self.note((&context.messages, calls));
    }
    fn after_response(&self, context: &AgentHookContext, response: &LlmResponse) {
        self.note((&context.messages, response));
    }
    fn after_iteration(&self, context: &AgentHookContext) {
        self.note(&context.messages);
    }
    fn finalize_content(&self, _: &AgentHookContext, content: String) -> String {
        self.note(&content);
        content
    }
}

fn recorded(
    message: &str,
    responses: Vec<LlmResponse>,
) -> Result<(String, String), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let tools = image_tools(&media, ok_images());
    let provider = Provider::new(responses);
    let seen = Arc::new(Mutex::new(String::new()));
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?
        .with_agent_hook(Arc::new(Recorder(seen.clone())));
    let answer = output(agent.process_message(inbound(message))?);
    let seen = seen.lock().expect("recorder log lock poisoned").clone();
    Ok((answer, seen))
}

#[test]
fn runtime_hooks_see_none_of_the_models_words_in_an_image_request() -> Result<(), Box<dyn Error>> {
    // Given
    let mut first = call("ask_user", json!({"question": "The blue cat is ready?"}));
    first.content = Some("Here is the blue cat!".to_owned());
    first.reasoning_content = Some("I will say the blue cat is done.".to_owned());
    // When
    let (answer, seen) = recorded(
        "Generate an image of a red square",
        vec![first, text("The blue cat is ready.")],
    )?;
    // Then
    assert!(
        answer.starts_with("No image was generated for this request"),
        "{answer}"
    );
    assert!(!seen.is_empty());
    let leak = seen
        .find("blue cat")
        .map(|at| &seen[at.saturating_sub(120)..at + 8]);
    assert_eq!(leak, None);
    Ok(())
}

#[test]
fn runtime_hooks_still_see_other_replies() -> Result<(), Box<dyn Error>> {
    // When
    let (answer, seen) = recorded("hello", vec![text("Hello there.")])?;
    // Then
    assert_eq!(answer, "Hello there.");
    assert!(seen.contains("Hello there."), "{seen}");
    Ok(())
}
