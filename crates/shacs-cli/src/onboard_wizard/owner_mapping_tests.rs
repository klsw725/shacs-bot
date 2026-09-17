use std::error::Error;

use shacs_config::{
    Config, CredentialFamily, CredentialResolutionInput, ProviderConfig, RawCredential,
};
use shacs_core::runtime::trusted_runtime::{
    build_trusted_runtime_projection, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_projection::{
    HookDenialProjection, HookDenialReason, HookRuntimeProjection, HookRuntimeStatus,
    Spec030Availability, Spec030RuntimeProjection, Spec030UnavailableReason,
};

use super::readiness;

#[test]
fn onboard_wizard_preserves_available_and_missing_credential_owner_inputs(
) -> Result<(), Box<dyn Error>> {
    for environment in [
        Some(RawCredential::api_key("isolated-owner-sentinel")),
        None,
    ] {
        let provider = ProviderConfig::default();
        let declaration =
            provider.credential_declaration(CredentialFamily::ApiKey, Some("FIXTURE_KEY"));
        let resolved = declaration.resolve(
            &provider,
            CredentialResolutionInput {
                environment,
                ..Default::default()
            },
        );
        let status = match resolved {
            Ok(credential) => credential.status(),
            Err(error) => error.status(),
        };
        let owner = Spec030FactStore::new(WorkspaceTrustObservation::Untrusted);
        owner.record_credential_status(status)?;
        let projection = build_trusted_runtime_projection(owner.snapshot().into_input())?;

        let actual = serde_json::to_value(readiness::external_owner_facts(
            &Config::default(),
            &projection,
        )?)?;

        assert_eq!(
            actual[0]["projection"]["credential"],
            serde_json::to_value(projection.credential())?
        );
        assert_eq!(actual[0]["projection"]["raw_content_possible"], true);
        assert_eq!(
            actual[0]["projection"]["disclosure_surfaces"],
            serde_json::to_value(&projection.disclosure().surfaces)?
        );
        assert!(!actual.to_string().contains("isolated-owner-sentinel"));
        println!("credential-owner-input={status:?} output={}", actual[0]);
    }
    Ok(())
}

#[test]
fn onboard_wizard_preserves_denial_without_creating_approval() -> Result<(), Box<dyn Error>> {
    for (reason, kind, state) in [
        (
            HookDenialReason::UserDenied,
            "ephemeral_confirmation",
            "confirmation_denied",
        ),
        (
            HookDenialReason::HeadlessConfirmationDenied,
            "ephemeral_confirmation",
            "headless_confirmation_denied",
        ),
        (
            HookDenialReason::ExtensionBlocked,
            "hook_denial",
            "hook_denied",
        ),
    ] {
        let owner = Spec030FactStore::new(WorkspaceTrustObservation::Untrusted);
        owner.update_hooks(HookRuntimeProjection {
            availability: Spec030Availability::Available,
            status: HookRuntimeStatus::Active,
            registered_handlers: 1,
            diagnostics: Vec::new(),
            recent_denials: vec![HookDenialProjection {
                hook_ref: "hook:fixture".into(),
                call_ref: "call:fixture".into(),
                reason,
            }],
        })?;
        let projection = build_trusted_runtime_projection(owner.snapshot().into_input())?;

        let actual = serde_json::to_value(readiness::external_owner_facts(
            &Config::default(),
            &projection,
        )?)?;

        let decisions = actual[0]["projection"]["decisions"]
            .as_array()
            .ok_or("missing decisions")?;
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0]["kind"], kind);
        assert_eq!(decisions[0]["details"]["state"], state);
        assert_eq!(decisions[0]["details"]["call_ref"], "call:fixture");
        assert!(!actual.to_string().contains("approval"));
        println!("denial-owner-input={reason:?} output={}", actual[0]);
    }
    Ok(())
}

#[test]
fn onboard_wizard_preserves_missing_owner_evidence() -> Result<(), Box<dyn Error>> {
    let projection =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);

    let actual = serde_json::to_value(readiness::external_owner_facts(
        &Config::default(),
        &projection,
    )?)?;

    assert_eq!(
        actual[0]["projection"]["credential"],
        serde_json::to_value(projection.credential())?
    );
    assert_eq!(
        actual[0]["projection"]["unavailable_reason"],
        "ownerFactsMissing"
    );
    assert_eq!(actual[1]["declarations"], serde_json::json!([]));
    println!("missing-owner-output={actual}");
    Ok(())
}

#[test]
fn onboard_wizard_preserves_explicit_source_kinds_and_profile_selection(
) -> Result<(), Box<dyn Error>> {
    let config: Config = serde_json::from_value(serde_json::json!({
        "profiles": {"providers": {"fixture": {"provider": "openrouter", "credentialSource": {
            "schemaVersion": 1, "localAuth": false,
            "sources": [{"kind": "literal"}, {"kind": "local_auth_entry", "entry": "private-entry"}]
        }}}, "selection": {"provider": "fixture"}}
    }))?;
    let projection =
        Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing);

    let actual = serde_json::to_value(readiness::external_owner_facts(&config, &projection)?)?;

    let declaration = &actual[1]["declarations"][0];
    assert_eq!(declaration["literal"], true);
    assert_eq!(declaration["local_auth_entry"], true);
    assert_eq!(declaration["local_auth"], false);
    assert_eq!(declaration["selected_profile"], true);
    assert!(!actual.to_string().contains("private-entry"));
    Ok(())
}
