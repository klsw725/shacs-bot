use serde_json::json;
use shacs_redaction::{
    RedactionEvidence, RedactionEvidenceRef, RedactionProfile, SecretRef, SecretRefError,
    SecretRefId,
};

fn valid_secret_ref_value() -> serde_json::Value {
    json!({
        "kind": "secret_ref",
        "schema_version": 1,
        "ref_id": "sec_prd001_env_happy",
        "source_kind": "env",
        "locator": {"kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET"},
        "owner": "spec035-config-profile",
        "scope": "provider-auth",
        "created_by": "config-profile",
        "created_at_ms": 0,
        "locator_digest": "sha256:locator",
        "staleness_token": "sha256:owner-state",
        "safe_summary": {"label": "env:SHACS_PRD001_HAPPY_SECRET", "required": true}
    })
}

#[test]
fn rejects_recursive_illegal_raw_secret_fields() {
    for (field, value) in [
        (
            "value",
            json!({"kind": "secret_ref", "value": "sk-live-secret"}),
        ),
        (
            "raw",
            json!({"kind": "redacted_value", "raw": "Bearer ghp_secret"}),
        ),
        (
            "env_value",
            json!({"kind": "secret_ref", "locator": {"env_value": "hunter2"}}),
        ),
    ] {
        let error = SecretRef::from_value(value).expect_err(field);
        assert_eq!(error, SecretRefError::IllegalRawField(field.to_owned()));
    }
}

#[test]
fn serde_deserialize_rejects_recursive_illegal_raw_secret_fields() {
    let error = serde_json::from_value::<SecretRef>(json!({
        "kind": "secret_ref",
        "schema_version": 1,
        "ref_id": "sec_prd001_raw_bad",
        "source_kind": "env",
        "locator": {"kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET", "env_value": "hunter2"},
        "owner": "spec035-config-profile",
        "scope": "provider-auth",
        "locator_digest": "sha256:locator",
        "staleness_token": "sha256:owner-state",
        "safe_summary": {"label": "env:SHACS_PRD001_HAPPY_SECRET", "required": true}
    }))
    .expect_err("direct serde must reject nested raw material");

    assert!(error
        .to_string()
        .contains("illegal raw secret field: env_value"));
}

#[test]
fn rejects_unsupported_source_kind() {
    let mut value = valid_secret_ref_value();
    value["source_kind"] = json!("hosted_vault");

    let error = SecretRef::from_value(value).expect_err("unsupported source must reject");

    assert!(matches!(error, SecretRefError::Serde(_)));
}

#[test]
fn rejects_missing_staleness_material() {
    let mut value = valid_secret_ref_value();
    value
        .as_object_mut()
        .expect("test fixture object")
        .remove("staleness_token");

    let error = SecretRef::from_value(value).expect_err("missing staleness must reject");

    assert!(matches!(error, SecretRefError::Serde(_)));
}

#[test]
fn serde_deserialize_rejects_unsupported_source_and_missing_staleness() {
    let mut unsupported = valid_secret_ref_value();
    unsupported["source_kind"] = json!("hosted_vault");
    assert!(serde_json::from_value::<SecretRef>(unsupported).is_err());

    let mut missing_staleness = valid_secret_ref_value();
    missing_staleness
        .as_object_mut()
        .expect("test fixture object")
        .remove("staleness_token");
    assert!(serde_json::from_value::<SecretRef>(missing_staleness).is_err());
}

#[test]
fn safe_secret_ref_roundtrips_without_raw_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let secret_ref = SecretRef::from_value(valid_secret_ref_value())?;

    let serialized = serde_json::to_value(&secret_ref)?;
    let parsed = SecretRef::from_value(serialized.clone())?;

    assert_eq!(parsed, secret_ref);
    assert_eq!(
        serialized["safe_summary"]["label"],
        "env:SHACS_PRD001_HAPPY_SECRET"
    );
    assert!(!serialized
        .to_string()
        .contains("sk-prd001-raw-fixture-value"));
    Ok(())
}

#[test]
fn redaction_evidence_forces_no_raw_persistence_and_best_effort_limits() {
    let evidence = RedactionEvidence::for_secret_ref(
        RedactionEvidenceRef::new("red_prd001_env_happy"),
        SecretRefId::new("sec_prd001_env_happy"),
        "approval_request",
        "sha256:safe-summary",
    );

    assert!(!evidence.raw_value_persisted);
    assert!(evidence.best_effort);
    assert_eq!(
        evidence.redaction_profile,
        RedactionProfile::ShacsRedactionV1
    );
    assert!(evidence
        .limits
        .iter()
        .any(|limit| limit == "not_exfiltration_prevention"));
}
