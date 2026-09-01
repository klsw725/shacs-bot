mod spec030_provider_credential_image_support;

use shacs_config::{
    AuthStore, LocalAuthStore, OAuthRefresh, OAuthRefreshRequest, ProviderAuth, ProviderConfig,
    ProvidersConfig, RawCredential,
};
use shacs_core::runtime::trusted_runtime::{Spec030FactStore, WorkspaceTrustObservation};
use shacs_core::runtime::{
    CredentialResolvingImageGenerationClient, OAuthCredentialRefresher,
    ProviderCredentialClientConfig, ProviderCredentialRuntime,
};
use shacs_providers::{ImageGenerationClient, ImageGenerationRequest, ProviderError};
use spec030_provider_credential_image_support::serve_image_responses;
use std::collections::BTreeMap;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct ImageOAuthRefresher(AtomicUsize);

impl OAuthCredentialRefresher for ImageOAuthRefresher {
    fn refresh(
        &self,
        _provider_id: &str,
        request: &OAuthRefreshRequest<'_>,
    ) -> Result<OAuthRefresh, String> {
        if request.refresh_token() != "refresh-old" {
            return Err("unexpected refresh token".to_owned());
        }
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(OAuthRefresh::new("oauth-renewed", None, u64::MAX)
            .with_account_id(Some("account-renewed".to_owned())))
    }
}

#[test]
fn spec030_image_client_uses_replaced_local_auth_without_rebuild() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let store = LocalAuthStore::new(&auth_path);
    store.save(&auth_store("image-token-old"))?;
    let (api_base, capture) = serve_image_responses(2)?;
    let runtime = Arc::new(ProviderCredentialRuntime::new(
        auth_path,
        root.path(),
        Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
    ));
    let client = CredentialResolvingImageGenerationClient::new(
        ProviderCredentialClientConfig {
            requested_provider: "openai".to_owned(),
            model: "gpt-image-2".to_owned(),
            providers: ProvidersConfig::from([(
                "openai".to_owned(),
                ProviderConfig {
                    api_base: Some(api_base),
                    ..ProviderConfig::default()
                },
            )]),
        },
        runtime,
    );

    // When
    client.generate_image(ImageGenerationRequest::new("first"))?;
    store.save(&auth_store("image-token-new"))?;
    client.generate_image(ImageGenerationRequest::new("second"))?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    // Then
    let captured = captured.to_ascii_lowercase();
    assert_eq!(
        captured
            .matches("authorization: bearer image-token-old")
            .count(),
        1
    );
    assert_eq!(
        captured
            .matches("authorization: bearer image-token-new")
            .count(),
        1
    );
    Ok(())
}

#[test]
fn spec030_image_client_rejects_logout_without_rebuild() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let store = LocalAuthStore::new(&auth_path);
    store.save(&auth_store("image-token"))?;
    let (api_base, capture) = serve_image_responses(1)?;
    let runtime = Arc::new(ProviderCredentialRuntime::new(
        auth_path,
        root.path(),
        Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
    ));
    let client = CredentialResolvingImageGenerationClient::new(
        ProviderCredentialClientConfig {
            requested_provider: "openai".to_owned(),
            model: "gpt-image-2".to_owned(),
            providers: ProvidersConfig::from([(
                "openai".to_owned(),
                ProviderConfig {
                    api_base: Some(api_base),
                    ..ProviderConfig::default()
                },
            )]),
        },
        runtime,
    );

    // When
    client.generate_image(ImageGenerationRequest::new("first"))?;
    store.save(&AuthStore::default())?;
    let error = client
        .generate_image(ImageGenerationRequest::new("second"))
        .expect_err("logged-out image request unexpectedly succeeded");
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    // Then
    assert!(matches!(
        error,
        ProviderError::AuthRequired { provider_id } if provider_id == "openai"
    ));
    assert_eq!(captured.matches("POST /images/generations").count(), 1);
    Ok(())
}

#[test]
fn spec030_image_client_refreshes_expired_codex_oauth_without_rebuild() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let auth_path = root.path().join("auth.json");
    let store = LocalAuthStore::new(&auth_path);
    let mut auth = AuthStore::default();
    auth.providers.insert(
        "openai_codex".to_owned(),
        ProviderAuth {
            kind: "oauth".to_owned(),
            access: "oauth-initial".to_owned(),
            refresh: Some("refresh-old".to_owned()),
            expires: Some(u64::MAX),
            account_id: Some("account-initial".to_owned()),
        },
    );
    store.save(&auth)?;
    let (api_base, capture) = serve_image_responses(2)?;
    let refresher = Arc::new(ImageOAuthRefresher(AtomicUsize::new(0)));
    let runtime = Arc::new(
        ProviderCredentialRuntime::new(
            auth_path,
            root.path(),
            Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
        )
        .with_oauth_refresher(refresher.clone()),
    );
    let client = CredentialResolvingImageGenerationClient::new(
        ProviderCredentialClientConfig {
            requested_provider: "openai_codex".to_owned(),
            model: "gpt-image-2".to_owned(),
            providers: ProvidersConfig::from([(
                "openai_codex".to_owned(),
                ProviderConfig {
                    api_base: Some(api_base),
                    extra_headers: Some(BTreeMap::from([
                        ("authorization".to_owned(), "Bearer oauth-stale".to_owned()),
                        ("chatgpt-account-id".to_owned(), "account-stale".to_owned()),
                    ])),
                    ..ProviderConfig::default()
                },
            )]),
        },
        runtime,
    );

    // When
    client.generate_image(ImageGenerationRequest::new("first"))?;
    let mut expired = store.load()?;
    expired
        .providers
        .get_mut("openai_codex")
        .ok_or("Codex auth missing")?
        .expires = Some(1);
    store.save(&expired)?;
    client.generate_image(ImageGenerationRequest::new("second"))?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    // Then
    let captured = captured.to_ascii_lowercase();
    assert_eq!(refresher.0.load(Ordering::SeqCst), 1);
    assert!(captured.contains("authorization: bearer oauth-initial"));
    assert!(captured.contains("authorization: bearer oauth-renewed"));
    assert!(captured.contains("chatgpt-account-id: account-initial"));
    assert!(captured.contains("chatgpt-account-id: account-renewed"));
    assert!(!captured.contains("oauth-stale"));
    assert!(!captured.contains("account-stale"));
    Ok(())
}

#[test]
fn spec030_codex_runtime_override_removes_stale_config_auth_headers() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let (api_base, capture) = serve_image_responses(1)?;
    let runtime = Arc::new(
        ProviderCredentialRuntime::new(
            root.path().join("auth.json"),
            root.path(),
            Spec030FactStore::new(WorkspaceTrustObservation::Trusted),
        )
        .with_runtime_override(
            "openai_codex",
            RawCredential::oauth("oauth-runtime", None, None),
        ),
    );
    let client = CredentialResolvingImageGenerationClient::new(
        ProviderCredentialClientConfig {
            requested_provider: "openai_codex".to_owned(),
            model: "gpt-image-2".to_owned(),
            providers: ProvidersConfig::from([(
                "openai_codex".to_owned(),
                ProviderConfig {
                    api_base: Some(api_base),
                    extra_headers: Some(BTreeMap::from([
                        ("Authorization".to_owned(), "Bearer oauth-stale".to_owned()),
                        ("ChatGPT-Account-Id".to_owned(), "account-stale".to_owned()),
                    ])),
                    ..ProviderConfig::default()
                },
            )]),
        },
        runtime,
    );

    // When
    client.generate_image(ImageGenerationRequest::new("runtime override"))?;
    let captured = capture.join().map_err(|_| "capture thread panicked")??;

    // Then
    let captured = captured.to_ascii_lowercase();
    assert!(captured.contains("authorization: bearer oauth-runtime"));
    assert!(!captured.contains("oauth-stale"));
    assert!(!captured.contains("chatgpt-account-id"));
    Ok(())
}

fn auth_store(access: &str) -> AuthStore {
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), ProviderAuth::api_key(access));
    auth
}
