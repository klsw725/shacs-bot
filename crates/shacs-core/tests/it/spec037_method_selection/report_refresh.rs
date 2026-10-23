use super::report::{read, report_tools};
use super::*;

const ASK: &str = "Write a report with sources about the notes";

#[test]
fn history_and_accept_recheck_sources_before_showing_a_verdict() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    let answer = output(agent.process_message(inbound(ASK))?);
    std::fs::write(dir.path().join("notes.md"), "Revenue fell 5%.")?;
    // When
    let history = output(agent.process_message(inbound("/method history"))?);
    let accepted = output(agent.process_message(inbound("/method accept"))?);
    // Then
    assert!(answer.contains("Report: source-linked"), "{answer}");
    assert!(history.contains("Report: citation problems"), "{history}");
    assert!(history.contains("cites notes.md (source changed after it was read)"));
    assert!(
        accepted.contains("Verification is unchanged: citation problems"),
        "{accepted}"
    );
    let acceptance = &methods(&agent)["closed"][0]["user_acceptance"];
    assert_eq!(acceptance["verification"], "citation problems");
    Ok(())
}

#[test]
fn sources_read_under_an_earlier_revision_are_not_listed_as_read_now() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        call("ask_user", json!({"question": "Which quarter?"})),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(ASK))?;
    agent.process_message(inbound("/method use general"))?;
    agent.process_message(inbound("/method use report"))?;
    // When
    let answer = output(agent.process_message(inbound("continue"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("Method: source_grounded_report (explicit, revision 3)"),
        "{answer}"
    );
    assert!(answer.contains("Report: no sources read"), "{answer}");
    assert!(!answer.contains("Sources read: notes.md"));
    assert!(
        status.contains("- #1 notes.md [Read] revision 1; superseded_not_inherited"),
        "{status}"
    );
    Ok(())
}
