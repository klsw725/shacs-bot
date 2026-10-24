use super::*;

struct ScriptedExec(Mutex<VecDeque<String>>);
impl Tool for ScriptedExec {
    fn name(&self) -> &str {
        "exec"
    }
    fn description(&self) -> &str {
        "Return scripted exec output without launching a process"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]})
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        ToolResult::Text(self.0.lock().unwrap().pop_front().expect("unexpected exec"))
    }
}

pub(super) fn exec_tools(outputs: &[String]) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    add_exec(&mut tools, outputs);
    tools
}
pub(super) fn add_exec(tools: &mut ToolRegistry, outputs: &[String]) {
    tools.register(ScriptedExec(Mutex::new(outputs.iter().cloned().collect())));
}
pub(super) fn exec(id: &str, command: &str) -> LlmResponse {
    LlmResponse {
        finish_reason: "tool_calls".to_owned(),
        tool_calls: vec![ToolCallRequest::new(
            id,
            "exec",
            serde_json::Map::from_iter([("command".to_owned(), json!(command))]),
        )],
        ..LlmResponse::default()
    }
}
pub(super) fn printed(answer: &str, role: &str) -> String {
    format!(
        "SHACS_CALC {}\n\nExit code: 0",
        json!({"role": role, "answer": answer, "unit": "m^2",
            "inputs": {"length": "3 m", "width": "200 cm"}, "assumptions": ["flat floor"]})
    )
}
pub(super) fn calculate(
    agent: &mut AgentLoop<'_>,
    approvals: usize,
) -> Result<String, Box<dyn Error>> {
    let mut last = output(agent.process_message(inbound("Calculate the floor area in m^2"))?);
    for _ in 0..approvals {
        last = output(agent.process_message(inbound("approve"))?);
    }
    Ok(last)
}

#[test]
fn agreeing_independent_check_is_verified_with_answer_first_and_details_on_request(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary"), printed("6.0", "check")]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(300*200/10000)'"),
        text("The floor area is 6 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = calculate(&mut agent, 2)?;
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(answer.starts_with("The floor area is 6 m^2."));
    assert!(answer.contains("Calculation: verified"));
    assert!(answer.contains("Verified answer: 6 m^2"));
    assert!(answer.contains("Inputs: length = 3 m; width = 200 cm"));
    assert!(answer.contains("Execution: primary #1 run-1 exit 0; check #2 run-2 exit 0"));
    assert!(answer.contains("User acceptance: not recorded"));
    assert!(status.contains("code: python3 -c 'print(300*200/10000)'"));
    assert!(status.contains("- #2 run-2 [check] exit 0"));
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[3]["check"], "independent check");
    assert_eq!(claims[3]["state"], "verified");
    Ok(())
}

#[test]
fn wrong_unit_conversion_surfaces_as_a_conflict_not_a_result() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("600", "primary"), printed("6", "check")]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*200)'"),
        exec("run-2", "python3 -c 'print(3*2)'"),
        text("The floor area is 600 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = calculate(&mut agent, 2)?;
    // Then
    assert!(
        answer.contains("Calculation: conflict — value mismatch: primary 600 m^2 vs check 6 m^2")
    );
    assert!(answer.contains("Recorded primary result (not verified): 600 m^2"));
    assert!(!answer.contains("Verified answer"));
    let claims = &methods(&agent)["closed"][0]["selection"]["validation_claims"];
    assert_eq!(claims[3]["state"], "failed");
    Ok(())
}

#[test]
fn rerun_after_a_fix_does_not_keep_the_earlier_verification() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[
        printed("6", "primary"),
        printed("6", "check"),
        printed("6", "primary"),
    ]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(6)'"),
        exec("run-3", "python3 -c 'print(3.0*2.0)'"),
        text("Recomputed: 6 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = calculate(&mut agent, 3)?;
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(answer.contains(
        "Calculation: not verified — no independent check was run after the latest primary result"
    ));
    assert!(status.contains("- #1 run-1 [primary] exit 0 [superseded by a later primary run]"));
    assert!(status.contains("- #2 run-2 [check] exit 0 [superseded by a later primary run]"));
    Ok(())
}

#[test]
fn failed_denied_or_missing_python_runs_report_no_calculated_result() -> Result<(), Box<dyn Error>>
{
    for (output_text, expected) in [
        (
            "Traceback\nZeroDivisionError: division by zero\n\nExit code: 1",
            "Calculation: failed",
        ),
        (
            "Error: Process launch blocked by permission gate (Denied)",
            "Calculation: unavailable — execution was denied",
        ),
        (
            "sh: 1: python3: not found\n\nExit code: 127",
            "Calculation: unavailable — Python is not available",
        ),
    ] {
        // Given
        let dir = tempfile::tempdir()?;
        let tools = exec_tools(&[output_text.to_owned()]);
        let provider = Provider::new(vec![
            exec("run-1", "python3 -c 'print(1/0)'"),
            text("I could not compute it."),
        ]);
        let mut agent = runtime(dir.path(), &tools, &provider)?;
        // When
        let answer = calculate(&mut agent, 1)?;
        // Then
        assert!(answer.contains(expected), "{answer}");
        assert!(!answer.contains("Verified answer"));
        assert!(!answer.contains("Recorded primary result"));
    }
    Ok(())
}

#[test]
fn acceptance_is_a_separate_fact_that_never_verifies() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary")]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        text("The floor area is 6 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    calculate(&mut agent, 1)?;
    // When
    let accepted = output(agent.process_message(inbound("/method accept"))?);
    let status = output(agent.process_message(inbound("/method status"))?);
    // Then
    assert!(accepted.contains("Verification is unchanged: not verified."));
    assert!(status.contains("Calculation: not verified"));
    assert!(status.contains("while verification was not verified"));
    let request = &methods(&agent)["closed"][0];
    assert_eq!(request["user_acceptance"]["verification"], "not verified");
    assert_eq!(
        request["selection"]["validation_claims"][3]["state"],
        "not_verified"
    );
    Ok(())
}

#[test]
fn changing_the_method_supersedes_calculation_checks() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary"), printed("6", "check")]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(6)'"),
        call("ask_user", json!({"question":"Round the result?"})),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    calculate(&mut agent, 2)?;
    // When
    let changed = output(agent.process_message(inbound("/method use general"))?);
    // Then
    assert!(changed
        .contains("Calculation evidence from revision 1: superseded_not_inherited (was verified)"));
    let request = &methods(&agent)["active"];
    assert_eq!(
        request["superseded_selections"][0]["validation_claims"][3]["state"],
        "superseded_not_inherited"
    );
    assert_eq!(request["selection"]["validation_claims"], json!([]));
    Ok(())
}
