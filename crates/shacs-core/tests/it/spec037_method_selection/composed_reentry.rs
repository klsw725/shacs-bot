use super::calculation::{add_exec, exec};
use super::composed_output::without;
use super::composed_quiet::Diagnostic;
use super::image_checkpoint::SessionFileSpy;
use super::report::report_tools;
use super::*;

const CALCULATION_AND_IMAGE: &str =
    "Calculate the floor area in m^2 and generate an image of the room";

/// A subagent spawn (it asks for approval, as `spawn` runs a process).
struct Spawn;
impl Tool for Spawn {
    fn name(&self) -> &str {
        "spawn"
    }
    fn description(&self) -> &str {
        "Start a subagent"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"task":{"type":"string"}}})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        "Subagent [Inspect] started (id: child-00000042). I'll notify you when it completes.".into()
    }
}

#[test]
fn a_request_made_an_image_one_again_keeps_none_of_the_words_saved_in_between(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &["The artwork is green\n\nExit code: 0".to_owned()],
    );
    tools.register(Spawn);
    let spawn = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "The-artwork-is-yellow",
            "spawn",
            serde_json::Map::from_iter([("task".to_owned(), json!("inspect"))]),
        )],
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(1)'"),
        exec("run-2", "python3 -c 'print(2)'"),
        spawn,
        call("ask_user", json!({"question": "Is the artwork blue?"})),
        text("Done."),
    ]);
    let seen = Arc::new(Mutex::new(String::new()));
    let spied = Arc::new(Mutex::new(String::new()));
    let hooks = CompositeHook::new(vec![
        Arc::new(Diagnostic(seen.clone())),
        Arc::new(SessionFileSpy {
            sessions: dir.path().join("sessions"),
            seen: spied.clone(),
        }),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?.with_agent_hook(Arc::new(hooks));
    agent.process_message(inbound(CALCULATION_AND_IMAGE))?;
    agent.process_message(inbound("/method use calculation"))?;
    agent.process_message(inbound("continue"))?;
    // The calculation alone shows its run, spawns a subagent, and the model asks a question.
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("approve"))?;
    seen.lock().unwrap().clear();
    spied.lock().unwrap().clear();
    // When
    let changed = output(agent.process_message(inbound("/method auto"))?);
    let reentered = saved(&agent)["metadata"]["runtime_execution"].to_string();
    let answer = output(agent.process_message(inbound("continue"))?);
    let later = saved(&agent)["metadata"]["runtime_execution"].to_string();
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        changed.contains("Method: composed [calculation + image]"),
        "{changed}"
    );
    assert!(
        changed.contains("Remaining question: (the model's question is withheld)"),
        "{changed}"
    );
    let context = format!("{:?}", provider.requests.lock().unwrap().last());
    assert!(context.contains("The artwork is green"), "{context}");
    // The method record keeps each calculation run as /method status shows it.
    let records = |text: &str| without(text, "\"task_method_requests_v1\":");
    let saved = records(&saved(&agent).to_string());
    let spied = records(&without(
        &spied.lock().unwrap(),
        "\"task_method_held_results_v1\":",
    ));
    let seen = seen.lock().unwrap().clone();
    for shown in [&changed, &answer] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    // Status lists the calculation runs as recorded, outputs included.
    assert!(status.contains("Remaining question: (the model's question is withheld)"));
    for kept in [&saved, &spied, &seen] {
        assert!(!kept.contains("artwork is green"), "{kept}");
    }
    assert!(!seen.contains("artwork"), "{seen}");
    // The execution record names the calls made in between by digests.
    assert!(reentered.contains("sha256:"), "{reentered}");
    for record in [&reentered, &later] {
        assert!(!record.contains("artwork"), "{record}");
    }
    Ok(())
}

#[test]
fn outputs_sharing_a_call_id_all_go_back_to_the_model_after_re_entry() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[
            "first output\n\nExit code: 0".to_owned(),
            "second output\n\nExit code: 0".to_owned(),
        ],
    );
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(1)'"),
        exec("dup", "python3 -c 'print(2)'"),
        exec("dup", "python3 -c 'print(3)'"),
        call("ask_user", json!({"question": "Go on?"})),
        text("Done."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(CALCULATION_AND_IMAGE))?;
    agent.process_message(inbound("/method use calculation"))?;
    agent.process_message(inbound("continue"))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("approve"))?;
    // When
    agent.process_message(inbound("/method auto"))?;
    agent.process_message(inbound("continue"))?;
    // Then
    let context = format!("{:?}", provider.requests.lock().unwrap().last());
    assert!(context.contains("first output"), "{context}");
    assert!(context.contains("second output"), "{context}");
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("first output") && !history.contains("second output"));
    Ok(())
}
