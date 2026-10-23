use super::calculation::{add_exec, exec, printed};
use super::composed::REPORT_AND_IMAGE;
use super::report::{read, report_tools};
use super::*;

fn write(path: &str, content: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            "w1",
            "write_file",
            serde_json::Map::from_iter([
                ("path".to_owned(), json!(path)),
                ("content".to_owned(), json!(content)),
            ]),
        )],
        ..LlmResponse::default()
    }
}

#[test]
fn with_an_image_part_a_file_written_during_the_request_is_not_quoted() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        write("art.md", "The artwork is blue."),
        read("r1", "art.md"),
        read("r2", "notes.md"),
        text("The artwork is blue. [source: art.md]\nRevenue grew 10%. [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains(
            "\n[runtime: line withheld — not a word-for-word quote of a file read in this request]\nRevenue grew 10%. [source #2]"
        ),
        "{answer}"
    );
    for shown in [&answer, &status] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}

#[test]
fn a_quoted_line_is_shown_with_only_its_quote_and_sources() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("- \"Revenue grew 10%.\" [source: notes.md] [unresolved: the artwork is blue]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("\n- \"Revenue grew 10%.\" [source #1]\n"),
        "{answer}"
    );
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &status, &history] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}

#[test]
fn with_an_image_part_the_calculation_line_is_the_runtimes_own() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "The room is 3 m by 200 cm.")?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed("6.0", "check")],
    );
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        text("The room is 3 m by 200 cm. [source: notes.md]\nThe floor area is 600 m^2 [calculation]\nThe picture shows it"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(
        "Write a report with sources on the room, calculate the floor area in m^2 and generate an image of the room",
    ))?;
    agent.process_message(inbound("approve"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains("Method: composed [calculation + report + image]"),
        "{answer}"
    );
    assert!(
        answer.contains("\nThe room is 3 m by 200 cm. [source #1]\n[Calculation: 6 m^2 — verified by a separate check run]\n[runtime: line withheld"),
        "{answer}"
    );
    assert!(
        !answer.contains("600") && !answer.contains("picture"),
        "{answer}"
    );
    Ok(())
}

#[test]
fn with_an_image_part_a_line_of_marks_or_a_cut_number_shows_none_of_its_words(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 100%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew 100%. [source: notes.md]\n[unresolved: the artwork is blue]\nRevenue grew 10 [source: notes.md]\n---"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("\nRevenue grew 100%. [source #1]\n[runtime: line withheld — not a word-for-word quote of a file read in this request]\n---\n"),
        "{answer}"
    );
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &status, &history] {
        assert!(
            !shown.contains("artwork") && !shown.contains("grew 10 "),
            "{shown}"
        );
    }
    Ok(())
}
