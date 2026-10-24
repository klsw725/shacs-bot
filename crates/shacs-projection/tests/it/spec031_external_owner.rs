use shacs_projection::{
    build_spec031_external_owner_projection, ExternalOwnerFact, ExternalOwnerFactInput,
    ExternalOwnerSpec, ExternalOwnerStatus, Spec031Availability, Spec031ConstructionViolation,
    Spec031ExternalCapability, Spec031ExternalOwnerReasonCode, Spec031ExternalOwnerReceiptRef,
    Spec031ExternalOwnerRef,
};
use std::error::Error;

fn app_fact(status: ExternalOwnerStatus) -> ExternalOwnerFact {
    ExternalOwnerFact::new(ExternalOwnerFactInput {
        owner: ExternalOwnerSpec::Spec032,
        capability: Spec031ExternalCapability::App,
        opaque_ref: Spec031ExternalOwnerRef::try_new("spec032://app/lifecycle/ref-1")
            .expect("safe app owner ref fixture"),
        status,
        reason_code: Spec031ExternalOwnerReasonCode::OwnerRecorded,
        receipt_ref: Some(
            Spec031ExternalOwnerReceiptRef::try_new("spec032://receipt/app-start-1")
                .expect("safe app receipt ref fixture"),
        ),
        stale: false,
    })
    .expect("consistent app owner fact fixture")
}

fn media_fact(status: ExternalOwnerStatus) -> ExternalOwnerFact {
    ExternalOwnerFact::new(ExternalOwnerFactInput {
        owner: ExternalOwnerSpec::Spec034,
        capability: Spec031ExternalCapability::Media,
        opaque_ref: Spec031ExternalOwnerRef::try_new("spec034://media/artifact/ref-1")
            .expect("safe media owner ref fixture"),
        status,
        reason_code: Spec031ExternalOwnerReasonCode::OwnerRecorded,
        receipt_ref: Some(
            Spec031ExternalOwnerReceiptRef::try_new("spec034://receipt/analyzer-1")
                .expect("safe media receipt ref fixture"),
        ),
        stale: false,
    })
    .expect("consistent media owner fact fixture")
}

fn stale_media_fact(status: ExternalOwnerStatus) -> ExternalOwnerFact {
    ExternalOwnerFact::new(ExternalOwnerFactInput {
        owner: ExternalOwnerSpec::Spec034,
        capability: Spec031ExternalCapability::Media,
        opaque_ref: Spec031ExternalOwnerRef::try_new("spec034://media/analyzer/ref-1")
            .expect("safe media analyzer ref fixture"),
        status,
        reason_code: Spec031ExternalOwnerReasonCode::OwnerRecorded,
        receipt_ref: Some(
            Spec031ExternalOwnerReceiptRef::try_new("spec034://receipt/analyzer-1")
                .expect("safe media receipt ref fixture"),
        ),
        stale: true,
    })
    .expect("consistent stale media owner fact fixture")
}

#[test]
fn unsafe_external_owner_refs_are_rejected_at_construction_boundary() {
    for value in [
        "/tmp/raw-media.png",
        "C:\\Users\\owner\\raw-media.png",
        "spec032://user:token@example.test/owner/ref",
        "token=shacs-secret-token",
        "prompt:raw prompt body",
        "body=raw-media-bytes",
    ] {
        assert!(Spec031ExternalOwnerRef::try_new(value).is_err());
        assert!(Spec031ExternalOwnerReceiptRef::try_new(value).is_err());
    }
}

#[test]
fn valid_external_owner_refs_round_trip_without_redaction() -> Result<(), Box<dyn Error>> {
    let fact = app_fact(ExternalOwnerStatus::Ready);
    let encoded = serde_json::to_string(&fact)?;
    let decoded = serde_json::from_str::<ExternalOwnerFact>(&encoded)?;

    assert_eq!(
        decoded.opaque_ref().as_str(),
        "spec032://app/lifecycle/ref-1"
    );
    assert_eq!(
        decoded
            .receipt_ref()
            .map(Spec031ExternalOwnerReceiptRef::as_str),
        Some("spec032://receipt/app-start-1")
    );
    Ok(())
}

#[test]
fn present_external_owner_facts_preserve_only_opaque_refs_and_reasons() -> Result<(), Box<dyn Error>>
{
    let projection = build_spec031_external_owner_projection(
        [app_fact(ExternalOwnerStatus::Ready)],
        [media_fact(ExternalOwnerStatus::Included)],
    );
    let encoded = serde_json::to_string(&projection)?;

    assert_eq!(projection.items[0].availability, Spec031Availability::Ready);
    assert_eq!(
        projection.items[1].owner_status,
        Some(ExternalOwnerStatus::Included)
    );
    assert!(encoded.contains("spec032://app/lifecycle/ref-1"));
    assert!(encoded.contains("owner_recorded"));
    assert!(encoded.contains("owner_status"));
    assert!(!encoded.contains("/tmp/raw-media.png"));
    assert!(!encoded.contains("raw prompt body"));
    assert!(projection.closure_blockers.is_empty());
    Ok(())
}

#[test]
fn app_present_media_missing_blocks_only_media() {
    let projection =
        build_spec031_external_owner_projection([app_fact(ExternalOwnerStatus::Ready)], []);

    assert_eq!(projection.closure_blockers.len(), 1);
    assert_eq!(
        projection.closure_blockers[0].owner,
        ExternalOwnerSpec::Spec034
    );
    assert_eq!(
        projection.closure_blockers[0].capability,
        Spec031ExternalCapability::Media
    );
    assert_eq!(
        projection.closure_blockers[0].reason_code,
        Spec031ExternalOwnerReasonCode::MissingExternalOwnerEvidence
    );
}

#[test]
fn missing_external_owner_facts_emit_blockers_without_ready_or_included(
) -> Result<(), Box<dyn Error>> {
    let projection = build_spec031_external_owner_projection([], []);
    let encoded = serde_json::to_string(&projection)?;

    assert!(projection.items.iter().all(
        |item| item.reason_code == Spec031ExternalOwnerReasonCode::MissingExternalOwnerEvidence
    ));
    assert!(projection
        .items
        .iter()
        .all(|item| item.availability != Spec031Availability::Ready));
    assert!(projection
        .items
        .iter()
        .all(|item| item.owner_status.is_none()));
    assert_eq!(projection.closure_blockers.len(), 2);
    assert!(encoded.contains("missing_external_owner_evidence"));
    assert!(!encoded.contains("owner_status"));
    Ok(())
}

#[test]
fn malformed_or_stale_external_owner_facts_cannot_ready_or_include() -> Result<(), Box<dyn Error>> {
    let malformed = serde_json::from_str::<ExternalOwnerFact>(
        r#"{"owner":"spec032","capability":"app","opaque_ref":"spec032://x","status":"ready","reason_code":"owner_recorded","unexpected":true}"#,
    );
    let unsafe_ref = serde_json::from_str::<ExternalOwnerFact>(
        r#"{"owner":"spec032","capability":"app","opaque_ref":"/tmp/raw-media.png","status":"ready","reason_code":"owner_recorded"}"#,
    );
    let projection = build_spec031_external_owner_projection(
        [app_fact(ExternalOwnerStatus::Ready)],
        [stale_media_fact(ExternalOwnerStatus::Included)],
    );

    assert!(malformed.is_err());
    assert!(unsafe_ref.is_err());
    assert_eq!(
        projection.items[1].availability,
        Spec031Availability::Unavailable
    );
    assert_eq!(
        projection.items[1].reason_code,
        Spec031ExternalOwnerReasonCode::StaleExternalOwnerEvidence
    );
    assert_eq!(projection.closure_blockers.len(), 1);
    assert_eq!(
        projection.closure_blockers[0].reason_code,
        Spec031ExternalOwnerReasonCode::StaleExternalOwnerEvidence
    );
    Ok(())
}

#[test]
fn owner_capability_and_ref_mismatch_rejects_before_projection() {
    let spec034_media_ref = Spec031ExternalOwnerRef::try_new("spec034://media/artifact/ref-1")
        .expect("safe media ref fixture");
    let spec032_receipt = Spec031ExternalOwnerReceiptRef::try_new("spec032://receipt/app-start-1")
        .expect("safe app receipt fixture");
    let spec034_raw_media = Spec031ExternalOwnerRef::try_new("spec034://media/raw-media/ref-1");
    let mismatched_owner = ExternalOwnerFact::new(ExternalOwnerFactInput {
        owner: ExternalOwnerSpec::Spec032,
        capability: Spec031ExternalCapability::App,
        opaque_ref: spec034_media_ref,
        status: ExternalOwnerStatus::Ready,
        reason_code: Spec031ExternalOwnerReasonCode::OwnerRecorded,
        receipt_ref: Some(spec032_receipt),
        stale: false,
    });

    assert!(spec034_raw_media.is_err());
    assert_eq!(
        mismatched_owner.map_err(|error| error.kind()),
        Err(Spec031ConstructionViolation::CapabilityFamilyMismatch)
    );
    assert!(serde_json::from_str::<ExternalOwnerFact>(
        r#"{"owner":"spec032","capability":"app","opaque_ref":"spec034://media/artifact/ref-1","status":"ready","reason_code":"owner_recorded","receipt_ref":"spec034://receipt/analyzer-1"}"#,
    )
    .is_err());
}
