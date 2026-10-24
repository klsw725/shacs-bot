use super::*;

struct DiskReader(std::path::PathBuf);
impl Tool for DiskReader {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read a workspace file"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]})
    }
    fn read_only(&self) -> bool {
        true
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params.get("path").and_then(Value::as_str).unwrap_or("");
        match std::fs::read_to_string(self.0.join(path)) {
            Ok(text) => ToolResult::Text(format!("1| {text}")),
            Err(_) => ToolResult::Text(format!("Error: File not found: {path}")),
        }
    }
}

struct DiskWriter(std::path::PathBuf);
impl Tool for DiskWriter {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write a workspace file"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]})
    }
    fn execute(&self, params: JsonMap) -> ToolResult {
        let path = params.get("path").and_then(Value::as_str).unwrap_or("");
        let content = params.get("content").and_then(Value::as_str).unwrap_or("");
        match std::fs::write(self.0.join(path), content) {
            Ok(()) => ToolResult::Text(format!("Wrote {} bytes to {path}", content.len())),
            Err(error) => ToolResult::Text(format!("Error: {error}")),
        }
    }
}

pub(super) fn report_tools(workspace: &Path) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    tools.register(DiskReader(workspace.to_path_buf()));
    tools.register(DiskWriter(workspace.to_path_buf()));
    tools
}
pub(super) fn read(id: &str, path: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            id,
            "read_file",
            serde_json::Map::from_iter([("path".to_owned(), json!(path))]),
        )],
        ..LlmResponse::default()
    }
}
const ASK: &str = "Write a report with sources about the notes";

#[test]
fn linked_report_shows_the_answer_first_and_sources_on_request() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew 10% [source: notes.md]\nThe trend is positive [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(ASK))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(answer.starts_with("Revenue grew 10% [source: notes.md]"));
    assert!(answer.contains("Report: source-linked"), "{answer}");
    assert!(answer.contains("Sources read: notes.md"));
    assert!(
        status.contains("Report sources:\n- #1 notes.md [Read]"),
        "{status}"
    );
    assert!(status.contains("\"Revenue grew 10%\" -> notes.md (read in this request)"));
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[1]["state"], "verified");
    Ok(())
}

#[test]
fn unread_failed_and_changed_sources_are_not_shown_as_backed() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        read("r2", "missing.md"),
        text("Revenue grew [source: notes.md]\nQ4 fell [source: missing.md]\nCosts rose [source: imaginary.pdf]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(ASK))?);
    std::fs::write(dir.path().join("notes.md"), "Revenue fell 5%.")?;
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("Report: citation problems — 2 of 3 citations"),
        "{answer}"
    );
    assert!(answer.contains("Not backed: \"Q4 fell\" cites missing.md (read failed)"));
    assert!(answer.contains("Not backed: \"Costs rose\" cites imaginary.pdf (never read)"));
    assert!(answer.contains("Sources that could not be read: missing.md"));
    assert!(
        status.contains(
            "Not backed: \"Revenue grew\" cites notes.md (source changed after it was read)"
        ),
        "{status}"
    );
    Ok(())
}

#[test]
fn conflicts_unresolved_points_and_uncited_lines_are_disclosed() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("a.md"), "Sales rose.")?;
    std::fs::write(dir.path().join("b.md"), "Sales fell.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "a.md"),
        read("r2", "b.md"),
        text("Sales direction is disputed [conflict: a.md; b.md] [source: a.md; b.md]\nQ4 is unknown [unresolved: no Q4 data]\nThis sentence carries no source marker at all."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(ASK))?);
    // Then
    assert!(answer.contains("Report: partly cited"), "{answer}");
    assert!(answer.contains("Open points: 1 conflicting-source conclusions, 1 unresolved"));
    assert!(answer.contains(
        "Lines without a source marker (not source-checked): 1 — \"This sentence carries no source marker at all.\""
    ));
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[2]["check"], "unresolved claims");
    assert_eq!(claims[2]["evidence_refs"].as_array().map(Vec::len), Some(2));
    Ok(())
}

#[test]
fn reports_without_reads_or_citations_are_not_source_grounded() -> Result<(), Box<dyn Error>> {
    for (responses, expected) in [
        (
            vec![text("Revenue grew 10% this year [source: notes.md]")],
            "Report: no sources read",
        ),
        (
            vec![
                read("r1", "notes.md"),
                text("Revenue grew 10% this year overall."),
            ],
            "Report: no citations",
        ),
    ] {
        // Given
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
        let tools = report_tools(dir.path());
        let provider = Provider::new(responses);
        let mut agent = runtime(dir.path(), &tools, &provider)?;
        // When
        let answer = output(agent.process_message(inbound(ASK))?);
        // Then
        assert!(answer.contains(expected), "{answer}");
    }
    Ok(())
}
