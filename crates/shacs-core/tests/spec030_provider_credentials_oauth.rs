use shacs_config::{
    AuthStore, CredentialFamily, CredentialSourceDeclaration, OAuthRefresh, OAuthRefreshRequest,
    ProviderAuth, ProviderConfig, ProvidersConfig,
};
use shacs_core::runtime::trusted_runtime::{
    LocalSpec030ProjectionProvider, Spec030FactStore, WorkspaceTrustObservation,
};
use shacs_core::runtime::{
    CredentialResolvingProviderClient, OAuthCredentialRefresher, ProviderClientResolutionRequest,
    ProviderCredentialRuntime,
};
use shacs_providers::{ProviderClient, ProviderRegistry};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

mod spec030_provider_credential_support;
use spec030_provider_credential_support::{request, serve_chat_responses};

struct CountingRefresher(AtomicUsize);

impl OAuthCredentialRefresher for CountingRefresher {
    fn refresh(
        &self,
        _provider_id: &str,
        request: &OAuthRefreshRequest<'_>,
    ) -> Result<OAuthRefresh, String> {
        assert_eq!(request.refresh_token(), "refresh-old");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(OAuthRefresh::new("access-renewed", None, u64::MAX))
    }
}

#[test]
fn spec030_provider_expired_oauth_refreshes_exactly_once_in_production_builder(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let declaration = oauth_declaration();
    let mut auth = AuthStore::default();
    auth.providers.insert(
        "openai_codex".to_owned(),
        ProviderAuth {
            kind: "oauth".to_owned(),
            access: "access-expired".to_owned(),
            refresh: Some("refresh-old".to_owned()),
            expires: Some(1),
            account_id: None,
        },
    );
    auth.set_fingerprint("openai_codex", declaration.fingerprint());
    shacs_config::LocalAuthStore::new(&auth_path).save(&auth)?;
    let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
    let runtime = ProviderCredentialRuntime::new(
        auth_path,
        root.path(),
        Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
    )
    .with_declaration("openai_codex", declaration)
    .with_oauth_refresher(refresher.clone());
    let providers = ProvidersConfig::from([("openai_codex".to_owned(), ProviderConfig::default())]);

    let registry = ProviderRegistry::new();
    for _ in 0..2 {
        runtime.resolve_provider_client(ProviderClientResolutionRequest {
            registry: &registry,
            requested_provider: "openai_codex",
            model: "gpt-5",
            providers: &providers,
        })?;
    }

    assert_eq!(refresher.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        shacs_config::LocalAuthStore::new(root.path().join("auth.json"))
            .load()?
            .providers["openai_codex"]
            .access,
        "access-renewed"
    );
    Ok(())
}

#[test]
fn spec030_long_lived_provider_refreshes_once_after_later_oauth_expiry(
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let declaration = oauth_declaration();
    let mut auth = AuthStore::default();
    auth.providers.insert(
        "github_copilot".to_owned(),
        ProviderAuth {
            kind: "oauth".to_owned(),
            access: "access-initial".to_owned(),
            refresh: Some("refresh-old".to_owned()),
            expires: Some(u64::MAX),
            account_id: None,
        },
    );
    auth.set_fingerprint("github_copilot", declaration.fingerprint());
    let store = shacs_config::LocalAuthStore::new(&auth_path);
    store.save(&auth)?;
    let refresher = Arc::new(CountingRefresher(AtomicUsize::new(0)));
    let facts = Spec030FactStore::new(WorkspaceTrustObservation::Trusted);
    let runtime = Arc::new(
        ProviderCredentialRuntime::new(auth_path, root.path(), facts.clone())
            .with_declaration("github_copilot", declaration)
            .with_oauth_refresher(refresher.clone()),
    );
    let (api_base, capture) = serve_chat_responses(3)?;
    let providers = ProvidersConfig::from([(
        "github_copilot".to_owned(),
        ProviderConfig {
            api_base: Some(api_base),
            ..ProviderConfig::default()
        },
    )]);
    let client =
        CredentialResolvingProviderClient::new("github_copilot", "gpt-4o", providers, runtime);

    client.chat(request())?;
    let mut expired = store.load()?;
    expired
        .providers
        .get_mut("github_copilot")
        .ok_or("copilot auth missing")?
        .expires = Some(1);
    store.save(&expired)?;
    client.chat(request())?;
    client.chat(request())?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    assert_eq!(refresher.0.load(Ordering::SeqCst), 1);
    assert!(captured.contains("Bearer access-initial"));
    assert_eq!(captured.matches("Bearer access-renewed").count(), 2);
    let projection = shacs_projection::Spec030ProjectionProvider::projection(
        &LocalSpec030ProjectionProvider::new(facts),
    );
    assert_eq!(
        projection.credential().status,
        shacs_projection::CredentialStatus::Resolved
    );
    Ok(())
}

fn oauth_declaration() -> CredentialSourceDeclaration {
    CredentialSourceDeclaration {
        family: CredentialFamily::OAuth,
        environment: None,
        local_auth: true,
        command: None,
    }
}
