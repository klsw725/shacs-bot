use super::composed::{REPORT_AND_CALCULATION, REPORT_AND_IMAGE};
use super::image_support::{add_image, image_runtime, ok_images};
use super::report::read;
use super::report_terminal::ViolatingLookup;
use super::*;

/// A reader whose error, taken from its output, holds the model's words (as
/// a file the model wrote could).
struct WordyReader;
impl Tool for WordyReader {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Fail with an error that quotes the model"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        ToolResult::Text("Error: The artwork is blue".to_owned())
    }
}

/// A policy that blocks image calls with a reason quoting the prompt.
struct Quoting;
impl AgentHook for Quoting {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        calls
            .iter()
            .filter(|call| call.name == "image_generate")
            .map(|call| RuntimeToolMessage {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: format!("Error: blocked by policy: {}", call.arguments["prompt"]),
            })
            .collect()
    }
}

#[test]
fn with_an_image_part_a_read_error_from_a_tool_output_stays_in_status() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = ToolRegistry::new();
    tools.register(WordyReader);
    let provider = Provider::new(vec![read("r1", "notes.md"), text("Nothing to report.")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    // Then
    assert!(
        answer.contains(
            "Sources that could not be read: source #1 (names and errors in /method status)"
        ),
        "{answer}"
    );
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &history] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}

#[test]
fn with_an_image_part_an_image_error_quoting_the_prompt_stays_in_status(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let mut tools = super::report::report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let provider = Provider::new(vec![
        call(
            "image_generate",
            json!({"prompt": "a blue chart of the room", "size": "64x48"}),
        ),
        text("Done."),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?
        .with_agent_hook(Arc::new(Quoting));
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    // Then
    assert!(
        answer.contains("the error is in /method status"),
        "{answer}"
    );
    assert!(!answer.contains("blue chart"), "{answer}");
    // The call itself (with its prompt) stays in the history for the provider.
    let saved = saved(&agent);
    let outputs: Vec<&Value> = saved["messages"]
        .as_array()
        .ok_or("messages")?
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect();
    assert!(!outputs.is_empty());
    assert!(
        !format!("{outputs:?}").contains("blue chart"),
        "{outputs:?}"
    );
    Ok(())
}

#[test]
fn a_tool_error_that_stops_the_turn_shows_only_its_digest() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let mut tools = super::report::report_tools(dir.path());
    tools.register(ViolatingLookup);
    let provider = Provider::new(vec![read("r1", "notes.md"), call("lookup", json!({}))]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_CALCULATION))?);
    // Then
    assert!(
        answer
            .contains("A tool call stopped this turn; its output is withheld (Error: <withheld: "),
        "{answer}"
    );
    let session = saved(&agent).to_string();
    for shown in [&answer, &session] {
        assert!(
            !shown.contains("outside the configured workspace"),
            "{shown}"
        );
    }
    Ok(())
}
