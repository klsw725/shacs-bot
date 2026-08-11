use super::{Spec030FactStore, Spec030FactStoreError};
use shacs_config::CredentialStatusSnapshot;
use shacs_projection::CredentialStatusProjection;

impl Spec030FactStore {
    pub fn record_credential_status(
        &self,
        snapshot: CredentialStatusSnapshot,
    ) -> Result<(), Spec030FactStoreError> {
        self.update_credential(CredentialStatusProjection::from(snapshot))
    }
}
