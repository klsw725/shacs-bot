mod spec030_provider_credential_support;

use shacs_config::{
    AuthStore, CredentialFamily, CredentialFingerprint, CredentialSourceDeclaration,
    LocalAuthStore, ProviderAuth, ProviderConfig, ProvidersConfig, RawCredential,
};
use shacs_core::runtime::trusted_runtime::{Spec030FactStore, WorkspaceTrustObservation};
use shacs_core::runtime::{
    CredentialResolvingProviderClient, ProviderClientResolutionRequest,
    ProviderCredentialInvocation, ProviderCredentialRuntime,
};
use shacs_projection::{CredentialSource, CredentialStatus, Spec030ProjectionProvider};
use shacs_providers::{
    LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRegistry, ProviderRequest,
};
use spec030_provider_credential_support::{request, serve_chat_responses};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct FakeTransport {
    called: AtomicBool,
}

impl ProviderClient for FakeTransport {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.called.store(true, Ordering::SeqCst);
        Ok(LlmResponse::default())
    }

    fn chat_stream(
        &self,
        request: ProviderRequest,
        _on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}

#[test]
fn spec030_provider_runtime_override_reaches_real_transport() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (api_base, capture) = serve_chat_responses(1)?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime = Arc::new(
        ProviderCredentialRuntime::new(root.path().join("auth.json"), root.path(), facts.clone())
            .with_environment("OPENAI_API_KEY", "environment-value"),
    );
    let providers = providers(api_base, "literal-value");

    let client = CredentialResolvingProviderClient::new("openai", "gpt-4o", providers, runtime)
        .with_invocation(ProviderCredentialInvocation::new(
            Some(RawCredential::api_key("runtime-value")),
            shacs_core::controlled_child::ControlledChildAbort::new(),
        ));
    client.chat(request())?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer runtime-value"));
    let credential =
        shacs_core::runtime::trusted_runtime::LocalSpec030ProjectionProvider::new(facts)
            .projection()
            .credential()
            .clone();
    assert_eq!(credential.status, CredentialStatus::Resolved);
    assert_eq!(credential.source, Some(CredentialSource::RuntimeOverride));
    Ok(())
}

#[test]
fn spec030_provider_environment_reaches_transport_before_literal() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (api_base, capture) = serve_chat_responses(1)?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime = ProviderCredentialRuntime::new(root.path().join("auth.json"), root.path(), facts)
        .with_environment("OPENAI_API_KEY", "environment-distinct");

    let registry = ProviderRegistry::new();
    let providers = providers(api_base, "literal-distinct");
    let resolved = runtime.resolve_provider_client(ProviderClientResolutionRequest {
        registry: &registry,
        requested_provider: "openai",
        model: "gpt-4o",
        providers: &providers,
    })?;
    resolved.client.chat(request())?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer environment-distinct"));
    assert!(!captured.contains("literal-distinct"));
    Ok(())
}

#[test]
fn spec030_fake_transport_still_runs_production_credential_resolution() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime = Arc::new(
        ProviderCredentialRuntime::new(root.path().join("auth.json"), root.path(), facts.clone())
            .with_environment("OPENAI_API_KEY", "environment-distinct"),
    );
    let fake = Arc::new(FakeTransport {
        called: AtomicBool::new(false),
    });
    let client = CredentialResolvingProviderClient::new(
        "openai",
        "gpt-4o",
        providers("http://127.0.0.1:1/v1".to_owned(), "literal-distinct"),
        runtime,
    )
    .with_transport_override(fake.clone());

    // When
    client.chat(request())?;

    // Then
    let credential =
        shacs_core::runtime::trusted_runtime::LocalSpec030ProjectionProvider::new(facts)
            .projection()
            .credential()
            .clone();
    assert!(fake.called.load(Ordering::SeqCst));
    assert_eq!(credential.status, CredentialStatus::Resolved);
    assert_eq!(credential.source, Some(CredentialSource::Environment));
    Ok(())
}

#[test]
fn spec030_provider_stale_local_source_transitions_to_command_transport(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), ProviderAuth::api_key("stale-local"));
    auth.set_fingerprint(
        "openai",
        CredentialFingerprint::from_descriptor("old-declaration"),
    );
    LocalAuthStore::new(&auth_path).save(&auth)?;
    let (api_base, capture) = serve_chat_responses(1)?;
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime = ProviderCredentialRuntime::new(auth_path, root.path(), facts.clone())
        .with_declaration(
            "openai",
            CredentialSourceDeclaration {
                family: CredentialFamily::ApiKey,
                environment: None,
                local_auth: true,
                command: Some("printf command-current".to_owned()),
            },
        );
    let providers = providers(api_base, "literal-distinct");
    let registry = ProviderRegistry::new();

    let resolved = runtime.resolve_provider_client(ProviderClientResolutionRequest {
        registry: &registry,
        requested_provider: "openai",
        model: "gpt-4o",
        providers: &providers,
    })?;
    resolved.client.chat(request())?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    assert!(captured
        .to_ascii_lowercase()
        .contains("authorization: bearer command-current"));
    let credential =
        shacs_core::runtime::trusted_runtime::LocalSpec030ProjectionProvider::new(facts)
            .projection()
            .credential()
            .clone();
    assert_eq!(credential.source, Some(CredentialSource::Command));
    Ok(())
}

fn providers(api_base: String, literal: &str) -> ProvidersConfig {
    ProvidersConfig::from([(
        "openai".to_owned(),
        ProviderConfig {
            api_key: Some(literal.to_owned()),
            api_base: Some(api_base),
            ..ProviderConfig::default()
        },
    )])
}
