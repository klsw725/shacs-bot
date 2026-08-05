use serde_json::{json, Map, Value};
use shacs_session::{
    find_legal_message_start, Session, SessionHistoryOptions, SessionManager,
    SessionProjectionOptions,
};
use std::error::Error;

#[test]
fn session_manager_saves_loads_metadata_and_history() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:direct");
    session
        .metadata
        .insert("keep".to_owned(), Value::String("value".to_owned()));
    session.add_message("assistant", "old", Map::new());
    session.add_message("user", "hello", Map::new());
    let mut media_extra = Map::new();
    media_extra.insert("media".to_owned(), json!(["image.png"]));
    session.add_message("assistant", "seen", media_extra);
    session.last_consolidated = 1;
    manager.save(&session)?;

    let loaded = manager.get_or_create("cli:direct");
    let history = loaded.get_history_with_options(SessionHistoryOptions {
        max_messages: 10,
        include_timestamps: true,
        ..SessionHistoryOptions::default()
    });

    if loaded.metadata["keep"] != "value"
        || loaded.last_consolidated != 1
        || history.len() != 2
        || !history[0]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("[Message Time:")
        || !history[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("[attachment omitted from history]")
        || history[1]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("image.png")
    {
        return Err(format!(
            "session persistence/history drifted: loaded={loaded:?} history={history:?}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn session_manager_writes_metadata_header_then_message_jsonl() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:shape");
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({ "phase": "awaiting_tools" }),
    );
    session.add_message("user", "hello", Map::new());
    session.add_message("assistant", "world", Map::new());
    session.last_consolidated = 1;

    manager.save_with_fsync(&session)?;
    let path = manager.session_path("cli:shape");
    let lines = std::fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;

    if lines.len() != 3
        || lines[0]["_type"] != "metadata"
        || lines[0]["key"] != "cli:shape"
        || lines[0]["metadata"]["runtime_checkpoint"]["phase"] != "awaiting_tools"
        || lines[0]["last_consolidated"] != 1
        || lines[1]["role"] != "user"
        || lines[1]["content"] != "hello"
        || lines[2]["role"] != "assistant"
        || lines[2]["content"] != "world"
    {
        return Err(format!("session JSONL shape drifted: {lines:?}").into());
    }
    Ok(())
}

#[test]
fn session_manager_exposes_python_compatibility_paths_and_payload() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let legacy = tempfile::tempdir()?;
    let manager = SessionManager::with_legacy_sessions_dir(workspace.path(), legacy.path())?;
    let mut session = Session::new("telegram:chat/1");
    session
        .metadata
        .insert("topic".to_owned(), Value::String("compat".to_owned()));
    session.add_message("user", "hello", Map::new());

    let payload = session.payload();
    if manager.workspace() != workspace.path()
        || manager.sessions_dir() != workspace.path().join("sessions")
        || manager.legacy_sessions_dir() != Some(legacy.path())
        || manager.get_session_path("telegram:chat/1") != manager.session_path("telegram:chat/1")
        || manager.legacy_session_path("telegram:chat/1")
            != Some(legacy.path().join(format!(
                "{}.jsonl",
                SessionManager::safe_key("telegram:chat/1")
            )))
        || payload.get("last_consolidated").is_some()
        || payload["key"] != "telegram:chat/1"
        || payload["metadata"]["topic"] != "compat"
        || payload["messages"].as_array().map(Vec::len) != Some(1)
    {
        return Err(format!("compat path/payload surface drifted: payload={payload:?}").into());
    }
    Ok(())
}

#[test]
fn session_ux_projection_hides_raw_values_but_preserves_query_semantics(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:ux");
    session.metadata.insert(
        "api_token".to_owned(),
        Value::String("secret-value".to_owned()),
    );
    session.metadata.insert(
        "runtime_checkpoint".to_owned(),
        json!({ "phase": "awaiting_tools", "raw": "hidden" }),
    );
    session.metadata.insert(
        "runtime_diagnostics".to_owned(),
        json!({ "refs": ["workflow://diag-safe", "", "workflow://diag-safe"], "raw": "sk-hidden" }),
    );
    session.metadata.insert(
        "runtime_workflow".to_owned(),
        json!({
            "raw_prompt": "do-not-show",
            "projection": {
                "schema_label": "024WorkflowProjection",
                "schema_version": "024WorkflowProjection.v1",
                "workflow_id": "wf-1",
                "objective_summary": "raw objective stays hidden",
                "pattern": "fan_out_and_synthesize",
                "state": "Succeeded",
                "progress_count": 2,
                "active_child_count": 0,
                "pending_barrier_count": 1,
                "verifier_status": "passed",
                "budget_usage": {
                    "known_tokens": 10,
                    "estimated_tokens": 20,
                    "child_runs": 2,
                    "verifier_runs": 1,
                    "heavy_commands": 0
                },
                "next_action": "none",
                "resume_available": true,
                "worktree_refs": ["diff --raw hidden"],
                "evidence_refs": [{"id": "hidden-evidence"}]
            }
        }),
    );
    session.add_message("user", "hello secret", Map::new());
    session.add_message("assistant", "world", Map::new());
    manager.save(&session)?;

    let summaries = manager.list_session_ux()?;
    let detail = manager
        .session_ux_detail("cli:ux")
        .ok_or("missing UX detail")?;
    let diagnostics = manager.session_ux_diagnostics("cli:ux");
    let history = manager
        .session_ux_history(
            "cli:ux",
            SessionProjectionOptions {
                max_messages: 10,
                include_timestamps: false,
                ..SessionProjectionOptions::default()
            },
        )
        .ok_or("missing UX history")?;

    if summaries.len() != 1
        || summaries[0].key != "cli:ux"
        || detail.metadata_keys
            != [
                "api_token",
                "runtime_checkpoint",
                "runtime_diagnostics",
                "runtime_workflow",
            ]
        || detail.recovery_markers != ["runtime_checkpoint", "runtime_diagnostics"]
        || detail.checkpoint_phase.as_deref() != Some("awaiting_tools")
        || detail.diagnostics_refs != ["workflow://diag-safe"]
        || detail
            .runtime_workflow
            .as_ref()
            .and_then(|workflow| workflow.workflow_id.as_deref())
            != Some("wf-1")
        || detail
            .runtime_workflow
            .as_ref()
            .and_then(|workflow| workflow.pattern.as_deref())
            != Some("fan_out_and_synthesize")
        || detail
            .runtime_workflow
            .as_ref()
            .and_then(|workflow| workflow.budget_usage.as_ref())
            .and_then(|budget| budget.child_runs)
            != Some(2)
        || detail
            .runtime_workflow
            .as_ref()
            .map(|workflow| workflow.worktree_ref_count)
            != Some(1)
        || detail
            .runtime_workflow
            .as_ref()
            .map(|workflow| workflow.evidence_ref_count)
            != Some(1)
        || diagnostics
            .runtime_workflow
            .as_ref()
            .and_then(|workflow| workflow.verifier_status.as_deref())
            != Some("passed")
        || detail.message_count != 2
        || detail.last_consolidated != 0
        || diagnostics.legal_start != 0
        || diagnostics.diagnostics_refs != ["workflow://diag-safe"]
        || history.history.len() != 2
        || history.history[0]["content"] != "hello secret"
    {
        return Err(format!(
            "UX projection drifted: summaries={summaries:?} detail={detail:?} diagnostics={diagnostics:?} history={history:?}"
        )
        .into());
    }

    let detail_json = serde_json::to_value(&detail)?;
    if detail_json.to_string().contains("secret-value")
        || detail_json.to_string().contains("hidden")
        || detail_json.to_string().contains("sk-hidden")
        || detail_json.to_string().contains("raw objective")
        || detail_json.to_string().contains("diff --raw")
        || detail_json.to_string().contains("do-not-show")
        || detail_json.get("messages").is_some()
    {
        return Err(format!("UX detail exposed raw values: {detail_json}").into());
    }
    Ok(())
}

#[test]
fn session_ux_history_omits_provider_and_tool_internals() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:history-safe");
    session.add_message("user", "visible question", Map::new());
    let mut assistant_extra = Map::new();
    assistant_extra.insert(
        "tool_calls".to_owned(),
        json!([{"id": "call-1", "type": "function", "function": {"name": "secret_tool", "arguments": "{\"secret\":true}"}}]),
    );
    assistant_extra.insert("reasoning_content".to_owned(), json!("hidden reasoning"));
    assistant_extra.insert("thinking_blocks".to_owned(), json!(["hidden thinking"]));
    session.add_message("assistant", "visible answer", assistant_extra);
    let mut tool_extra = Map::new();
    tool_extra.insert("tool_call_id".to_owned(), json!("call-1"));
    tool_extra.insert("name".to_owned(), json!("secret_tool"));
    session.add_message("tool", "hidden tool payload", tool_extra);
    manager.save(&session)?;

    let history = manager
        .session_ux_history(
            "cli:history-safe",
            SessionProjectionOptions {
                max_messages: 10,
                include_timestamps: false,
                ..SessionProjectionOptions::default()
            },
        )
        .ok_or("missing UX history")?;

    assert_eq!(history.history.len(), 2);
    assert_eq!(history.history[0]["role"], "user");
    assert_eq!(history.history[0]["content"], "visible question");
    assert_eq!(history.history[1]["role"], "assistant");
    assert_eq!(history.history[1]["content"], "visible answer");
    let history_text = serde_json::to_string(&history)?;
    assert!(!history_text.contains("tool_calls"));
    assert!(!history_text.contains("tool_call_id"));
    assert!(!history_text.contains("secret_tool"));
    assert!(!history_text.contains("hidden reasoning"));
    assert!(!history_text.contains("hidden thinking"));
    assert!(!history_text.contains("hidden tool payload"));
    Ok(())
}

#[test]
fn session_ux_projects_runtime_execution_without_raw_ledger_values() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:runtime-execution");
    let outcomes = (0..22)
        .map(|index| {
            let (domain, outcome, decision, locator) = match index {
                0 => (
                    "provider",
                    json!("completed"),
                    json!({"kind": "accepted"}),
                    json!({"locator": "/tmp/secret-output.txt", "digest": "hidden"}),
                ),
                1 => (
                    "tool",
                    json!({"kind": "failed", "class": "fatal"}),
                    json!({"kind": "duplicate_ignored", "reason": "same secret correlation"}),
                    json!({"locator": "../secret-output.txt", "digest": "hidden"}),
                ),
                2 => (
                    "subagent",
                    json!("timed_out"),
                    json!({"kind": "discarded_late", "reason": "late secret"}),
                    json!({"locator": ".nanobot/tool-results/default/call.json", "digest": "safe"}),
                ),
                3 => (
                    "provider",
                    json!("stale"),
                    json!({"kind": "discarded_stale", "reason": "stale secret"}),
                    Value::Null,
                ),
                _ => (
                    "tool",
                    json!("completed"),
                    json!({"kind": "accepted"}),
                    Value::Null,
                ),
            };
            json!({
                "fact": {
                    "identity": {
                        "scope": {"session_id": "cli:runtime-execution", "turn_id": "turn-secret"},
                        "effect_id": if index == 2 {
                            "Authorization: Bearer ghp_effect_secret".to_owned()
                        } else {
                            format!("effect-{index:02}")
                        },
                        "correlation_id": format!("corr-secret-{index}"),
                        "attempt_id": "attempt-secret",
                        "idempotency_key": "idempotency-secret"
                    },
                    "outcome": {"domain": domain, "outcome": outcome},
                    "finished_at_ms": index,
                    "detail": "raw secret detail",
                    "artifact_ref": locator
                },
                "decision": decision
            })
        })
        .collect::<Vec<_>>();
    session.metadata.insert(
        "runtime_execution".to_owned(),
        json!({
            "pending": [
                {"domain": "provider", "identity": {"effect_id": "pending-provider", "correlation_id": "hidden"}},
                {"domain": "tool", "identity": {"effect_id": "pending-tool", "correlation_id": "hidden"}},
                {"domain": "subagent", "identity": {"effect_id": "pending-subagent", "correlation_id": "hidden"}},
                {"domain": "other", "identity": {"effect_id": "pending-other", "correlation_id": "hidden"}}
            ],
            "outcomes": outcomes,
            "raw_payload": "sk-hidden-runtime-payload"
        }),
    );
    manager.save(&session)?;

    let detail = manager
        .session_ux_detail("cli:runtime-execution")
        .ok_or("missing UX detail")?;
    let diagnostics = manager.session_ux_diagnostics("cli:runtime-execution");
    let execution = detail
        .runtime_execution
        .as_ref()
        .ok_or("missing runtime execution projection")?;

    assert_eq!(execution.pending_count, 4);
    assert_eq!(execution.outcome_count, 22);
    assert_eq!(execution.pending_by_domain.provider, 1);
    assert_eq!(execution.pending_by_domain.tool, 1);
    assert_eq!(execution.pending_by_domain.subagent, 1);
    assert_eq!(execution.pending_by_domain.unknown, 1);
    assert_eq!(execution.outcomes_by_domain.provider, 2);
    assert_eq!(execution.outcomes_by_domain.tool, 19);
    assert_eq!(execution.outcomes_by_domain.subagent, 1);
    assert_eq!(execution.decisions.accepted, 19);
    assert_eq!(execution.decisions.duplicate, 1);
    assert_eq!(execution.decisions.late, 1);
    assert_eq!(execution.decisions.stale, 1);
    assert_eq!(execution.artifact_ref_count, 3);
    assert_eq!(execution.safe_artifact_ref_count, 1);
    assert_eq!(execution.recent_outcomes.len(), 20);
    assert!(execution.recent_outcomes[0]
        .effect_ref
        .starts_with("subagent:sha256:"));
    assert_eq!(execution.recent_outcomes[0].domain, "subagent");
    assert_eq!(execution.recent_outcomes[0].outcome, "timed_out");
    assert_eq!(execution.recent_outcomes[0].decision, "late");
    assert_eq!(
        execution.recent_outcomes[0].artifact_locator.as_deref(),
        Some(".nanobot/tool-results/default/call.json")
    );
    assert_eq!(
        diagnostics
            .runtime_execution
            .as_ref()
            .map(|execution| execution.outcome_count),
        Some(22)
    );

    let detail_text = serde_json::to_string(&detail)?;
    for forbidden in [
        "sk-hidden-runtime-payload",
        "raw secret detail",
        "corr-secret",
        "attempt-secret",
        "idempotency-secret",
        "turn-secret",
        "/tmp/secret-output.txt",
        "../secret-output.txt",
        "same secret correlation",
        "late secret",
        "stale secret",
        "ghp_effect_secret",
    ] {
        assert!(
            !detail_text.contains(forbidden),
            "runtime execution projection leaked {forbidden}: {detail_text}"
        );
    }
    Ok(())
}

#[test]
fn session_ux_runtime_workflow_preserves_explicit_zero_but_not_missing_values(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut session = Session::new("cli:runtime-workflow-zero");
    session.metadata.insert(
        "runtime_workflow".to_owned(),
        json!({
            "projection": {
                "schema_version": "024WorkflowProjection.v1",
                "workflow_id": "wf-zero",
                "progress_count": 0,
                "active_child_count": 0,
                "budget_usage": {"known_tokens": 0, "child_runs": 0},
                "resume_available": false
            }
        }),
    );
    manager.save(&session)?;

    let workflow = manager
        .session_ux_detail("cli:runtime-workflow-zero")
        .ok_or("missing UX detail")?
        .runtime_workflow
        .ok_or("missing runtime workflow projection")?;
    let budget = workflow
        .budget_usage
        .ok_or("missing workflow budget projection")?;

    assert_eq!(workflow.progress_count, Some(0));
    assert_eq!(workflow.active_child_count, Some(0));
    assert_eq!(workflow.pending_barrier_count, None);
    assert_eq!(budget.known_tokens, Some(0));
    assert_eq!(budget.child_runs, Some(0));
    assert_eq!(budget.estimated_tokens, None);
    assert_eq!(workflow.worktree_ref_count, 0);
    assert_eq!(workflow.evidence_ref_count, 0);
    assert!(!workflow.resume_available);
    Ok(())
}

#[test]
fn session_ux_ignores_absent_and_malformed_runtime_execution() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let mut absent = Session::new("cli:runtime-execution-absent");
    absent.add_message("user", "hello", Map::new());
    manager.save(&absent)?;

    let mut malformed = Session::new("cli:runtime-execution-malformed");
    malformed.metadata.insert(
        "runtime_execution".to_owned(),
        json!({"pending": "old", "outcomes": 7}),
    );
    manager.save(&malformed)?;

    assert!(manager
        .session_ux_detail("cli:runtime-execution-absent")
        .ok_or("missing absent detail")?
        .runtime_execution
        .is_none());
    assert!(manager
        .session_ux_detail("cli:runtime-execution-malformed")
        .ok_or("missing malformed detail")?
        .runtime_execution
        .is_none());
    Ok(())
}

#[test]
fn session_manager_repairs_orphan_tool_boundaries_and_file_cap() -> Result<(), Box<dyn Error>> {
    let mut session = Session::new("cli:tools");
    session.add_message("user", "start", Map::new());
    let mut assistant_extra = Map::new();
    assistant_extra.insert(
        "tool_calls".to_owned(),
        json!([{"id": "valid", "type": "function", "function": {"name": "repeat", "arguments": "{}"}}]),
    );
    session.add_message("assistant", "", assistant_extra);
    let mut wrong_tool = Map::new();
    wrong_tool.insert("tool_call_id".to_owned(), json!("wrong"));
    wrong_tool.insert("name".to_owned(), json!("repeat"));
    session.add_message("tool", "wrong", wrong_tool);
    session.add_message("user", "next", Map::new());

    let history = session.get_history(20, false);
    if history.len() != 1 || history[0]["content"] != "next" {
        return Err(format!("orphan repair drifted: {history:?}").into());
    }

    let mut assistant_suffix = Session::new("cli:assistant-suffix");
    assistant_suffix.add_message("user", "question", Map::new());
    assistant_suffix.add_message("assistant", "answer", Map::new());
    let one_message_history = assistant_suffix.get_history(1, false);
    if one_message_history.len() != 1 || one_message_history[0]["content"] != "question" {
        return Err(
            format!("history should restore user boundary: {one_message_history:?}").into(),
        );
    }

    let messages = vec![json!({"role": "tool", "tool_call_id": "missing", "content": "orphan"})];
    if find_legal_message_start(&messages) != 1 {
        return Err("legal start should skip leading orphan tool result".into());
    }

    for index in 0..12 {
        session.add_message("user", format!("msg {index}"), Map::new());
    }
    let archived = session.enforce_file_cap_with_limit(8);
    if archived.is_empty() || session.messages.len() > 8 {
        return Err(format!("file cap drifted: archived={archived:?} session={session:?}").into());
    }
    Ok(())
}

#[test]
fn session_manager_reads_clears_and_deletes_legacy_nanobot_filename() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let manager = SessionManager::new(workspace.path())?;
    let legacy_path = manager.legacy_nanobot_session_path("cli:direct");
    std::fs::write(
        &legacy_path,
        concat!(
            "{\"_type\":\"metadata\",\"key\":\"cli:direct\",\"metadata\":{\"keep\":true},\"last_consolidated\":1}\n",
            "{\"role\":\"user\",\"content\":\"legacy prompt\"}\n",
            "{\"role\":\"assistant\",\"content\":\"legacy answer\"}\n"
        ),
    )?;

    let loaded = manager
        .load_existing("cli:direct")
        .ok_or("legacy session should load")?;
    assert_eq!(loaded.messages.len(), 2);
    assert_eq!(loaded.metadata["keep"], true);
    assert_eq!(
        manager.existing_session_path("cli:direct"),
        Some(legacy_path.clone())
    );

    let mut manager = manager;
    let cleared = manager.clear_session("cli:direct")?;
    assert_eq!(cleared, Some(2));
    let cleared_session = manager
        .load_existing("cli:direct")
        .ok_or("cleared legacy session should remain")?;
    assert!(cleared_session.messages.is_empty());
    assert_eq!(cleared_session.last_consolidated, 0);
    assert_eq!(cleared_session.metadata["keep"], true);

    assert!(manager.delete_session("cli:direct")?);
    assert!(!legacy_path.exists());
    assert!(!manager.delete_session("cli:direct")?);

    let mut canonical = Session::new("cli:coexist");
    canonical.add_message("user", "canonical prompt", Map::new());
    manager.save(&canonical)?;
    let coexist_legacy_path = manager.legacy_nanobot_session_path("cli:coexist");
    std::fs::write(
        &coexist_legacy_path,
        concat!(
            "{\"_type\":\"metadata\",\"key\":\"cli:coexist\"}\n",
            "{\"role\":\"user\",\"content\":\"stale raw legacy\"}\n"
        ),
    )?;

    assert_eq!(manager.clear_session("cli:coexist")?, Some(1));
    assert!(!coexist_legacy_path.exists());
    let cleared_coexist = manager
        .load_existing("cli:coexist")
        .ok_or("cleared canonical session should remain")?;
    assert!(cleared_coexist.messages.is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn session_manager_rejects_symlink_session_file_on_delete() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir()?;
    let outside = tempfile::NamedTempFile::new()?;
    let mut manager = SessionManager::new(workspace.path())?;
    let path = manager.session_path("cli:link");
    symlink(outside.path(), &path)?;

    assert!(manager.delete_session("cli:link").is_err());
    assert!(path.exists());
    Ok(())
}
