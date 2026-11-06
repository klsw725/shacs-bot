use super::status::{
    CredentialError, CredentialFingerprintStatus, CredentialSource, CredentialStatus,
    CredentialStatusSnapshot, RefreshSerializationStatus,
};
use super::types::{CredentialFingerprint, RawCredential, ResolvedCredential};
use crate::{load_auth_store, save_auth_store_to_path, AuthStore, ConfigError, ProviderAuth};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalAuthStore {
    path: PathBuf,
}

impl LocalAuthStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AuthStore, ConfigError> {
        load_auth_store(&self.path)
    }

    pub fn save(&self, store: &AuthStore) -> Result<(), ConfigError> {
        let _lock = self.lock()?;
        save_auth_store_to_path(store, &self.path)
    }

    pub fn resolve_oauth<F>(
        &self,
        provider_id: &str,
        expected_fingerprint: &CredentialFingerprint,
        now: u64,
        refresh: F,
    ) -> Result<ResolvedCredential, CredentialError>
    where
        F: FnOnce(&OAuthRefreshRequest<'_>) -> Result<OAuthRefresh, String>,
    {
        let _lock = self
            .lock()
            .map_err(|_| store_error(CredentialStatus::RefreshFailed, false))?;
        let mut store = self
            .load()
            .map_err(|_| store_error(CredentialStatus::RefreshFailed, false))?;
        let auth = stored_oauth(&store, provider_id, expected_fingerprint)?;
        if unexpired(&auth, now) {
            return Ok(local_resolved(auth));
        }
        let refresh_token = auth
            .refresh
            .as_deref()
            .ok_or_else(|| store_error(CredentialStatus::Expired, false))?;
        let renewed = refresh(&OAuthRefreshRequest { refresh_token })
            .map_err(|_| store_error(CredentialStatus::RefreshFailed, false))?;
        let refreshed = ProviderAuth {
            kind: "oauth".to_owned(),
            access: renewed.access,
            refresh: renewed.refresh.or(auth.refresh),
            expires: Some(renewed.expires),
            account_id: renewed.account_id.or(auth.account_id),
        };
        store
            .providers
            .insert(provider_id.to_owned(), refreshed.clone());
        store.set_fingerprint(provider_id, expected_fingerprint.clone());
        save_auth_store_to_path(&store, &self.path)
            .map_err(|_| store_error(CredentialStatus::RefreshFailed, false))?;
        Ok(local_resolved(refreshed))
    }

    /// The stored OAuth credential without taking the lock or refreshing,
    /// for commands that write nothing: an expired one is refused. Saves
    /// replace the file whole, so a read without the lock still sees one
    /// version of it.
    pub fn read_oauth(
        &self,
        provider_id: &str,
        expected_fingerprint: &CredentialFingerprint,
        now: u64,
    ) -> Result<ResolvedCredential, CredentialError> {
        let store = self
            .load()
            .map_err(|_| store_error(CredentialStatus::Missing, false))?;
        let auth = stored_oauth(&store, provider_id, expected_fingerprint)?;
        if unexpired(&auth, now) {
            Ok(local_resolved(auth))
        } else {
            Err(store_error(CredentialStatus::Expired, false))
        }
    }

    fn lock(&self) -> Result<File, ConfigError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let lock_path = self.path.with_extension("json.lock");
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(lock_path).map_err(ConfigError::Io)?;
        file.lock_exclusive().map_err(ConfigError::Io)?;
        Ok(file)
    }
}

pub struct OAuthRefreshRequest<'a> {
    refresh_token: &'a str,
}

impl OAuthRefreshRequest<'_> {
    pub const fn refresh_token(&self) -> &str {
        self.refresh_token
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct OAuthRefresh {
    access: String,
    refresh: Option<String>,
    expires: u64,
    account_id: Option<String>,
}

impl std::fmt::Debug for OAuthRefresh {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OAuthRefresh([REDACTED])")
    }
}

impl OAuthRefresh {
    pub fn new(access: impl Into<String>, refresh: Option<String>, expires: u64) -> Self {
        Self {
            access: access.into(),
            refresh,
            expires,
            account_id: None,
        }
    }

    pub fn with_account_id(mut self, account_id: Option<String>) -> Self {
        self.account_id = account_id;
        self
    }
}

fn stored_oauth(
    store: &AuthStore,
    provider_id: &str,
    expected_fingerprint: &CredentialFingerprint,
) -> Result<ProviderAuth, CredentialError> {
    let fingerprint_is_current = match store.fingerprint(provider_id) {
        Some(actual) => &actual == expected_fingerprint,
        None => true,
    };
    if !fingerprint_is_current {
        return Err(store_error(CredentialStatus::Missing, true));
    }
    let auth = store
        .providers
        .get(provider_id)
        .cloned()
        .ok_or_else(|| store_error(CredentialStatus::Missing, false))?;
    if auth.kind != "oauth" {
        return Err(store_error(CredentialStatus::Missing, false));
    }
    Ok(auth)
}

fn unexpired(auth: &ProviderAuth, now: u64) -> bool {
    auth.expires.is_none_or(|expires| expires > now)
}

fn local_resolved(auth: ProviderAuth) -> ResolvedCredential {
    let ProviderAuth {
        access,
        refresh,
        expires,
        account_id,
        ..
    } = auth;
    ResolvedCredential {
        raw: RawCredential::oauth(access, refresh, expires),
        account_id,
        source: CredentialSource::LocalAuthStore,
        fingerprint: CredentialFingerprintStatus::Current,
        refresh_serialization: RefreshSerializationStatus::Active,
    }
}

const fn store_error(status: CredentialStatus, stale: bool) -> CredentialError {
    CredentialError::new(
        CredentialStatusSnapshot {
            status,
            source: Some(CredentialSource::LocalAuthStore),
            fingerprint: if stale {
                CredentialFingerprintStatus::Stale
            } else {
                CredentialFingerprintStatus::Current
            },
            fingerprint_stale: stale,
            refresh_serialization: RefreshSerializationStatus::Active,
        },
        "local auth credential resolution failed",
    )
}
