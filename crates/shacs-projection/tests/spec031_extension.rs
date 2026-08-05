use shacs_projection::{
    spec031_extension_catalog, spec031_extension_diagnostic, Spec031ExtensionDiagnosticSeverity,
    Spec031ExtensionEnabledState, Spec031ExtensionProjection, Spec031ExtensionReadiness,
    Spec031ExtensionReason, Spec031ExtensionSurfaceKind, Spec031ExtensionSurfaceProjection,
    SPEC031_EXTENSION_SCHEMA_VERSION,
};

#[test]
fn spec031_extension_projection_roundtrips_canonical_state_and_redacted_diagnostics() {
    let projection = spec031_extension_catalog(vec![Spec031ExtensionProjection {
        extension_ref: "ext_sha256:abc".to_owned(),
        label: "demo-plugin".to_owned(),
        owner_source: "user_data".to_owned(),
        enabled_state: Spec031ExtensionEnabledState::Enabled,
        readiness: Spec031ExtensionReadiness::Blocked,
        reason: Spec031ExtensionReason::Blocked,
        diagnostics: vec![spec031_extension_diagnostic(
            Spec031ExtensionDiagnosticSeverity::Error,
            "missing_environment_refs",
            "token sk-spec031-extension-secret missing",
        )],
        surfaces: vec![Spec031ExtensionSurfaceProjection {
            kind: Spec031ExtensionSurfaceKind::Hook,
            name: "tool:before".to_owned(),
            execution_enabled: false,
        }],
    }]);

    let serialized = serde_json::to_string(&projection).expect("projection serializes");
    let parsed: shacs_projection::Spec031ExtensionCatalogProjection =
        serde_json::from_str(&serialized).expect("projection parses");

    assert_eq!(parsed.schema_version, SPEC031_EXTENSION_SCHEMA_VERSION);
    assert_eq!(
        parsed.extensions[0].readiness,
        Spec031ExtensionReadiness::Blocked
    );
    assert_eq!(parsed.extensions[0].reason, Spec031ExtensionReason::Blocked);
    assert!(!parsed.extensions[0].surfaces[0].execution_enabled);
    assert!(!serialized.contains("sk-spec031-extension-secret"));
    assert!(serialized.contains("[REDACTED]"));
}
