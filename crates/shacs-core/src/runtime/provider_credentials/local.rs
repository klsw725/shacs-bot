use super::ProviderCredentialRuntime;
use shacs_config::{
    CredentialFamily, CredentialFingerprintStatus, CredentialResolutionInput, CredentialSource,
    CredentialSourceDeclaration, CredentialStatus, CredentialStatusSnapshot, LocalAuthStore,
    ProviderConfig, RawCredential, RefreshSerializationStatus, ResolvedCredential,
};
use std::time::{SystemTime, UNIX_EPOCH};

impl ProviderCredentialRuntime {
    pub(super) fn resolve_local(
        &self,
        provider_id: &str,
        declaration: &CredentialSourceDeclaration,
    ) -> Result<Option<ResolvedCredential>, shacs_config::CredentialError> {
        let store = LocalAuthStore::new(self.auth_path());
        let auth = store.load().map_err(|_| {
            shacs_config::CredentialError::from_status(
                CredentialStatusSnapshot {
                    status: CredentialStatus::Missing,
                    source: Some(CredentialSource::LocalAuthStore),
                    fingerprint: CredentialFingerprintStatus::Unavailable,
                    fingerprint_stale: false,
                    refresh_serialization: RefreshSerializationStatus::Active,
                },
                "local auth store could not be read",
            )
        })?;
        let Some(entry) = auth.providers.get(provider_id) else {
            return Ok(None);
        };
        match declaration.family {
            CredentialFamily::ApiKey if entry.kind == "apiKey" => declaration
                .resolve(
                    &ProviderConfig::default(),
                    CredentialResolutionInput {
                        local_auth: Some(RawCredential::api_key(&entry.access)),
                        local_auth_fingerprint: auth.fingerprint(provider_id),
                        ..CredentialResolutionInput::default()
                    },
                )
                .map(Some),
            CredentialFamily::OAuth if entry.kind == "oauth" => store
                .resolve_oauth(
                    provider_id,
                    &declaration.fingerprint(),
                    now_millis(),
                    |request| {
                        self.oauth_refresher
                            .as_ref()
                            .ok_or_else(|| "OAuth refresher is unavailable".to_owned())?
                            .refresh(provider_id, request)
                    },
                )
                .map(Some),
            CredentialFamily::ApiKey | CredentialFamily::OAuth => Ok(None),
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
