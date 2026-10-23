use super::report::{read, report_tools};
use super::*;

const ASK: &str = "Write a report with sources about the notes";

#[test]
fn revising_a_report_rechecks_carried_sources_instead_of_inheriting() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew [source: notes.md]"),
        text("Revenue grew [source: notes.md]\nCosts are flat [source: notes.md]"),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    // When
    let pending = output(agent.process_message(inbound("/method revise"))?);
    let unchanged = output(agent.process_message(inbound("Add a sentence about costs"))?);
    std::fs::write(dir.path().join("notes.md"), "Revenue fell 5%.")?;
    agent.process_message(inbound("/method revise"))?;
    let changed = output(agent.process_message(inbound("Tighten the wording"))?);
    // Then
    assert!(pending.contains("revises the report from method-request-1 as version 2"));
    assert!(unchanged.contains("Report: source-linked"), "{unchanged}");
    assert!(unchanged.contains("Version 2 revising method-request-1: 1 new or changed conclusions"));
    assert!(
        unchanged.contains("New or changed: \"Costs are flat\" (linked)"),
        "{unchanged}"
    );
    assert!(unchanged.contains("Sources read: notes.md (from version 1)"));
    let context = provider.requests.lock().unwrap()[2].messages[0].to_string();
    assert!(
        context.contains("version 2 revising the report from method-request-1"),
        "{context}"
    );
    assert!(context.contains(&format!("Original request: {ASK}")));
    assert!(changed.contains("Report: citation problems"), "{changed}");
    assert!(changed.contains("cites notes.md (source changed after it was read)"));
    let state = methods(&agent);
    assert_eq!(state["closed"][1]["selection"]["reason"], "report_revision");
    assert_eq!(state["closed"][2]["report"]["version"], 3);
    Ok(())
}

#[test]
fn a_report_written_to_a_file_is_checked_from_its_content() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let write = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "w1",
            "write_file",
            serde_json::Map::from_iter([
                ("path".to_owned(), json!("report.md")),
                (
                    "content".to_owned(),
                    json!("# Report\nRevenue grew [source: notes.md]\nCosts rose [source: costs.xlsx]"),
                ),
            ]),
        )],
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        write,
        text("Saved the report to report.md."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains("Report: citation problems — 1 of 2 citations"),
        "{answer}"
    );
    assert!(answer.contains("Not backed: \"Costs rose\" cites costs.xlsx (never read)"));
    let outputs = &methods(&agent)["closed"][0]["report"]["outputs"];
    assert!(outputs
        .as_array()
        .is_some_and(|outputs| outputs.iter().any(|output| output["kind"] == "file")));
    Ok(())
}

#[test]
fn revise_needs_a_finished_report() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let none = output(agent.process_message(inbound("/method revise"))?);
    // Then
    assert!(none.contains("No completed report or image request to revise."));
    assert!(methods(&agent)["pending_revision"].is_null());
    Ok(())
}

#[test]
fn an_approval_prompt_is_not_parsed_as_report_text() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let write = LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "w1",
            "write_file",
            serde_json::Map::from_iter([
                ("path".to_owned(), json!("report.md")),
                (
                    "content".to_owned(),
                    json!("Costs rose [source: costs.xlsx]"),
                ),
            ]),
        )],
        ..LlmResponse::default()
    };
    let provider = Provider::new(vec![read("r1", "notes.md"), write]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let prompt = output(agent.process_message(inbound(ASK))?);
    // Then
    assert!(prompt.contains("Permission approval required"), "{prompt}");
    assert!(prompt.contains("Report: not written"), "{prompt}");
    assert!(!prompt.contains("Not backed"));
    Ok(())
}
