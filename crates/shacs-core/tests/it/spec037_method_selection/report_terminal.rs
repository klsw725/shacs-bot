use super::report::{read, report_tools};
use super::*;

const ASK: &str = "Write a report with sources about the notes";

pub(super) struct ViolatingLookup;
impl Tool for ViolatingLookup {
    fn name(&self) -> &str {
        "lookup"
    }
    fn description(&self) -> &str {
        "Return a workspace violation that echoes a citation"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        "Error: path is outside the configured workspace [source: notes.md]".into()
    }
}

#[test]
fn runtime_failure_text_is_never_checked_as_report_text() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let mut tools = report_tools(dir.path());
    tools.register(ViolatingLookup);
    let provider = Provider::new(vec![read("r1", "notes.md"), call("lookup", json!({}))]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(ASK))?);
    // Then
    assert!(
        answer.contains("outside the configured workspace"),
        "{answer}"
    );
    assert!(answer.contains("Report: not written"), "{answer}");
    assert!(!answer.contains("source-linked"));
    assert_eq!(methods(&agent)["closed"][0]["state"], "failed");
    Ok(())
}

struct PageFetch;
impl Tool for PageFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }
    fn description(&self) -> &str {
        "Fetch a web page"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        ToolResult::Json(json!({"url": "https://example.com/a", "status": 200, "text": "page"}))
    }
}

#[test]
fn a_read_skipped_while_another_call_waits_for_approval_keeps_its_locator(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let mut tools = report_tools(dir.path());
    tools.register(PageFetch);
    let batch = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![
            ToolCallRequest::new(
                "f1",
                "web_fetch",
                serde_json::Map::from_iter([("url".to_owned(), json!("https://example.com/a"))]),
            ),
            ToolCallRequest::new(
                "r1",
                "read_file",
                serde_json::Map::from_iter([("path".to_owned(), json!("notes.md"))]),
            ),
        ],
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![batch]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let prompt = output(agent.process_message(inbound(ASK))?);
    // Then
    assert!(prompt.contains("Permission approval required"), "{prompt}");
    assert!(
        prompt.contains("Sources that could not be read: notes.md ([Tool call skipped"),
        "{prompt}"
    );
    Ok(())
}
