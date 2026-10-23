use shacs_core::runtime::{
    evaluate_containment_permission, BlockedExternalSurfaceReason, ContainmentBoundaryRef,
    ContainmentEvidenceState, ContainmentPermissionInput, PermissionCeilingProofInput,
    PermissionMode, PolicySafetyDigest, ProcessEnvelopeAdmission, RuntimeBoundaryKind,
    RuntimeBoundaryOrigin, SafetyCapability, WorkspaceScopeProof,
};
use std::error::Error;

#[test]
fn external_boundaries_serialize_blocked_external_surface() -> Result<(), Box<dyn Error>> {
    for boundary_kind in [
        RuntimeBoundaryKind::AppProcess,
        RuntimeBoundaryKind::DependencyPreparation,
        RuntimeBoundaryKind::VerifiedEntrypoint,
    ] {
        let proof = evaluate_containment_permission(input(child(boundary_kind)))?;
        let blocked = proof
            .blocked_external_surface
            .as_ref()
            .ok_or("missing blocked surface")?;
        let serialized = serde_json::to_value(blocked)?;
        assert_eq!(blocked.status, "BLOCKED_EXTERNAL_SURFACE");
        assert!(matches!(
            blocked.reason,
            BlockedExternalSurfaceReason::MissingOwnerEvidence
        ));
        assert!(serialized.get("owner").is_some());
        assert!(serialized.get("evidence_reason").is_some());
        assert_ne!(proof.admission, ProcessEnvelopeAdmission::Admit);
    }
    Ok(())
}

fn input(child: ContainmentBoundaryRef) -> ContainmentPermissionInput {
    ContainmentPermissionInput {
        parent: child.parent_boundary(),
        child,
        policy_safety_digest: PolicySafetyDigest("digest-current".to_owned()),
        process_envelope_id: "envelope-current".to_owned(),
        now_unix_ms: 1_000,
        cancelled_at_unix_ms: None,
        untrusted_metadata: None,
    }
}

fn child(boundary_kind: RuntimeBoundaryKind) -> ContainmentBoundaryRef {
    ContainmentBoundaryRef {
        boundary_id: format!("child-{boundary_kind:?}"),
        boundary_kind,
        origin: RuntimeBoundaryOrigin::UserTurn,
        containment_state: ContainmentEvidenceState::ConfirmedEquivalent,
        containment_digest: Some("containment-current".to_owned()),
        workspace_scope: WorkspaceScopeProof::same("workspace", "scope"),
        permission_ceiling: PermissionCeilingProofInput {
            parent_mode: PermissionMode::Default,
            requested_mode: PermissionMode::Default,
            parent_capabilities: vec![SafetyCapability::FsRead, SafetyCapability::ProcExec],
            requested_capabilities: vec![SafetyCapability::ProcExec],
            approved_scope_refs: vec!["scope".to_owned()],
            requested_scope_ref: "scope".to_owned(),
            per_action_evaluation_required: true,
        },
        created_at_unix_ms: 1_000,
    }
}
