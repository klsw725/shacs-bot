use super::composed::REPORT_AND_IMAGE;
use super::image_support::{add_image, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

const PROMPT: &str = "a blue chart of the room";

/// A diagnostic hook, like the verbose log or observability: it does not ask
/// for raw tool arguments.
pub(super) struct Diagnostic(pub(super) Arc<Mutex<String>>);
impl AgentHook for Diagnostic {
    fn before_iteration(&self, context: &AgentHookContext) {
        self.0
            .lock()
            .expect("diagnostic log lock poisoned")
            .push_str(&format!("{:?}\n", context.messages));
    }
    fn before_execute_tools(&self, context: &AgentHookContext, calls: &[RuntimeToolCall]) {
        let seen = format!("{:?} {calls:?}\n", context.messages);
        self.0
            .lock()
            .expect("diagnostic log lock poisoned")
            .push_str(&seen);
    }
    fn after_response(&self, context: &AgentHookContext, response: &LlmResponse) {
        let seen = format!("{:?} {response:?}\n", context.messages);
        self.0
            .lock()
            .expect("diagnostic log lock poisoned")
            .push_str(&seen);
    }
}

/// A policy hook, like a plugin's `tool:before`: it judges each call as the
/// model wrote it and may block it.
struct Policy(Arc<Mutex<String>>);
impl AgentHook for Policy {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        let judged = format!("{calls:?}");
        self.0
            .lock()
            .expect("policy log lock poisoned")
            .push_str(&judged);
        calls
            .iter()
            .filter(|call| call.arguments.to_string().contains("blue"))
            .map(|call| RuntimeToolMessage {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: "Error: blocked by policy".to_owned(),
            })
            .collect()
    }
}

#[test]
fn diagnostics_see_no_words_of_a_call_and_a_policy_judges_it_as_written(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let mut repeat = ToolCallRequest::new(
        "i2",
        "image_generate",
        serde_json::Map::from_iter([
            ("prompt".to_owned(), json!("a red chart of the room")),
            ("size".to_owned(), json!("64x48")),
        ]),
    );
    let field = |name: &str| {
        Some(serde_json::Map::from_iter([(
            "note".to_owned(),
            json!(name),
        )]))
    };
    repeat.extra_content = field("canary-extra-content");
    repeat.provider_specific_fields = field("canary-call-fields");
    repeat.function_provider_specific_fields = field("canary-function-fields");
    let again = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![repeat],
        error_kind: Some("canary-error-kind".to_owned()),
        error_type: Some("canary-error-type".to_owned()),
        error_code: Some("canary-error-code".to_owned()),
        ..LlmResponse::default()
    };
    let mut reply = text("Revenue grew 10%. [source: notes.md]");
    reply.finish_reason = "canary-finish".to_owned();
    reply.usage = std::collections::BTreeMap::from([("canary-usage".to_owned(), 7)]);
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        call("image_generate", json!({"prompt": PROMPT, "size": "64x48"})),
        again,
        reply,
    ]);
    let diagnostic = Arc::new(Mutex::new(String::new()));
    let judged = Arc::new(Mutex::new(String::new()));
    let events = Arc::new(Mutex::new(String::new()));
    let sink = events.clone();
    let hooks = CompositeHook::new(vec![
        Arc::new(Diagnostic(diagnostic.clone())),
        Arc::new(Policy(judged.clone())),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?
        .with_agent_hook(Arc::new(hooks))
        .with_tool_event_callback(Arc::new(move |event| {
            sink.lock().unwrap().push_str(&format!("{event:?}\n"));
        }));
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(judged.lock().unwrap().contains(PROMPT));
    assert!(
        answer
            .contains("Parts (each checked on its own): report — source-linked; image — generated"),
        "{answer}"
    );
    let diagnostic = diagnostic.lock().unwrap().clone();
    let events = events.lock().unwrap().clone();
    for seen in [&diagnostic, &events] {
        assert!(seen.contains("image_generate"), "{seen}");
        assert!(seen.contains("<withheld: "), "{seen}");
        assert!(
            !seen.contains("blue") && !seen.contains("red chart") && !seen.contains("canary"),
            "{seen}"
        );
    }
    // Paths and call ids are the model's words too.
    assert!(!events.contains("notes.md"), "{events}");
    assert!(!events.contains("Some(\"i2\")"), "{events}");
    Ok(())
}
