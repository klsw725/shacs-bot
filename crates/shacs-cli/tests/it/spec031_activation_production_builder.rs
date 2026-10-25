use std::error::Error;

#[path = "spec031_activation_production_builder/support.rs"]
mod support;

#[test]
fn production_builder_attaches_no_activation_ref_inside_leftover_app_bundle(
) -> Result<(), Box<dyn Error>> {
    // Given / When
    let snapshot = support::run()?;

    // Then
    assert!(snapshot
        .selected_resources
        .iter()
        .any(|resource| resource.identity == "resource:formatter"));
    assert!(snapshot
        .selected_resources
        .iter()
        .all(|resource| resource.activation_ref.is_none()));
    assert!(!snapshot.replay.live_execution_authorized);
    assert!(!serde_json::to_string(&snapshot)?.contains("authorization"));
    Ok(())
}
