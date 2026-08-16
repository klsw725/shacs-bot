use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Deref;

pub const IMAGE_GENERATION_RESPONSE_MAX_BYTES: usize = 32 * 1024 * 1024;
pub const IMAGE_GENERATION_RESPONSE_READ_LIMIT: u64 = 32 * 1024 * 1024 + 1;
pub const IMAGE_GENERATION_PROVIDER_ERROR_CODE: &str = "image_generation_provider_error";
pub const IMAGE_GENERATION_RESPONSE_BODY_TOO_LARGE_CODE: &str =
    "image_generation_response_body_too_large";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ImageGenerationRequestId(String);

impl ImageGenerationRequestId {
    pub fn from_provider(value: &str) -> Self {
        Self(opaque_digest("request", value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ImageGenerationItemId(String);

impl ImageGenerationItemId {
    pub fn from_provider(value: &str) -> Self {
        Self(opaque_digest("item", value))
    }

    pub(crate) fn from_projected(value: &str) -> Option<Self> {
        is_fixed_digest(value, "item").then(|| Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl Deref for ImageGenerationItemId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl From<ImageGenerationItemId> for String {
    fn from(value: ImageGenerationItemId) -> Self {
        value.into_string()
    }
}

impl From<&ImageGenerationItemId> for String {
    fn from(value: &ImageGenerationItemId) -> Self {
        value.as_str().to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ImageMimeType {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    Webp,
}

impl ImageMimeType {
    pub fn parse_provider(value: &str) -> Option<Self> {
        match value {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

impl fmt::Display for ImageMimeType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl From<ImageMimeType> for String {
    fn from(value: ImageMimeType) -> Self {
        value.as_str().to_owned()
    }
}

impl From<&ImageMimeType> for String {
    fn from(value: &ImageMimeType) -> Self {
        value.as_str().to_owned()
    }
}

impl Deref for ImageGenerationRequestId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImageGenerationUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

impl ImageGenerationUsage {
    pub fn from_provider(value: &Value) -> Option<Self> {
        let usage = Self {
            input_tokens: numeric_field(value, "input_tokens"),
            output_tokens: numeric_field(value, "output_tokens"),
            prompt_tokens: numeric_field(value, "prompt_tokens"),
            completion_tokens: numeric_field(value, "completion_tokens"),
            total_tokens: numeric_field(value, "total_tokens"),
        };
        usage.has_accounting().then_some(usage)
    }

    pub fn from_token_counts(counts: &BTreeMap<String, u64>) -> Option<Self> {
        let usage = Self {
            input_tokens: counts.get("input_tokens").copied(),
            output_tokens: counts.get("output_tokens").copied(),
            prompt_tokens: counts.get("prompt_tokens").copied(),
            completion_tokens: counts.get("completion_tokens").copied(),
            total_tokens: counts.get("total_tokens").copied(),
        };
        usage.has_accounting().then_some(usage)
    }

    const fn has_accounting(&self) -> bool {
        self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.prompt_tokens.is_some()
            || self.completion_tokens.is_some()
            || self.total_tokens.is_some()
    }
}

impl PartialEq<Value> for ImageGenerationUsage {
    fn eq(&self, other: &Value) -> bool {
        serde_json::to_value(self).is_ok_and(|value| value == *other)
    }
}

fn numeric_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(Value::as_u64)
}

fn opaque_digest(domain: &str, value: &str) -> String {
    format!("{domain}_sha256_{:x}", Sha256::digest(value.as_bytes()))
}

fn is_fixed_digest(value: &str, domain: &str) -> bool {
    let Some(digest) = value.strip_prefix(&format!("{domain}_sha256_")) else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}
