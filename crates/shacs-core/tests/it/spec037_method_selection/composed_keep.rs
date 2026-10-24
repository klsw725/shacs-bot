use super::calculation::{add_exec, exec, printed};
use super::composed::{REPORT_AND_CALCULATION, REPORT_AND_IMAGE};
use super::image_hooks::Recorder;
use super::image_support::{add_image, generate, image_runtime, ok_images};
use super::report::{read, report_tools};
use super::*;

#[test]
fn a_calculation_kept_across_a_method_change_keeps_its_runs() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "The room is 3 m by 200 cm.")?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &[
            printed("6", "primary"),
            printed("6", "primary"),
            printed("6.0", "check"),
        ],
    );
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        exec("run-3", "python3 -c 'print(3*2)'"),
        exec("run-4", "python3 -c 'print(300*200/10000)'"),
        text("The floor area is 6 m^2."),
    ]);
    let seen = Arc::new(Mutex::new(String::new()));
    let mut agent =
        runtime(dir.path(), &tools, &provider)?.with_agent_hook(Arc::new(Recorder(seen.clone())));
    agent.process_message(inbound(REPORT_AND_CALCULATION))?;
    agent.process_message(inbound("approve"))?;
    // When
    let changed = output(agent.process_message(inbound("/method use calculation"))?);
    let ledger = saved(&agent)["metadata"]["runtime_execution"].to_string();
    agent.process_message(inbound("continue"))?;
    agent.process_message(inbound("approve"))?;
    let answer = output(agent.process_message(inbound("approve"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        changed.contains("Cancelled pending approval (method_changed)"),
        "{changed}"
    );
    assert!(
        answer.contains("Method: reproducible_calculation (explicit, revision 2)"),
        "{answer}"
    );
    assert!(answer.contains("Calculation: verified"), "{answer}");
    assert!(answer.contains("Verified answer: 6 m^2"));
    assert!(answer.contains("Execution: primary #3 run-3 exit 0; check #4 run-4 exit 0"));
    let context = format!("{:?}", provider.requests.lock().unwrap().last());
    assert!(context.contains("The room is 3 m by 200 cm."), "{context}");
    let history = saved(&agent)["messages"].to_string();
    assert!(!history.contains("The room is 3 m by 200 cm."), "{history}");
    let hooks = seen.lock().unwrap().clone();
    assert!(!hooks.contains("The room is 3 m by 200 cm."), "{hooks}");
    for earlier in ["r1", "run-1", "run-2"] {
        let id = format!("String(\"{earlier}\")");
        assert!(!hooks.contains(&id), "{earlier}: {hooks}");
        for raw in [format!(":{earlier}\""), format!("\"{earlier}\"")] {
            assert!(!ledger.contains(&raw), "{ledger}");
        }
    }
    assert!(ledger.contains("sha256:"), "{ledger}");
    assert!(
        hooks.contains("String(\"run-3\")"),
        "this run's own calls stay"
    );
    for kept in [
        "#1 run-1 [primary] exit 0",
        "#2 run-2 [no result line] cancelled",
    ] {
        assert!(
            status.contains(&format!(
                "{kept} [revision 1; kept across the method change and judged again]"
            )),
            "{status}"
        );
    }
    Ok(())
}

#[test]
fn an_image_kept_across_a_method_change_stays_the_result() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    let media = dir.path().join("media");
    let mut tools = report_tools(dir.path());
    add_image(&mut tools, &media, ok_images());
    let provider = Provider::new(vec![
        read("r1", "notes.md"),
        generate("64x48"),
        call(
            "write_file",
            json!({"path": "report.md", "content": "Revenue grew [source: notes.md]"}),
        ),
        text("Here you go."),
    ]);
    let mut agent = image_runtime(dir.path(), vec![media], &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    agent.process_message(inbound("approve"))?;
    // When
    let changed = output(agent.process_message(inbound("/method use image"))?);
    let answer = output(agent.process_message(inbound("continue"))?);
    // Then
    assert!(
        changed.contains("Cancelled pending approval (method_changed)"),
        "{changed}"
    );
    assert!(
        answer.starts_with("Image saved and checked on disk (see below)."),
        "{answer}"
    );
    assert!(answer.contains("Method: image_generation_review (explicit, revision 2)"));
    assert!(answer.contains("Images: media/image-generation/img-"));
    assert!(!answer.contains("Here you go."));
    Ok(())
}
