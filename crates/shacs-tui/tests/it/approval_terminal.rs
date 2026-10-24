use shacs_session::{
    PermissionApprovalReceipt, PermissionApprovalTerminalState, Session, SessionManager,
};
use shacs_tui::live_source::{RuntimeProjectionSource, SessionRuntimeSource};
use shacs_tui::state::TuiState;

#[test]
fn pending_status_never_promotes_old_receipt_or_executing_marker_to_allow(
) -> Result<(), Box<dyn std::error::Error>> {
    use shacs_tui::state::{ApprovalActionState, ApprovalLineage, ApprovalStatus, PendingApproval};
    let runtime = shacs_projection::Spec030RuntimeProjection::unavailable(
        shacs_projection::Spec030UnavailableReason::OwnerFactsMissing,
    );
    let old = PermissionApprovalReceipt {
        approval_request_id: "approval_old".to_owned(),
        action_digest: "a".repeat(64),
        snapshot_digest: "b".repeat(64),
        state: PermissionApprovalTerminalState::Consumed,
        recorded_at_unix_ms: 123,
    };
    for status in [
        ApprovalStatus::Executing,
        ApprovalStatus::Unknown,
        ApprovalStatus::Pending,
    ] {
        let pending = PendingApproval {
            lineage: ApprovalLineage::new("approval_current")?,
            tool_name: "exec".to_owned(),
            status,
            expires_at_unix_ms: None,
            action: ApprovalActionState::unavailable("fixture"),
        };
        let projection: serde_json::Value = serde_json::from_str(
            &shacs_tui::revised_projection_view::spec035_revised_tui_view(
                &runtime,
                Some(&pending),
                Some(&old),
            )?,
        )?;
        if status == ApprovalStatus::Unknown {
            assert_eq!(projection["decisions"], serde_json::json!([]));
        } else {
            assert_eq!(projection["decisions"][0]["details"]["state"], "pending");
            assert_eq!(
                projection["decisions"][0]["details"]["approval_ref"],
                "approval_current"
            );
        }
    }
    Ok(())
}

#[test]
fn live_source_exposes_terminal_receipt_without_pending_action(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let workspace = root.path().join("ws");
    let config = root.path().join("config.json");
    std::fs::write(&config, "{}")?;
    let mut manager = SessionManager::new(&workspace)?;
    let mut session = Session::new("cli:terminal");
    session.record_permission_approval_receipt(PermissionApprovalReceipt {
        approval_request_id: "approval_terminal".to_owned(),
        action_digest: "a".repeat(64),
        snapshot_digest: "b".repeat(64),
        state: PermissionApprovalTerminalState::Consumed,
        recorded_at_unix_ms: 123,
    });
    manager.save(&session)?;
    let snapshot = SessionRuntimeSource::with_config(Some(config), workspace).load()?;
    assert!(snapshot.sessions[0].pending_approval.is_none());
    assert_eq!(
        snapshot.sessions[0].permission_approval_receipts,
        session.permission_approval_receipts()
    );
    let state = TuiState::from_snapshot(snapshot, None);
    let view = shacs_tui::view::render_lines_for_width(&state, 2000);
    assert!(view
        .iter()
        .any(|line| line.contains("approval_terminal") && line.contains("consumed")));
    let projection: serde_json::Value = serde_json::from_str(
        &shacs_tui::revised_projection_view::spec035_revised_tui_view(
            &state.trusted_runtime,
            None,
            state.sessions[0].permission_approval_receipts.last(),
        )?,
    )?;
    assert_eq!(projection["decisions"][0]["details"]["state"], "consumed");
    assert_eq!(
        projection["decisions"][0]["details"]["action_digest"],
        "a".repeat(64)
    );
    assert!(projection["decisions"][0]["details"]
        .get("remembered_allow")
        .is_none());
    Ok(())
}
