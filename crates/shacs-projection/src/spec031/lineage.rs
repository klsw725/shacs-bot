use super::redaction::{
    construction_error, Spec031ConstructionError, Spec031ConstructionViolation,
};
use super::text_safety::{sanitized_summary, validate_opaque_ref};
use super::{Spec031Freshness, Spec031ReasonCode, Spec031SourceOwner};
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! spec031_ref_newtype {
    ($name:ident, $field:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn try_new(value: &str) -> Result<Self, Spec031ConstructionError> {
                validate_opaque_ref(value).map_err(|()| {
                    construction_error($field, Spec031ConstructionViolation::UnsafeOpaqueRef)
                })?;
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_new(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

spec031_ref_newtype!(Spec031SubjectRef, "lineage.subject_ref");
spec031_ref_newtype!(Spec031ParentRef, "lineage.parent_ref");
spec031_ref_newtype!(Spec031ActionRef, "lineage.action_ref");
spec031_ref_newtype!(Spec031Digest, "lineage.digest");

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Spec031SafeSummary(String);

impl Spec031SafeSummary {
    pub fn try_new(value: &str) -> Result<Self, Spec031ConstructionError> {
        let value = sanitized_summary(value).map_err(|()| {
            construction_error(
                "reason.safe_summary",
                Spec031ConstructionViolation::UnsafeSummary,
            )
        })?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Spec031SafeSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spec031Count(u64);

impl Spec031Count {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Spec031ObservedAtUnixMs(u64);

impl Spec031ObservedAtUnixMs {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031Lineage {
    pub subject_ref: Spec031SubjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_ref: Option<Spec031ParentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_ref: Option<Spec031ActionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<Spec031Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031Reason {
    pub code: Spec031ReasonCode,
    pub safe_summary: Spec031SafeSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031Source {
    pub owner: Spec031SourceOwner,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at_unix_ms: Option<Spec031ObservedAtUnixMs>,
    pub freshness: Spec031Freshness,
}
