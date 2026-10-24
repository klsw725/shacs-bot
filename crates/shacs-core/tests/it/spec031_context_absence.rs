use shacs_core::runtime::{
    project_spec031_context_evidence, Spec031ContextEvidenceInput, Spec031ContextEvidenceReason,
    Spec031ContextOwnerRef,
};
use shacs_projection::{
    Spec031Availability, Spec031ConstructionViolation, Spec031Freshness, Spec031InclusionReason,
};

#[test]
fn spec031_context_projection_rejects_malformed_owner_refs_and_prompt_absence_is_missing() {
    let malformed =
        Spec031ContextOwnerRef::try_new("/tmp/raw/path").expect_err("raw path rejected");
    assert_eq!(
        malformed.kind(),
        Spec031ConstructionViolation::UnsafeOpaqueRef
    );

    let projection = project_spec031_context_evidence(Spec031ContextEvidenceInput {
        batch_ref: None,
        owner_freshness: Spec031Freshness::Unavailable,
        inline_artifacts: &[],
        context_files: &[],
        provider_handoff: None,
    })
    .expect("prompt absence should project as missing evidence");

    assert_eq!(projection.rows.len(), 1);
    assert_eq!(projection.rows[0].reason, Spec031InclusionReason::Missing);
    assert_eq!(
        projection.rows[0].evidence_reason,
        Spec031ContextEvidenceReason::PromptAbsent
    );
    assert_eq!(
        projection.envelopes[0].state(),
        Spec031Availability::Unavailable
    );
}
