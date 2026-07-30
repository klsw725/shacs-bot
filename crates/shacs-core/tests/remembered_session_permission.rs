mod remembered_session_permission_support;

use remembered_session_permission_support::{
    expire_legacy_session_approval, registry, runtime, seed_matching_remembered_allow,
    seed_oversized_remembered_permissions, tool_call_response, MockProvider,
};
use serde_json::json;
use shacs_core::runtime::MessageBus;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn remembered_session_permission_allow_survives_request_expiry_and_reload(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        tool_call_response("exec-1", "cargo fmt --check"),
        LlmResponse {
            content: Some("approved".to_owned()),
            ..LlmResponse::default()
        },
        tool_call_response("exec-2", "cargo fmt --check"),
        LlmResponse {
            content: Some("reused".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut first_runtime = runtime(workspace.path(), bus.clone(), &registry, &client)?;
    assert_eq!(
        first_runtime
            .process_direct("start", Some("cli:remembered-allow"))?
            .stop_reason,
        "ask_user"
    );
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    let approved = first_runtime.process_direct("approve_session", Some("cli:remembered-allow"))?;
    assert_eq!(approved.final_content.as_deref(), Some("approved"));
    let _approved_outbound = bus.consume_outbound().ok_or("missing approved outbound")?;
    drop(first_runtime);

    expire_legacy_session_approval(workspace.path(), "cli:remembered-allow")?;
    let mut reloaded_runtime = runtime(workspace.path(), bus, &registry, &client)?;
    let calls_before_reuse = calls.load(Ordering::SeqCst);

    let reused = reloaded_runtime.process_direct("again", Some("cli:remembered-allow"))?;

    assert_eq!(calls.load(Ordering::SeqCst), calls_before_reuse + 1);
    assert_ne!(reused.stop_reason, "ask_user");
    assert_eq!(reused.final_content.as_deref(), Some("reused"));
    Ok(())
}

#[test]
fn remembered_session_permission_deny_cancels_matching_action_without_prompt(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let bus = MessageBus::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        tool_call_response("exec-1", "cargo test"),
        tool_call_response("exec-2", "cargo test --workspace"),
    ]);
    let mut loop_runtime = runtime(workspace.path(), bus.clone(), &registry, &client)?;
    assert_eq!(
        loop_runtime
            .process_direct("start", Some("cli:remembered-deny"))?
            .stop_reason,
        "ask_user"
    );
    let _approval_outbound = bus.consume_outbound().ok_or("missing approval outbound")?;
    assert_eq!(
        loop_runtime
            .process_direct("deny_session", Some("cli:remembered-deny"))?
            .stop_reason,
        "permission_denied_by_user"
    );

    let blocked = loop_runtime.process_direct("again", Some("cli:remembered-deny"))?;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(blocked.stop_reason, "error");
    assert!(blocked.ask_user_options.is_empty());
    Ok(())
}

#[test]
fn remembered_session_permission_load_prunes_to_limit() -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    seed_oversized_remembered_permissions(workspace.path(), "cli:remembered-bound")?;
    let client = MockProvider::new(vec![
        tool_call_response("exec-bound", "cargo fmt --check"),
        LlmResponse {
            content: Some("bounded".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    let mut loop_runtime = runtime(workspace.path(), MessageBus::new(), &registry, &client)?;

    let reused = loop_runtime.process_direct("again", Some("cli:remembered-bound"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:remembered-bound")
        .ok_or("missing bounded session")?;

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_ne!(reused.stop_reason, "ask_user");
    assert_eq!(
        raw["metadata"]["session_remembered_permissions_v1"]["rules"]
            .as_array()
            .map(Vec::len),
        Some(32)
    );
    Ok(())
}

#[test]
fn remembered_session_permission_static_ask_required_blocks_remembered_allow(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    seed_matching_remembered_allow(workspace.path(), "cli:remembered-static-ask")?;
    let client = MockProvider::new(vec![tool_call_response("exec-static", "cargo test")]);
    let mut loop_runtime = runtime(workspace.path(), MessageBus::new(), &registry, &client)?;

    let result = loop_runtime.process_direct("again", Some("cli:remembered-static-ask"))?;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.stop_reason, "ask_user");
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:remembered-static-ask")
        .ok_or("missing static ask session")?;
    assert_eq!(
        raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["matcher"]["tokens"],
        json!(["cargo", "test"])
    );
    Ok(())
}
