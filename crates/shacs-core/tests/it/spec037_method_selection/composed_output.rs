use super::calculation::{add_exec, exec};
use super::composed::REPORT_AND_IMAGE;
use super::composed_quiet::Diagnostic;
use super::image_checkpoint::SessionFileSpy;
use super::report::{read, report_tools};
use super::*;

#[test]
fn tool_outputs_reach_no_surface_but_the_models_own_context() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("notes.md"), "Revenue grew 10%.")?;
    std::fs::write(dir.path().join("art.md"), "The artwork is blue.")?;
    let mut tools = report_tools(dir.path());
    add_exec(
        &mut tools,
        &["The artwork is green\n\nExit code: 0".to_owned()],
    );
    let provider = Provider::new(vec![
        read("r1", "art.md"),
        read("r2", "notes.md"),
        exec("run-1", "python3 -c 'print(1)'"),
        text("Revenue grew 10%. [source: notes.md]"),
    ]);
    let diagnostic = Arc::new(Mutex::new(String::new()));
    let spied = Arc::new(Mutex::new(String::new()));
    let events = Arc::new(Mutex::new(String::new()));
    let sink = events.clone();
    let hooks = CompositeHook::new(vec![
        Arc::new(Diagnostic(diagnostic.clone())),
        Arc::new(SessionFileSpy {
            sessions: dir.path().join("sessions"),
            seen: spied.clone(),
        }),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?
        .with_agent_hook(Arc::new(hooks))
        .with_tool_event_callback(Arc::new(move |event| {
            sink.lock().unwrap().push_str(&format!("{event:?}\n"));
        }));
    // When
    let waiting = output(agent.process_message(inbound(REPORT_AND_IMAGE))?);
    let answer = output(agent.process_message(inbound("approve"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(
        answer.contains("\nRevenue grew 10%. [source #2]\n"),
        "{answer}"
    );
    let context = format!("{:?}", provider.requests.lock().unwrap().last());
    assert!(context.contains("The artwork is blue."), "{context}");
    assert!(context.contains("The artwork is green"), "{context}");
    let spied = spied.lock().unwrap().clone();
    assert!(
        spied.contains("runtime_checkpoint"),
        "the spy saw no checkpoint"
    );
    assert!(spied.contains(HELD), "the spy saw no held outputs");
    let saved = saved(&agent).to_string();
    assert!(!saved.contains(HELD), "held outputs outlive their request");
    let shown = [
        &waiting,
        &answer,
        &status,
        &saved,
        &without_held(&spied),
        &diagnostic.lock().unwrap().clone(),
        &events.lock().unwrap().clone(),
    ];
    for shown in shown {
        let leak = shown
            .find("artwork is")
            .map(|at| &shown[at.saturating_sub(160)..at + 10]);
        assert_eq!(leak, None);
    }
    assert!(saved.contains("<withheld: "), "{saved}");
    Ok(())
}

#[test]
fn an_approved_call_whose_output_stops_the_turn_shows_only_its_digest() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    let stopped = "Error: blocked by safety guard. The artwork is blue";
    add_exec(&mut tools, &[stopped.to_owned()]);
    let provider = Provider::new(vec![exec("run-1", "python3 -c 'print(1)'")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(REPORT_AND_IMAGE))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.starts_with(
            "The approved tool call stopped this turn; its output is withheld (Error: <withheld: "
        ),
        "{answer}"
    );
    assert!(
        methods(&agent)["active"].is_null(),
        "the request is still open"
    );
    let saved = saved(&agent).to_string();
    assert!(!saved.contains(HELD), "{saved}");
    for shown in [&answer, &saved] {
        assert!(!shown.contains("artwork"), "{shown}");
    }
    Ok(())
}

/// Session metadata keeping the open request's tool outputs for its own
/// later runs; no surface shows it.
const HELD: &str = "\"task_method_held_results_v1\":";

/// Saved session text without the held outputs (see [`without`]).
fn without_held(text: &str) -> String {
    without(text, HELD)
}

/// Saved session text without the value of each `key` (a `"name":` prefix):
/// the value is cut out up to its closing brace (strings may hold braces).
pub(super) fn without(text: &str, key: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(key) {
        kept.push_str(&rest[..at]);
        let value = &rest[at + key.len()..];
        let (mut depth, mut quoted, mut escaped, mut end) = (0, false, false, value.len());
        for (index, ch) in value.char_indices() {
            match (quoted, escaped, ch) {
                (true, true, _) => escaped = false,
                (true, false, '\\') => escaped = true,
                (_, _, '"') => quoted = !quoted,
                (false, _, '{') => depth += 1,
                (false, _, '}') => {
                    depth -= 1;
                    if depth == 0 {
                        end = index + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &value[end..];
    }
    kept.push_str(rest);
    kept
}
