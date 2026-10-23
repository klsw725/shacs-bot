use shacs_core::runtime::ActivationStatus;
use std::error::Error;

#[path = "spec031_activation_production_builder/support.rs"]
mod support;

#[test]
fn production_builder_persists_exact_admitted_activation_ref() -> Result<(), Box<dyn Error>> {
    // Given / When
    let snapshot = support::run(support::Scenario::active())?;

    // Then
    assert_eq!(
        snapshot.selected_resources[0].activation_ref.as_deref(),
        Some("activation:formatter:v1")
    );
    assert!(!snapshot.replay.live_execution_authorized);
    assert!(!serde_json::to_string(&snapshot)?.contains("authorization"));
    Ok(())
}

#[test]
fn production_builder_omits_non_admitted_activation_refs() -> Result<(), Box<dyn Error>> {
    for scenario in [
        support::Scenario::missing(),
        support::Scenario::status(ActivationStatus::Disabled),
        support::Scenario::status(ActivationStatus::Revoked),
        support::Scenario::digest_mismatch(),
    ] {
        // Given / When
        let snapshot = support::run(scenario)?;

        // Then
        assert_eq!(snapshot.selected_resources[0].activation_ref, None);
        assert!(!snapshot.replay.live_execution_authorized);
    }
    Ok(())
}
