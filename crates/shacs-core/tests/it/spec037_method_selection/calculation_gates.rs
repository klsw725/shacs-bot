use super::calculation::{calculate, exec, exec_tools, printed};
use super::*;

struct BlockExec;
impl AgentHook for BlockExec {
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        calls
            .iter()
            .filter(|call| call.name == "exec")
            .map(|call| RuntimeToolMessage {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: "Error: Tool `exec` blocked by plugin hook `policy` for event `tool:before`: not allowed".to_owned(),
            })
            .collect()
    }
}

#[test]
fn exec_blocked_by_an_earlier_hook_is_recorded_as_unavailable() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        text("The hook blocked the calculation."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?.with_agent_hook(Arc::new(BlockExec));
    // When
    let answer = calculate(&mut agent, 0)?;
    // Then
    assert!(
        answer.contains("Calculation: unavailable — execution was denied"),
        "{answer}"
    );
    let run = &methods(&agent)["closed"][0]["calculation"]["runs"][0];
    assert_eq!(run["status"], "denied");
    assert_eq!(run["code"], "python3 -c 'print(3*2)'");
    Ok(())
}

#[test]
fn reused_tool_call_ids_still_show_the_runs_that_were_judged() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary"), printed("6", "check")]);
    let provider = Provider::new(vec![
        call("exec", json!({"command": "python3 -c 'print(3*2)'"})),
        call("exec", json!({"command": "python3 -c 'print(6)'"})),
        text("The floor area is 6 m^2."),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let answer = calculate(&mut agent, 2)?;
    // Then
    assert!(answer.contains("Calculation: verified"), "{answer}");
    assert!(answer.contains("Execution: primary #1 call-exec exit 0; check #2 call-exec exit 0"));
    Ok(())
}

struct BlockMarkedExec;
impl AgentHook for BlockMarkedExec {
    fn receives_tool_arguments(&self) -> bool {
        true
    }
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        calls
            .iter()
            .filter(|call| call.arguments.to_string().contains("blocked-attempt"))
            .map(|call| RuntimeToolMessage {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: "Error: Tool `exec` blocked by plugin hook `policy` for event `tool:before`: not allowed".to_owned(),
            })
            .collect()
    }
}

#[test]
fn a_hook_blocked_retry_with_a_reused_id_withdraws_earlier_verification(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[
        printed("6", "primary"),
        printed("6", "check"),
        printed("6", "check"),
    ]);
    let provider = Provider::new(vec![
        exec("same", "python3 -c 'print(3*2)'"),
        exec("same", "python3 -c 'print(6)'"),
        exec("same", "python3 -c 'blocked-attempt'"),
        exec("same", "python3 -c 'print(2*3)'"),
        text("The floor area is 6 m^2."),
    ]);
    let mut agent =
        runtime(dir.path(), &tools, &provider)?.with_agent_hook(Arc::new(BlockMarkedExec));
    // When
    let answer = calculate(&mut agent, 3)?;
    // Then
    assert!(
        answer.contains("Calculation: unavailable — a later run could not complete because execution was denied"),
        "{answer}"
    );
    let runs = &methods(&agent)["closed"][0]["calculation"]["runs"];
    assert_eq!(runs[2]["status"], "denied");
    assert_eq!(runs[2]["code"], "python3 -c 'blocked-attempt'");
    Ok(())
}

struct BlockWithResultLine;
impl AgentHook for BlockWithResultLine {
    fn block_tool_calls(
        &self,
        _: &AgentHookContext,
        calls: &[RuntimeToolCall],
    ) -> Vec<RuntimeToolMessage> {
        calls
            .iter()
            .map(|call| RuntimeToolMessage {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: format!(
                    "Error: Tool `exec` blocked by plugin hook `policy` for event `tool:before`:\n{}",
                    printed("6", "primary")
                ),
            })
            .collect()
    }
}

#[test]
fn a_hook_block_reason_cannot_smuggle_in_a_calculation_result() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        text("Blocked."),
    ]);
    let mut agent =
        runtime(dir.path(), &tools, &provider)?.with_agent_hook(Arc::new(BlockWithResultLine));
    // When
    let answer = calculate(&mut agent, 0)?;
    // Then
    assert!(
        answer.contains("Calculation: unavailable — execution was denied"),
        "{answer}"
    );
    assert!(!answer.contains("Recorded primary result"));
    assert!(methods(&agent)["closed"][0]["calculation"]["runs"][0]["result"].is_null());
    Ok(())
}

#[test]
fn denying_a_later_exec_withdraws_earlier_verification() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = exec_tools(&[printed("6", "primary"), printed("6", "check")]);
    let provider = Provider::new(vec![
        exec("run-1", "python3 -c 'print(3*2)'"),
        exec("run-2", "python3 -c 'print(6)'"),
        exec("run-3", "python3 -c 'print(2*3)'"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    calculate(&mut agent, 2)?;
    // When
    let denied = output(agent.process_message(inbound("deny"))?);
    // Then
    assert!(
        denied.contains("Calculation: unavailable — a later run could not complete because execution was denied"),
        "{denied}"
    );
    let request = &methods(&agent)["closed"][0];
    assert_eq!(request["state"], "cancelled");
    assert_eq!(request["calculation"]["runs"][2]["status"], "denied");
    Ok(())
}
