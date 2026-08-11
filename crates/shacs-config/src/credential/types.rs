use super::status::{
    CredentialFingerprintStatus, CredentialSource, CredentialStatus, CredentialStatusSnapshot,
    RefreshSerializationStatus,
};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialFamily {
    ApiKey,
    OAuth,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CredentialFingerprint(String);

impl CredentialFingerprint {
    pub fn from_descriptor(descriptor: &str) -> Self {
        let digest = Sha256::digest(descriptor.as_bytes());
        Self(format!("sha256:{digest:x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) const fn from_stored(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Debug for CredentialFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("CredentialFingerprint")
            .field(&self.0)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum RawCredential {
    ApiKey {
        access: String,
    },
    OAuth {
        access: String,
        refresh: Option<String>,
        expires: Option<u64>,
    },
}

impl RawCredential {
    pub fn api_key(access: impl Into<String>) -> Self {
        Self::ApiKey {
            access: access.into(),
        }
    }

    pub fn oauth(access: impl Into<String>, refresh: Option<String>, expires: Option<u64>) -> Self {
        Self::OAuth {
            access: access.into(),
            refresh,
            expires,
        }
    }

    pub(crate) fn matches_family(&self, family: CredentialFamily) -> bool {
        matches!(
            (self, family),
            (Self::ApiKey { .. }, CredentialFamily::ApiKey)
                | (Self::OAuth { .. }, CredentialFamily::OAuth)
        )
    }

    pub(crate) fn access(&self) -> &str {
        match self {
            Self::ApiKey { access } | Self::OAuth { access, .. } => access,
        }
    }
}

impl fmt::Debug for RawCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey { .. } => formatter.write_str("RawCredential::ApiKey([REDACTED])"),
            Self::OAuth { .. } => formatter.write_str("RawCredential::OAuth([REDACTED])"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCredentialOutcome {
    Succeeded,
    NonZero,
    TimedOut,
    Aborted,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub enum CommandCredentialInput {
    #[default]
    NotRun,
    Result {
        outcome: CommandCredentialOutcome,
        stdout: String,
    },
    Cached(RawCredential),
}

impl CommandCredentialInput {
    pub fn result(outcome: CommandCredentialOutcome, stdout: impl Into<String>) -> Self {
        Self::Result {
            outcome,
            stdout: stdout.into(),
        }
    }

    pub fn succeeded(stdout: impl Into<String>) -> Self {
        Self::result(CommandCredentialOutcome::Succeeded, stdout)
    }

    pub fn cached(credential: RawCredential) -> Self {
        Self::Cached(credential)
    }
}

impl fmt::Debug for CommandCredentialInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRun => formatter.write_str("CommandCredentialInput::NotRun"),
            Self::Result { outcome, .. } => formatter
                .debug_struct("CommandCredentialInput::Result")
                .field("outcome", outcome)
                .field("stdout", &"[REDACTED]")
                .finish(),
            Self::Cached(_) => formatter.write_str("CommandCredentialInput::Cached([REDACTED])"),
        }
    }
}

pub struct CredentialTransport<'a> {
    value: &'a str,
}

impl CredentialTransport<'_> {
    pub const fn value(&self) -> &str {
        self.value
    }
}

pub struct ResolvedCredential {
    pub(crate) raw: RawCredential,
    pub(crate) source: CredentialSource,
    pub(crate) fingerprint: CredentialFingerprintStatus,
    pub(crate) refresh_serialization: RefreshSerializationStatus,
}

impl ResolvedCredential {
    pub const fn source(&self) -> CredentialSource {
        self.source
    }

    pub fn transport(&self) -> CredentialTransport<'_> {
        CredentialTransport {
            value: self.raw.access(),
        }
    }

    pub const fn status(&self) -> CredentialStatusSnapshot {
        CredentialStatusSnapshot {
            status: CredentialStatus::Resolved,
            source: Some(self.source),
            fingerprint: self.fingerprint,
            fingerprint_stale: matches!(self.fingerprint, CredentialFingerprintStatus::Stale),
            refresh_serialization: self.refresh_serialization,
        }
    }
}

impl fmt::Debug for ResolvedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedCredential")
            .field("raw", &"[REDACTED]")
            .field("source", &self.source)
            .field("fingerprint", &self.fingerprint)
            .field("refresh_serialization", &self.refresh_serialization)
            .finish()
    }
}
