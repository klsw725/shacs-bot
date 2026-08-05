use shacs_projection::{
    ExternalOwnerFact, Spec031ExternalOwnerReceiptRef, Spec031ExternalOwnerRef,
};
use std::error::Error;

#[test]
fn external_owner_refs_accept_only_closed_owner_grammar() {
    for value in [
        "spec032://app/lifecycle/ref-1",
        "spec032://app/readiness/ref-1",
        "spec034://media/artifact/ref-1",
        "spec034://media/analyzer/ref-alpha_2",
    ] {
        let ref_value = Spec031ExternalOwnerRef::try_new(value).expect("valid owner ref fixture");
        assert_eq!(ref_value.as_str(), value);
    }

    for value in invalid_refs() {
        assert!(
            Spec031ExternalOwnerRef::try_new(value).is_err(),
            "accepted unsafe owner ref: {value}"
        );
    }
}

#[test]
fn external_receipt_refs_accept_only_closed_receipt_grammar() {
    for value in [
        "spec032://receipt/app-start-1",
        "spec034://receipt/analyzer-1",
    ] {
        let ref_value =
            Spec031ExternalOwnerReceiptRef::try_new(value).expect("valid receipt ref fixture");
        assert_eq!(ref_value.as_str(), value);
    }

    for value in invalid_refs() {
        assert!(
            Spec031ExternalOwnerReceiptRef::try_new(value).is_err(),
            "accepted unsafe receipt ref: {value}"
        );
    }
    assert!(Spec031ExternalOwnerReceiptRef::try_new("spec034://media/artifact/ref-1").is_err());
}

#[test]
fn external_owner_fact_deserialization_rejects_unsafe_refs() {
    for value in invalid_refs() {
        let json = format!(
            r#"{{"owner":"spec032","capability":"app","opaque_ref":"{value}","status":"ready","reason_code":"owner_recorded"}}"#
        );
        assert!(serde_json::from_str::<ExternalOwnerFact>(&json).is_err());
    }
}

#[test]
fn external_owner_fact_round_trips_valid_refs() -> Result<(), Box<dyn Error>> {
    let json = r#"{"owner":"spec034","capability":"media","opaque_ref":"spec034://media/analyzer/ref-alpha_2","status":"included","reason_code":"owner_recorded","receipt_ref":"spec034://receipt/analyzer-1"}"#;
    let fact = serde_json::from_str::<ExternalOwnerFact>(json)?;
    let encoded = serde_json::to_string(&fact)?;

    assert!(encoded.contains("spec034://media/analyzer/ref-alpha_2"));
    assert!(encoded.contains("spec034://receipt/analyzer-1"));
    Ok(())
}

fn invalid_refs() -> &'static [&'static str] {
    &[
        "",
        "/tmp/raw-media.png",
        "C:\\Users\\owner\\raw-media.png",
        "spec032://user:token@example.test/owner/ref",
        "spec032://app/lifecycle/ref-1?raw=prompt",
        "spec034://media/artifact/ref-1#body",
        "spec034://media/artifact/%2e%2e",
        "spec034://media/artifact/..",
        "spec034://media/artifact/.",
        "spec034://media/artifact/",
        "spec034://media//ref-1",
        "spec032://prompt:raw-prompt",
        "spec034://body=raw-bytes",
        "spec034://media=raw-bytes",
        "spec034://media/artifact/prompt",
        "spec034://media/artifact/body",
        "spec034://media/artifact/payload",
        "spec034://media/artifact/secret-token",
        "spec034://media/artifact/ref.1",
        "spec035://media/artifact/ref-1",
        "spec032://receipt/app-start-1/extra",
    ]
}
