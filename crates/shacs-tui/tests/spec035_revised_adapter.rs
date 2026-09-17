use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec035OwnerSurface, Spec035RevisedOwnerFacts, Spec035TransportMutationRejection,
};
use shacs_tui::revised_projection_view::{
    spec035_revised_json_view, spec035_transport_rejection_view,
};
use std::error::Error;

const SENSITIVE_VALUES: [(&str, &str); 6] = [
    ("secret", "sk-spec035-adapter-secret"),
    (
        "credential_url",
        "https://user:password@example.invalid/path",
    ),
    ("absolute_path", "/tmp/spec035-private"),
    ("process_handle", "spec035-process-handle-4242"),
    ("stdout", "spec035-raw-stdout"),
    ("stderr", "spec035-raw-stderr"),
];

#[test]
fn tui_adapter_preserves_canonical_projection_and_rejects_sensitive_input(
) -> Result<(), Box<dyn Error>> {
    // Given: owner-backed output and independent forged sensitive fields.
    let trusted_runtime =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);
    let canonical = serde_json::to_string(&project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(Spec035OwnerSurface::Tui, &trusted_runtime),
    )?)?;

    // When: the owner output crosses the TUI view adapter boundary.
    let rendered = spec035_revised_json_view(&canonical)?;

    // Then: canonical fields are unchanged and every sensitive field is rejected independently.
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&rendered)?,
        serde_json::from_str::<serde_json::Value>(&canonical)?
    );
    for (field, sentinel) in SENSITIVE_VALUES {
        let mut sensitive: serde_json::Value = serde_json::from_str(&canonical)?;
        sensitive[field] = sentinel.into();
        let rejected = spec035_revised_json_view(&serde_json::to_string(&sensitive)?)
            .expect_err("unsafe fixture must fail");
        assert!(!rejected.to_string().contains(sentinel));
    }
    Ok(())
}

#[test]
fn tui_adapter_displays_canonical_unsupported_capability_without_side_effects() {
    let rejection = Spec035TransportMutationRejection::capability_unavailable();

    let rendered = spec035_transport_rejection_view(&rejection);

    assert_eq!(rendered, "unsupported: capability_unavailable");
}
