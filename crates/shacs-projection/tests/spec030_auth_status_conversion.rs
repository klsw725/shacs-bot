use shacs_config::{
    CredentialFamily, CredentialResolutionInput, CredentialSourceDeclaration, ProviderConfig,
    RawCredential,
};
use shacs_projection::{
    CredentialSource, CredentialStatus, CredentialStatusProjection, Spec030Availability,
};

#[test]
fn spec030_auth_status_conversion_excludes_raw_material() {
    let declaration = CredentialSourceDeclaration {
        family: CredentialFamily::OAuth,
        environment: Some("OAUTH_TOKEN".to_owned()),
        local_auth: true,
        command: None,
    };
    let resolved = declaration
        .resolve(
            &ProviderConfig::default(),
            CredentialResolutionInput {
                runtime_override: Some(RawCredential::oauth(
                    "ACCESS_CANARY",
                    Some("REFRESH_CANARY".to_owned()),
                    None,
                )),
                ..CredentialResolutionInput::default()
            },
        )
        .expect("credential resolves");

    let projection = CredentialStatusProjection::from(resolved.status());
    let serialized = serde_json::to_string(&projection).expect("projection serializes");

    assert_eq!(projection.availability, Spec030Availability::Available);
    assert_eq!(projection.status, CredentialStatus::Resolved);
    assert_eq!(projection.source, Some(CredentialSource::RuntimeOverride));
    assert!(!serialized.contains("ACCESS_CANARY"));
    assert!(!serialized.contains("REFRESH_CANARY"));
}
