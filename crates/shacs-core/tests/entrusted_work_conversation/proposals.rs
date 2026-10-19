use super::*;
use shacs_core::runtime::ENTRUSTED_WORK_METADATA_KEY;

#[test]
fn proposal_requires_exact_sender_bound_acceptance_not_bare_yes() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(ProposeEntrustedWorkTool::new());
    let provider = MockProvider::new(vec![
        proposal_response("proposal-accept", "token=RAW_SECRET"),
        text_response("proposal prepared"),
        text_response("ordinary yes response"),
    ]);
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let proposed = runtime.process_message(inbound(
        "chat",
        "alice",
        "room",
        "please suggest monitoring",
    ))?;
    let proposal_id = proposed
        .final_content
        .as_deref()
        .and_then(|text| text.split("/work accept ").nth(1))
        .and_then(|rest| rest.split('`').next())
        .ok_or("missing proposal id")?
        .to_owned();
    let pending = runtime
        .session_manager()
        .read_session_file("chat:room")
        .ok_or("missing pending session")?["metadata"]["pending_entrusted_work_proposal"]
        .clone();

    // When
    runtime.process_message(inbound("chat", "alice", "room", "yes"))?;
    let accepted = runtime.process_message(inbound(
        "chat",
        "alice",
        "room",
        &format!("/work accept {proposal_id}"),
    ))?;

    // Then
    assert!(!pending.to_string().contains("RAW_SECRET"));
    assert!(accepted
        .final_content
        .as_deref()
        .is_some_and(|text| text.starts_with("Accepted entrusted work")));
    let raw = runtime
        .session_manager()
        .read_session_file("chat:room")
        .ok_or("missing session")?;
    assert!(!raw["metadata"][ENTRUSTED_WORK_METADATA_KEY]
        .to_string()
        .contains("RAW_SECRET"));
    assert!(raw["metadata"]
        .get("pending_entrusted_work_proposal")
        .is_none());
    Ok(())
}

#[test]
fn proposal_decline_and_wrong_sender_leave_no_work() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(ProposeEntrustedWorkTool::new());
    let provider = MockProvider::new(vec![
        proposal_response("proposal-decline", "watch verified changes"),
        text_response("proposal prepared"),
        text_response("ordinary follow-up"),
    ]);
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let proposed = runtime.process_message(inbound("chat", "alice", "room", "suggest work"))?;
    let proposal_id = pending_proposal_id(&runtime, "chat:room")?;

    // When
    let rejected = runtime.process_message(inbound(
        "chat",
        "mallory",
        "room",
        &format!("/work accept {proposal_id}"),
    ))?;
    let declined = runtime.process_message(inbound(
        "chat",
        "alice",
        "room",
        &format!("/work decline {proposal_id}"),
    ))?;
    runtime.process_message(inbound("chat", "alice", "room", "thanks"))?;

    // Then
    assert_eq!(proposed.stop_reason, "entrusted_work_proposed");
    assert_eq!(rejected.stop_reason, "work_rejected");
    assert!(declined
        .final_content
        .as_deref()
        .is_some_and(|text| text.starts_with("Declined entrusted work proposal")));
    let raw = runtime
        .session_manager()
        .read_session_file("chat:room")
        .ok_or("missing session")?;
    assert!(raw["metadata"].get(ENTRUSTED_WORK_METADATA_KEY).is_none());
    assert!(raw["metadata"]
        .get("pending_entrusted_work_proposal")
        .is_none());
    Ok(())
}

#[test]
fn proposal_rejects_cross_session_and_survives_restart() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let mut registry = ToolRegistry::new();
    registry.register(ProposeEntrustedWorkTool::new());
    let provider = MockProvider::new(vec![
        proposal_response("proposal-restart", "watch verified changes"),
        text_response("proposal prepared"),
    ]);
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    runtime.process_message(inbound("chat", "alice", "room-a", "suggest work"))?;
    let proposal_id = pending_proposal_id(&runtime, "chat:room-a")?;

    // When
    let cross_session = runtime.process_message(inbound(
        "chat",
        "alice",
        "room-b",
        &format!("/work accept {proposal_id}"),
    ))?;
    drop(runtime);
    let restarted_provider = MockProvider::new(Vec::new());
    let mut restarted = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &restarted_provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let accepted = restarted.process_message(inbound(
        "chat",
        "alice",
        "room-a",
        &format!("/work accept {proposal_id}"),
    ))?;

    // Then
    assert_eq!(cross_session.stop_reason, "work_rejected");
    assert!(accepted
        .final_content
        .as_deref()
        .is_some_and(|text| text.starts_with("Accepted entrusted work")));
    let raw = restarted
        .session_manager()
        .read_session_file("chat:room-a")
        .ok_or("missing restarted session")?;
    assert_eq!(
        raw["metadata"][ENTRUSTED_WORK_METADATA_KEY]["works"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}
