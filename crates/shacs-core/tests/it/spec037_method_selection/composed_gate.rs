use super::composed::REPORT_AND_IMAGE;
use super::image_support::{add_image, generate, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

fn notes(dir: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(dir.join("notes.md"), "Revenue grew 10%.")?;
    Ok(())
}

#[test]
fn with_an_image_part_only_lines_quoting_a_read_file_are_shown() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    notes(dir.path())?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("# Quarterly notes\nRevenue grew 10%. [source: notes.md]\nThe chart is attached below.\n## Blue chart notes\nThe chart is not included [unresolved: image generation is unavailable]\nThe blue chart matches the notes [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    let withheld =
        "[runtime: line withheld — not a word-for-word quote of a file read in this request]";
    assert!(
        answer.contains(&format!(
            "\n{withheld}\nRevenue grew 10%. [source #1]\n{withheld}"
        )),
        "{answer}"
    );
    assert_eq!(answer.matches(withheld).count(), 2, "{answer}");
    assert!(answer.contains("the report shows only lines that quote a file read in this request"));
    assert!(answer.contains(
        "Lines without a source marker (not source-checked): 1 (withheld from the answer)"
    ));
    assert!(
        status.contains("- (withheld) -> notes.md (read in this request)"),
        "{status}"
    );
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &status, &history] {
        for words in ["attached below", "Blue chart", "blue chart", "not included"] {
            assert!(!shown.contains(words), "{words}: {shown}");
        }
    }
    Ok(())
}

#[test]
fn a_denied_image_call_in_a_composed_request_withdraws_the_earlier_image(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    notes(dir.path())?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        generate("64x48"),
        generate("32x32"),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    agent.process_message(inbound("approve"))?;
    // When
    let denied = output(agent.process_message(inbound("deny"))?);
    // Then
    assert!(denied.contains("image — not generated"), "{denied}");
    assert!(denied.contains("(denied)"), "{denied}");
    Ok(())
}

#[test]
fn a_report_part_dropped_and_added_again_does_not_revive_old_sources() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    notes(dir.path())?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        generate("64x48"),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    agent.process_message(inbound("/method use report"))?;
    agent.process_message(inbound("/method use image"))?;
    agent.process_message(inbound("/method auto"))?;
    // When
    let answer = output(agent.process_message(inbound("continue"))?);
    // Then
    assert!(
        answer.contains("Method: composed [report + image] (auto, revision 4)"),
        "{answer}"
    );
    assert!(!answer.contains("report — source-linked"), "{answer}");
    assert!(answer.contains("report — no sources read"), "{answer}");
    let status = output(agent.process_message(inbound("/method status"))?);
    assert!(status.contains("-> notes.md (never read)"), "{status}");
    Ok(())
}

#[test]
fn accepting_a_composed_request_records_every_parts_state() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    notes(dir.path())?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    let accepted = output(agent.process_message(inbound("/method accept"))?);
    // Then
    assert!(
        accepted
            .contains("Verification is unchanged: report — source-linked; image — unavailable."),
        "{accepted}"
    );
    let acceptance = &methods(&agent)["closed"][0]["user_acceptance"]["verification"];
    assert_eq!(acceptance, "report — source-linked; image — unavailable");
    Ok(())
}
