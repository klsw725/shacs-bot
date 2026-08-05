use serde_json::json;
use shacs_config::{
    RememberedPermissionEffect, RememberedPermissionMatcher, RememberedPermissionRule,
    RememberedPermissionStore, WorkspacePermissionId,
};
use shacs_eval::evaluator::{EvidenceKind, EvidenceRef, RedactionStatus};
use shacs_projection::{
    build_remembered_permission_projection, build_spec018_projection, build_spec024_projection,
    runtime_spec018_channel_projection, runtime_spec018_local_api_projection,
    runtime_spec024_channel_projection, runtime_spec024_local_api_projection,
    RememberedPermissionProjectionInput, RememberedPermissionStoreHealthInput,
    RuntimeSpec018ProjectionInput, RuntimeSpec024ProjectionInput,
};
use std::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceState {
    Implemented,
    Partial,
    Absent,
}

fn evidence_ref(owner_spec: &str, id: &str, redaction_status: RedactionStatus) -> EvidenceRef {
    EvidenceRef {
        kind: EvidenceKind::DiagnosticRecord,
        id: id.to_owned(),
        digest: format!("digest-{id}"),
        summary: format!("summary-{id}"),
        redaction_status,
        owner_spec: Some(owner_spec.to_owned()),
        locator: Some(format!("spec{owner_spec}://{id}")),
        retention_hint: Some("baseline".to_owned()),
    }
}

#[test]
fn spec031_baseline_pins_existing_spec018_and_spec024_projection_wrappers(
) -> Result<(), Box<dyn Error>> {
    let spec018 = build_spec018_projection(RuntimeSpec018ProjectionInput {
        generated_at_ms: 31,
        session_id: "session-031-baseline",
        goal_summaries: &[],
        automation_summaries: &[],
        approval_summaries: &[],
        blocked_summaries: &[],
        verification_summaries: &[],
        replay_summaries: &[],
        recent_evaluator_decision_summaries: &[],
    });

    let spec018_local_api = runtime_spec018_local_api_projection(&spec018);
    let spec018_channel = runtime_spec018_channel_projection(&spec018);
    assert_eq!(spec018.schema_label, "018Projection");
    assert_eq!(spec018.schema_version, "018Projection.v1");
    assert_eq!(spec018_local_api.schema_version, spec018.schema_version);
    assert_eq!(spec018_channel.schema_version, spec018.schema_version);

    let safe_ref = evidence_ref("024", "safe", RedactionStatus::Redacted);
    let unsafe_ref = evidence_ref("024", "unsafe", RedactionStatus::RedactionFailed);
    let wrong_owner_ref = evidence_ref("018", "wrong-owner", RedactionStatus::Redacted);
    let payload = json!({ "state": "blocked", "reason": "baseline" });
    let spec024 = build_spec024_projection(RuntimeSpec024ProjectionInput {
        generated_at_ms: 31,
        session_id: "session-031-baseline",
        surface: "local_api",
        projection: &payload,
        evidence_refs: &[safe_ref.clone(), unsafe_ref, wrong_owner_ref],
    });

    let spec024_local_api = runtime_spec024_local_api_projection(&spec024);
    let spec024_channel = runtime_spec024_channel_projection(&spec024);
    assert_eq!(spec024.schema_label, "024ProjectionWrapper");
    assert_eq!(spec024.schema_version, "024ProjectionWrapper.v1");
    assert_eq!(spec024.evidence_refs, vec![safe_ref.clone()]);
    assert_eq!(spec024_local_api.evidence_refs, vec![safe_ref.clone()]);
    assert_eq!(spec024_channel.evidence_refs, vec![safe_ref]);

    Ok(())
}

#[test]
fn spec031_baseline_pins_remembered_permission_projection_current_read_model(
) -> Result<(), Box<dyn Error>> {
    let workspace_id =
        WorkspacePermissionId::from_canonical_workspace_path("/tmp/spec031-workspace");
    let mut store = RememberedPermissionStore::default();
    store.upsert_rule(
        workspace_id.clone(),
        RememberedPermissionRule::new(
            RememberedPermissionEffect::Allow,
            RememberedPermissionMatcher::ExecPrefix {
                tokens: vec!["cargo".to_owned(), "test".to_owned()],
            },
            31,
        ),
    );

    let available = build_remembered_permission_projection(RememberedPermissionProjectionInput {
        store: Some(&store),
        workspace_id: &workspace_id,
        health: RememberedPermissionStoreHealthInput::available(),
    });
    let unavailable = build_remembered_permission_projection(RememberedPermissionProjectionInput {
        store: None,
        workspace_id: &workspace_id,
        health: RememberedPermissionStoreHealthInput::unavailable("store is missing"),
    });

    assert_eq!(available.schema_version, 1);
    assert_eq!(available.status, "available");
    assert_eq!(available.rules.len(), 1);
    assert_eq!(available.rules[0].effect, RememberedPermissionEffect::Allow);
    assert_eq!(available.rules[0].matcher_kind, "exec_prefix");
    assert_eq!(available.rules[0].created_unix_ms, 31);
    assert_eq!(unavailable.status, "unavailable");
    assert!(unavailable.rules.is_empty());

    Ok(())
}

#[test]
fn spec031_baseline_inventory_marks_absent_capabilities_not_ready() {
    let inventory = [
        (
            "spec018_projection_wrapper",
            SurfaceState::Implemented,
            true,
        ),
        (
            "spec024_projection_wrapper",
            SurfaceState::Implemented,
            true,
        ),
        ("remembered_permissions", SurfaceState::Implemented, true),
        (
            "session_workflow_optional_projection",
            SurfaceState::Partial,
            false,
        ),
        (
            "workflow_missing_checkpoint_defaults",
            SurfaceState::Partial,
            false,
        ),
        ("cli_command_surface", SurfaceState::Partial, false),
        ("local_api_diagnostics", SurfaceState::Partial, false),
        ("websocket_channel_events", SurfaceState::Partial, false),
        (
            "tui_static_session_workflow_view",
            SurfaceState::Partial,
            false,
        ),
        ("interactive_tui", SurfaceState::Absent, false),
        ("repl_agent", SurfaceState::Absent, false),
        ("onboard_wizard", SurfaceState::Absent, false),
        ("readiness_aggregation", SurfaceState::Absent, false),
        ("dropped_progress_accounting", SurfaceState::Absent, false),
        ("spec031_release_runner", SurfaceState::Absent, false),
    ];

    assert!(inventory
        .iter()
        .any(|(_, state, _)| *state == SurfaceState::Implemented));
    assert!(inventory
        .iter()
        .any(|(_, state, _)| *state == SurfaceState::Partial));
    assert!(inventory
        .iter()
        .any(|(_, state, _)| *state == SurfaceState::Absent));
    assert!(inventory
        .iter()
        .filter(|(_, state, _)| *state == SurfaceState::Absent)
        .all(|(_, _, success_ready)| !success_ready));
    assert!(inventory
        .iter()
        .any(|(name, _, _)| *name == "spec031_release_runner"));
}
