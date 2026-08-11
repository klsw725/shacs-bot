use shacs_config::{
    AuthStore, CredentialFingerprint, CredentialStatus, LocalAuthStore, OAuthRefresh, ProviderAuth,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

fn oauth(access: &str, refresh: &str, expires: u64) -> ProviderAuth {
    ProviderAuth {
        kind: "oauth".to_owned(),
        access: access.to_owned(),
        refresh: Some(refresh.to_owned()),
        expires: Some(expires),
        account_id: Some("account-1".to_owned()),
    }
}

#[test]
fn spec030_auth_local_store_atomic_write_is_user_only() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), ProviderAuth::api_key("API_CANARY"));

    store.save(&auth)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(store.path())?.permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(store.load()?, auth);
    Ok(())
}

#[test]
fn spec030_auth_oauth_current_token_does_not_refresh() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let fingerprint = CredentialFingerprint::from_descriptor("oauth-source");
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), oauth("current", "refresh", 200));
    auth.set_fingerprint("openai", fingerprint.clone());
    store.save(&auth)?;

    let result = store.resolve_oauth("openai", &fingerprint, 100, |_| {
        panic!("current token must not refresh")
    })?;

    assert_eq!(result.status().status, CredentialStatus::Resolved);
    assert_eq!(result.transport().value(), "current");
    Ok(())
}

#[test]
fn spec030_auth_expired_oauth_refreshes_and_saves() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let fingerprint = CredentialFingerprint::from_descriptor("oauth-source");
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), oauth("expired", "refresh", 50));
    auth.set_fingerprint("openai", fingerprint.clone());
    store.save(&auth)?;

    let result = store.resolve_oauth("openai", &fingerprint, 100, |request| {
        assert_eq!(request.refresh_token(), "refresh");
        Ok(OAuthRefresh::new(
            "renewed",
            Some("next-refresh".to_owned()),
            300,
        ))
    })?;

    assert_eq!(result.transport().value(), "renewed");
    assert_eq!(store.load()?.providers["openai"].access, "renewed");
    Ok(())
}

#[test]
fn spec030_auth_refresh_failure_has_raw_free_status() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let fingerprint = CredentialFingerprint::from_descriptor("oauth-source");
    let mut auth = AuthStore::default();
    auth.providers.insert(
        "openai".to_owned(),
        oauth("ACCESS_CANARY", "REFRESH_CANARY", 50),
    );
    auth.set_fingerprint("openai", fingerprint.clone());
    store.save(&auth)?;

    let error = store
        .resolve_oauth("openai", &fingerprint, 100, |_| {
            Err("refresh denied".to_owned())
        })
        .expect_err("refresh fails");
    let serialized = serde_json::to_string(&error.status())?;

    assert_eq!(error.status().status, CredentialStatus::RefreshFailed);
    assert!(!serialized.contains("ACCESS_CANARY"));
    assert!(!serialized.contains("REFRESH_CANARY"));
    Ok(())
}

#[test]
fn spec030_auth_expired_oauth_without_refresh_token_reports_expired(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let fingerprint = CredentialFingerprint::from_descriptor("oauth-source");
    let mut auth = AuthStore::default();
    let mut expired = oauth("expired", "unused", 50);
    expired.refresh = None;
    auth.providers.insert("openai".to_owned(), expired);
    auth.set_fingerprint("openai", fingerprint.clone());
    store.save(&auth)?;

    let error = store
        .resolve_oauth("openai", &fingerprint, 100, |_| {
            panic!("refresh is unavailable")
        })
        .expect_err("expired token has no refresh path");

    assert_eq!(error.status().status, CredentialStatus::Expired);
    Ok(())
}

#[test]
fn spec030_auth_stale_oauth_source_is_not_refreshed() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let old = CredentialFingerprint::from_descriptor("old-source");
    let current = CredentialFingerprint::from_descriptor("current-source");
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), oauth("expired", "refresh", 50));
    auth.set_fingerprint("openai", old);
    store.save(&auth)?;

    let error = store
        .resolve_oauth("openai", &current, 100, |_| {
            panic!("stale source must not refresh")
        })
        .expect_err("stale source is rejected");

    assert_eq!(error.status().status, CredentialStatus::Missing);
    assert!(error.status().fingerprint_stale);
    Ok(())
}

#[test]
fn spec030_auth_concurrent_refresh_executes_exactly_once() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let store = LocalAuthStore::new(root.path().join("auth.json"));
    let fingerprint = CredentialFingerprint::from_descriptor("oauth-source");
    let mut auth = AuthStore::default();
    auth.providers
        .insert("openai".to_owned(), oauth("expired", "refresh", 50));
    auth.set_fingerprint("openai", fingerprint.clone());
    store.save(&auth)?;
    let calls = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(3));

    let handles = (0..2)
        .map(|_| {
            let store = store.clone();
            let fingerprint = fingerprint.clone();
            let calls = Arc::clone(&calls);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                store.resolve_oauth("openai", &fingerprint, 100, |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(OAuthRefresh::new("renewed", None, 300))
                })
            })
        })
        .collect::<Vec<_>>();
    start.wait();
    for handle in handles {
        let result = handle.join().expect("refresh thread")?;
        assert_eq!(result.transport().value(), "renewed");
    }

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
