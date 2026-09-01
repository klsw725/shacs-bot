use super::status::{
    CredentialError, CredentialFingerprintStatus, CredentialSource, CredentialStatus,
    CredentialStatusSnapshot, RefreshSerializationStatus,
};
use super::types::{
    CommandCredentialInput, CommandCredentialOutcome, CredentialFamily, CredentialFingerprint,
    RawCredential, ResolvedCredential,
};
use crate::ProviderConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialSourceDeclaration {
    pub family: CredentialFamily,
    pub environment: Option<String>,
    pub local_auth: bool,
    pub command: Option<String>,
}

impl CredentialSourceDeclaration {
    pub fn compatibility(family: CredentialFamily, environment: Option<&str>) -> Self {
        Self {
            family,
            environment: environment.map(str::to_owned),
            local_auth: true,
            command: None,
        }
    }

    pub fn fingerprint(&self) -> CredentialFingerprint {
        CredentialFingerprint::from_descriptor(&format!(
            "family={:?};environment={:?};local_auth={};command={:?}",
            self.family, self.environment, self.local_auth, self.command
        ))
    }

    pub fn resolve(
        &self,
        provider: &ProviderConfig,
        input: CredentialResolutionInput,
    ) -> Result<ResolvedCredential, CredentialError> {
        if let Some(raw) = input
            .runtime_override
            .filter(|raw| raw.matches_family(self.family))
        {
            return Ok(resolved(raw, CredentialSource::RuntimeOverride));
        }
        if self.environment.is_some() {
            if let Some(raw) = input
                .environment
                .filter(|raw| raw.matches_family(self.family))
            {
                return Ok(resolved(raw, CredentialSource::Environment));
            }
        }
        let expected_fingerprint = self.fingerprint();
        let local_is_current = match input.local_auth_fingerprint.as_ref() {
            Some(actual) => actual == &expected_fingerprint,
            None => true,
        };
        if self.local_auth && local_is_current {
            if let Some(raw) = input
                .local_auth
                .filter(|raw| raw.matches_family(self.family))
            {
                return Ok(resolved(raw, CredentialSource::LocalAuthStore));
            }
        }
        if self.command.is_some() {
            if let Some(raw) = command_raw(self.family, input.command)? {
                return Ok(resolved(raw, CredentialSource::Command));
            }
        }
        if let Some(access) = provider
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let raw = match self.family {
                CredentialFamily::ApiKey => RawCredential::api_key(access),
                CredentialFamily::OAuth => RawCredential::oauth(access, None, None),
            };
            return Ok(resolved(raw, CredentialSource::ProviderLiteral));
        }
        Err(CredentialError::new(
            CredentialStatusSnapshot {
                status: CredentialStatus::Missing,
                source: None,
                fingerprint: if local_is_current {
                    CredentialFingerprintStatus::Unavailable
                } else {
                    CredentialFingerprintStatus::Stale
                },
                fingerprint_stale: !local_is_current,
                refresh_serialization: RefreshSerializationStatus::Inactive,
            },
            "credential is missing",
        ))
    }
}

impl ProviderConfig {
    pub fn credential_declaration(
        &self,
        family: CredentialFamily,
        default_environment: Option<&str>,
    ) -> CredentialSourceDeclaration {
        let environment = self
            .api_key_ref
            .as_ref()
            .and_then(|reference| match &reference.locator {
                shacs_redaction::SecretLocator::EnvVar { name } => Some(name.clone()),
                shacs_redaction::SecretLocator::AuthStore { .. }
                | shacs_redaction::SecretLocator::LocalSecretStore { .. }
                | shacs_redaction::SecretLocator::AppBinding { .. }
                | shacs_redaction::SecretLocator::SkillTrustBinding { .. } => None,
            })
            .or_else(|| default_environment.map(str::to_owned));
        let source = self.credential_source.as_ref();
        CredentialSourceDeclaration {
            family,
            environment: source
                .and_then(|source| source.environment_name().map(str::to_owned))
                .or(environment),
            local_auth: source.map_or(true, |source| source.local_auth),
            command: source.and_then(|source| source.command_line().map(str::to_owned)),
        }
    }
}

#[derive(Debug, Default)]
pub struct CredentialResolutionInput {
    pub runtime_override: Option<RawCredential>,
    pub environment: Option<RawCredential>,
    pub local_auth: Option<RawCredential>,
    pub local_auth_fingerprint: Option<CredentialFingerprint>,
    pub command: CommandCredentialInput,
}

fn command_raw(
    family: CredentialFamily,
    input: CommandCredentialInput,
) -> Result<Option<RawCredential>, CredentialError> {
    match input {
        CommandCredentialInput::Cached(raw) => Ok(raw.matches_family(family).then_some(raw)),
        CommandCredentialInput::Result {
            outcome: CommandCredentialOutcome::Succeeded,
            stdout,
        } => {
            let access = stdout.trim();
            if access.is_empty() {
                Err(command_error())
            } else {
                Ok(Some(match family {
                    CredentialFamily::ApiKey => RawCredential::api_key(access),
                    CredentialFamily::OAuth => RawCredential::oauth(access, None, None),
                }))
            }
        }
        CommandCredentialInput::NotRun => Ok(None),
        CommandCredentialInput::Result {
            outcome:
                CommandCredentialOutcome::NonZero
                | CommandCredentialOutcome::TimedOut
                | CommandCredentialOutcome::Aborted,
            ..
        } => Err(command_error()),
    }
}

const fn command_error() -> CredentialError {
    CredentialError::new(
        CredentialStatusSnapshot {
            status: CredentialStatus::Missing,
            source: Some(CredentialSource::Command),
            fingerprint: CredentialFingerprintStatus::Current,
            fingerprint_stale: false,
            refresh_serialization: RefreshSerializationStatus::Inactive,
        },
        "credential command did not produce a credential",
    )
}

fn resolved(raw: RawCredential, source: CredentialSource) -> ResolvedCredential {
    ResolvedCredential {
        raw,
        account_id: None,
        source,
        fingerprint: CredentialFingerprintStatus::Current,
        refresh_serialization: RefreshSerializationStatus::Inactive,
    }
}
