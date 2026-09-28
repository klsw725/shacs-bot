#[path = "support/recent_retry.rs"]
mod support;

use serde_json::{json, Value};
use shacs_core::runtime::{AgentLoop, ContextBuilder, MessageBus, SessionManager};
use shacs_core::tools::ToolRegistry;
use shacs_tui::live_source::{RuntimeProjectionSource, SessionRuntimeSource};
use shacs_tui::state::{ApprovalActionState, TuiState};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn same_owner_recent_retry_preserves_terminal_lineage_when_consumed() -> Result<(), Box<dyn Error>>
{
    consumed_retry(false)
}

#[test]
fn same_owner_recent_retry_preserves_failed_outcome_when_consumed() -> Result<(), Box<dyn Error>> {
    consumed_retry(true)
}

fn consumed_retry(fatal: bool) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().canonicalize()?.join("ws");
    let config_path = root.path().join("config.json");
    std::fs::write(&config_path, "{}")?;
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = ToolRegistry::new();
    registry.register(support::CountedTool {
        count: count.clone(),
        fatal,
    });
    let provider = support::LocalProvider(AtomicUsize::new(0));
    let mut owner = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&workspace)?,
        ContextBuilder::new(&workspace),
        &registry,
        &provider,
        support::config(&workspace),
    );
    let source = SessionRuntimeSource::with_config(Some(config_path), &workspace);
    let session_key = "cli:retry";
    let initial = owner.process_direct("run the tests", Some(session_key))?;
    assert_eq!(initial.stop_reason, "ask_user");
    let original = SessionManager::new(&workspace)?
        .get_or_create(session_key)
        .metadata["pending_permission_approval"]["approval_request"]
        .clone();
    owner.process_direct("deny", Some(session_key))?;
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let denied = SessionManager::new(&workspace)?.get_or_create(session_key);
    let denial_receipt = denied
        .permission_approval_receipts()
        .pop()
        .ok_or("denial receipt missing")?;
    assert_eq!(json!(denial_receipt.state), "denied");
    assert_eq!(
        json!(denial_receipt.approval_request_id),
        original["approval_request_id"]
    );
    println!(
        "{}",
        json!({"phase":"denied", "original":original, "receipt":denial_receipt,
        "executor_count":0, "view":shacs_tui::view::render_lines_for_width(&TuiState::from_snapshot(source.load()?, None), 2000)})
    );
    let recent = owner.process_direct("/permission recent", Some(session_key))?;
    let text = recent.final_content.ok_or("recent output missing")?;
    let denial_id = text
        .split_whitespace()
        .find_map(|part| part.strip_prefix("id="))
        .ok_or("denial id missing")?;
    let command = format!("/permission recent retry {denial_id}");
    let retry = owner.process_direct(&command, Some(session_key))?;
    assert_eq!(retry.stop_reason, "permission_recent_retry_pending");
    let pending = SessionManager::new(&workspace)?
        .get_or_create(session_key)
        .metadata["pending_recent_retry_approval"]
        .clone();
    let before = TuiState::from_snapshot(source.load()?, None);
    let displayed = before.sessions[0]
        .pending_approval
        .as_ref()
        .ok_or("pending view missing")?;
    assert!(matches!(
        displayed.action,
        ApprovalActionState::Unavailable { .. }
    ));
    println!(
        "{}",
        json!({"phase":"retry_pending", "pending":pending, "executor_count":0,
        "view":shacs_tui::view::render_lines_for_width(&before, 2000)})
    );

    let result = owner.process_direct("approve", Some(session_key))?;

    assert_eq!(count.load(Ordering::SeqCst), 1);
    let terminal = SessionManager::new(&workspace)?.get_or_create(session_key);
    let after = TuiState::from_snapshot(source.load()?, None);
    let receipts = terminal.permission_approval_receipts();
    let receipt = receipts.last().ok_or("terminal receipt missing")?;
    println!(
        "{}",
        json!({"phase":"retry_terminal", "fatal":fatal, "pending":pending, "receipts":receipts,
        "executor_count":count.load(Ordering::SeqCst), "stop_reason":result.stop_reason,
        "outcomes":terminal.metadata["runtime_execution"],
        "view":shacs_tui::view::render_lines_for_width(&after, 2000)})
    );
    assert_eq!(
        json!(receipt.approval_request_id),
        pending["approval_request_id"]
    );
    assert_eq!(json!(receipt.state), "consumed");
    assert_eq!(json!(receipt.action_digest), pending["action_digest"]);
    assert_eq!(json!(receipt.snapshot_digest), pending["snapshot_digest"]);
    assert_eq!(receipts.first(), Some(&denial_receipt));
    assert!(after.sessions[0].pending_approval.is_none());
    assert!(terminal
        .metadata
        .get("pending_recent_retry_approval")
        .is_none());
    assert!(terminal
        .metadata
        .get("session_permission_approvals")
        .is_none());
    assert_eq!(
        terminal.metadata["session_remembered_permissions_v1"]["rules"],
        json!([])
    );
    let receipt_json = serde_json::to_value(receipt)?;
    assert_eq!(
        receipt_json
            .as_object()
            .ok_or("receipt object missing")?
            .len(),
        5
    );
    if fatal {
        assert_eq!(result.stop_reason, "tool_error");
        assert_eq!(provider.0.load(Ordering::SeqCst), 2);
        assert!(terminal.metadata["runtime_execution"]["outcomes"]
            .as_array()
            .ok_or("outcomes missing")?
            .iter()
            .any(|record| record["fact"]["outcome"]["domain"] == "tool"
                && record["fact"]["outcome"]["outcome"]["kind"] == "failed"
                && record["fact"]["outcome"]["outcome"]["class"] == "fatal"));
    }
    assert!(shacs_tui::view::render_lines_for_width(&after, 2000)
        .iter()
        .any(|line| line.contains("approval terminal:")
            && line.contains(&receipt.approval_request_id)
            && line.contains("consumed")));
    let projection: Value = serde_json::from_str(
        &shacs_tui::revised_projection_view::spec035_revised_tui_view(
            &after.trusted_runtime,
            None,
            after.sessions[0].permission_approval_receipts.last(),
        )?,
    )?;
    assert_eq!(projection["decisions"][0]["details"]["state"], "consumed");
    assert_eq!(
        projection["decisions"][0]["details"]["approval_ref"],
        pending["approval_request_id"]
    );
    let replay = owner.process_direct(&command, Some(session_key))?;
    assert_eq!(replay.stop_reason, "permission_recent_retry_closed");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    println!(
        "{}",
        json!({"phase":"one_use", "result":replay.final_content, "executor_count":1, "projection":projection})
    );
    drop(owner);
    let mut reloaded = AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(&workspace)?,
        ContextBuilder::new(&workspace),
        &registry,
        &provider,
        support::config(&workspace),
    );
    let replay = reloaded.process_direct(&command, Some(session_key))?;
    assert_eq!(replay.stop_reason, "permission_recent_retry_closed");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    println!(
        "{}",
        json!({"phase":"reload_no_grant", "retry":replay.final_content,
        "executor_count":1})
    );
    if !fatal {
        let fresh = reloaded.process_direct("run the tests", Some(session_key))?;
        assert_eq!(fresh.stop_reason, "ask_user");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        println!(
            "{}",
            json!({"phase":"receipt_not_authority", "fresh_action":fresh.stop_reason,
            "executor_count":1})
        );
    }
    drop(reloaded);
    root.close()?;
    Ok(())
}
