use super::calculation::{add_exec, exec};
use super::composed::{REPORT_AND_CALCULATION, REPORT_AND_IMAGE};
use super::report::{read, report_tools};
use super::*;

const HELD: &str = "task_method_held_results_v1";

/// A provider that fails once its scripted responses run out.
struct Failing(Provider);
impl ProviderClient for Failing {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        if self
            .0
            .responses
            .lock()
            .expect("responses lock poisoned")
            .is_empty()
        {
            return Err(ProviderError::Api {
                status: Some(500),
                message: "provider unavailable".to_owned(),
                retryable: false,
                headers: Default::default(),
                body: None,
            });
        }
        self.0.chat(request)
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

/// Runs `ask` until a read of art.md waits behind an approval for a command,
/// then sends `then`: the saved session after the request has ended.
fn ended_by(ask: &str, then: &[&str]) -> Result<String, Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("art.md"), "The artwork is blue.")?;
    let mut tools = report_tools(dir.path());
    add_exec(&mut tools, &["ok\n\nExit code: 0".to_owned()]);
    let provider = Failing(Provider::new(vec![
        read("r1", "art.md"),
        exec("run-1", "python3 -c 'print(1)'"),
    ]));
    let mut config = AgentLoopConfig::new(dir.path(), "test-model");
    config.permission_interactive = true;
    let mut agent = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(dir.path())?,
        ContextBuilder::new(dir.path()),
        &tools,
        &provider,
        config,
    );
    agent.process_message(inbound(ask))?;
    assert!(saved(&agent).to_string().contains(HELD), "nothing was held");
    for message in then {
        agent.process_message(inbound(message))?;
    }
    assert!(
        methods(&agent)["active"].is_null(),
        "the request is still open"
    );
    Ok(saved(&agent).to_string())
}

#[test]
fn held_outputs_go_however_the_request_ends() -> Result<(), Box<dyn Error>> {
    for (ask, then) in [
        (REPORT_AND_IMAGE, vec!["deny"]),
        (REPORT_AND_IMAGE, vec!["/new"]),
        (REPORT_AND_IMAGE, vec!["approve"]),
        (
            REPORT_AND_CALCULATION,
            vec!["/method use calculation", "/stop"],
        ),
    ] {
        // When
        let saved = ended_by(ask, &then)?;
        // Then
        assert!(!saved.contains(HELD), "{then:?}: {saved}");
        assert!(!saved.contains("artwork is"), "{then:?}: {saved}");
    }
    Ok(())
}

#[test]
fn with_an_image_part_a_missing_package_is_not_named_from_the_output() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = report_tools(dir.path());
    let missing =
        "Traceback\nModuleNotFoundError: No module named 'The-artwork-is-blue'\n\nExit code: 1";
    add_exec(&mut tools, &[missing.to_owned()]);
    let provider = Provider::new(vec![exec("run-1", "python3 -c 'print(1)'"), text("Done.")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound(
        "Write a report with sources on the room, calculate the floor area in m^2 and generate an image of the room",
    ))?;
    // When
    let answer = output(agent.process_message(inbound("approve"))?);
    // Then
    assert!(
        answer.contains("Calculation: unavailable — a required Python package is not installed (see /method status)"),
        "{answer}"
    );
    assert!(!answer.contains("artwork"), "{answer}");
    Ok(())
}
