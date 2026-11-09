use super::{LocalAuthAccess, ProviderCredentialRuntime};
use shacs_config::{CredentialSourceDeclaration, LocalAuthStore, ResolvedCredential};
use std::time::{SystemTime, UNIX_EPOCH};

impl ProviderCredentialRuntime {
    /// The local OAuth credential at the place the slot search chose: read
    /// again and, unless this runtime writes nothing, refreshed and saved
    /// under the store's lock when expired.
    pub(super) fn read_local_oauth(
        &self,
        provider_id: &str,
        declaration: &CredentialSourceDeclaration,
    ) -> Result<ResolvedCredential, shacs_config::CredentialError> {
        let store = LocalAuthStore::new(self.auth_path());
        match self.local_auth_access {
            LocalAuthAccess::ReadOnly => {
                store.read_oauth(provider_id, &declaration.fingerprint(), now_millis())
            }
            LocalAuthAccess::RefreshAndSave => store.resolve_oauth(
                provider_id,
                &declaration.fingerprint(),
                now_millis(),
                |request| {
                    self.oauth_refresher
                        .as_ref()
                        .ok_or_else(|| "OAuth refresher is unavailable".to_owned())?
                        .refresh(provider_id, request)
                },
            ),
        }
    }
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
