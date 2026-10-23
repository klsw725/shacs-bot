use super::*;

#[test]
fn system_turn_neither_starts_a_request_nor_obeys_method_availability() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("system processed")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    agent.process_message(
        InboundMessage::new("system", "subagent", "room", "Calculate 2 plus 3")
            .with_session_key_override("chat:room"),
    )?;
    // Then
    assert_eq!(provider.calls(), 1);
    assert!(saved(&agent)["metadata"]
        .get("task_method_requests_v1")
        .is_none());
    Ok(())
}

#[test]
fn non_user_turns_cannot_change_the_method() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    let before = methods(&agent);
    for channel in ["system", "cron", "automation"] {
        for command in [
            "/method use general",
            "/method auto",
            "/method accept",
            "/method revise",
        ] {
            // When
            let result = output(
                agent.process_message(
                    InboundMessage::new(channel, "subagent", "room", command)
                        .with_session_key_override("chat:room"),
                )?,
            );
            // Then
            assert!(result.contains("method state is unchanged"), "{result}");
            assert_eq!(methods(&agent), before);
        }
    }
    assert_eq!(provider.calls(), 0);
    Ok(())
}

fn automation(sender: &str, route: &str, content: &str) -> InboundMessage {
    InboundMessage::new("chat", sender, "room", content).with_metadata(serde_json::Map::from_iter(
        [("automation_route".to_owned(), json!(route))],
    ))
}

#[test]
fn automation_routes_on_user_channels_neither_consume_nor_obey_overrides(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("notified"), text("verified")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("/method use calculation"))?;
    let before = methods(&agent);
    // When
    agent.process_message(automation("automation", "notify", "Report the result."))?;
    agent.process_message(automation("automation-verifier", "verify", "Verify it."))?;
    // Then
    assert_eq!(provider.calls(), 2);
    assert_eq!(methods(&agent), before);
    agent.process_message(inbound("hello"))?;
    let state = methods(&agent);
    assert_eq!(state["active"]["selection"]["reason"], "one_shot_override");
    assert_eq!(state["active"]["state"], "blocked");
    assert_eq!(provider.calls(), 2);
    Ok(())
}

#[test]
fn automation_answer_to_a_pending_question_is_not_a_user_decision() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["exec"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        call("ask_user", json!({"question":"Which units?"})),
        text("continued by automation"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    // When
    agent.process_message(automation(
        "automation",
        "continue",
        "Continue the goal once.",
    ))?;
    // Then
    let state = methods(&agent);
    assert_eq!(state["closed"][0]["state"], "answered");
    assert_eq!(state["closed"][0]["user_decisions"], json!([]));
    Ok(())
}

#[test]
fn non_user_answer_to_a_pending_question_finishes_the_request() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["exec"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![
        call("ask_user", json!({"question":"Which units?"})),
        text("answered after subagent"),
        text("new request"),
    ]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate the amount"))?;
    let id = methods(&agent)["active"]["request_id"].clone();
    // When
    agent.process_message(
        InboundMessage::new("system", "subagent", "room", "subagent finished")
            .with_session_key_override("chat:room"),
    )?;
    agent.process_message(inbound("hello"))?;
    // Then
    let state = methods(&agent);
    assert_eq!(state["closed"][0]["request_id"], id);
    assert_eq!(state["closed"][0]["state"], "answered");
    assert_eq!(state["closed"][0]["user_decisions"], json!([]));
    assert_eq!(state["closed"][1]["goal_preview"], "hello");
    assert_eq!(provider.calls(), 3);
    Ok(())
}

#[test]
fn system_turn_bypasses_availability_without_changing_a_blocked_request(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("system processed")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    let before = methods(&agent);
    // When
    let result = agent.process_message(
        InboundMessage::new("system", "subagent", "room", "Calculate 4 plus 5")
            .with_session_key_override("chat:room"),
    )?;
    // Then
    assert_eq!(result.final_content.as_deref(), Some("system processed"));
    assert_eq!(provider.calls(), 1);
    assert_eq!(methods(&agent), before);
    Ok(())
}
