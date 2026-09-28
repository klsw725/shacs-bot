#[path = "support/recent_retry.rs"]
mod support;

use serde_json::json;
use shacs_core::runtime::{AgentLoop, ContextBuilder, MessageBus, SessionManager};
use shacs_core::tools::ToolRegistry;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn recent_retry_fail_closed_gates_never_record_consumed_or_execute() -> Result<(), Box<dyn Error>> {
    for scenario in [
        "action",
        "argument",
        "snapshot",
        "policy",
        "expiry",
        "requester",
        "session_scope",
        "project_scope",
        "deny",
        "reload",
    ] {
        let root = tempfile::tempdir()?;
        let workspace = root.path().canonicalize()?.join("ws");
        let count = Arc::new(AtomicUsize::new(0));
        let mut registry = ToolRegistry::new();
        registry.register(support::CountedTool {
            count: count.clone(),
            fatal: false,
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
        let session_key = "cli:gates";
        assert_eq!(
            owner
                .process_direct("run the tests", Some(session_key))?
                .stop_reason,
            "ask_user"
        );
        owner.process_direct("deny", Some(session_key))?;
        let recent = owner
            .process_direct("/permission recent", Some(session_key))?
            .final_content
            .ok_or("recent output missing")?;
        let denial_id = recent
            .split_whitespace()
            .find_map(|part| part.strip_prefix("id="))
            .ok_or("denial id missing")?;
        let command = format!("/permission recent retry {denial_id}");
        owner.process_direct(&command, Some(session_key))?;
        let manager = owner.session_manager_mut();
        let mut session = manager.get_or_create(session_key);
        let original_receipts = session.permission_approval_receipts();
        let pending = &mut session.metadata["pending_recent_retry_approval"];
        let (reply, expected) = match scenario {
            "action" => {
                pending["action_digest"] = json!("f".repeat(64));
                ("approve", "permission_recent_retry_closed")
            }
            "argument" => {
                pending["argument_digest"] = json!("f".repeat(64));
                ("approve", "permission_recent_retry_closed")
            }
            "snapshot" => {
                pending["snapshot_digest"] = json!("f".repeat(64));
                ("approve", "permission_recent_retry_closed")
            }
            "policy" => {
                pending["policy_safety_snapshot_ref"]["policy_safety_digest"] =
                    json!("f".repeat(64));
                ("approve", "permission_recent_retry_closed")
            }
            "expiry" => {
                pending["expires_at_unix_ms"] = json!(0);
                ("approve", "permission_recent_retry_closed")
            }
            "requester" => {
                pending["requester_digest"] = json!("f".repeat(64));
                ("approve", "permission_recent_retry_pending")
            }
            "session_scope" => ("approve_session", "permission_recent_retry_rejected"),
            "project_scope" => ("approve_project", "permission_recent_retry_rejected"),
            "deny" => ("deny", "permission_recent_retry_denied"),
            "reload" => ("approve", "permission_recent_retry_closed"),
            _ => unreachable!(),
        };
        manager.save(&session)?;
        if scenario == "reload" {
            drop(owner);
            owner = AgentLoop::new(
                MessageBus::new(),
                SessionManager::new(&workspace)?,
                ContextBuilder::new(&workspace),
                &registry,
                &provider,
                support::config(&workspace),
            );
        }

        let result = owner.process_direct(reply, Some(session_key))?;

        assert_eq!(result.stop_reason, expected, "{scenario}");
        assert_eq!(count.load(Ordering::SeqCst), 0, "{scenario}");
        let terminal = SessionManager::new(&workspace)?.get_or_create(session_key);
        assert_eq!(
            terminal.permission_approval_receipts(),
            original_receipts,
            "{scenario}"
        );
        assert!(terminal
            .metadata
            .get("session_permission_approvals")
            .is_none());
        assert_eq!(
            terminal.metadata["session_remembered_permissions_v1"]["rules"],
            json!([])
        );
        assert_eq!(
            terminal
                .metadata
                .contains_key("pending_recent_retry_approval"),
            scenario == "requester"
        );
        println!(
            "{}",
            json!({"scenario":scenario, "executor_count":0, "stop_reason":result.stop_reason,
            "receipts":terminal.permission_approval_receipts(), "response":result.final_content})
        );
        if scenario != "requester" {
            assert_eq!(
                owner
                    .process_direct(&command, Some(session_key))?
                    .stop_reason,
                "permission_recent_retry_closed"
            );
        }
        drop(owner);
        root.close()?;
    }
    Ok(())
}
