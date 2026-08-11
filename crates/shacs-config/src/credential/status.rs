use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialStatus {
    Resolved,
    Missing,
    Expired,
    RefreshFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialSource {
    RuntimeOverride,
    Environment,
    LocalAuthStore,
    Command,
    ProviderLiteral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialFingerprintStatus {
    Current,
    Stale,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshSerializationStatus {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatusSnapshot {
    pub status: CredentialStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<CredentialSource>,
    pub fingerprint: CredentialFingerprintStatus,
    pub fingerprint_stale: bool,
    pub refresh_serialization: RefreshSerializationStatus,
}

pub struct CredentialError {
    status: CredentialStatusSnapshot,
    reason: &'static str,
}

impl CredentialError {
    pub(crate) const fn new(status: CredentialStatusSnapshot, reason: &'static str) -> Self {
        Self { status, reason }
    }

    pub const fn from_status(status: CredentialStatusSnapshot, reason: &'static str) -> Self {
        Self { status, reason }
    }

    pub const fn status(&self) -> CredentialStatusSnapshot {
        self.status
    }
}

impl fmt::Debug for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialError")
            .field("status", &self.status)
            .field("reason", &self.reason)
            .finish()
    }
}

impl fmt::Display for CredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.reason)
    }
}

impl std::error::Error for CredentialError {}
