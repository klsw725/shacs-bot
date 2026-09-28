use super::{
    Spec035TransportParseError, Spec035TransportValidationError,
    Spec035TransportValidationErrorKind, SPEC035_TRANSPORT_IDENTIFIER_MAX_CHARS,
};
use serde::{Deserialize, Deserializer, Serialize};

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= SPEC035_TRANSPORT_IDENTIFIER_MAX_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Spec035TransportClientId(String);

impl Spec035TransportClientId {
    pub fn try_new(value: &str) -> Result<Self, Spec035TransportValidationError> {
        if !valid_identifier(value) {
            return Err(Spec035TransportValidationError::new(
                Spec035TransportValidationErrorKind::InvalidIdentifier,
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Spec035TransportClientId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Spec035TransportGeneration(String);

impl Spec035TransportGeneration {
    pub fn try_new(value: &str) -> Result<Self, Spec035TransportValidationError> {
        if !valid_identifier(value) {
            return Err(Spec035TransportValidationError::new(
                Spec035TransportValidationErrorKind::InvalidIdentifier,
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Spec035TransportGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Spec035TransportSchemaVersion(u32);

impl Spec035TransportSchemaVersion {
    pub const fn try_new(value: u32) -> Result<Self, Spec035TransportValidationError> {
        if value == 0 {
            return Err(Spec035TransportValidationError::new(
                Spec035TransportValidationErrorKind::InvalidSchemaVersion,
            ));
        }
        Ok(Self(value))
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Spec035TransportSchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u32::deserialize(deserializer)?;
        Self::try_new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spec035TransportSequence(u64);

impl Spec035TransportSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportCapability {
    TaskPause,
    TaskResume,
    TaskStop,
    TaskRetry,
    TaskRecover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportUnsupportedReason {
    CapabilityUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spec035TransportCapabilitySupport {
    Supported,
    Unsupported {
        reason: Spec035TransportUnsupportedReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportEventKind {
    Snapshot,
    Delta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TransportResumeCursor {
    generation: Spec035TransportGeneration,
    sequence: Spec035TransportSequence,
}

impl Spec035TransportResumeCursor {
    pub const fn new(
        generation: Spec035TransportGeneration,
        sequence: Spec035TransportSequence,
    ) -> Self {
        Self {
            generation,
            sequence,
        }
    }

    pub const fn generation(&self) -> &Spec035TransportGeneration {
        &self.generation
    }

    pub const fn sequence(&self) -> Spec035TransportSequence {
        self.sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TransportEventMetadata {
    kind: Spec035TransportEventKind,
    generation: Spec035TransportGeneration,
    sequence: Spec035TransportSequence,
}

impl Spec035TransportEventMetadata {
    pub const fn try_new(
        kind: Spec035TransportEventKind,
        generation: Spec035TransportGeneration,
        sequence: Spec035TransportSequence,
    ) -> Result<Self, Spec035TransportValidationError> {
        Ok(Self {
            kind,
            generation,
            sequence,
        })
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec035TransportParseError> {
        serde_json::from_str(input).map_err(Spec035TransportParseError::from_serde)
    }

    pub const fn kind(&self) -> Spec035TransportEventKind {
        self.kind
    }

    pub const fn generation(&self) -> &Spec035TransportGeneration {
        &self.generation
    }

    pub const fn sequence(&self) -> Spec035TransportSequence {
        self.sequence
    }
}
