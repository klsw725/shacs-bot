use super::calculation::{add_exec, exec, printed};
use super::composed::REPORT_AND_IMAGE;
use super::image_support::{add_image, generate, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

#[test]
fn an_unavailable_image_part_leaves_the_report_part_delivered_alone() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew 10%. [source: notes.md]\nThe chart is not included [unresolved: image generation is unavailable]\nThe chart shows the growth [image]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(answer.contains("unavailable parts: image"), "{answer}");
    assert!(
        answer.starts_with("Not every part of this request is confirmed (image — unavailable).")
    );
    assert!(answer
        .contains("Parts (each checked on its own): report — source-linked; image — unavailable"));
    assert!(answer.contains("\n[Image: none — the image part is unavailable]\n"));
    assert!(answer.contains(
        "Image: unavailable — image_generate is not registered, so this part is unavailable"
    ));
    assert!(answer.contains("Open points: 0 conflicting-source conclusions, 1 unresolved"));
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &status, &history] {
        assert!(!shown.contains("The chart shows the growth"), "{shown}");
    }
    assert!(history.contains("Not every part of this request is confirmed"));
    assert_eq!(provider.calls(), 2);
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[3]["check"], "image: generated artifact");
    assert_eq!(claims[3]["state"], "not_verified");
    assert_eq!(
        claims[3]["evidence_refs"][0],
        "unavailable: image_generate is not registered"
    );
    Ok(())
}

#[test]
fn a_saved_image_takes_the_place_of_the_reports_image_line() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let mut call = generate("64x48");
    call.content = Some("Here is your blue chart!".to_owned());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        call,
        text("Revenue grew 10%. [source: notes.md]\nThe blue chart shows the growth [image]"),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.starts_with("Revenue grew 10%. [source #1]\n[Image: media/image-generation/img-"),
        "{answer}"
    );
    assert!(answer.contains(" — saved and checked on disk; content and look not checked]"));
    assert!(
        answer
            .contains("Parts (each checked on its own): report — source-linked; image — generated"),
        "{answer}"
    );
    assert!(answer.contains("Image: generated — 1 image saved and checked on disk"));
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &status, &history] {
        assert!(!shown.contains("blue chart"), "{shown}");
    }
    Ok(())
}

#[test]
fn without_a_report_part_a_composed_reply_is_withheld_like_an_image_reply(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let media = dir.path().join("media");
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed("6.0", "check")],
    );
    add_image(&mut tools, &media, ok_images());
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        generate("64x48"),
        text("The area is 6 m^2 and the blue room picture is ready."),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(
        "Calculate the floor area in m^2 and generate an image of the room",
    ))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("approve"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.starts_with("Parts (each checked on its own): calculation — verified; image — generated. Image saved and checked on disk (see below)."),
        "{answer}"
    );
    assert!(answer.contains("The model's own reply is not shown"));
    assert!(answer.contains("Verified answer: 6 m^2"));
    let history = saved(&agent)["messages"].to_string();
    for shown in [&answer, &history] {
        assert!(!shown.contains("blue room"), "{shown}");
    }
    let closed = &methods(&agent)["closed"][0];
    assert_eq!(
        closed["image"]["withheld_reply"],
        "The area is 6 m^2 and the blue room picture is ready."
    );
    Ok(())
}
