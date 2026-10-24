use super::composed::REPORT_AND_IMAGE;
use super::image_stream::Streaming;
use super::image_support::{add_image, generate, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

#[test]
fn changing_a_composed_request_keeps_the_goal_and_rechecks_what_was_read(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        generate("64x48"),
        text("Revenue grew [source: notes.md]"),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    let pending = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    // When
    let changed = output(agent.process_message(inbound("/method use report"))?);
    let answer = output(agent.process_message(inbound("continue"))?);
    // Then
    assert!(
        pending.contains("Permission approval required"),
        "{pending}"
    );
    assert!(
        changed.contains("Method changed. Cancelled pending approval (method_changed)"),
        "{changed}"
    );
    assert!(answer.starts_with("Revenue grew [source: notes.md]"));
    assert!(
        answer.contains("Method: source_grounded_report (explicit, revision 2)"),
        "{answer}"
    );
    assert!(answer.contains("Report: source-linked"));
    assert!(answer.contains("Sources read: notes.md (read before the method change)"));
    assert!(answer.contains("Image evidence from revision 1: superseded_not_inherited"));
    let request = &methods(&agent)["closed"][0];
    assert_eq!(request["goal_preview"], REPORT_AND_IMAGE);
    let earlier = &request["superseded_selections"][0];
    assert_eq!(earlier["method"], "composed");
    assert!(earlier["validation_claims"]
        .as_array()
        .is_some_and(|claims| claims
            .iter()
            .all(|claim| claim["state"] == "superseded_not_inherited")));
    assert_eq!(
        request["selection"]["validation_claims"][1]["state"],
        "verified"
    );
    Ok(())
}

#[test]
fn a_composed_reply_is_not_streamed_before_its_parts_are_checked() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let provider = Streaming(Provider::new(vec![
        read("r1", "notes.md"),
        text("Revenue grew [source: notes.md]\nHere is the chart."),
    ]));
    let streamed = Arc::new(Mutex::new(String::new()));
    let sink = streamed.clone();
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        AgentLoopConfig::new(dir.path(), "test-model"),
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let ProviderEvent::TextDelta { text } = event {
            sink.lock().unwrap().push_str(text);
        }
    }));
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    // Then
    assert!(
        answer.starts_with("Not every part of this request is confirmed"),
        "{answer}"
    );
    assert!(answer.contains(
        "\n[runtime: line withheld — not a word-for-word quote of a file read in this request]"
    ));
    assert!(!answer.contains("Here is the chart"));
    assert!(streamed.lock().unwrap().is_empty());
    let history = saved(&agent)["messages"].to_string();
    assert!(history.contains("Not every part of this request is confirmed"));
    Ok(())
}

#[test]
fn a_long_review_style_composed_request_is_not_sent_to_a_workflow() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let tools = report_tools(dir.path());
    let reply = || text("Revenue grew 10%. [source: notes.md]");
    let provider = Provider::new(vec![read("r1", "notes.md"), reply(), reply(), reply()]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(
        "Write a report with sources about the notes and generate an image of the chart, then review both parts carefully",
    ))?);
    // Then
    assert!(
        answer.contains("Method: composed [report + image]"),
        "{answer}"
    );
    assert!(answer.contains("\nRevenue grew 10%. [source #1]\n"));
    assert!(!answer.contains("Workflow"), "{answer}");
    Ok(())
}
