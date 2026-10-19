use super::*;

#[test]
fn lost_or_malformed_decision_state_cannot_fall_back_to_generic_ask_resume(
) -> Result<(), Box<dyn Error>> {
    for state in [None, Some(json!({"stage":"pending"})), Some(Value::Null)] {
        let workspace = tempfile::tempdir()?;
        let work = setup(workspace.path())?;
        let counter = Arc::new(AtomicUsize::new(0));
        let tools = registry(&counter);
        let provider = Provider::new(vec![response(vec![proposal(&work.work.work_id)])]);
        let mut agent = runtime(workspace.path(), &tools, &provider)?;
        agent.process_message(inbound("suggest action"))?;
        let mut manager = SessionManager::new(workspace.path())?;
        let mut session = manager.get_or_create("chat:room");
        match state {
            Some(value) => {
                session
                    .metadata
                    .insert("entrusted_action_decision".to_owned(), value);
            }
            None => {
                session.metadata.remove("entrusted_action_decision");
            }
        }
        manager.save(&session)?;

        let result = agent.process_message(inbound("yes"))?;

        assert_eq!(result.stop_reason, "entrusted_action_closed");
        assert_eq!(provider.calls(), 1);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[test]
fn checkpoint_before_proposal_binding_cannot_resume_after_recovery() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let call = proposal(&work.work.work_id);
    let runtime_call = RuntimeToolCall::new(call.id, call.name, Value::Object(call.arguments));
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = manager.get_or_create("chat:room");
    session.metadata.insert("runtime_checkpoint".to_owned(), json!({
        "phase":"awaiting_tools",
        "assistant_message":{"role":"assistant","tool_calls":[{"id":runtime_call.id,"type":"function","function":{"name":runtime_call.name,"arguments":runtime_call.arguments.to_string()}}]},
        "pending_tool_calls":[runtime_call], "completed_tool_results":[]
    }));
    manager.save(&session)?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let provider = Provider::new(Vec::new());
    let mut agent = runtime(workspace.path(), &tools, &provider)?;

    let result = agent.process_message(inbound("yes"))?;

    assert_eq!(result.stop_reason, "entrusted_action_closed");
    assert_eq!(provider.calls(), 0);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn malformed_or_unbound_proposals_stop_the_batch_without_provider_continuation(
) -> Result<(), Box<dyn Error>> {
    for invalid in [
        json!(null),
        json!({}),
        json!({"category":"delete"}),
        json!({"summary":"long".repeat(100)}),
        json!({"work_id":"work-other"}),
        json!({"reference":"https://host/?token=secret"}),
    ] {
        let workspace = tempfile::tempdir()?;
        let work = setup(workspace.path())?;
        let counter = Arc::new(AtomicUsize::new(0));
        let tools = registry(&counter);
        let mut call = proposal(&work.work.work_id);
        if let Some(fields) = invalid.as_object().filter(|fields| !fields.is_empty()) {
            call.arguments
                .get_mut("external_action")
                .and_then(Value::as_object_mut)
                .unwrap()
                .extend(fields.clone());
        } else {
            call.arguments.insert("external_action".to_owned(), invalid);
        }
        let provider = Provider::new(vec![response(vec![send_call(), call])]);
        let mut agent = runtime(workspace.path(), &tools, &provider)?;

        let result = agent.process_message(inbound("suggest action"))?;

        assert!(matches!(
            result.stop_reason.as_str(),
            "entrusted_action_closed" | "tool_error"
        ));
        assert_eq!(provider.calls(), 1);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[test]
fn each_category_has_a_safe_prompt_and_new_id_even_when_provider_reuses_call_id(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let work = setup(workspace.path())?;
    let counter = Arc::new(AtomicUsize::new(0));
    let tools = registry(&counter);
    let responses = ["reserve", "purchase", "send"]
        .into_iter()
        .map(|category| {
            let mut call = proposal(&work.work.work_id);
            call.arguments.get_mut("external_action").unwrap()["category"] = json!(category);
            call.arguments.get_mut("external_action").unwrap()["summary"] =
                json!("send token=RAW_PRIVATE_TOKEN");
            response(vec![call])
        })
        .collect();
    let provider = Provider::new(responses);
    let mut agent = runtime(workspace.path(), &tools, &provider)?;
    let mut ids = std::collections::BTreeSet::new();

    for _ in 0..3 {
        let pending = agent.process_message(inbound("suggest action"))?;
        let id = proposal_id(&pending)?;
        assert!(ids.insert(id.clone()));
        assert!(!pending
            .final_content
            .as_deref()
            .unwrap()
            .contains("RAW_PRIVATE_TOKEN"));
        agent.process_message(inbound(&format!("/action decline {id}")))?;
    }

    assert_eq!(provider.calls(), 3);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    Ok(())
}
