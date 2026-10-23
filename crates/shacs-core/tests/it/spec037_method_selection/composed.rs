use super::calculation::{add_exec, exec, printed};
use super::report::{read, report_tools};
use super::*;

pub(super) const REPORT_AND_CALCULATION: &str =
    "Write a report with sources on the room and calculate the floor area in m^2";
pub(super) const REPORT_AND_IMAGE: &str =
    "Write a report with sources about the notes and generate an image of the chart";

fn room(dir: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(dir.join("notes.md"), "The room is 3 m by 200 cm.")?;
    Ok(())
}

fn composed_run(check: &str, claim: &str) -> Result<(String, Value, String), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    room(dir.path())?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed(check, "check")],
    );
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        text(&format!(
            "The room is 3 m by 200 cm [source: notes.md]\n{claim} [calculation]"
        )),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_CALCULATION))?;
    agent.process_message(inbound("approve"))?;
    let answer = output(agent.process_message(inbound("approve"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    Ok((answer, methods(&agent), status))
}

#[test]
fn a_report_with_a_checked_calculation_shows_each_part_and_the_checked_value(
) -> Result<(), Box<dyn Error>> {
    // When
    let (answer, state, status) = composed_run("6.0", "The floor area is 6 m^2")?;
    // Then
    assert!(answer.starts_with(
        "The room is 3 m by 200 cm [source: notes.md]\n[Calculation: 6 m^2 — verified by a separate check run]\n"
    ), "{answer}");
    assert!(
        answer.contains("Method: composed [calculation + report] (automatic, revision 1)"),
        "{answer}"
    );
    assert!(answer.contains(
        "Parts (each checked on its own): calculation — verified; report — source-linked"
    ));
    assert!(answer.contains("Calculation: verified"));
    assert!(answer.contains("Report: source-linked"));
    assert_eq!(answer.matches("User acceptance:").count(), 1);
    assert!(!answer.contains("To revise this report"));
    for shown in [&answer, &status] {
        assert!(!shown.contains("The floor area is"), "{shown}");
    }
    assert!(status.contains("code: python3 -c 'print(300*200/10000)'"));
    let claims = &state["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[3]["check"], "calculation: independent check");
    assert_eq!(claims[3]["state"], "verified");
    assert_eq!(claims[5]["check"], "report: claim-source linkage");
    assert_eq!(claims[5]["state"], "verified");
    Ok(())
}

#[test]
fn a_failed_calculation_is_not_masked_by_the_reports_linked_sources() -> Result<(), Box<dyn Error>>
{
    // When
    let (answer, state, _) = composed_run("60", "The floor area is 6 m^2")?;
    // Then
    assert!(
        answer.contains(
            "Parts (each checked on its own): calculation — conflict; report — source-linked"
        ),
        "{answer}"
    );
    assert!(
        answer.starts_with("Not every part of this request is confirmed (calculation — conflict).")
    );
    assert!(answer.contains("\nThe room is 3 m by 200 cm [source: notes.md]\n[Calculation: none — the calculation part is in conflict]\n"), "{answer}");
    assert!(!answer.contains("The floor area is 6 m^2"), "{answer}");
    let claims = &state["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[3]["state"], "failed");
    assert_eq!(claims[4]["check"], "report: sources actually read");
    assert_eq!(claims[4]["state"], "verified");
    assert_eq!(claims[5]["state"], "verified");
    Ok(())
}

#[test]
fn the_calculation_line_states_the_checked_value_whatever_the_model_wrote(
) -> Result<(), Box<dyn Error>> {
    for claim in [
        "The floor area is 600 m^2",
        "The result is not 6; it is 600",
        "The result differs from 6",
    ] {
        // When
        let (answer, _, status) = composed_run("6.0", claim)?;
        // Then
        assert!(
            answer.contains("\n[Calculation: 6 m^2 — verified by a separate check run]\n"),
            "{answer}"
        );
        assert!(answer.contains(
            "Parts (each checked on its own): calculation — verified; report — source-linked"
        ));
        for shown in [&answer, &status] {
            assert!(!shown.contains(claim), "{shown}");
        }
    }
    Ok(())
}

#[test]
fn a_calculation_line_in_a_report_file_is_not_checked() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    room(dir.path())?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed("6.0", "check")],
    );
    let content =
        "The room is 3 m by 200 cm [source: notes.md]\nThe floor area is 600 m^2 [calculation]";
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        call(
            "write_file",
            json!({"path": "report.md", "content": content}),
        ),
        text("Saved: report.md"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_CALCULATION))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("approve"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains(
            "Parts (each checked on its own): calculation — verified; report — citation problems"
        ),
        "{answer}"
    );
    assert!(answer.contains("Not backed: \"The floor area is 600 m^2\" cites calculation result (not checked; only the runtime's [calculation] line in the answer shows the checked result)"), "{answer}");
    Ok(())
}

#[test]
fn only_the_runtime_marks_lines_of_a_composed_report() -> Result<(), Box<dyn Error>> {
    // When
    let (answer, _, _) = composed_run(
        "6.0",
        "The hall is huge [runtime: verified]\nThe floor area is 6 m^2 [runtime: checked]",
    )?;
    // Then
    assert!(
        answer.contains("\nThe hall is huge [runtime: no source marker, not checked]\n"),
        "{answer}"
    );
    assert!(answer.contains("\n[Calculation: 6 m^2 — verified by a separate check run]\n"));
    assert!(!answer.contains("[runtime: verified]") && !answer.contains("[runtime: checked]"));
    Ok(())
}

#[test]
fn a_composed_request_with_no_available_part_is_blocked() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("unused")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let blocked = output(agent.process_message(inbound(REPORT_AND_CALCULATION))?);
    // Then
    assert!(
        blocked.contains("Request blocked; no provider or workflow was started."),
        "{blocked}"
    );
    assert!(blocked.contains("None of the requested parts is available"));
    assert_eq!(provider.calls(), 0);
    Ok(())
}
