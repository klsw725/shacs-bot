use shacs_core::runtime::{
    blocked_skill_trust_external_surface, validate_skill_trust_permission,
    BlockedExternalSurfaceReason, ProcessEnvelopeAdmission, SkillTrustActionKind,
    SkillTrustDigestPair, SkillTrustGuardInput, SkillTrustPermissionDecisionKind,
    SkillTrustPermissionInput, SkillTrustPermissionSchemaId, SkillTrustRejectionReason,
    TrustLifecycleStatus,
};
use std::error::Error;

#[test]
fn skill_trust_allows_dependency_preparation_only_for_active_exact_match(
) -> Result<(), Box<dyn Error>> {
    let input = exact_input(SkillTrustActionKind::DependencyPreparation);

    let decision = validate_skill_trust_permission(&input, &admitting_guards());

    assert_eq!(decision.kind, SkillTrustPermissionDecisionKind::Validated);
    assert_eq!(decision.dispatch_count, 0);
    assert!(decision.blocked_external_surface.is_none());
    Ok(())
}

#[test]
fn skill_trust_allows_verified_entrypoint_only_for_active_exact_match() -> Result<(), Box<dyn Error>>
{
    let input = exact_input(SkillTrustActionKind::VerifiedEntrypoint);

    let decision = validate_skill_trust_permission(&input, &admitting_guards());

    assert_eq!(decision.kind, SkillTrustPermissionDecisionKind::Validated);
    assert_eq!(decision.dispatch_count, 0);
    Ok(())
}

#[test]
fn skill_trust_rejects_lifecycle_statuses_before_policy() -> Result<(), Box<dyn Error>> {
    for status in [
        TrustLifecycleStatus::Stale,
        TrustLifecycleStatus::Revoked,
        TrustLifecycleStatus::Removed,
        TrustLifecycleStatus::Pending,
        TrustLifecycleStatus::Malformed,
        TrustLifecycleStatus::Missing,
    ] {
        let mut input = exact_input(SkillTrustActionKind::DependencyPreparation);
        input.lifecycle_status = status;

        let decision = validate_skill_trust_permission(&input, &admitting_guards());

        assert_eq!(decision.kind, SkillTrustPermissionDecisionKind::Rejected);
        assert_eq!(
            decision.reason,
            Some(SkillTrustRejectionReason::LifecycleStatus)
        );
        assert_eq!(decision.dispatch_count, 0);
    }
    Ok(())
}

#[test]
fn skill_trust_rejects_digest_mismatch_manifest_outside_and_cancellation(
) -> Result<(), Box<dyn Error>> {
    let mut mismatch = exact_input(SkillTrustActionKind::VerifiedEntrypoint);
    mismatch.content_digest.current = "sha256:changed-content".to_owned();
    assert_rejected(mismatch, SkillTrustRejectionReason::DigestMismatch);

    let mut manifest_outside = exact_input(SkillTrustActionKind::DependencyPreparation);
    manifest_outside.dependency_manifest_digest.current = "sha256:outside-manifest".to_owned();
    assert_rejected(
        manifest_outside,
        SkillTrustRejectionReason::ManifestOutsideDependency,
    );

    let mut cancelled = exact_input(SkillTrustActionKind::DependencyPreparation);
    cancelled.cancellation_ref = Some("cancel:turn-1".to_owned());
    assert_rejected(cancelled, SkillTrustRejectionReason::Cancelled);
    Ok(())
}

#[test]
fn skill_trust_cannot_override_static_policy_ceiling_or_containment_proof(
) -> Result<(), Box<dyn Error>> {
    let input = exact_input(SkillTrustActionKind::VerifiedEntrypoint);

    let static_deny = validate_skill_trust_permission(
        &input,
        &SkillTrustGuardInput {
            static_policy_admits: false,
            ceiling_admits: true,
            containment_admission: ProcessEnvelopeAdmission::Admit,
        },
    );
    assert_eq!(
        static_deny.reason,
        Some(SkillTrustRejectionReason::StaticPolicy)
    );

    let ceiling_deny = validate_skill_trust_permission(
        &input,
        &SkillTrustGuardInput {
            static_policy_admits: true,
            ceiling_admits: false,
            containment_admission: ProcessEnvelopeAdmission::Admit,
        },
    );
    assert_eq!(
        ceiling_deny.reason,
        Some(SkillTrustRejectionReason::PermissionCeiling)
    );

    let proof_deny = validate_skill_trust_permission(
        &input,
        &SkillTrustGuardInput {
            static_policy_admits: true,
            ceiling_admits: true,
            containment_admission: ProcessEnvelopeAdmission::Deny,
        },
    );
    assert_eq!(
        proof_deny.reason,
        Some(SkillTrustRejectionReason::ContainmentProof)
    );
    Ok(())
}

#[test]
fn production_skill_trust_surface_blocks_without_external_owner_evidence(
) -> Result<(), Box<dyn Error>> {
    let decision =
        blocked_skill_trust_external_surface(SkillTrustActionKind::DependencyPreparation);

    assert_eq!(
        decision.kind,
        SkillTrustPermissionDecisionKind::BlockedExternalSurface
    );
    assert_eq!(decision.dispatch_count, 0);
    let blocked = decision
        .blocked_external_surface
        .ok_or("missing blocked surface")?;
    assert_eq!(blocked.status, "BLOCKED_EXTERNAL_SURFACE");
    assert_eq!(
        blocked.reason,
        BlockedExternalSurfaceReason::MissingOwnerEvidence
    );
    assert!(blocked.owner.contains("spec032"));
    assert!(blocked.owner.contains("spec035"));
    Ok(())
}

fn assert_rejected(input: SkillTrustPermissionInput, reason: SkillTrustRejectionReason) {
    let decision = validate_skill_trust_permission(&input, &admitting_guards());
    assert_eq!(decision.kind, SkillTrustPermissionDecisionKind::Rejected);
    assert_eq!(decision.reason, Some(reason));
    assert_eq!(decision.dispatch_count, 0);
}

fn admitting_guards() -> SkillTrustGuardInput {
    SkillTrustGuardInput {
        static_policy_admits: true,
        ceiling_admits: true,
        containment_admission: ProcessEnvelopeAdmission::Admit,
    }
}

fn exact_input(action_kind: SkillTrustActionKind) -> SkillTrustPermissionInput {
    SkillTrustPermissionInput {
        schema_id: SkillTrustPermissionSchemaId::V1,
        input_id: "skill-trust-input:fixture".to_owned(),
        action_kind,
        trust_record_ref: "trust:spec032:fixture".to_owned(),
        trust_owner_ref: "owner:spec032".to_owned(),
        lifecycle_status: TrustLifecycleStatus::Active,
        lifecycle_status_digest: digest_pair("lifecycle"),
        staleness_token: "owner-token-active".to_owned(),
        skill_descriptor_digest: digest_pair("descriptor"),
        source_digest: digest_pair("source"),
        content_digest: digest_pair("content"),
        dependency_manifest_digest: digest_pair("dependency-manifest"),
        package_set_digest: digest_pair("package-set"),
        capability_scope_digest: digest_pair("capability-scope"),
        entrypoint_digest: entrypoint_digest(action_kind),
        policy_safety_snapshot_ref: "policy-safety:fixture".to_owned(),
        process_envelope_id: "process:session-1:turn-1:action".to_owned(),
        containment_proof_ref: Some("containment-proof:fixture".to_owned()),
        execution_snapshot_ref: Some("execution-snapshot:spec035".to_owned()),
        declared_capabilities: vec!["proc_exec".to_owned()],
        cancellation_ref: None,
        canonical_input_digest: "sha256:canonical-input".to_owned(),
    }
}

fn digest_pair(label: &str) -> SkillTrustDigestPair {
    SkillTrustDigestPair {
        approved: format!("sha256:{label}"),
        current: format!("sha256:{label}"),
        envelope: format!("sha256:{label}"),
    }
}

fn entrypoint_digest(action_kind: SkillTrustActionKind) -> Option<SkillTrustDigestPair> {
    match action_kind {
        SkillTrustActionKind::DependencyPreparation => None,
        SkillTrustActionKind::VerifiedEntrypoint => Some(digest_pair("entrypoint")),
    }
}
