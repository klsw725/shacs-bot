use super::calculation::{calculate, exec, exec_tools, printed};
use super::*;

struct OutsideReader;
impl Tool for OutsideReader {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Return a workspace violation without reading"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        ToolResult::Text("Error: path is outside the configured workspace".to_owned())
    }
}

#[test]
fn a_malformed_exec_after_a_verified_pair_is_recorded_and_withdraws_verification(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary"), printed("6", "check")]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(6)'"),
        call("exec", json!({"cmd": "python3 -c 'print(2*3)'"})),
        text("The floor area is 6 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = calculate(&mut agent, 2)?;
    // Then
    assert!(
        answer.contains("Calculation: not verified — a run after the latest primary result failed"),
        "{answer}"
    );
    let runs = &methods(&agent)["closed"][0]["calculation"]["runs"];
    assert_eq!(runs[2]["status"], "failed");
    assert_eq!(runs[2]["code"], "<exec call rejected before it ran>");
    Ok(())
}

#[test]
fn an_exec_skipped_after_a_fatal_tool_error_is_recorded_as_cancelled() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = exec_tools(&[printed("6", "primary"), printed("6", "check")]);
    tools.register(OutsideReader);
    let batch = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![
            ToolCallRequest::new(
                "read-1",
                "read_file",
                serde_json::Map::from_iter([("path".to_owned(), json!("../outside.txt"))]),
            ),
            ToolCallRequest::new(
                "run-4",
                "exec",
                serde_json::Map::from_iter([(
                    "command".to_owned(),
                    json!("python3 -c 'print(6)'"),
                )]),
            ),
        ],
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(6)'"),
        batch,
        text("Stopped."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    calculate(&mut agent, 2)?;
    // Then
    let request = &methods(&agent)["closed"][0];
    let runs = request["calculation"]["runs"].as_array().ok_or("runs")?;
    let skipped = runs
        .iter()
        .find(|run| run["call_id"] == "run-4")
        .ok_or("skipped run")?;
    assert_eq!(skipped["status"], "cancelled");
    assert_ne!(request["calculation"]["verdict"]["state"], "verified");
    Ok(())
}

#[test]
fn an_exec_skipped_while_another_waits_for_approval_is_recorded() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary"), printed("6", "check")]);
    let command = |id: &str, text: &str| {
        ToolCallRequest::new(
            id,
            "exec",
            serde_json::Map::from_iter([("command".to_owned(), json!(text))]),
        )
    };
    let batch = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![
            command("run-3", "python3 -c 'print(2*3)'"),
            command("run-4", "python3 -c 'print(6.0)'"),
        ],
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(6)'"),
        batch,
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let waiting = calculate(&mut agent, 2)?;
    // Then
    assert!(waiting.contains("Calculation: not verified"), "{waiting}");
    let runs = &methods(&agent)["active"]["calculation"]["runs"];
    let skipped = runs
        .as_array()
        .ok_or("runs")?
        .iter()
        .find(|run| run["call_id"] == "run-4")
        .ok_or("skipped run")?;
    assert_eq!(skipped["status"], "cancelled");
    Ok(())
}
