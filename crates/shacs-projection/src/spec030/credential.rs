use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CredentialStatusProjection {
    pub availability: Spec030Availability,
    pub status: CredentialStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<CredentialSource>,
    pub fingerprint: CredentialFingerprintStatus,
    pub refresh_serialization: RefreshSerializationStatus,
}

impl CredentialStatusProjection {
    pub(super) const fn unavailable() -> Self {
        Self {
            availability: Spec030Availability::Unavailable,
            status: CredentialStatus::Unavailable,
            source: None,
            fingerprint: CredentialFingerprintStatus::Unavailable,
            refresh_serialization: RefreshSerializationStatus::Unavailable,
        }
    }
}

impl From<shacs_config::CredentialStatusSnapshot> for CredentialStatusProjection {
    fn from(snapshot: shacs_config::CredentialStatusSnapshot) -> Self {
        Self {
            availability: match snapshot.status {
                shacs_config::CredentialStatus::Resolved => Spec030Availability::Available,
                shacs_config::CredentialStatus::Missing
                | shacs_config::CredentialStatus::Expired
                | shacs_config::CredentialStatus::RefreshFailed => Spec030Availability::Degraded,
            },
            status: match snapshot.status {
                shacs_config::CredentialStatus::Resolved => CredentialStatus::Resolved,
                shacs_config::CredentialStatus::Missing => CredentialStatus::Missing,
                shacs_config::CredentialStatus::Expired => CredentialStatus::Expired,
                shacs_config::CredentialStatus::RefreshFailed => CredentialStatus::RefreshFailed,
            },
            source: snapshot.source.map(|source| match source {
                shacs_config::CredentialSource::RuntimeOverride => {
                    CredentialSource::RuntimeOverride
                }
                shacs_config::CredentialSource::Environment => CredentialSource::Environment,
                shacs_config::CredentialSource::LocalAuthStore => CredentialSource::LocalAuthStore,
                shacs_config::CredentialSource::Command => CredentialSource::Command,
                shacs_config::CredentialSource::ProviderLiteral => CredentialSource::ProviderConfig,
            }),
            fingerprint: match snapshot.fingerprint {
                shacs_config::CredentialFingerprintStatus::Current => {
                    CredentialFingerprintStatus::Current
                }
                shacs_config::CredentialFingerprintStatus::Stale => {
                    CredentialFingerprintStatus::Stale
                }
                shacs_config::CredentialFingerprintStatus::Unavailable => {
                    CredentialFingerprintStatus::Unavailable
                }
            },
            refresh_serialization: match snapshot.refresh_serialization {
                shacs_config::RefreshSerializationStatus::Active => {
                    RefreshSerializationStatus::Active
                }
                shacs_config::RefreshSerializationStatus::Inactive => {
                    RefreshSerializationStatus::Inactive
                }
            },
        }
    }
}
