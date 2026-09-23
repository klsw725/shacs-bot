use serde_json::json;
use shacs_session::{
    PermissionApprovalReceipt, PermissionApprovalTerminalState, Session, SessionManager,
    MAX_PERMISSION_APPROVAL_RECEIPTS,
};

fn receipt(index: usize) -> PermissionApprovalReceipt {
    PermissionApprovalReceipt {
        approval_request_id: format!("approval_{index}"),
        action_digest: "a".repeat(64),
        snapshot_digest: "b".repeat(64),
        state: PermissionApprovalTerminalState::Consumed,
        recorded_at_unix_ms: 100,
    }
}

#[test]
fn receipts_retain_latest_32_after_session_reload_without_pending_or_grants(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut manager = SessionManager::new(root.path())?;
    let mut session = Session::new("cli:receipts");
    for index in 0..MAX_PERMISSION_APPROVAL_RECEIPTS + 1 {
        session.record_permission_approval_receipt(receipt(index));
    }
    manager.save(&session)?;
    let reloaded = SessionManager::new(root.path())?.get_or_create("cli:receipts");
    let receipts = reloaded.permission_approval_receipts();
    assert_eq!(receipts.len(), 32);
    assert_eq!(receipts[0].approval_request_id, "approval_1");
    assert_eq!(receipts[31].approval_request_id, "approval_32");
    assert_eq!(reloaded.metadata.len(), 1);
    let detail = manager
        .session_ux_detail("cli:receipts")
        .ok_or("missing detail")?;
    assert_eq!(detail.permission_approval_receipts, receipts);
    Ok(())
}

#[test]
fn receipts_fail_closed_for_unknown_schema_wrong_session_oversize_or_raw_payload() {
    let mut original = Session::new("cli:receipts");
    original.record_permission_approval_receipt(receipt(0));
    for (field, value) in [
        ("schema_version", json!(2)),
        ("session_key", json!("cli:other")),
        ("receipts", json!(vec![receipt(0); 33])),
        ("raw_arguments", json!("secret")),
        ("receipts", json!([{"approval_request_id":"malformed"}])),
    ] {
        let mut session = original.clone();
        session.metadata["permission_approval_receipts"][field] = value;
        assert!(session.permission_approval_receipts().is_empty());
    }
}

#[test]
fn receipts_do_not_overwrite_terminal_lineage_or_retain_sensitive_fields() {
    let mut session = Session::new("cli:receipts");
    session.record_permission_approval_receipt(receipt(0));
    let mut conflicting = receipt(0);
    conflicting.state = PermissionApprovalTerminalState::Denied;
    session.record_permission_approval_receipt(conflicting);
    let mut unsafe_ref = receipt(1);
    unsafe_ref.approval_request_id = "/private/secret".to_owned();
    session.record_permission_approval_receipt(unsafe_ref);
    assert_eq!(session.permission_approval_receipts(), vec![receipt(0)]);
}
