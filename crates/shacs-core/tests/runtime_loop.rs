use serde_json::{json, Map, Value};
use shacs_core::runtime::{
    build_subagent_tool_registry, format_partial_progress_from_tool_events, ActiveLoopTask,
    AgentLoop, AgentLoopCommandResult, AgentLoopConfig, AgentLoopError, AutoCompact,
    CancellationToken, ChildResultEnvelope, ChildResultStatus, ContextBuilder, DreamLifecycle,
    InboundMessage, LoopTaskRegisterResult, McpLifecycle, MergeDecision, MessageBus,
    ProviderHotSwapResult, ProviderSelectionSnapshot, RuntimeCapabilityStatus, RuntimeContextTools,
    Session, SessionManager, SessionTurnAcquireError, SessionTurnLock, StaticProviderSelector,
    SubagentExecutionConfig, SubagentProgressUpdate, SubagentRuntime, SubagentRuntimeConfig,
    ToolEvent, ToolStatus,
};
use shacs_core::tools::{AskUserTool, MessageTool, SpawnRequest, SpawnTool, ToolRegistry};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest,
    ToolCallRequest,
};
use shacs_utils::gitstore::{GitCliStore, GitStore};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::sync::{Arc, Mutex};

#[test]
fn loop_process_direct_saves_turn_and_publishes_outbound() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("hello back".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("hello", Some("cli:thread-1"))?;
    if result.session_key != "cli:thread-1"
        || result.final_content.as_deref() != Some("hello back")
        || result.outbound_count != 1
    {
        return Err(format!("unexpected loop result: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing outbound")?;
    if outbound.content != "hello back" || outbound.metadata["session_key"] != "cli:thread-1" {
        return Err(format!("unexpected outbound: {outbound:?}").into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:thread-1")
        .ok_or("missing session")?;
    if raw["messages"].as_array().map(Vec::len) != Some(2)
        || raw["messages"][0]["role"] != "user"
        || raw["messages"][0]["content"] != "hello"
        || raw["messages"][1]["role"] != "assistant"
        || raw["messages"][1]["content"] != "hello back"
    {
        return Err(format!("session messages drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_history_command_returns_recent_visible_messages_without_provider_call(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut sessions = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:history");
    session.add_message("user", "alpha", Map::new());
    session.add_message("assistant", "beta", Map::new());
    session.add_message("user", "gamma", Map::new());
    sessions.save_with_fsync(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        sessions,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/history 2", Some("cli:history"))?;

    assert_eq!(result.command, Some(AgentLoopCommandResult::History));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        0
    );
    let outbound = bus.consume_outbound().ok_or("missing history outbound")?;
    assert!(outbound.content.contains("assistant: beta"));
    assert!(outbound.content.contains("user: gamma"));
    assert!(!outbound.content.contains("alpha"));
    Ok(())
}

#[test]
fn loop_invalid_history_and_help_publish_without_provider_call() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let invalid = loop_runtime.process_direct("/history abc", Some("cli:commands"))?;
    assert_eq!(invalid.command, Some(AgentLoopCommandResult::History));
    let invalid_outbound = bus
        .consume_outbound()
        .ok_or("missing invalid history outbound")?;
    assert!(invalid_outbound.content.contains("Usage: /history [n]"));

    let help = loop_runtime.process_direct("/help", Some("cli:commands"))?;
    assert_eq!(help.command, Some(AgentLoopCommandResult::Help));
    let help_outbound = bus.consume_outbound().ok_or("missing help outbound")?;
    assert!(help_outbound.content.contains("/dream-restore"));
    assert!(client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty());
    Ok(())
}

#[test]
fn loop_exact_command_with_extra_text_runs_as_normal_user_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("normal turn".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/status now", Some("cli:direct"))?;

    assert_eq!(result.command, None);
    assert_eq!(result.final_content.as_deref(), Some("normal turn"));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn loop_restart_and_dream_commands_publish_local_responses() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let restart = loop_runtime.process_direct("/restart", Some("cli:commands"))?;
    assert_eq!(
        restart.command,
        Some(AgentLoopCommandResult::RestartRequested)
    );
    assert!(bus
        .consume_outbound()
        .ok_or("missing restart outbound")?
        .content
        .contains("Restart requested"));

    let dream = loop_runtime.process_direct("/dream", Some("cli:commands"))?;
    assert_eq!(dream.command, Some(AgentLoopCommandResult::Dream));
    assert!(bus
        .consume_outbound()
        .ok_or("missing dream outbound")?
        .content
        .contains("Dream idle"));

    let log = loop_runtime.process_direct("/dream-log", Some("cli:commands"))?;
    assert_eq!(log.command, Some(AgentLoopCommandResult::DreamLog));
    assert!(bus
        .consume_outbound()
        .ok_or("missing dream log outbound")?
        .content
        .contains("no saved versions"));

    let restore = loop_runtime.process_direct("/dream-restore", Some("cli:commands"))?;
    assert_eq!(restore.command, Some(AgentLoopCommandResult::DreamRestore));
    assert!(bus
        .consume_outbound()
        .ok_or("missing dream restore outbound")?
        .content
        .contains("no saved versions"));
    Ok(())
}

#[test]
fn loop_dream_log_defaults_to_latest_diff_and_restore_lists_versions() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let git = GitCliStore::new(
        workspace.path(),
        [
            "memory/MEMORY.md".to_owned(),
            "SOUL.md".to_owned(),
            "USER.md".to_owned(),
        ],
    );
    git.init()?;
    std::fs::write(workspace.path().join("memory/MEMORY.md"), "remember this\n")?;
    let sha = git
        .auto_commit("dream: update memory")?
        .ok_or("missing dream commit")?;

    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let log = loop_runtime.process_direct("/dream-log", Some("cli:commands"))?;
    assert_eq!(log.command, Some(AgentLoopCommandResult::DreamLog));
    let log_outbound = bus.consume_outbound().ok_or("missing dream log outbound")?;
    assert!(log_outbound
        .content
        .contains("Here is the latest Dream memory change"));
    assert!(log_outbound.content.contains("```diff"));
    assert!(log_outbound.content.contains("remember this"));

    let restore = loop_runtime.process_direct("/dream-restore", Some("cli:commands"))?;
    assert_eq!(restore.command, Some(AgentLoopCommandResult::DreamRestore));
    let restore_outbound = bus
        .consume_outbound()
        .ok_or("missing dream restore outbound")?;
    assert!(restore_outbound.content.contains("## Dream Restore"));
    assert!(restore_outbound.content.contains(&sha));
    Ok(())
}

#[test]
fn loop_consolidates_over_budget_session_before_building_context_and_preserves_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("old turn summary".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("fresh answer".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("direct:thread");
    session.add_message("user", "old question ".repeat(900), Map::new());
    session.add_message("assistant", "old answer ".repeat(900), Map::new());
    session.add_message("user", "recent question", Map::new());
    session.add_message("assistant", "recent answer", Map::new());
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.context_window_tokens = Some(2_200);
    config.settings = GenerationSettings {
        max_tokens: 1,
        ..GenerationSettings::default()
    };
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    let result = loop_runtime.process_direct("fresh question", Some("direct:thread"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("direct:thread")
        .ok_or("missing consolidated session")?;
    let history = shacs_core::runtime::MemoryStore::new(workspace.path())?.read_entries();
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let archive_prompt = requests
        .first()
        .and_then(|request| request.messages.get(1))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if result.final_content.as_deref() != Some("fresh answer")
        || raw["last_consolidated"] != 2
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
        || raw["metadata"]["_last_summary"]["text"] != "old turn summary"
        || history.first().map(|entry| entry.content.as_str()) != Some("old turn summary")
        || requests.len() != 2
        || !archive_prompt.contains("truncated")
        || archive_prompt.chars().count() > 820
    {
        return Err(format!(
            "loop token consolidation drifted: result={result:?} raw={raw:?} history={history:?} archive_prompt_len={}",
            archive_prompt.chars().count()
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_token_consolidation_raw_fallback_advances_cursor_and_keeps_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![
        LlmResponse {
            content: Some("provider refused".to_owned()),
            finish_reason: "error".to_owned(),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("fresh answer".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("direct:thread");
    session.add_message("user", "old question ".repeat(900), Map::new());
    session.add_message("assistant", "old answer ".repeat(900), Map::new());
    session.add_message("user", "recent question", Map::new());
    session.add_message("assistant", "recent answer", Map::new());
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.context_window_tokens = Some(2_200);
    config.settings = GenerationSettings {
        max_tokens: 1,
        ..GenerationSettings::default()
    };
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );

    loop_runtime.process_direct("fresh question", Some("direct:thread"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("direct:thread")
        .ok_or("missing fallback session")?;
    let history = shacs_core::runtime::MemoryStore::new(workspace.path())?.read_entries();
    if raw["last_consolidated"] != 2
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
        || raw["metadata"].get("_last_summary").is_some()
        || history.len() != 1
        || !history[0].content.contains("[RAW] 2 messages")
        || !history[0].content.contains("truncated")
    {
        return Err(format!(
            "loop raw fallback consolidation drifted: raw={raw:?} history={history:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_priority_new_clears_session_and_publishes_without_provider() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:direct");
    session.add_message("user", "old", Map::new());
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/new", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::NewSession) || result.outbound_count != 1 {
        return Err(format!("/new result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /new outbound")?;
    if !outbound.content.contains("new session") {
        return Err(format!("/new outbound drifted: {outbound:?}").into());
    }
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:direct")
        .ok_or("missing cleared session")?;
    if raw["messages"].as_array().map(Vec::is_empty) != Some(true) {
        return Err(format!("/new did not clear session: {raw:?}").into());
    }
    if !client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("/new should not call provider".into());
    }
    Ok(())
}

#[test]
fn loop_priority_new_cancels_registered_task_before_clearing_session() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    let register_result = loop_task_registry.register(ActiveLoopTask::new(
        "cli:direct",
        "task-1",
        cancellation.clone(),
    ));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    let result = loop_runtime.process_direct("/new", Some("cli:direct"))?;

    assert_eq!(result.command, Some(AgentLoopCommandResult::NewSession));
    assert!(cancellation.is_cancelled());
    Ok(())
}

#[test]
fn loop_priority_status_publishes_without_provider_call() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/status", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::Status) {
        return Err(format!("/status result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /status outbound")?;
    if !outbound.content.contains("no active task") {
        return Err(format!("/status outbound drifted: {outbound:?}").into());
    }
    if !client
        .requests
        .lock()
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("/status should not call provider".into());
    }
    Ok(())
}

#[test]
fn loop_priority_status_reports_registered_async_task() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let register_result = loop_task_registry.register(ActiveLoopTask::new(
        "cli:direct",
        "task-1",
        CancellationToken::new(),
    ));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    let result = loop_runtime.process_direct("/status", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::Status) {
        return Err(format!("/status result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /status outbound")?;
    if !outbound.content.contains("active async task task-1") {
        return Err(format!("/status should report registered task: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn session_turn_lock_rejects_duplicate_active_session() -> Result<(), Box<dyn Error>> {
    let lock = SessionTurnLock::new();
    let guard = lock
        .acquire("cli:direct")
        .map_err(|error| format!("first acquire should succeed: {error:?}"))?;
    let duplicate = lock.acquire("cli:direct");
    if !matches!(
        duplicate,
        Err(SessionTurnAcquireError::AlreadyActive { ref session_key }) if session_key == "cli:direct"
    ) {
        return Err(format!("duplicate acquire should fail: {duplicate:?}").into());
    }
    if lock.active_session_keys() != ["cli:direct".to_owned()] {
        return Err("active session was not tracked".into());
    }
    drop(guard);
    if lock.acquire("cli:direct").is_err() {
        return Err("guard drop should release active session".into());
    }
    Ok(())
}

#[test]
fn stop_without_async_task_preserves_current_message() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    if result.command != Some(AgentLoopCommandResult::StopRequested) {
        return Err(format!("/stop result drifted: {result:?}").into());
    }
    let outbound = bus.consume_outbound().ok_or("missing /stop outbound")?;
    if !outbound.content.contains("No async task is running") {
        return Err(format!("/stop message should preserve no-task text: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn stop_without_async_task_does_not_block_next_user_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("after stop".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    let _ = bus.consume_outbound().ok_or("missing /stop outbound")?;
    let result = loop_runtime.process_direct("hello again", Some("cli:direct"))?;

    assert_eq!(result.command, None);
    assert_eq!(result.final_content.as_deref(), Some("after stop"));
    assert_eq!(
        client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn stop_requests_cancel_for_registered_task() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    let register_result = loop_task_registry.register(ActiveLoopTask::new(
        "cli:direct",
        "task-1",
        cancellation.clone(),
    ));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    let outbound = bus.consume_outbound().ok_or("missing /stop outbound")?;
    if !cancellation.is_cancelled() || !outbound.content.contains("Cancellation requested") {
        return Err(format!("/stop should request cancellation only: {outbound:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_result_with_wrong_child_id_is_stale() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let expected = shacs_core::runtime::SpawnEnvelope::new("cli:parent", "child-a", "inspect");
    let result = ChildResultEnvelope::new("cli:parent", "child-b", "done");

    let decision = runtime.classify_result(&expected, &result);
    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("child id mismatch"))
    {
        return Err(format!("wrong child id should be stale: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_result_with_matching_parent_and_child_accepts_summary() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let expected = shacs_core::runtime::SpawnEnvelope::new("cli:parent", "child-a", "inspect");
    let result = ChildResultEnvelope::new("cli:parent", "child-a", "done");

    let decision = runtime.classify_result(&expected, &result);
    if decision != MergeDecision::AcceptSummaryOnly {
        return Err(format!("matching child result should be accepted: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_result_with_wrong_parent_session_is_stale() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let expected = shacs_core::runtime::SpawnEnvelope::new("cli:parent", "child-a", "inspect");
    let result = ChildResultEnvelope::new("cli:other", "child-a", "done");

    let decision = runtime.classify_result(&expected, &result);
    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("parent session mismatch"))
    {
        return Err(format!("wrong parent session should be stale: {decision:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_spawn_registers_active_task_and_cancels_by_session() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect docs".to_owned(),
        label: Some("docs".to_owned()),
        origin_channel: "telegram".to_owned(),
        origin_chat_id: "chat-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;

    if runtime.running_count() != 1
        || runtime.running_count_by_session("session-1") != 1
        || !outcome.user_message.contains("Subagent [docs] started")
    {
        return Err(format!("subagent spawn tracking drifted: {outcome:?}").into());
    }
    let status = runtime
        .snapshot(&outcome.envelope.child_task_id)
        .ok_or("missing subagent status")?;
    if status.label != "docs" || status.task_description != "Inspect docs" {
        return Err(format!("subagent status drifted: {status:?}").into());
    }
    let status = runtime
        .update_progress(
            &outcome.envelope.child_task_id,
            SubagentProgressUpdate {
                phase: "awaiting_tools".to_owned(),
                iteration: 2,
                tool_events: vec![json!({"name":"read_file","status":"ok"})],
                usage: json!({"input_tokens": 10}),
                error: None,
            },
        )
        .ok_or("missing updated subagent status")?;
    if status.iteration != 2
        || status.tool_events.len() != 1
        || status.usage["input_tokens"] != 10
        || status.state != shacs_core::runtime::SubagentState::Running
    {
        return Err(format!("subagent progress update drifted: {status:?}").into());
    }
    if runtime.cancel_by_session("session-1") != 1 {
        return Err("cancel_by_session should cancel one active child".into());
    }
    let status = runtime
        .snapshot(&outcome.envelope.child_task_id)
        .ok_or("missing cancelled subagent status")?;
    if status.state != shacs_core::runtime::SubagentState::Cancelled {
        return Err(format!("cancelled status drifted: {status:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_spawn_inherits_snapshot_contract() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect docs".to_owned(),
        label: Some("docs".to_owned()),
        origin_channel: "telegram".to_owned(),
        origin_chat_id: "chat-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;

    if outcome.envelope.inherited_context_snapshot["origin_channel"] != "telegram"
        || outcome.envelope.inherited_context_snapshot["origin_chat_id"] != "chat-1"
        || outcome.envelope.inherited_policy_snapshot["capability_ceiling"] != "parent"
        || outcome.envelope.parent_turn_id != "turn:session-1"
        || outcome.envelope.parallelism_group != "session-1"
    {
        return Err(format!("subagent spawn snapshots drifted: {outcome:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_finish_publishes_synthetic_inbound_and_closes_active_task() -> Result<(), Box<dyn Error>>
{
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: None,
        origin_channel: "slack".to_owned(),
        origin_chat_id: "thread-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let result = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Runtime summary",
    );

    let decision = runtime.publish_child_result(result);
    if decision != MergeDecision::AcceptSummaryOnly || runtime.running_count() != 0 {
        return Err(format!("subagent finish drifted: {decision:?}").into());
    }
    let inbound = bus
        .consume_inbound()
        .ok_or("missing synthetic subagent inbound")?;
    if inbound.channel != "system"
        || inbound.sender_id != "subagent"
        || inbound.session_key_override.as_deref() != Some("session-1")
        || inbound.metadata["injected_event"] != "subagent_result"
        || inbound.metadata["subagent_task_id"] != outcome.envelope.child_task_id
        || !inbound
            .content
            .contains("[Subagent 'Summarize runtime' completed successfully]")
        || !inbound.content.contains("Task: Summarize runtime")
        || !inbound.content.contains("Runtime summary")
        || inbound.content.contains("Merge decision")
    {
        return Err(format!("synthetic subagent inbound drifted: {inbound:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_run_spawn_executes_agent_and_publishes_result() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let skill_dir = workspace.path().join("skills").join("configured-env");
    std::fs::create_dir_all(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\ndescription: Configured env skill\nrequires.env: SHACS_SUBAGENT_TEST_CONFIGURED_ENV_ONLY\n---\nUse configured env.\n",
    )?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: Some("runtime".to_owned()),
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "cli:direct".to_owned(),
    })?;
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("subagent done".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut config = SubagentExecutionConfig::new(workspace.path(), "test-model");
    config.exec_env = BTreeMap::from([(
        "SHACS_SUBAGENT_TEST_CONFIGURED_ENV_ONLY".to_owned(),
        "configured".to_owned(),
    )]);
    let result = runtime.run_spawn(outcome.envelope.clone(), &client, config);
    if result.status != ChildResultStatus::Completed
        || result.summary != "subagent done"
        || runtime.running_count() != 0
    {
        return Err(format!("subagent run_spawn drifted: result={result:?}").into());
    }
    let inbound = bus
        .consume_inbound()
        .ok_or("missing run_spawn synthetic inbound")?;
    if !inbound.content.contains("subagent done")
        || inbound.session_key_override.as_deref() != Some("cli:direct")
    {
        return Err(format!("run_spawn announcement drifted: {inbound:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let system_prompt = requests[0]
        .messages
        .first()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if requests.len() != 1
        || requests[0].model != "test-model"
        || !requests[0]
            .tools
            .iter()
            .any(|tool| tool.to_string().contains("read_file"))
        || requests[0]
            .tools
            .iter()
            .any(|tool| tool.to_string().contains("spawn"))
        || !system_prompt.contains("# Subagent")
        || !system_prompt.contains("**configured-env**")
        || system_prompt.contains("SHACS_SUBAGENT_TEST_CONFIGURED_ENV_ONLY")
    {
        return Err(format!("run_spawn provider request drifted: {requests:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_cancel_before_run_cleans_without_announcement() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Long task".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "cli:direct".to_owned(),
    })?;
    if runtime.cancel_by_session("cli:direct") != 1 {
        return Err("cancel_by_session should cancel spawned child".into());
    }
    let client = MockProvider::new(Vec::new());
    let result = runtime.run_spawn(
        outcome.envelope,
        &client,
        SubagentExecutionConfig::new(workspace.path(), "test-model"),
    );
    if result.status != ChildResultStatus::Cancelled
        || runtime.running_count() != 0
        || bus.try_consume_inbound().is_some()
    {
        return Err(format!("cancelled subagent cleanup drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_tool_registry_excludes_parent_only_tools() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut config = SubagentExecutionConfig::new(workspace.path(), "test-model");
    config.allow_side_effect_tools = true;
    config.enable_exec = true;
    config.enable_web = true;
    let registry = build_subagent_tool_registry(&config);
    for expected in [
        "read_file",
        "write_file",
        "edit_file",
        "list_dir",
        "glob",
        "grep",
        "exec",
        "web_fetch",
        "web_search",
    ] {
        if !registry.has(expected) {
            return Err(format!("subagent registry missing {expected}").into());
        }
    }
    for forbidden in ["spawn", "message", "ask_user", "my", "cron"] {
        if registry.has(forbidden) {
            return Err(format!("subagent registry should exclude {forbidden}").into());
        }
    }
    Ok(())
}

#[test]
fn subagent_partial_progress_formats_completed_steps_and_failure() -> Result<(), Box<dyn Error>> {
    let events = vec![
        ToolEvent {
            name: "read".to_owned(),
            status: ToolStatus::Ok,
            detail: "read docs".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "grep".to_owned(),
            status: ToolStatus::Ok,
            detail: "found patterns".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "write".to_owned(),
            status: ToolStatus::Ok,
            detail: "drafted patch".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "clippy".to_owned(),
            status: ToolStatus::Ok,
            detail: "checked".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
        ToolEvent {
            name: "test".to_owned(),
            status: ToolStatus::Error,
            detail: "failed assertion".to_owned(),
            call_id: None,
            arguments: None,
            result: None,
        },
    ];
    let progress = format_partial_progress_from_tool_events(&events, None);
    if progress.contains("read docs")
        || !progress.contains("Completed steps:")
        || !progress.contains("- grep: found patterns")
        || !progress.contains("- write: drafted patch")
        || !progress.contains("- clippy: checked")
        || !progress.contains("Failure:")
        || !progress.contains("- test: failed assertion")
    {
        return Err(format!("subagent partial progress drifted: {progress}").into());
    }
    Ok(())
}

#[test]
fn subagent_stale_result_does_not_publish_or_close_active_child() -> Result<(), Box<dyn Error>> {
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Summarize runtime".to_owned(),
        label: None,
        origin_channel: "slack".to_owned(),
        origin_chat_id: "thread-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let mut stale = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "Wrong turn summary",
    );
    stale.parent_turn_id = "turn:stale".to_owned();

    let decision = runtime.publish_child_result(stale);
    if !matches!(&decision, MergeDecision::DiscardAsStale { reason } if reason.contains("parent turn mismatch"))
        || runtime.running_count() != 1
        || bus.try_consume_inbound().is_some()
    {
        return Err(
            format!("stale result should not publish or close active child: {decision:?}").into(),
        );
    }
    let active = runtime
        .snapshot(&outcome.envelope.child_task_id)
        .ok_or("stale result should leave active child available")?;
    if active.state != shacs_core::runtime::SubagentState::Spawned {
        return Err(format!("stale result should not mutate active state: {active:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_stale_inbound_is_not_persisted_as_session_content() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let runtime = SubagentRuntime::with_bus(bus.clone());
    let outcome = runtime.spawn_from_request(SpawnRequest {
        task: "Inspect stale".to_owned(),
        label: None,
        origin_channel: "slack".to_owned(),
        origin_chat_id: "thread-1".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let mut stale = ChildResultEnvelope::from_spawn(
        &outcome.envelope,
        ChildResultStatus::Completed,
        "SHOULD_NOT_PERSIST",
    );
    stale.spawn_effect_id = "spawn:stale".to_owned();
    let decision = runtime.publish_child_result(stale);
    if !matches!(decision, MergeDecision::DiscardAsStale { .. })
        || bus.try_consume_inbound().is_some()
    {
        return Err("stale result should stay off the AgentLoop inbound path".into());
    }

    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("normal reply".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let normal = InboundMessage::new("cli", "user", "direct", "hello");
    loop_runtime.process_message(normal)?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:direct")
        .ok_or("missing session")?;
    if raw.to_string().contains("SHOULD_NOT_PERSIST") {
        return Err(format!("stale subagent result leaked into session: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn subagent_parallelism_limit_rejects_excess_children() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::with_config(SubagentRuntimeConfig { max_parallelism: 1 });
    runtime.spawn_from_request(SpawnRequest {
        task: "first".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    })?;
    let second = runtime.spawn_from_request(SpawnRequest {
        task: "second".to_owned(),
        label: None,
        origin_channel: "cli".to_owned(),
        origin_chat_id: "direct".to_owned(),
        session_key: "session-1".to_owned(),
    });
    if !matches!(second, Err(ref error) if error.contains("parallelism limit")) {
        return Err(format!("parallelism limit should reject second child: {second:?}").into());
    }
    Ok(())
}

#[test]
fn spawn_tool_can_delegate_to_subagent_runtime() -> Result<(), Box<dyn Error>> {
    let runtime = SubagentRuntime::new();
    let tool = SpawnTool::new(Arc::new(runtime.clone()));
    tool.set_context("telegram", "chat-1", Some("session-1".to_owned()));
    let result = shacs_core::tools::Tool::execute(
        &tool,
        Map::from_iter([("task".to_owned(), json!("Inspect workspace"))]),
    )
    .into_text();
    if !result.contains("Subagent [Inspect workspace] started")
        || runtime.running_count_by_session("session-1") != 1
    {
        return Err(format!("spawn tool/runtime integration drifted: {result}").into());
    }
    Ok(())
}

#[test]
fn loop_lifecycle_reports_structured_status() -> Result<(), Box<dyn Error>> {
    let reports = [McpLifecycle::new().status(), DreamLifecycle::new().status()];
    let components = reports
        .iter()
        .map(|report| report.component.as_str())
        .collect::<Vec<_>>();
    if components != ["mcp_lifecycle", "dream_lifecycle"] {
        return Err(format!("lifecycle component names drifted: {reports:?}").into());
    }
    if reports[0].status != RuntimeCapabilityStatus::Unavailable
        || reports[1].status != RuntimeCapabilityStatus::Unavailable
        || reports.iter().any(|report| report.reason.trim().is_empty())
        || McpLifecycle::from_counts(2, 1, 1).status().status != RuntimeCapabilityStatus::Available
        || DreamLifecycle::configured().status().status != RuntimeCapabilityStatus::Available
    {
        return Err(format!("lifecycle status reports drifted: {reports:?}").into());
    }
    let subagent_status = SubagentRuntime::new().status();
    if subagent_status.component != "subagent_runtime"
        || subagent_status.status != RuntimeCapabilityStatus::Available
        || !subagent_status.reason.contains("synthetic reentry")
    {
        return Err(format!("subagent runtime status drifted: {subagent_status:?}").into());
    }
    Ok(())
}

#[test]
fn static_provider_selector_rejects_hot_swap_without_mutating_current_turn(
) -> Result<(), Box<dyn Error>> {
    let current = ProviderSelectionSnapshot::new("openai", "gpt-5");
    let mut selector = StaticProviderSelector::new(current.clone());
    let result = selector.request_hot_swap(ProviderSelectionSnapshot::new("anthropic", "claude"));

    if result
        != (ProviderHotSwapResult::Unsupported {
            current: current.clone(),
        })
        || selector.select_snapshot() != current
    {
        return Err(format!("provider hot-swap contract drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_ask_user_interrupt_publishes_buttons_and_resumes_as_tool_result(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "ask-1",
                "ask_user",
                Map::from_iter([
                    ("question".to_owned(), json!("Continue?")),
                    ("options".to_owned(), json!(["Yes", "No"])),
                ]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("resumed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let first = loop_runtime.process_direct("start", Some("cli:ask"))?;
    if first.stop_reason != "ask_user" || first.ask_user_options != ["Yes", "No"] {
        return Err(format!("ask interrupt result drifted: {first:?}").into());
    }
    let ask_outbound = bus.consume_outbound().ok_or("missing ask outbound")?;
    if !ask_outbound.content.contains("1. Yes") || !ask_outbound.buttons.is_empty() {
        return Err(format!("ask outbound should render plain options: {ask_outbound:?}").into());
    }

    let second = loop_runtime.process_direct("Yes", Some("cli:ask"))?;
    if second.final_content.as_deref() != Some("resumed") {
        return Err(format!("ask resume result drifted: {second:?}").into());
    }
    let _final_outbound = bus.consume_outbound().ok_or("missing resumed outbound")?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:ask")
        .ok_or("missing ask session")?;
    if !raw["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|message| {
            message["role"] == "tool"
                && message["name"] == "ask_user"
                && message["tool_call_id"] == "ask-1"
                && message["content"] == "Yes"
        })
    {
        return Err(format!("ask answer was not persisted as tool result: {raw:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if !requests
        .get(1)
        .into_iter()
        .flat_map(|request| &request.messages)
        .any(|message| {
            message["role"] == "tool"
                && message["tool_call_id"] == "ask-1"
                && message["content"] == "Yes"
        })
    {
        return Err(
            format!("ask answer was not sent to provider as tool result: {requests:?}").into(),
        );
    }
    let resume_request = requests.get(1).ok_or("missing resume request")?;
    let last_message = resume_request
        .messages
        .last()
        .ok_or("resume request should include messages")?;
    if last_message["role"] != "tool"
        || last_message["tool_call_id"] != "ask-1"
        || last_message["content"] != "Yes"
    {
        return Err(format!("ask resume request suffix drifted: {resume_request:?}").into());
    }
    Ok(())
}

#[test]
fn loop_pending_user_turn_recovery_closes_interrupted_prior_turn() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("new reply".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:recover");
    session.add_message("user", "unfinished", Map::new());
    session
        .metadata
        .insert("pending_user_turn".to_owned(), Value::Bool(true));
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("next", Some("cli:recover"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:recover")
        .ok_or("missing recovered session")?;
    if raw["metadata"].get("pending_user_turn").is_some()
        || !raw["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| {
                message["role"] == "assistant"
                    && message["_interrupted"] == true
                    && message["content"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("interrupted")
            })
    {
        return Err(format!("pending turn was not recovered: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_runtime_checkpoint_materializes_placeholders_and_clears_metadata(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:checkpoint");
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({
            "phase": "awaiting_tools",
            "assistant_message": {
                "role": "assistant",
                "content": "using tools",
                "tool_calls": [
                    {"id": "done", "type": "function", "function": {"name": "done_tool", "arguments": "{}"}},
                    {"id": "pending", "type": "function", "function": {"name": "pending_tool", "arguments": "{}"}}
                ]
            },
            "completed_tool_results": [
                {"tool_call_id": "done", "name": "done_tool", "content": "ok"}
            ],
            "pending_tool_calls": [
                {"id": "pending", "type": "function", "function": {"name": "pending_tool", "arguments": "{}"}}
            ]
        }),
    );
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/status", Some("cli:checkpoint"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:checkpoint")
        .ok_or("missing checkpoint session")?;
    let messages = raw["messages"]
        .as_array()
        .ok_or("messages should be array")?;
    if raw["metadata"].get("runtime_checkpoint").is_some()
        || messages.len() != 3
        || messages[0]["tool_calls"].as_array().map(Vec::len) != Some(2)
        || messages[1]["tool_call_id"] != "done"
        || messages[2]["tool_call_id"] != "pending"
        || !messages[2]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("interrupted or lost")
    {
        return Err(format!("checkpoint materialization drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn auto_compact_skips_active_sessions_and_preserves_checkpointed_agent_config(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:compact");
    session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(10)).to_rfc3339();
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({"phase": "awaiting_tools", "assistant_message": {"content": "using tools"}}),
    );
    session.metadata.insert(
        "agent_configuration".to_owned(),
        json!({"model": "test-model", "provider": "mock"}),
    );
    for index in 0..12 {
        session.add_message("user", format!("message {index}"), Map::new());
    }
    session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(10)).to_rfc3339();
    manager.save(&session)?;

    let mut compact = AutoCompact::new(1);
    let skipped = compact.mark_expired_sessions(&manager, ["cli:compact".to_owned()])?;
    if !skipped.is_empty() || compact.is_archiving("cli:compact") {
        return Err(format!("active compact session should be skipped: {skipped:?}").into());
    }

    let expired = compact.mark_expired_sessions(&manager, Vec::<String>::new())?;
    if expired != vec!["cli:compact".to_owned()] || !compact.is_archiving("cli:compact") {
        return Err(format!("expired compact session not marked: {expired:?}").into());
    }

    let outcome =
        compact.archive_session_with_summary(&mut manager, "cli:compact", Some("summary"))?;
    let raw = manager
        .read_session_file("cli:compact")
        .ok_or("missing compacted session")?;
    if outcome.archived_messages.len() != 4
        || outcome.kept_messages.len() != 8
        || raw["messages"].as_array().map(Vec::len) != Some(8)
        || raw["metadata"]["runtime_checkpoint"]["phase"] != "awaiting_tools"
        || raw["metadata"]["agent_configuration"]["model"] != "test-model"
        || raw["metadata"].get("_last_summary").is_none()
        || raw["last_consolidated"].as_u64().unwrap_or_default() != 0
    {
        return Err(
            format!("auto compact archive drifted: outcome={outcome:?} raw={raw:?}").into(),
        );
    }

    let loaded = manager.get_or_create("cli:compact");
    let (prepared, summary) = compact.prepare_session(&mut manager, loaded, "cli:compact")?;
    if !summary
        .as_deref()
        .unwrap_or_default()
        .contains("Previous conversation summary: summary")
        || prepared.metadata.get("_last_summary").is_some()
        || prepared.metadata["runtime_checkpoint"]["phase"] != "awaiting_tools"
        || prepared.metadata["agent_configuration"]["provider"] != "mock"
    {
        return Err(format!(
            "auto compact prepare lost metadata: summary={summary:?} prepared={prepared:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_consumes_auto_compact_summary_when_building_context() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("fresh answer".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:compact-summary");
    session.add_message("user", "old", Map::new());
    session.metadata.insert(
        "_last_summary".to_owned(),
        json!({"text": "archived facts", "last_active": chrono::Local::now().to_rfc3339()}),
    );
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_auto_compact(AutoCompact::new(60));

    let result = loop_runtime.process_direct("fresh", Some("cli:compact-summary"))?;
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let prompt = requests
        .first()
        .and_then(|request| request.messages.last())
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:compact-summary")
        .ok_or("missing prepared session")?;
    if result.final_content.as_deref() != Some("fresh answer")
        || !prompt.contains("Previous conversation summary: archived facts")
        || raw["metadata"].get("_last_summary").is_some()
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
    {
        return Err(format!(
            "loop autocompact summary drifted: result={result:?} prompt={prompt:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_idle_auto_compact_archives_expired_sessions_with_provider_summary(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("idle summary".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:idle");
    for index in 0..12 {
        session.add_message("user", format!("idle message {index}"), Map::new());
    }
    session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(5)).to_rfc3339();
    session
        .metadata
        .insert("runtime_checkpoint".to_owned(), json!({"phase": "kept"}));
    session
        .metadata
        .insert("agent_configuration".to_owned(), json!({"model": "kept"}));
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_auto_compact(AutoCompact::new(1));

    let outcomes = loop_runtime.run_idle_auto_compact()?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:idle")
        .ok_or("missing idle compact session")?;
    let history = shacs_core::runtime::MemoryStore::new(workspace.path())?.read_entries();
    if outcomes.len() != 1
        || outcomes[0].archived_messages.len() != 4
        || raw["messages"].as_array().map(Vec::len) != Some(8)
        || raw["metadata"]["_last_summary"]["text"] != "idle summary"
        || raw["metadata"]["runtime_checkpoint"]["phase"] != "kept"
        || raw["metadata"]["agent_configuration"]["model"] != "kept"
        || history.first().map(|entry| entry.content.as_str()) != Some("idle summary")
    {
        return Err(format!(
            "idle autocompact drifted: outcomes={outcomes:?} raw={raw:?} history={history:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_idle_auto_compact_releases_all_markers_on_batch_failure() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    for key in ["cli:first", "cli:second"] {
        let mut session = Session::new(key);
        for index in 0..12 {
            session.add_message("user", format!("old {index}"), Map::new());
        }
        session.updated_at = (chrono::Local::now() - chrono::Duration::minutes(5)).to_rfc3339();
        manager.save(&session)?;
    }
    std::fs::create_dir_all(workspace.path().join("memory/history.jsonl"))?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_auto_compact(AutoCompact::new(1));

    let result = loop_runtime.run_idle_auto_compact();
    if result.is_ok() {
        return Err(format!(
            "first idle compaction should fail without mock responses: result={result:?}"
        )
        .into());
    }
    std::fs::remove_dir(workspace.path().join("memory/history.jsonl"))?;
    client.push_response(LlmResponse {
        content: Some("first summary".to_owned()),
        ..LlmResponse::default()
    })?;
    client.push_response(LlmResponse {
        content: Some("second summary".to_owned()),
        ..LlmResponse::default()
    })?;
    let retried = loop_runtime.run_idle_auto_compact()?;
    if retried.len() != 2 {
        return Err(format!(
            "failed batch should release all archiving markers for retry: retried={retried:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_rejects_duplicate_active_turn_for_same_session() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("cli:direct")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result = loop_runtime.process_direct("hello", Some("cli:direct"));
    if !matches!(
        result,
        Err(AgentLoopError::DuplicateActiveTurn { ref session_key }) if session_key == "cli:direct"
    ) {
        return Err(format!("duplicate active turn should fail: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_priority_status_bypasses_active_session_lock() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let mut sessions = SessionManager::new(workspace.path())?;
    let mut active_session = Session::new("telegram:chat-1");
    active_session
        .metadata
        .insert("pending_user_turn".to_owned(), json!(true));
    sessions.save(&active_session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        sessions,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result = loop_runtime.process_message(InboundMessage::new(
        "telegram", "user-1", "chat-1", "/status",
    ))?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::Status));
    assert_eq!(result.stop_reason, "status");
    let raw = loop_runtime
        .session_manager()
        .read_session_file("telegram:chat-1")
        .ok_or("missing active session")?;
    assert_eq!(raw["metadata"]["pending_user_turn"], true);
    assert_eq!(raw["messages"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn loop_new_command_recovers_from_stopped_state() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/stop", Some("cli:direct"))?;
    let _ = bus.consume_outbound().ok_or("missing stop outbound")?;
    let result = loop_runtime.process_direct("/new", Some("cli:direct"))?;
    assert_eq!(result.command, Some(AgentLoopCommandResult::NewSession));
    assert_eq!(result.stop_reason, "new_session");
    Ok(())
}

#[test]
fn loop_exact_commands_do_not_bypass_active_session_lock() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let turn_lock = SessionTurnLock::new();
    let _guard = turn_lock
        .acquire("telegram:chat-1")
        .map_err(|error| format!("test lock acquire failed: {error:?}"))?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_session_turn_lock(turn_lock);

    let result =
        loop_runtime.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "/new"));
    assert!(matches!(
        result,
        Err(AgentLoopError::DuplicateActiveTurn { ref session_key }) if session_key == "telegram:chat-1"
    ));
    Ok(())
}

#[test]
fn loop_observes_registered_cancellation_token_before_provider_call() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let loop_task_registry = shacs_core::runtime::LoopTaskRegistry::new();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let register_result =
        loop_task_registry.register(ActiveLoopTask::new("cli:cancelled", "task-1", cancellation));
    if register_result != LoopTaskRegisterResult::Registered {
        return Err(format!("task registration drifted: {register_result:?}").into());
    }
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_loop_task_registry(loop_task_registry);

    let result = loop_runtime.process_direct("hello", Some("cli:cancelled"))?;
    if result.stop_reason != "cancelled"
        || result.final_content.as_deref() != Some("Turn cancelled before completion.")
        || !client
            .requests
            .lock()
            .map_err(|error| error.to_string())?
            .is_empty()
    {
        return Err(format!("cancelled turn drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_preserves_channel_chat_and_session_key_in_tool_context() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("inspect"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    let mut inbound = InboundMessage::new("telegram", "user-1", "chat-1", "go");
    inbound.session_key_override = Some("thread-42".to_owned());

    loop_runtime.process_message(inbound)?;
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let request = captured.first().ok_or("spawn was not called")?;
    if request.origin_channel != "telegram"
        || request.origin_chat_id != "chat-1"
        || request.session_key != "thread-42"
    {
        return Err(format!("tool context drifted: {request:?}").into());
    }
    Ok(())
}

#[test]
fn loop_explicit_session_override_wins_over_unified_session() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("ok".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.unified_session_key = Some("unified".to_owned());
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    );
    let mut inbound = InboundMessage::new("slack", "user-1", "chat-1", "hello");
    inbound.session_key_override = Some("slack:thread-1".to_owned());

    let result = loop_runtime.process_message(inbound)?;
    if result.session_key != "slack:thread-1"
        || loop_runtime
            .session_manager()
            .read_session_file("unified")
            .is_some()
    {
        return Err(format!("session override precedence drifted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_deserialized_session_override_is_ignored() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(vec![LlmResponse {
        content: Some("ok".to_owned()),
        ..LlmResponse::default()
    }]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );
    let inbound: InboundMessage = serde_json::from_value(json!({
        "channel": "slack",
        "sender_id": "user-1",
        "chat_id": "chat-1",
        "content": "hello",
        "session_key_override": "attacker:chosen"
    }))?;

    let result = loop_runtime.process_message(inbound)?;
    if result.session_key != "slack:chat-1"
        || loop_runtime
            .session_manager()
            .read_session_file("attacker:chosen")
            .is_some()
    {
        return Err(format!("deserialized override was trusted: {result:?}").into());
    }
    Ok(())
}

#[test]
fn loop_message_tool_delivery_suppresses_final_and_blocks_cross_target(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let message_tool = MessageTool::new(workspace.path());
    let mut registry = ToolRegistry::new();
    registry.register(message_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "msg-1",
                "message",
                Map::from_iter([("content".to_owned(), json!("tool says hi"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("final should be suppressed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_message_tool_delivery(message_tool);

    let result =
        loop_runtime.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
    if result.outbound_count != 0 {
        return Err(format!("final outbound should be suppressed: {result:?}").into());
    }
    let outbound = bus
        .consume_outbound()
        .ok_or("missing message tool outbound")?;
    if outbound.content != "tool says hi" || bus.consume_outbound().is_some() {
        return Err(format!("message tool outbound drifted: {outbound:?}").into());
    }

    let multi_bus = MessageBus::new();
    let multi_tool = MessageTool::new(workspace.path());
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut multi_registry = ToolRegistry::new();
    multi_registry.register(multi_tool.clone());
    multi_registry.register(spawn_tool.clone());
    let multi_client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "msg-3",
                "message",
                Map::from_iter([("content".to_owned(), json!("first iteration message"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-after-message",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("continue"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("multi-iteration final should be suppressed".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut multi_loop = AgentLoop::new(
        multi_bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &multi_registry,
        &multi_client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool))
    .with_message_tool_delivery(multi_tool);
    let multi_result =
        multi_loop.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
    if multi_result.outbound_count != 0 {
        return Err(format!(
            "multi-iteration final outbound should be suppressed: {multi_result:?}"
        )
        .into());
    }
    let multi_outbound = multi_bus
        .consume_outbound()
        .ok_or("missing multi-iteration message outbound")?;
    if multi_outbound.content != "first iteration message" || multi_bus.consume_outbound().is_some()
    {
        return Err(
            format!("multi-iteration message suppression drifted: {multi_outbound:?}").into(),
        );
    }

    let guarded_bus = MessageBus::new();
    let guarded_tool = MessageTool::new(workspace.path());
    let mut guarded_registry = ToolRegistry::new();
    guarded_registry.register(guarded_tool.clone());
    let guarded_client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "msg-2",
                "message",
                Map::from_iter([
                    ("content".to_owned(), json!("wrong target")),
                    ("chat_id".to_owned(), json!("other-chat")),
                ]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("guarded final".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut guarded_loop = AgentLoop::new(
        guarded_bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &guarded_registry,
        &guarded_client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_message_tool_delivery(guarded_tool);
    guarded_loop.process_message(InboundMessage::new("telegram", "user-1", "chat-1", "go"))?;
    let guarded_outbound = guarded_bus
        .consume_outbound()
        .ok_or("missing guarded final outbound")?;
    if guarded_outbound.content != "guarded final" || guarded_bus.consume_outbound().is_some() {
        return Err(format!("cross-target guard drifted: {guarded_outbound:?}").into());
    }
    Ok(())
}

#[test]
fn loop_checkpoint_callback_persists_during_tool_execution_and_success_clears(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-checkpoint",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("checkpoint"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));

    loop_runtime.process_direct("go", Some("cli:checkpoint-callback"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:checkpoint-callback")
        .ok_or("missing checkpoint callback session")?;
    if raw["metadata"].get("runtime_checkpoint").is_some()
        || raw["metadata"].get("pending_user_turn").is_some()
        || !raw["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|message| {
                message["role"] == "tool" && message["tool_call_id"] == "spawn-checkpoint"
            })
    {
        return Err(format!("successful run did not clear checkpoint markers: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_ask_user_interrupt_checkpoint_materializes_pending_placeholder(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:ask-checkpoint");
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({
            "phase": "awaiting_tools",
            "assistant_message": {
                "role": "assistant",
                "content": "need input",
                "tool_calls": [
                    {"id": "ask-crash", "type": "function", "function": {"name": "ask_user", "arguments": "{\"question\":\"Continue?\"}"}}
                ]
            },
            "completed_tool_results": [],
            "pending_tool_calls": [
                {"id": "ask-crash", "type": "function", "function": {"name": "ask_user", "arguments": "{\"question\":\"Continue?\"}"}}
            ]
        }),
    );
    manager.save(&session)?;
    let mut loop_runtime = AgentLoop::new(
        bus,
        manager,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    loop_runtime.process_direct("/status", Some("cli:ask-checkpoint"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:ask-checkpoint")
        .ok_or("missing ask checkpoint session")?;
    let messages = raw["messages"]
        .as_array()
        .ok_or("messages should be array")?;
    if messages.len() != 2
        || messages[0]["tool_calls"][0]["function"]["name"] != "ask_user"
        || messages[1]["role"] != "tool"
        || messages[1]["tool_call_id"] != "ask-crash"
        || !messages[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("interrupted or lost")
    {
        return Err(format!("ask checkpoint placeholder drifted: {raw:?}").into());
    }
    Ok(())
}

#[test]
fn loop_run_until_idle_dispatches_bus_messages_and_drains_same_session_injection(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("first"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("after injection".to_owned()),
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("other session".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-1", "start"));
    bus.publish_inbound(InboundMessage::new(
        "telegram",
        "user-1",
        "chat-1",
        "follow-up",
    ));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-2", "other"));

    let summary = loop_runtime.run_until_idle(1)?;
    if summary.processed != 1
        || summary.results.first().map(|result| result.had_injections) != Some(true)
        || bus.inbound_size() != 1
        || !loop_runtime.active_session_keys().is_empty()
    {
        return Err(format!("dispatcher/injection summary drifted: {summary:?}").into());
    }
    let first_outbound = bus.consume_outbound().ok_or("missing first outbound")?;
    if first_outbound.content != "after injection" {
        return Err(format!("injected turn final drifted: {first_outbound:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    if !requests
        .get(1)
        .into_iter()
        .flat_map(|request| &request.messages)
        .any(|message| {
            message["role"] == "user" && message["content"].to_string().contains("follow-up")
        })
    {
        return Err(format!("same-session follow-up was not injected: {requests:?}").into());
    }
    drop(requests);

    let second = loop_runtime
        .process_next_inbound()?
        .ok_or("missing deferred other session")?;
    if second.session_key != "telegram:chat-2"
        || second.final_content.as_deref() != Some("other session")
    {
        return Err(format!("deferred other session drifted: {second:?}").into());
    }
    Ok(())
}

#[test]
fn loop_mid_turn_injection_preserves_bus_fifo_after_limit() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("first"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("after injections".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-1", "start"));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "follow-1",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-2", "other-a",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "follow-2",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-3", "other-b",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "follow-3",
    ));
    bus.publish_inbound(InboundMessage::new(
        "telegram", "user-1", "chat-1", "/status",
    ));
    bus.publish_inbound(InboundMessage::new("telegram", "user-1", "chat-4", "tail"));

    let summary = loop_runtime.run_until_idle(1)?;
    if summary.results.first().map(|result| result.had_injections) != Some(true) {
        return Err(format!("expected injection summary: {summary:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let second_request = requests.get(1).ok_or("missing second provider request")?;
    for follow_up in ["follow-1", "follow-2", "follow-3"] {
        if !second_request.messages.iter().any(|message| {
            message["role"] == "user" && message["content"].to_string().contains(follow_up)
        }) {
            return Err(format!("missing injected follow-up {follow_up}: {requests:?}").into());
        }
    }
    drop(requests);

    let mut retained = Vec::new();
    while let Some(message) = bus.try_consume_inbound() {
        retained.push(message.content);
    }
    if retained != ["other-a", "other-b", "/status", "tail"] {
        return Err(format!("deferred bus FIFO order drifted: {retained:?}").into());
    }
    Ok(())
}

#[test]
fn loop_mid_turn_injection_uses_explicit_override_before_unified_key() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let captured = Arc::new(Mutex::new(Vec::<SpawnRequest>::new()));
    let captured_clone = captured.clone();
    let spawn_tool = SpawnTool::new(Arc::new(move |request: SpawnRequest| {
        captured_clone
            .lock()
            .map_err(|error| error.to_string())?
            .push(request);
        Ok("spawned".to_owned())
    }));
    let mut registry = ToolRegistry::new();
    registry.register(spawn_tool.clone());
    let client = MockProvider::new(vec![
        LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "spawn-1",
                "spawn",
                Map::from_iter([("task".to_owned(), json!("first"))]),
            )],
            ..LlmResponse::default()
        },
        LlmResponse {
            content: Some("after explicit injection".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
    config.unified_session_key = Some("unified".to_owned());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        config,
    )
    .with_context_tools(RuntimeContextTools::new().with_spawn(spawn_tool));
    let mut current = InboundMessage::new("telegram", "user-1", "chat-1", "start");
    current.session_key_override = Some("explicit".to_owned());
    let mut explicit_follow_up =
        InboundMessage::new("telegram", "user-1", "chat-2", "explicit follow-up");
    explicit_follow_up.session_key_override = Some("explicit".to_owned());
    bus.publish_inbound(current);
    bus.publish_inbound(explicit_follow_up);
    bus.publish_inbound(InboundMessage::new(
        "telegram",
        "user-1",
        "chat-3",
        "unified follow-up",
    ));

    let summary = loop_runtime.run_until_idle(1)?;
    if summary
        .results
        .first()
        .map(|result| result.session_key.as_str())
        != Some("explicit")
        || summary.results.first().map(|result| result.had_injections) != Some(true)
    {
        return Err(format!("explicit override summary drifted: {summary:?}").into());
    }
    let requests = client.requests.lock().map_err(|error| error.to_string())?;
    let second_request = requests.get(1).ok_or("missing second provider request")?;
    if !second_request.messages.iter().any(|message| {
        message["content"]
            .to_string()
            .contains("explicit follow-up")
    }) {
        return Err(format!("explicit follow-up was not injected: {requests:?}").into());
    }
    drop(requests);
    let retained = bus
        .try_consume_inbound()
        .ok_or("missing unified follow-up")?;
    if retained.content != "unified follow-up" || bus.inbound_size() != 0 {
        return Err(format!("unified message should remain deferred: {retained:?}").into());
    }
    Ok(())
}

#[test]
fn loop_forwards_tool_and_provider_progress_callbacks() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let mut registry = ToolRegistry::new();
    registry.register(AskUserTool::new());
    let client = StreamMockProvider::new(
        vec![LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "ask-progress",
                "ask_user",
                Map::from_iter([("question".to_owned(), json!("Continue?"))]),
            )],
            ..LlmResponse::default()
        }],
        vec![ProviderEvent::TextDelta {
            text: "thinking".to_owned(),
        }],
    );
    let provider_events = Arc::new(Mutex::new(Vec::<ProviderEvent>::new()));
    let provider_events_clone = provider_events.clone();
    let tool_events = Arc::new(Mutex::new(Vec::<ToolStatus>::new()));
    let tool_events_clone = tool_events.clone();
    let mut loop_runtime = AgentLoop::new(
        bus,
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = provider_events_clone.lock() {
            events.push(event.clone());
        }
    }))
    .with_tool_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = tool_events_clone.lock() {
            events.push(event.status.clone());
        }
    }));

    let result = loop_runtime.process_direct("start", Some("cli:progress"))?;
    let provider_events = provider_events.lock().map_err(|error| error.to_string())?;
    let tool_events = tool_events.lock().map_err(|error| error.to_string())?;
    if result.stop_reason != "ask_user"
        || provider_events.first()
            != Some(&ProviderEvent::TextDelta {
                text: "thinking".to_owned(),
            })
        || !tool_events.contains(&ToolStatus::Waiting)
    {
        return Err(format!(
            "progress callbacks drifted: result={result:?} provider={provider_events:?} tool={tool_events:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_does_not_persist_provider_stream_delta_as_session_content() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = StreamMockProvider::new(
        vec![LlmResponse {
            content: Some("FINAL_PROVIDER_OUTPUT".to_owned()),
            ..LlmResponse::default()
        }],
        vec![ProviderEvent::TextDelta {
            text: "STREAM_DELTA_SHOULD_NOT_PERSIST".to_owned(),
        }],
    );
    let provider_events = Arc::new(Mutex::new(Vec::<ProviderEvent>::new()));
    let provider_events_clone = provider_events.clone();
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    )
    .with_provider_event_callback(Arc::new(move |event| {
        if let Ok(mut events) = provider_events_clone.lock() {
            events.push(event.clone());
        }
    }));

    let result = loop_runtime.process_direct("stream please", Some("cli:provider-stream"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:provider-stream")
        .ok_or("missing session")?;
    let raw_text = raw.to_string();
    let provider_events = provider_events.lock().map_err(|error| error.to_string())?;
    if result.final_content.as_deref() != Some("FINAL_PROVIDER_OUTPUT")
        || provider_events.first()
            != Some(&ProviderEvent::TextDelta {
                text: "STREAM_DELTA_SHOULD_NOT_PERSIST".to_owned(),
            })
        || raw_text.contains("STREAM_DELTA_SHOULD_NOT_PERSIST")
        || !raw_text.contains("FINAL_PROVIDER_OUTPUT")
    {
        return Err(format!(
            "provider stream delta should stay observational: result={result:?} events={provider_events:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn loop_provider_error_publishes_error_and_clears_runtime_markers() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let registry = ToolRegistry::new();
    let client = MockProvider::new(Vec::new());
    let mut loop_runtime = AgentLoop::new(
        bus.clone(),
        SessionManager::new(workspace.path())?,
        ContextBuilder::new(workspace.path()),
        &registry,
        &client,
        AgentLoopConfig::new(workspace.path(), "test-model"),
    );

    let result = loop_runtime.process_direct("fail provider", Some("cli:provider-error"))?;
    let outbound = bus
        .consume_outbound()
        .ok_or("missing provider error outbound")?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:provider-error")
        .ok_or("missing session")?;
    if result.stop_reason != "error"
        || !result
            .final_content
            .as_deref()
            .unwrap_or_default()
            .contains("no mock response")
        || outbound.metadata["stop_reason"] != "error"
        || !outbound.content.contains("no mock response")
        || raw["metadata"].get("pending_user_turn").is_some()
        || raw["metadata"].get("runtime_checkpoint").is_some()
        || raw["messages"].as_array().map(Vec::len) != Some(2)
        || raw["messages"][1]["role"] != "assistant"
        || !raw["messages"][1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("no mock response")
    {
        return Err(format!(
            "provider error should be session-visible only through runtime boundary: result={result:?} outbound={outbound:?} raw={raw:?}"
        )
        .into());
    }
    Ok(())
}

struct StreamMockProvider {
    inner: MockProvider,
    events: Vec<ProviderEvent>,
}

impl StreamMockProvider {
    fn new(responses: Vec<LlmResponse>, events: Vec<ProviderEvent>) -> Self {
        Self {
            inner: MockProvider::new(responses),
            events,
        }
    }
}

impl ProviderClient for StreamMockProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.inner.chat(request)
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        for event in &self.events {
            on_event(event.clone());
        }
        self.inner.chat(request)
    }
}

struct MockProvider {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<ProviderRequest>>,
}

impl MockProvider {
    fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn push_response(&self, response: LlmResponse) -> Result<(), Box<dyn Error>> {
        self.responses
            .lock()
            .map_err(|error| error.to_string())?
            .push_back(response);
        Ok(())
    }
}

impl ProviderClient for MockProvider {
    fn chat(&self, request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.requests
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .push(request);
        self.responses
            .lock()
            .map_err(|error| provider_error(error.to_string()))?
            .pop_front()
            .ok_or_else(|| provider_error("no mock response"))
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

fn provider_error(message: impl Into<String>) -> ProviderError {
    ProviderError::Api {
        status: None,
        message: message.into(),
        retryable: false,
        headers: BTreeMap::new(),
        body: None,
    }
}
