use super::*;

struct ProcessDeniedExec;
impl Tool for ProcessDeniedExec {
    fn name(&self) -> &str {
        "exec"
    }
    fn description(&self) -> &str {
        "Return the existing process-owner gate denial without launching a process"
    }
    fn parameters(&self) -> Value {
        json!({"type":"object","properties":{"command":{"type":"string"}},"required":["command"]})
    }
    fn execute(&self, _: JsonMap) -> ToolResult {
        "Error: Process launch blocked by permission gate (Denied)".into()
    }
}

#[test]
fn process_gate_denial_after_approval_is_recorded_as_denied() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let mut tools = ToolRegistry::new();
    tools.register(ProcessDeniedExec);
    let provider = Provider::new(vec![
        call("exec", json!({"command":"printf isolated"})),
        text("attempt returned"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    // When
    agent.process_message(inbound("approve"))?;
    // Then
    assert_eq!(
        methods(&agent)["closed"][0]["external_effect_facts"][0]["outcome"],
        "denied"
    );
    Ok(())
}

#[test]
fn changed_method_rejects_old_permission_and_requires_a_new_approval() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = tools(&["exec"], &counter);
    let provider = Provider::new(vec![
        call("exec", json!({"command":"printf method-test"})),
        call("exec", json!({"command":"printf method-test"})),
        text("done"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    let approval = saved(&agent)["metadata"]["pending_permission_approval"].clone();
    assert!(approval.is_object());
    agent.process_message(inbound("/method use general"))?;
    // When
    agent.process_message(inbound("continue"))?;
    // Then
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    assert!(saved(&agent)["metadata"]["pending_permission_approval"].is_object());
    assert!(saved(&agent).to_string().contains("rejected"));
    agent.process_message(inbound("approve"))?;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn executed_effect_is_not_reverted_and_superseded_revision_survives_restart(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = tools(&["exec"], &counter);
    let provider = Provider::new(vec![
        call("exec", json!({"command":"printf method-test"})),
        call("ask_user", json!({"question":"Continue?"})),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    agent.process_message(inbound("approve"))?;
    agent.process_message(inbound("/method use general"))?;
    drop(agent);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let history = output(agent.process_message(inbound("/method history"))?);
    // Then
    assert!(history.contains("not reverted"));
    assert!(history.contains("[superseded]"));
    let state = methods(&agent);
    assert_eq!(
        state["active"]["external_effect_facts"][0]["outcome"],
        "completed"
    );
    assert_eq!(
        state["active"]["external_effect_facts"][0]["selection_revision"],
        1
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    assert!(!state["active"]["external_effect_facts"]
        .to_string()
        .contains("printf method-test"));
    assert_eq!(
        state["active"]["calculation"]["runs"][0]["code"],
        "printf method-test"
    );
    Ok(())
}
