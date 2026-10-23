use super::*;

#[test]
fn automatic_general_preserves_one_provider_call_and_unadorned_output() -> Result<(), Box<dyn Error>>
{
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("hello back")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let result = agent.process_message(inbound("hello"))?;
    // Then
    assert_eq!(result.final_content.as_deref(), Some("hello back"));
    assert_eq!(provider.calls(), 1);
    assert!(!provider.requests.lock().unwrap()[0].messages[0]
        .to_string()
        .contains("<task-method>"));
    assert_eq!(
        methods(&agent)["closed"][0]["selection"]["method"],
        "general"
    );
    Ok(())
}

#[test]
fn specialized_methods_project_context_and_unverified_checks() -> Result<(), Box<dyn Error>> {
    // Given
    for (prompt, names, method, readiness) in [
        (
            "Calculate 20 percent of 80",
            vec!["exec"],
            "reproducible_calculation",
            "available",
        ),
        (
            "출처를 연결한 보고서를 작성해",
            vec!["read_file"],
            "source_grounded_report",
            "available",
        ),
        (
            "Generate an image of a tree",
            vec!["image_generate"],
            "image_generation_review",
            "requires_runtime_check",
        ),
    ] {
        let dir = tempfile::tempdir()?;
        let tools = tools(&names, &Arc::new(AtomicUsize::new(0)));
        let provider = Provider::new(vec![text("prepared")]);
        let mut agent = runtime(dir.path(), &tools, &provider)?;
        // When
        let result = output(agent.process_message(inbound(prompt))?);
        // Then
        assert!(result.contains(method));
        assert!(result.contains("not verified"));
        assert!(provider.requests.lock().unwrap()[0].messages[0]
            .to_string()
            .contains("<task-method>"));
        let state = methods(&agent);
        assert_eq!(
            state["closed"][0]["selection"]["availability"]["readiness"],
            readiness
        );
        let claims = state["closed"][0]["selection"]["validation_claims"]
            .as_array()
            .ok_or("claims")?;
        let expected = match method {
            "reproducible_calculation" => 4,
            "source_grounded_report" | "image_generation_review" => 3,
            _ => 0,
        };
        assert_eq!(claims.len(), expected);
        // The one-word reply has no source marker, so the report lists it as unchecked.
        let unmarked = json!(["no source marker: prepared"]);
        assert!(claims.iter().all(|claim| claim["state"] == "not_verified"
            && (claim["evidence_refs"] == json!([]) || claim["evidence_refs"] == unmarked)));
        assert!(!saved(&agent)["messages"]
            .to_string()
            .contains("Details: /method status"));
    }
    Ok(())
}

#[test]
fn missing_tool_blocks_without_provider_and_preserves_history_for_override(
) -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("continued")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    // Then
    assert_eq!(provider.calls(), 0);
    assert_eq!(methods(&agent)["active"]["state"], "blocked");
    assert_eq!(
        methods(&agent)["active"]["selection"]["availability"]["missing_required_tools"],
        json!(["exec"])
    );
    assert!(saved(&agent)["messages"]
        .to_string()
        .contains("Calculate 2 plus 3"));
    Ok(())
}

#[test]
fn one_shot_override_is_consumed_by_exactly_one_request() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = ToolRegistry::new();
    let provider = Provider::new(vec![text("first")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    agent.process_message(inbound("/method use general"))?;
    agent.process_message(inbound("Calculate 2 plus 3"))?;
    // When
    agent.process_message(inbound("Calculate 5 plus 6"))?;
    // Then
    assert_eq!(provider.calls(), 1);
    let state = methods(&agent);
    assert_eq!(
        state["closed"][0]["selection"]["reason"],
        "one_shot_override"
    );
    assert_eq!(state["active"]["selection"]["reason"], "calculation_intent");
    assert_eq!(state["active"]["state"], "blocked");
    Ok(())
}

#[test]
fn report_with_local_reader_is_available_without_web_tools() -> Result<(), Box<dyn Error>> {
    // Given
    let dir = tempfile::tempdir()?;
    let tools = tools(&["read_file"], &Arc::new(AtomicUsize::new(0)));
    let provider = Provider::new(vec![text("local report")]);
    let mut agent = runtime(dir.path(), &tools, &provider)?;
    // When
    let result = output(agent.process_message(inbound("Write a report with sources"))?);
    // Then
    assert!(result.contains("web research unavailable"));
    assert_eq!(
        methods(&agent)["closed"][0]["selection"]["availability"]["missing_optional_tools"],
        json!(["web_search", "web_fetch"])
    );
    Ok(())
}
