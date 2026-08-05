use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{error::Error, fmt};

pub const SPEC031_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spec031SchemaVersion(u32);

impl Spec031SchemaVersion {
    pub const CURRENT: Self = Self(SPEC031_SCHEMA_VERSION);

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub fn try_from_raw(raw: u32) -> Result<Self, Spec031VersionError> {
        Self::try_from(raw)
    }
}

impl TryFrom<u32> for Spec031SchemaVersion {
    type Error = Spec031VersionError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == SPEC031_SCHEMA_VERSION {
            Ok(Self(value))
        } else {
            Err(Spec031VersionError::Unsupported { found: value })
        }
    }
}

impl Serialize for Spec031SchemaVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for Spec031SchemaVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = u32::deserialize(deserializer)?;
        Self::try_from_raw(raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031VersionError {
    Unsupported { found: u32 },
}

impl fmt::Display for Spec031VersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { found } => {
                write!(formatter, "unsupported Spec031 schema version: {found}")
            }
        }
    }
}

impl Error for Spec031VersionError {}
