use super::GeneratedMediaContractError;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const PROVIDER_ID_MAX_CHARS: usize = 128;
const MODEL_ID_MAX_CHARS: usize = 128;
const OPTION_NAME_MAX_CHARS: usize = 64;
const OPTION_VALUE_MAX_CHARS: usize = 256;

macro_rules! safe_fact {
    ($name:ident, $max:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, GeneratedMediaContractError> {
                let value = value.into();
                validate_persisted_string(&value, $max)?;
                Ok(Self(value))
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
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

safe_fact!(SafeProviderId, PROVIDER_ID_MAX_CHARS);
safe_fact!(SafeModelId, MODEL_ID_MAX_CHARS);
safe_fact!(SafeOptionValue, OPTION_VALUE_MAX_CHARS);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct SafeOptionName(String);

impl SafeOptionName {
    pub fn new(value: impl Into<String>) -> Result<Self, GeneratedMediaContractError> {
        let value = value.into();
        validate_persisted_string(&value, OPTION_NAME_MAX_CHARS)?;
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || contains_credential_marker(&value.to_ascii_lowercase())
        {
            return Err(GeneratedMediaContractError::UnsafePersistedString);
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for SafeOptionName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(transparent)]
pub struct GenerationOptionsSummary(BTreeMap<SafeOptionName, SafeOptionValue>);

impl GenerationOptionsSummary {
    pub fn new(options: BTreeMap<String, String>) -> Result<Self, GeneratedMediaContractError> {
        options
            .into_iter()
            .map(|(name, value)| Ok((SafeOptionName::new(name)?, SafeOptionValue::new(value)?)))
            .collect::<Result<BTreeMap<_, _>, GeneratedMediaContractError>>()
            .map(Self)
    }
}

impl<'de> Deserialize<'de> for GenerationOptionsSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let options = BTreeMap::<String, String>::deserialize(deserializer)?;
        Self::new(options).map_err(serde::de::Error::custom)
    }
}

fn validate_persisted_string(
    value: &str,
    max_chars: usize,
) -> Result<(), GeneratedMediaContractError> {
    let lower = value.to_ascii_lowercase();
    let windows_absolute = value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic);
    let forbidden_marker = [
        "://",
        "data:",
        "file:",
        "base64",
        "raw_content",
        "raw-content",
        "raw_payload",
        "raw payload",
        "rawcontent",
    ]
    .iter()
    .any(|marker| lower.contains(marker));
    if value.is_empty()
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
        || Path::new(value).is_absolute()
        || windows_absolute
        || value.starts_with("\\\\")
        || lower.starts_with("www.")
        || value.contains('?')
        || value.contains('&')
        || value.contains('#')
        || contains_credential_marker(&lower)
        || forbidden_marker
    {
        return Err(GeneratedMediaContractError::UnsafePersistedString);
    }
    Ok(())
}

fn contains_credential_marker(value: &str) -> bool {
    [
        "token",
        "secret",
        "password",
        "credential",
        "api_key",
        "apikey",
        "signature",
        "authorization",
        "cookie",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}
