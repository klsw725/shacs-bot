use super::{
    Spec035MediaValidationError, Spec035MediaValidationErrorKind, SPEC035_MEDIA_DIGEST_CHARS,
    SPEC035_MEDIA_OPAQUE_REF_MAX_CHARS,
};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Spec035MediaOpaqueRef(String);

impl Spec035MediaOpaqueRef {
    pub fn try_new(value: &str) -> Result<Self, Spec035MediaValidationError> {
        if value.is_empty()
            || value.len() > SPEC035_MEDIA_OPAQUE_REF_MAX_CHARS
            || value.starts_with('/')
            || value.contains(['/', '\\', '?', '#', '@', '=', '%'])
            || value
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_')))
        {
            return Err(Spec035MediaValidationError::new(
                Spec035MediaValidationErrorKind::UnsafeOwnerFact,
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Spec035MediaOpaqueRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Spec035MediaDigest(String);

impl Spec035MediaDigest {
    pub fn try_new(value: &str) -> Result<Self, Spec035MediaValidationError> {
        let valid = value.len() == SPEC035_MEDIA_DIGEST_CHARS
            && value.strip_prefix("sha256:").is_some_and(|digest| {
                digest.len() == SPEC035_MEDIA_DIGEST_CHARS - "sha256:".len()
                    && digest
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            });
        if !valid {
            return Err(Spec035MediaValidationError::new(
                Spec035MediaValidationErrorKind::UnsafeOwnerFact,
            ));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Spec035MediaDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_new(&value).map_err(serde::de::Error::custom)
    }
}
