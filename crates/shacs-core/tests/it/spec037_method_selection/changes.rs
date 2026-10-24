use super::*;

#[test]
fn override_unblocks_the_same_request_and_preserves_its_goal() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("continued")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    let id = methods(&agent)["active"]["request_id"].clone();
    let changed = output(agent.process_message(inbound("/method use general"))?);
    // When
    agent.process_message(inbound("continue"))?;
    // Then
    let state = methods(&agent);
    assert!(
        changed.contains("Nothing was pending to cancel"),
        "{changed}"
    );
    assert_eq!(provider.calls(), 1);
    assert_eq!(state["closed"][0]["request_id"], id);
    assert_eq!(state["closed"][0]["goal_preview"], "Calculate 2 plus 3");
    assert_eq!(state["closed"][0]["selection"]["revision"], 2);
    Ok(())
}

#[test]
fn method_change_cancels_pending_question_but_retains_it_as_remaining() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["exec"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        call("ask_user", json!({"question":"Which units?"})),
        text("continued"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    let id = methods(&agent)["active"]["request_id"].clone();
    // When
    let changed = output(agent.process_message(inbound("/method use general"))?);
    // Then
    let state = methods(&agent);
    assert!(
        changed.contains("Cancelled pending question (method_changed)"),
        "{changed}"
    );
    assert_eq!(state["active"]["state"], "awaiting_continuation");
    assert_eq!(
        state["active"]["remaining_questions"],
        json!(["Which units?"])
    );
    let persisted = saved(&agent);
    let messages: Vec<Value> = serde_json::from_value(persisted["messages"].clone())?;
    assert!(shacs_core::tools::pending_ask_user_id(&messages).is_none());
    assert_eq!(state["active"]["request_id"], id);
    Ok(())
}

#[test]
fn answered_questions_remain_user_decisions_after_a_change() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["exec"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        call("ask_user", json!({"question":"Which units?"})),
        call("ask_user", json!({"question":"Which precision?"})),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    agent.process_message(inbound("meters"))?;
    // When
    agent.process_message(inbound("/method use general"))?;
    // Then
    assert_eq!(
        methods(&agent)["active"]["user_decisions"],
        json!([{"question":"Which units?","answer":"meters"}])
    );
    Ok(())
}

#[test]
fn validation_claims_are_superseded_instead_of_inherited() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    drop(agent);
    {
        let _guard = shacs_session::SessionMutationGuard::acquire(dir.path(), "chat:room")?;
        let mut manager = SessionManager::new(dir.path())?;
        let mut session = manager.get_or_create("chat:room");
        session.metadata["task_method_requests_v1"]["active"]["selection"]["validation_claims"] = json!([{"check":"independent check","state":"verified","evidence_refs":["fixture:evidence"]}]);
        manager.save(&session)?;
    }
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    agent.process_message(inbound("/method use general"))?;
    // Then
    let state = methods(&agent);
    assert_eq!(state["active"]["selection"]["validation_claims"], json!([]));
    assert_eq!(
        state["active"]["superseded_selections"][0]["validation_claims"][0]["state"],
        "superseded_not_inherited"
    );
    Ok(())
}

#[test]
fn goal_preview_is_bounded_and_redacted() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    let goal = format!(
        "Calculate api_key=sk-secret-sensitive-value {}",
        "x".repeat(1000)
    );
    // When
    agent.process_message(inbound(&goal))?;
    // Then
    let state = methods(&agent);
    let preview = state["active"]["goal_preview"].as_str().unwrap();
    assert!(preview.chars().count() <= 512);
    assert!(!preview.contains("sk-secret-sensitive-value"));
    Ok(())
}

#[test]
fn automatic_reselection_uses_the_original_goal_after_an_explicit_change(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    agent.process_message(inbound("/method use general"))?;
    // When
    agent.process_message(inbound("/method auto"))?;
    // Then
    let state = methods(&agent);
    assert_eq!(
        state["active"]["selection"]["method"],
        "reproducible_calculation"
    );
    assert_eq!(state["active"]["selection"]["source"], "auto");
    assert_eq!(state["active"]["selection"]["revision"], 3);
    assert_eq!(provider.calls(), 0);
    Ok(())
}

#[test]
fn stop_keeps_a_pending_approval_usable_and_its_request_in_progress() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = tools(&["exec"], &counter);
    let provider = Provider::new(vec![
        call("exec", json!({"command":"printf stop-test"})),
        text("done"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    // When
    agent.process_message(inbound("/stop"))?;
    // Then
    assert_eq!(methods(&agent)["active"]["state"], "in_progress");
    agent.process_message(inbound("approve"))?;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn stop_closes_an_idle_changed_request_and_the_next_message_starts_a_new_one(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("fresh answer")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    agent.process_message(inbound("/method use general"))?;
    // When
    agent.process_message(inbound("/stop"))?;
    agent.process_message(inbound("hello"))?;
    // Then
    let state = methods(&agent);
    assert_eq!(state["closed"][0]["state"], "cancelled");
    assert_eq!(state["closed"][0]["goal_preview"], "Calculate 2 plus 3");
    assert_eq!(state["closed"][1]["goal_preview"], "hello");
    assert_eq!(state["closed"][1]["selection"]["revision"], 1);
    assert_eq!(provider.calls(), 1);
    Ok(())
}

#[test]
fn new_session_closes_the_request_without_method_change_handling() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["exec"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![call("ask_user", json!({"question":"Which units?"}))]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    // When
    agent.process_message(inbound("/new"))?;
    // Then
    let state = methods(&agent);
    assert!(state["active"].is_null());
    assert_eq!(state["closed"][0]["state"], "cancelled");
    assert_eq!(state["closed"][0]["remaining_questions"], json!([]));
    Ok(())
}

#[test]
fn selecting_the_current_method_does_not_cancel_a_pending_question() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["exec"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![call("ask_user", json!({"question":"Which units?"}))]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate a length"))?;
    let before = methods(&agent);
    // When
    agent.process_message(inbound("/method use calculation"))?;
    // Then
    assert_eq!(methods(&agent), before);
    assert_eq!(provider.calls(), 1);
    let messages: Vec<Value> = serde_json::from_value(saved(&agent)["messages"].clone())?;
    assert!(shacs_core::tools::pending_ask_user_id(&messages).is_some());
    Ok(())
}
