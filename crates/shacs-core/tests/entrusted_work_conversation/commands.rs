use super::*;
use shacs_core::runtime::ENTRUSTED_WORK_METADATA_KEY;
use shacs_session::durable_replay::evaluate_durable_recovery;

#[test]
fn explicit_work_commands_drive_owner_lifecycle() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let registry = ToolRegistry::new();
    let provider = MockProvider::new(Vec::new());
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    // When
    let created = runtime.process_direct(
        "/work create monitor verified release readiness",
        Some("direct:work-lifecycle"),
    )?;
    let work_id = created
        .final_content
        .as_deref()
        .and_then(backticked_id)
        .ok_or("missing work id")?
        .to_owned();
    let read = runtime.process_direct(
        format!("/work read {work_id}"),
        Some("direct:work-lifecycle"),
    )?;
    let edited = runtime.process_direct(
        format!("/work edit {work_id} 1 notify only on verified changes"),
        Some("direct:work-lifecycle"),
    )?;
    let paused = runtime.process_direct(
        format!("/work pause {work_id}"),
        Some("direct:work-lifecycle"),
    )?;
    let resumed = runtime.process_direct(
        format!("/work resume {work_id}"),
        Some("direct:work-lifecycle"),
    )?;
    let done = runtime.process_direct(
        format!("/work done {work_id}"),
        Some("direct:work-lifecycle"),
    )?;
    let cleared = runtime.process_direct(
        format!("/work clear {work_id}"),
        Some("direct:work-lifecycle"),
    )?;

    // Then
    assert!(read
        .final_content
        .as_deref()
        .is_some_and(|text| text.contains("revision=1")));
    assert!(edited
        .final_content
        .as_deref()
        .is_some_and(|text| text.contains("revision=2")));
    for (result, status) in [
        (paused, "Paused"),
        (resumed, "Active"),
        (done, "Done"),
        (cleared, "Cleared"),
    ] {
        assert!(result
            .final_content
            .as_deref()
            .is_some_and(|text| text.contains(&format!("status={status}"))));
    }
    Ok(())
}

#[test]
fn ordinary_prose_does_not_create_entrusted_work() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let registry = ToolRegistry::new();
    let provider = MockProvider::new(vec![text_response(
        "Could you clarify whether this should persist?",
    )]);
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &provider,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    // When
    runtime.process_direct("keep an eye on releases maybe", Some("direct:ambiguous"))?;

    // Then
    let raw = runtime
        .session_manager()
        .read_session_file("direct:ambiguous")
        .ok_or("missing session")?;
    assert!(raw["metadata"].get(ENTRUSTED_WORK_METADATA_KEY).is_none());
    assert!(raw["metadata"]
        .get("pending_entrusted_work_proposal")
        .is_none());
    Ok(())
}

#[test]
fn consecutive_work_commands_keep_runtime_recovery_writable() -> Result<(), Box<dyn Error>> {
    // Given
    let workspace = tempfile::tempdir()?;
    let event_root = workspace.path().join("events");
    let registry = ToolRegistry::new();
    let provider = MockProvider::new(Vec::new());
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.durable_event_root = Some(event_root.clone());
    let mut runtime = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &provider,
        config,
    );
    let created = runtime.process_direct("/work create track releases", Some("direct:durable"))?;
    let work_id = created
        .final_content
        .as_deref()
        .and_then(backticked_id)
        .ok_or("missing work id")?;

    // When
    runtime.process_direct(format!("/work read {work_id}"), Some("direct:durable"))?;

    // Then
    let recovery = evaluate_durable_recovery(&event_root, workspace.path().join("checkpoints"));
    assert!(recovery.writable, "{:?}", recovery.issues);
    Ok(())
}
