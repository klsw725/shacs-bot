use serde_json::json;
use shacs_projection::*;
use std::error::Error;

#[test]
fn spec031_reason_code_preserves_existing_wire_values_and_rejects_unknown(
) -> Result<(), Box<dyn Error>> {
    for reason in Spec031InclusionReason::ALL {
        let code = Spec031ReasonCode::from(reason);
        assert_eq!(serde_json::to_value(code)?, serde_json::to_value(reason)?);
    }

    let envelope = spec031_missing_external_owner_evidence(Spec031FixtureFamily::Readiness)?;
    let serialized = serde_json::to_value(&envelope)?;
    let parsed = Spec031Envelope::from_json_value(serialized)?;
    assert_eq!(
        parsed.reason().code,
        Spec031ReasonCode::MissingExternalOwnerEvidence
    );

    assert_eq!(
        serde_json::to_value(Spec031ReasonCode::RetryConsumed)?,
        json!("retry_consumed")
    );
    assert_eq!(
        serde_json::from_value::<Spec031ReasonCode>(json!("retry_consumed"))?,
        Spec031ReasonCode::RetryConsumed
    );

    let mut unknown_reason = serde_json::to_value(envelope)?;
    unknown_reason["reason"]["code"] = json!("not_a_reason");
    assert!(Spec031Envelope::from_json_value(unknown_reason).is_err());
    Ok(())
}
