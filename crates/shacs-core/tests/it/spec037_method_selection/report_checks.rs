use super::report::{read, report_tools};
use super::*;

const ASK: &str = "Write a report with sources about the notes";

struct FailingFetch;
impl Tool for FailingFetch {
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
    fn execute(&self, params: JsonMap) -> ToolResult {
        let url = params.get("url").and_then(Value::as_str).unwrap_or("");
        ToolResult::Json(json!({"error": "HTTP status 404", "url": url}))
    }
}

#[test]
fn a_web_fetch_that_returns_an_error_is_a_failed_read() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    tools.register(FailingFetch);
    let url = "https://example.com/missing";
    let provider = Provider::new(vec![
        call("web_fetch", json!({ "url": url })),
        text(&format!("Prices rose sharply [source: {url}]")),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    let prompt = output(agent.process_message(inbound(ASK))?);
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(prompt.contains("Permission approval required"), "{prompt}");
    assert!(answer.contains("Report: unavailable"), "{answer}");
    assert!(answer.contains(&format!(
        "Sources that could not be read: {url} (HTTP status 404)"
    )));
    assert!(answer.contains(&format!(
        "Not backed: \"Prices rose sharply\" cites {url} (read failed)"
    )));
    let sources = &methods(&agent)["closed"][0]["report"]["sources"];
    assert_eq!(sources[0]["status"], "failed");
    assert!(sources[0]["content_digest"].is_null());
    Ok(())
}

#[test]
fn a_denied_web_fetch_is_listed_as_a_source_that_could_not_be_read() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    tools.register(FailingFetch);
    let url = "https://example.com/private";
    let provider = Provider::new(vec![call("web_fetch", json!({ "url": url }))]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    // When
    let denied = output(agent.process_message(inbound("deny"))?);
    // Then
    assert!(
        denied.contains(&format!(
            "Sources that could not be read: {url} (Permission denied by user.)"
        )),
        "{denied}"
    );
    let sources = &methods(&agent)["closed"][0]["report"]["sources"];
    assert_eq!(sources[0]["status"], "failed");
    Ok(())
}

#[test]
fn a_citation_must_name_a_source_as_it_was_read() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("docs"))?;
    std::fs::write(dir.path().join("docs/notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "docs/notes.md"),
        text("Revenue grew [source: imaginary/docs/notes.md]\nCosts rose [source: notes.md]\nMargins held [source: ./docs/notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(ASK))?);
    // Then
    assert!(
        answer.contains("Report: citation problems — 2 of 3 citations"),
        "{answer}"
    );
    assert!(
        answer.contains("Not backed: \"Revenue grew\" cites imaginary/docs/notes.md (never read)")
    );
    assert!(answer.contains("Not backed: \"Costs rose\" cites notes.md (never read)"));
    assert!(!answer.contains("\"Margins held\" cites"));
    Ok(())
}

#[test]
fn lines_without_a_source_marker_keep_a_report_from_being_source_linked(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Here is the summary of the notes:\nRevenue grew 10% [source: notes.md]\nCosts doubled over the last two years."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(ASK))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("Report: partly cited — 1 report line"),
        "{answer}"
    );
    assert!(!answer.contains("source-linked"));
    assert!(answer.contains(
        "Lines without a source marker (not source-checked): 1 — \"Costs doubled over the last two years.\""
    ));
    assert!(
        status.contains("- \"Costs doubled over the last two years.\" -> no source marker"),
        "{status}"
    );
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[1]["check"], "claim-source linkage");
    assert_eq!(claims[1]["state"], "not_verified");
    Ok(())
}

#[test]
fn a_revision_lists_conclusions_it_removed() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew [source: notes.md]\nCosts are flat [source: notes.md]"),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    agent.process_message(inbound("/method revise"))?;
    // When
    let revised = output(agent.process_message(inbound("Drop the sentence about costs"))?);
    // Then
    assert!(
        revised.contains(
            "Version 2 revising method-request-1: 0 new or changed conclusions, 1 earlier conclusions removed or reworded"
        ),
        "{revised}"
    );
    assert!(revised.contains("Removed or reworded: \"Costs are flat\""));
    assert_eq!(
        methods(&agent)["closed"][1]["report"]["verdict"]["removed_claims"][0],
        "Costs are flat"
    );
    Ok(())
}

#[test]
fn a_revision_compares_unmarked_lines_too() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew [source: notes.md]\nThe outlook looks stable overall."),
        text("Revenue grew [source: notes.md]\nCosts are expected to fall next year."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    agent.process_message(inbound("/method revise"))?;
    // When
    let revised = output(agent.process_message(inbound("Replace the outlook sentence"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(status.contains(
        "Removed or reworded since the earlier version:\n- \"The outlook looks stable overall.\""
    ));
    assert!(
        revised.contains("1 new or changed conclusions, 1 earlier conclusions removed or reworded"),
        "{revised}"
    );
    assert!(revised
        .contains("New or changed: \"Costs are expected to fall next year.\" (no source marker)"));
    assert!(revised.contains("Removed or reworded: \"The outlook looks stable overall.\""));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_cited_symlink_pointed_at_another_file_is_stale() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("a.md"), "Revenue grew 10%.")?;
    std::fs::write(dir.path().join("b.md"), "Revenue fell 5%.")?;
    std::os::unix::fs::symlink("a.md", dir.path().join("link.md"))?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "link.md"),
        text("Revenue grew [source: link.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    let answer = output(agent.process_message(inbound(ASK))?);
    std::fs::remove_file(dir.path().join("link.md"))?;
    std::os::unix::fs::symlink("b.md", dir.path().join("link.md"))?;
    // When
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(answer.contains("Report: source-linked"), "{answer}");
    assert!(
        status.contains("\"Revenue grew\" -> link.md (source changed after it was read)"),
        "{status}"
    );
    Ok(())
}
