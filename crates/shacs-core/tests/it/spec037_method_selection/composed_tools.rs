use super::calculation::{add_exec, exec, printed};
use super::composed::{REPORT_AND_CALCULATION, REPORT_AND_IMAGE};
use super::image_checkpoint::SessionFileSpy;
use super::image_hooks::Recorder;
use super::image_support::{add_image, generate, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

#[test]
fn a_composed_request_offers_no_tool_that_speaks_for_the_model() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "The room is 3 m by 200 cm.")?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed("6.0", "check")],
    );
    let counter = Arc::new(AtomicUsize::new(0));
    for name in ["message", "spawn", "propose_entrusted_work"] {
        tools.register(RegisteredTool {
            name,
            counter: counter.clone(),
            read_only: false,
        });
    }
    let provider = Provider::new(vec![
        call(
            "ask_user",
            json!({"question": "The area is 600 m^2, right?"}),
        ),
        call("message", json!({"content": "The area is 600 m^2."})),
        read("r1", "notes.md"),
        text("The room is 3 m by 200 cm [source: notes.md]"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = output(agent.process_message(inbound(REPORT_AND_CALCULATION))?);
    // Then
    assert!(
        answer.contains("Method: composed [calculation + report]"),
        "{answer}"
    );
    assert!(answer.contains("\nThe room is 3 m by 200 cm [source: notes.md]\n"));
    assert!(!answer.contains("600 m^2"), "{answer}");
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    let offered: Vec<String> = provider.requests.lock().unwrap()[0]
        .tools
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect();
    for name in ["ask_user", "message", "spawn", "propose_entrusted_work"] {
        assert!(!offered.iter().any(|tool| tool == name), "{offered:?}");
    }
    assert!(offered.iter().any(|tool| tool == "read_file"));
    assert!(offered.iter().any(|tool| tool == "exec"));
    Ok(())
}

#[test]
fn runtime_hooks_get_a_quiet_view_of_a_composed_request_with_an_image_part(
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
        text("Revenue grew [source: notes.md]\nThe blue chart shows the growth [image]"),
    ]);
    let seen = Arc::new(Mutex::new(String::new()));
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?
        .with_agent_hook(Arc::new(Recorder(seen.clone())));
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains("[Image: media/image-generation/img-"),
        "{answer}"
    );
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty());
    let leak = seen
        .find("blue chart")
        .map(|at| &seen[at.saturating_sub(120)..at + 10]);
    assert_eq!(leak, None);
    Ok(())
}

#[test]
fn a_crash_mid_composed_turn_with_an_image_part_leaves_no_model_words() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let mut call = generate("64x48");
    call.content = Some("Here is the blue chart!".to_owned());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        call,
        text("Revenue grew [source: notes.md]\nThe blue chart shows the growth [image]"),
    ]);
    let seen = Arc::new(Mutex::new(String::new()));
    let spy = SessionFileSpy {
        sessions: dir.path().join("sessions"),
        seen: seen.clone(),
    };
    let mut agent =
        image_runtime(dir.path(), vec![media], &tools, &provider)?.with_agent_hook(Arc::new(spy));
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    agent.process_message(inbound("approve"))?;
    // Then
    let seen = seen.lock().unwrap();
    assert!(seen.contains("runtime_checkpoint"));
    let leak = seen
        .find("blue chart")
        .map(|at| &seen[at.saturating_sub(160)..at + 10]);
    assert_eq!(leak, None);
    Ok(())
}

#[test]
fn a_composed_report_keeps_the_models_raw_words_out_of_hooks_and_history(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "The room is 3 m by 200 cm.")?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed("6.0", "check")],
    );
    let mut first = read("r1", "notes.md");
    first.content = Some("The area is surely 600 m^2.".to_owned());
    let provider = Provider::new(vec![
        first,
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        text("The room is 3 m by 200 cm [source: notes.md]\nThe floor is huge"),
    ]);
    let seen = Arc::new(Mutex::new(String::new()));
    let mut agent =
        runtime(dir.path(), &tools, &provider)?.with_agent_hook(Arc::new(Recorder(seen.clone())));
    agent.process_message(inbound(REPORT_AND_CALCULATION))?;
    agent.process_message(inbound("approve"))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains("\nThe floor is huge [runtime: no source marker, not checked]\n"),
        "{answer}"
    );
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("600 m^2"), "{history}");
    assert!(history.contains("The floor is huge [runtime: no source marker, not checked]"));
    let seen = seen.lock().unwrap();
    assert!(!seen.is_empty());
    for words in ["600 m^2", "The floor is huge"] {
        let leak = seen
            .find(words)
            .map(|at| &seen[at.saturating_sub(120)..at + words.len()]);
        assert_eq!(leak, None);
    }
    Ok(())
}
