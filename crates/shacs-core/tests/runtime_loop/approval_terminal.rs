use super::*;

#[test]
fn terminal_receipts_preserve_denial_expiry_and_mismatched_action_without_execution(
) -> Result<(), Box<dyn Error>> {
    for (reply, mutation, expected) in [
        ("deny", None, "denied"),
        ("approve", Some(("expires_at_unix_ms", json!(1))), "expired"),
        (
            "approve",
            Some(("action_digest", json!("f".repeat(64)))),
            "rejected",
        ),
    ] {
        let workspace = tempfile::tempdir()?;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(ProcExecCountingTool {
            calls: calls.clone(),
        });
        let provider = MockProvider::new(vec![
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "exec-terminal",
                    "exec",
                    Map::from_iter([("command".to_owned(), json!("cargo test"))]),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("finished".to_owned()),
                ..LlmResponse::default()
            },
        ]);
        let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
        config.permission_mode_snapshot.mode = PermissionMode::Auto;
        config.permission_interactive = true;
        let mut runtime = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &registry,
            &provider,
            config,
        );
        runtime.process_direct("start", Some("cli:terminal"))?;
        let manager = runtime.session_manager_mut();
        let mut pending = manager.get_or_create("cli:terminal");
        if let Some((field, value)) = mutation {
            pending.metadata["pending_permission_approval"]["approval_request"][field] = value;
            manager.save(&pending)?;
        }
        let request = pending.metadata["pending_permission_approval"]["approval_request"].clone();

        runtime.process_direct(reply, Some("cli:terminal"))?;

        let reloaded = SessionManager::new(workspace.path())?.get_or_create("cli:terminal");
        let receipt = &reloaded.metadata["permission_approval_receipts"]["receipts"][0];
        assert_eq!(receipt["state"], expected);
        assert_eq!(
            receipt["approval_request_id"],
            request["approval_request_id"]
        );
        assert_eq!(receipt["action_digest"], request["action_digest"]);
        assert!(reloaded
            .metadata
            .get("pending_permission_approval")
            .is_none());
        assert!(reloaded
            .metadata
            .get("session_permission_approvals")
            .is_none());
        assert_eq!(
            reloaded.metadata["session_remembered_permissions_v1"]["rules"],
            json!([])
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[test]
fn terminal_receipts_reject_bridge_mismatch_without_execution_or_session_grant(
) -> Result<(), Box<dyn Error>> {
    for (pointer, value) in [
        ("/approval_request/action_digest", json!("f".repeat(64))),
        ("/approval_request/snapshot_digest", json!("e".repeat(64))),
        ("/approval_request/requested_scope", json!("cli:other")),
        (
            "/tool_call/arguments/arguments/command",
            json!("cargo clippy"),
        ),
        ("/tool_call/arguments/name", json!("mcp_missing")),
    ] {
        let workspace = tempfile::tempdir()?;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(NamedProcExecCountingTool {
            name: "mcp_exec",
            calls: calls.clone(),
        });
        let provider = MockProvider::new(vec![
            LlmResponse {
                finish_reason: "tool_calls".to_owned(),
                tool_calls: vec![ToolCallRequest::new(
                    "bridge-terminal",
                    "tool_call",
                    Map::from_iter([
                        ("name".to_owned(), json!("mcp_exec")),
                        ("arguments".to_owned(), json!({"command": "cargo test"})),
                    ]),
                )],
                ..LlmResponse::default()
            },
            LlmResponse {
                content: Some("finished".to_owned()),
                ..LlmResponse::default()
            },
        ]);
        let mut config = AgentLoopConfig::new(workspace.path(), "test-model");
        config.permission_mode_snapshot.mode = PermissionMode::Auto;
        config.permission_interactive = true;
        config.tool_search.enabled = ToolSearchMode::On;
        let mut runtime = AgentLoop::new(
            MessageBus::new(),
            SessionManager::new(workspace.path())?,
            ContextBuilder::new(workspace.path()),
            &registry,
            &provider,
            config,
        );
        let paused = runtime.process_direct("start", Some("cli:bridge-negative"))?;
        assert_eq!(paused.stop_reason, "ask_user");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let manager = runtime.session_manager_mut();
        let mut pending = manager.get_or_create("cli:bridge-negative");
        *pending.metadata["pending_permission_approval"]
            .pointer_mut(pointer)
            .ok_or("missing mutation field")? = value;
        let request = pending.metadata["pending_permission_approval"]["approval_request"].clone();
        manager.save(&pending)?;

        runtime.process_direct("approve_session", Some("cli:bridge-negative"))?;

        let reloaded = SessionManager::new(workspace.path())?.get_or_create("cli:bridge-negative");
        let receipt = &reloaded.metadata["permission_approval_receipts"]["receipts"][0];
        assert_eq!(receipt["state"], "rejected", "{pointer}");
        assert_eq!(
            receipt["approval_request_id"],
            request["approval_request_id"]
        );
        assert_eq!(receipt["action_digest"], request["action_digest"]);
        assert!(reloaded
            .metadata
            .get("pending_permission_approval")
            .is_none());
        assert!(reloaded
            .metadata
            .get("session_permission_approvals")
            .is_none());
        assert_eq!(
            reloaded.metadata["session_remembered_permissions_v1"]["rules"],
            json!([])
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{pointer}");
    }
    Ok(())
}
