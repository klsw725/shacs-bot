#[path = "spec030_diagnostics_aggregate/fixtures.rs"]
mod fixtures;

use shacs_core::runtime::{
    build_core_diagnostics_aggregate, CoreDiagnosticsAggregateInput, SkillTrustPermissionDecision,
    SkillTrustPermissionDecisionKind,
};
use std::error::Error;

use fixtures::{
    classifier_evidence, containment_proof, permission_summary, policy_ref, process_receipt,
};

#[test]
fn core_diagnostics_aggregate_serializes_only_safe_refs_states_and_counts(
) -> Result<(), Box<dyn Error>> {
    let policy_ref = policy_ref("aggregate-safe", None);
    let permission_summary = permission_summary(policy_ref.clone());
    let receipt = process_receipt(policy_ref.clone());
    let proof = containment_proof(&receipt);
    let classifier = classifier_evidence(policy_ref.clone());
    let trust = SkillTrustPermissionDecision {
        kind: SkillTrustPermissionDecisionKind::BlockedExternalSurface,
        reason: None,
        blocked_external_surface: Some(shacs_core::runtime::BlockedExternalSurface {
            status: "BLOCKED_EXTERNAL_SURFACE".to_owned(),
            owner: "spec032+spec035".to_owned(),
            evidence_reason: "missing owner evidence".to_owned(),
            reason: shacs_core::runtime::BlockedExternalSurfaceReason::MissingOwnerEvidence,
        }),
        dispatch_count: 0,
    };

    let aggregate = build_core_diagnostics_aggregate(CoreDiagnosticsAggregateInput {
        permission: &permission_summary,
        process_receipts: &[receipt],
        containment_proofs: &[proof],
        classifier_evidence: &[classifier],
        trust_decisions: &[trust],
    })?;
    let serialized = serde_json::to_string(&aggregate)?;

    assert!(serialized.contains("snapshot-aggregate-safe"));
    assert!(serialized.contains("BLOCKED_EXTERNAL_SURFACE"));
    assert!(serialized.contains("failed_closed"));
    assert!(serialized.contains("admit"));
    for raw_marker in [
        "sk-spec030-raw-token",
        "RAW_STDOUT_SPEC030",
        "RAW_STDERR_SPEC030",
        "provider-secret-payload",
        "/Users/spec030/raw/path",
        "process_handle",
    ] {
        assert!(
            !serialized.contains(raw_marker),
            "raw marker leaked: {raw_marker}"
        );
    }
    Ok(())
}

#[test]
fn core_diagnostics_aggregate_rejects_raw_material_fixture_before_serialization(
) -> Result<(), Box<dyn Error>> {
    let mut permission_summary = permission_summary(policy_ref("raw-material", None));
    permission_summary.secret_refs.items[0].safe_summary = "sk-spec030-raw-token".to_owned();

    let result = build_core_diagnostics_aggregate(CoreDiagnosticsAggregateInput {
        permission: &permission_summary,
        process_receipts: &[],
        containment_proofs: &[],
        classifier_evidence: &[],
        trust_decisions: &[],
    });

    assert!(result.is_err());
    let error_text = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(!error_text.contains("sk-spec030-raw-token"));
    Ok(())
}

#[test]
fn core_diagnostics_aggregate_rejects_raw_diagnostic_semantics_under_safe_fields() {
    let mut permission_summary = permission_summary(policy_ref("raw-safe-summary", None));
    permission_summary.secret_refs.items[0].safe_summary =
        "stdout_ref carries raw stdout content".to_owned();

    let result = build_core_diagnostics_aggregate(CoreDiagnosticsAggregateInput {
        permission: &permission_summary,
        process_receipts: &[],
        containment_proofs: &[],
        classifier_evidence: &[],
        trust_decisions: &[],
    });

    assert!(result.is_err());
    let error_text = result
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(!error_text.contains("raw stdout content"));
    assert_eq!(error_text, "raw diagnostic material rejected");
}

#[test]
fn core_diagnostics_aggregate_direct_serialization_redacts_mutated_raw_fields(
) -> Result<(), Box<dyn Error>> {
    let policy_ref = policy_ref("direct-serialize", None);
    let permission_summary = permission_summary(policy_ref);
    let mut aggregate = build_core_diagnostics_aggregate(CoreDiagnosticsAggregateInput {
        permission: &permission_summary,
        process_receipts: &[],
        containment_proofs: &[],
        classifier_evidence: &[],
        trust_decisions: &[],
    })?;
    aggregate.secrets.refs[0].ref_id = "raw stdout content hidden in ref".to_owned();
    aggregate.secrets.refs[0].redaction_evidence_ref = "C:\\Users\\spec030\\secret.txt".to_owned();

    let serialized = serde_json::to_string(&aggregate)?;

    assert!(!serialized.contains("raw stdout content"));
    assert!(!serialized.contains("C:\\Users"));
    assert!(serialized.contains("[REDACTED]"));
    Ok(())
}
