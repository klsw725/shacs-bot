use super::calculation::{add_exec, exec, printed};
use super::composed::REPORT_AND_IMAGE;
use super::composed_quiet::Diagnostic;
use super::report::{read, report_tools};
use super::*;

/// Runs `ask` through `then` with a diagnostic hook and a progress callback:
/// what the user saw after the request, and what the diagnostics saw.
fn observed(
    tools: &ToolRegistry,
    responses: Vec<LlmResponse>,
    ask: &str,
    then: &[&str],
) -> Result<(Vec<String>, String), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let provider = Provider::new(responses);
    let seen = Arc::new(Mutex::new(String::new()));
    let sink = seen.clone();
    let mut agent = runtime(dir.path(), tools, &provider)?
        .with_agent_hook(Arc::new(Diagnostic(seen.clone())))
        .with_tool_event_callback(Arc::new(move |event| {
            sink.lock().unwrap().push_str(&format!("{event:?}\n"));
        }));
    agent.process_message(inbound(ask))?;
    let mut shown = Vec::new();
    for message in then {
        shown.push(output(agent.process_message(inbound(message))?));
    }
    let seen = seen.lock().unwrap().clone();
    Ok((shown, seen))
}

#[test]
fn with_an_image_part_paths_the_model_chose_show_as_numbers() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = report_tools(dir.path());
    let written = call(
        "write_file",
        json!({"path": "The artwork is red.md", "content": "x"}),
    );
    let responses = vec![read("r1", "The artwork is blue.md"), written, text("Done.")];
    // When
    let (shown, seen) = observed(
        &tools,
        responses,
        REPORT_AND_IMAGE,
        &["approve", "/method history"],
    )?;
    // Then
    assert!(
        shown[0].contains(
            "Sources that could not be read: source #1 (names and errors in /method status)"
        ),
        "{}",
        shown[0]
    );
    assert!(
        shown[1].contains("Observed written file: <withheld: "),
        "{}",
        shown[1]
    );
    for shown in shown.iter().chain([&seen]) {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}

#[test]
fn with_an_image_part_a_calculation_run_shows_as_its_number() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[printed("6", "primary"), printed("6.0", "check")],
    );
    let responses = vec![
        exec("The-artwork-is-blue", "python3 -c 'print(3*2)'"),
        exec("The-artwork-is-red", "python3 -c 'print(300*200/10000)'"),
        text("Done."),
    ];
    let ask = "Calculate the floor area in m^2 and generate an image of the room";
    // When
    let (shown, seen) = observed(&tools, responses, ask, &["approve", "approve"])?;
    // Then
    assert!(
        shown[1].contains("Execution: primary #1 exit 0; check #2 exit 0"),
        "{}",
        shown[1]
    );
    for shown in [&shown[1], &seen] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}
