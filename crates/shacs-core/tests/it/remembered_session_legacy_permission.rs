mod remembered_session_legacy_support;

use remembered_session_legacy_support::{
    registry, runtime, seed_legacy_session_approvals, seed_malformed_legacy_session_approval,
    seed_mismatched_legacy_session_approval, tool_call_response, MockProvider,
};
use serde_json::json;
use shacs_core::runtime::MessageBus;
use shacs_providers::LlmResponse;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn remembered_session_permission_imports_only_valid_legacy_allow_once() -> Result<(), Box<dyn Error>>
{
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    let client = MockProvider::new(vec![
        tool_call_response("exec-legacy", "cargo fmt --check"),
        LlmResponse {
            content: Some("legacy reused".to_owned()),
            ..LlmResponse::default()
        },
    ]);
    seed_legacy_session_approvals(workspace.path(), "cli:legacy-import")?;
    let mut loop_runtime = runtime(workspace.path(), MessageBus::new(), &registry, &client)?;

    let reused = loop_runtime.process_direct("again", Some("cli:legacy-import"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:legacy-import")
        .ok_or("missing legacy import session")?;

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_ne!(reused.stop_reason, "ask_user");
    assert_eq!(remembered_rule_count(&raw), Some(1));
    assert_eq!(
        raw["metadata"]["session_remembered_permissions_v1"]["rules"][0]["legacy_imported"],
        true
    );
    Ok(())
}

#[test]
fn remembered_session_permission_legacy_context_mismatch_does_not_import_or_grant(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    seed_mismatched_legacy_session_approval(workspace.path(), "cli:legacy-mismatch")?;
    let client = MockProvider::new(vec![tool_call_response("exec-mismatch", "cargo test")]);
    let mut loop_runtime = runtime(workspace.path(), MessageBus::new(), &registry, &client)?;

    let result = loop_runtime.process_direct("again", Some("cli:legacy-mismatch"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:legacy-mismatch")
        .ok_or("missing legacy mismatch session")?;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.stop_reason, "ask_user");
    assert_eq!(remembered_rule_count(&raw), Some(0));
    Ok(())
}

#[test]
fn remembered_session_permission_malformed_legacy_metadata_is_diagnostic_only(
) -> Result<(), Box<dyn Error>> {
    let workspace = tempfile::tempdir()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(calls.clone());
    seed_malformed_legacy_session_approval(workspace.path(), "cli:legacy-malformed")?;
    let client = MockProvider::new(vec![tool_call_response("exec-malformed", "cargo test")]);
    let mut loop_runtime = runtime(workspace.path(), MessageBus::new(), &registry, &client)?;

    let result = loop_runtime.process_direct("again", Some("cli:legacy-malformed"))?;
    let raw = loop_runtime
        .session_manager()
        .read_session_file("cli:legacy-malformed")
        .ok_or("missing malformed legacy session")?;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(result.stop_reason, "ask_user");
    assert_eq!(remembered_rule_count(&raw), Some(0));
    assert_eq!(
        raw["metadata"]["session_remembered_permissions_v1"]["diagnostics"],
        json!([{ "code": "malformed_legacy_session_permission_approvals" }])
    );
    Ok(())
}

fn remembered_rule_count(raw: &serde_json::Value) -> Option<usize> {
    raw["metadata"]["session_remembered_permissions_v1"]["rules"]
        .as_array()
        .map(Vec::len)
}
