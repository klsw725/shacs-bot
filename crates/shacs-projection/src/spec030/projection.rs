use super::*;
use serde::{Deserialize, Deserializer, Serialize};
use std::{error::Error, fmt};

pub const SPEC030_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec030RuntimeProjection {
    pub(super) schema_version: u32,
    pub(super) availability: Spec030Availability,
    pub(super) status: Spec030RuntimeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) unavailable_reason: Option<Spec030UnavailableReason>,
    pub(super) profile: TrustedRuntimeProfileProjection,
    pub(super) lifecycle_boundaries: Vec<LifecycleBoundaryProjection>,
    pub(super) hooks: HookRuntimeProjection,
    pub(super) process_adapters: Vec<ProcessAdapterProjection>,
    pub(super) credential: CredentialStatusProjection,
    pub(super) sandbox: SandboxStatusProjection,
    pub(super) resources: Vec<ResourceCandidateProjection>,
    pub(super) disclosure: DataDisclosureProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec030RuntimeProjectionInput {
    pub availability: Spec030Availability,
    pub status: Spec030RuntimeStatus,
    pub unavailable_reason: Option<Spec030UnavailableReason>,
    pub profile: TrustedRuntimeProfileProjection,
    pub lifecycle_boundaries: Vec<LifecycleBoundaryProjection>,
    pub hooks: HookRuntimeProjection,
    pub process_adapters: Vec<ProcessAdapterProjection>,
    pub credential: CredentialStatusProjection,
    pub sandbox: SandboxStatusProjection,
    pub resources: Vec<ResourceCandidateProjection>,
    pub disclosure: DataDisclosureProjection,
}

impl Spec030RuntimeProjection {
    pub fn try_new(input: Spec030RuntimeProjectionInput) -> Result<Self, Spec030ValidationError> {
        let projection = Self {
            schema_version: SPEC030_SCHEMA_VERSION,
            availability: input.availability,
            status: input.status,
            unavailable_reason: input.unavailable_reason,
            profile: input.profile,
            lifecycle_boundaries: input.lifecycle_boundaries,
            hooks: input.hooks,
            process_adapters: input.process_adapters,
            credential: input.credential,
            sandbox: input.sandbox,
            resources: input.resources,
            disclosure: input.disclosure,
        };
        projection.validate()?;
        Ok(projection)
    }

    pub fn unavailable(reason: Spec030UnavailableReason) -> Self {
        Self {
            schema_version: SPEC030_SCHEMA_VERSION,
            availability: Spec030Availability::Unavailable,
            status: Spec030RuntimeStatus::Unavailable,
            unavailable_reason: Some(reason),
            profile: TrustedRuntimeProfileProjection::unavailable(),
            lifecycle_boundaries: Vec::new(),
            hooks: HookRuntimeProjection::unavailable(),
            process_adapters: Vec::new(),
            credential: CredentialStatusProjection::unavailable(),
            sandbox: SandboxStatusProjection::unavailable(),
            resources: Vec::new(),
            disclosure: DataDisclosureProjection::unavailable(),
        }
    }

    pub fn validate(&self) -> Result<(), Spec030ValidationError> {
        validate_runtime(self)
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn availability(&self) -> Spec030Availability {
        self.availability
    }

    pub const fn status(&self) -> Spec030RuntimeStatus {
        self.status
    }

    pub const fn unavailable_reason(&self) -> Option<Spec030UnavailableReason> {
        self.unavailable_reason
    }

    pub const fn profile(&self) -> &TrustedRuntimeProfileProjection {
        &self.profile
    }

    pub fn lifecycle_boundaries(&self) -> &[LifecycleBoundaryProjection] {
        &self.lifecycle_boundaries
    }

    pub const fn hooks(&self) -> &HookRuntimeProjection {
        &self.hooks
    }

    pub fn process_adapters(&self) -> &[ProcessAdapterProjection] {
        &self.process_adapters
    }

    pub const fn credential(&self) -> &CredentialStatusProjection {
        &self.credential
    }

    pub const fn sandbox(&self) -> &SandboxStatusProjection {
        &self.sandbox
    }

    pub fn resources(&self) -> &[ResourceCandidateProjection] {
        &self.resources
    }

    pub const fn disclosure(&self) -> &DataDisclosureProjection {
        &self.disclosure
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec030ParseError> {
        serde_json::from_str(input).map_err(Spec030ParseError::from_serde)
    }

    pub fn from_json_value(value: serde_json::Value) -> Result<Self, Spec030ParseError> {
        serde_json::from_value(value).map_err(Spec030ParseError::from_serde)
    }
}

impl<'de> Deserialize<'de> for Spec030RuntimeProjection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Spec030RuntimeProjectionWire::deserialize(deserializer)?;
        if wire.schema_version != SPEC030_SCHEMA_VERSION {
            return Err(serde::de::Error::custom(
                "unsupported Spec030 schema version",
            ));
        }
        Self::try_new(wire.into_input()).map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Spec030RuntimeProjectionWire {
    schema_version: u32,
    availability: Spec030Availability,
    status: Spec030RuntimeStatus,
    #[serde(default)]
    unavailable_reason: Option<Spec030UnavailableReason>,
    profile: TrustedRuntimeProfileProjection,
    lifecycle_boundaries: Vec<LifecycleBoundaryProjection>,
    hooks: HookRuntimeProjection,
    process_adapters: Vec<ProcessAdapterProjection>,
    credential: CredentialStatusProjection,
    sandbox: SandboxStatusProjection,
    resources: Vec<ResourceCandidateProjection>,
    disclosure: DataDisclosureProjection,
}

impl Spec030RuntimeProjectionWire {
    fn into_input(self) -> Spec030RuntimeProjectionInput {
        Spec030RuntimeProjectionInput {
            availability: self.availability,
            status: self.status,
            unavailable_reason: self.unavailable_reason,
            profile: self.profile,
            lifecycle_boundaries: self.lifecycle_boundaries,
            hooks: self.hooks,
            process_adapters: self.process_adapters,
            credential: self.credential,
            sandbox: self.sandbox,
            resources: self.resources,
            disclosure: self.disclosure,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030ParseErrorKind {
    InvalidJson,
    InvalidSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec030ParseError {
    kind: Spec030ParseErrorKind,
}

impl Spec030ParseError {
    pub const fn kind(self) -> Spec030ParseErrorKind {
        self.kind
    }

    fn from_serde(error: serde_json::Error) -> Self {
        let kind = if error.is_syntax() || error.is_eof() {
            Spec030ParseErrorKind::InvalidJson
        } else {
            Spec030ParseErrorKind::InvalidSchema
        };
        Self { kind }
    }
}

impl fmt::Display for Spec030ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            Spec030ParseErrorKind::InvalidJson => write!(formatter, "invalid Spec030 JSON"),
            Spec030ParseErrorKind::InvalidSchema => write!(formatter, "invalid Spec030 schema"),
        }
    }
}

impl Error for Spec030ParseError {}
