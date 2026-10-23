use super::calculation::{add_exec, exec, printed};
use super::composed::{REPORT_AND_CALCULATION, REPORT_AND_IMAGE};
use super::image_support::{add_image, generate, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

fn notes(dir: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(dir.join("notes.md"), "Revenue grew 10%.")?;
    Ok(())
}

#[test]
fn with_an_image_part_a_cited_line_that_does_not_quote_its_source_is_withheld(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    notes(dir.path())?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew 10%. [source: notes.md]\nThe artwork is blue [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("\nRevenue grew 10%. [source #1]\n"),
        "{answer}"
    );
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &status, &history] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}

#[test]
fn a_report_part_kept_again_after_it_was_dropped_does_not_revive_old_sources(
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
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    agent.process_message(inbound("/method use image"))?;
    agent.process_message(inbound("/method auto"))?;
    agent.process_message(inbound("/method use report"))?;
    // When
    let answer = output(agent.process_message(inbound("continue"))?);
    // Then
    assert!(
        answer.contains("Method: source_grounded_report (explicit, revision 4)"),
        "{answer}"
    );
    assert!(!answer.contains("Report: source-linked"), "{answer}");
    Ok(())
}

#[test]
fn a_method_change_records_the_cancelled_image_call_of_a_kept_part() -> Result<(), Box<dyn Error>> {
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
        text("Done."),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("/method use image"))?;
    // When
    let answer = output(agent.process_message(inbound("continue"))?);
    // Then
    assert!(
        answer.starts_with("No new image was saved for this request"),
        "{answer}"
    );
    assert!(answer.contains("Earlier saved image in this request (not the current result)"));
    Ok(())
}

#[test]
fn a_method_change_records_the_cancelled_rerun_of_a_kept_calculation() -> Result<(), Box<dyn Error>>
{
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
        exec("run-3", "python3 -c 'print(3*2)'"),
        text("The floor area is 6 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_CALCULATION))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("/method use calculation"))?;
    // When
    let answer = output(agent.process_message(inbound("continue"))?);
    // Then
    assert!(!answer.contains("Calculation: verified"), "{answer}");
    assert!(
        answer.contains(
            "Calculation: not verified — a run after the latest primary result was cancelled"
        ),
        "{answer}"
    );
    Ok(())
}
