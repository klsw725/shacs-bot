use crate::redact_string;
use serde::{de, Deserialize, Deserializer, Serialize};
use serde_json::Value;

const SECRET_REF_SCHEMA_VERSION: u8 = 1;
const REDACTION_EVIDENCE_SCHEMA_VERSION: u8 = 1;
const DEFAULT_LIMITS: &[&str] = &[
    "not_exfiltration_prevention",
    "not_raw_payload_integrity_proof",
];
const ILLEGAL_RAW_FIELDS: &[&str] = &[
    "value",
    "raw",
    "secret",
    "token",
    "password",
    "env_value",
    "header_value",
    "private_key",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretRefError {
    IllegalRawField(String),
    UnsupportedSchemaVersion { kind: &'static str, version: u8 },
    RawValuePersisted,
    Serde(String),
}

impl std::fmt::Display for SecretRefError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IllegalRawField(field) => write!(formatter, "illegal raw secret field: {field}"),
            Self::UnsupportedSchemaVersion { kind, version } => {
                write!(formatter, "unsupported {kind} schema version: {version}")
            }
            Self::RawValuePersisted => {
                write!(formatter, "redaction evidence persisted a raw value")
            }
            Self::Serde(error) => write!(formatter, "secret ref JSON failed: {error}"),
        }
    }
}

impl std::error::Error for SecretRefError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretRefId(String);

impl SecretRefId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RedactionEvidenceRef(String);

impl RedactionEvidenceRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretSourceKind {
    Env,
    AuthStore,
    LocalSecretStore,
    AppBinding,
    SkillTrustBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecretLocator {
    EnvVar {
        name: String,
    },
    AuthStore {
        provider: String,
        credential_slot: String,
    },
    LocalSecretStore {
        entry_id: String,
    },
    AppBinding {
        app_id: String,
        manifest_digest: String,
        name: String,
    },
    SkillTrustBinding {
        trust_ref: String,
        slot: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafeSecretSummary {
    pub label: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    pub kind: SecretRefKind,
    pub schema_version: u8,
    pub ref_id: SecretRefId,
    pub source_kind: SecretSourceKind,
    pub locator: SecretLocator,
    pub owner: String,
    pub scope: String,
    pub created_by: Option<String>,
    pub created_at_ms: Option<u64>,
    pub locator_digest: String,
    pub staleness_token: String,
    pub safe_summary: SafeSecretSummary,
}

impl SecretRef {
    pub fn from_value(value: Value) -> Result<Self, SecretRefError> {
        reject_illegal_raw_fields(&value)?;
        let secret_ref = serde_json::from_value::<SecretRefUnchecked>(value)
            .map_err(|error| SecretRefError::Serde(error.to_string()))?;
        Self::from_unchecked(secret_ref)
    }

    fn from_unchecked(secret_ref: SecretRefUnchecked) -> Result<Self, SecretRefError> {
        if secret_ref.schema_version != SECRET_REF_SCHEMA_VERSION {
            return Err(SecretRefError::UnsupportedSchemaVersion {
                kind: "secret_ref",
                version: secret_ref.schema_version,
            });
        }
        Ok(secret_ref.into_sanitized())
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretRefUnchecked {
    kind: SecretRefKind,
    schema_version: u8,
    ref_id: SecretRefId,
    source_kind: SecretSourceKind,
    locator: SecretLocator,
    owner: String,
    scope: String,
    created_by: Option<String>,
    created_at_ms: Option<u64>,
    locator_digest: String,
    staleness_token: String,
    safe_summary: SafeSecretSummary,
}

impl SecretRefUnchecked {
    fn into_sanitized(self) -> SecretRef {
        let locator = sanitize_locator(self.locator);
        let safe_summary = SafeSecretSummary {
            label: redact_string(&self.safe_summary.label),
            required: self.safe_summary.required,
        };
        SecretRef {
            kind: self.kind,
            schema_version: self.schema_version,
            ref_id: self.ref_id,
            source_kind: self.source_kind,
            locator,
            owner: self.owner,
            scope: self.scope,
            created_by: self.created_by,
            created_at_ms: self.created_at_ms,
            locator_digest: self.locator_digest,
            staleness_token: self.staleness_token,
            safe_summary,
        }
    }
}

fn sanitize_locator(locator: SecretLocator) -> SecretLocator {
    match locator {
        SecretLocator::EnvVar { name } => SecretLocator::EnvVar {
            name: redact_string(&name),
        },
        SecretLocator::AuthStore {
            provider,
            credential_slot,
        } => SecretLocator::AuthStore {
            provider: redact_string(&provider),
            credential_slot: redact_string(&credential_slot),
        },
        SecretLocator::LocalSecretStore { entry_id } => SecretLocator::LocalSecretStore {
            entry_id: redact_string(&entry_id),
        },
        SecretLocator::AppBinding {
            app_id,
            manifest_digest,
            name,
        } => SecretLocator::AppBinding {
            app_id: redact_string(&app_id),
            manifest_digest,
            name: redact_string(&name),
        },
        SecretLocator::SkillTrustBinding { trust_ref, slot } => SecretLocator::SkillTrustBinding {
            trust_ref: redact_string(&trust_ref),
            slot: redact_string(&slot),
        },
    }
}

impl From<SecretRefUnchecked> for SecretRef {
    fn from(value: SecretRefUnchecked) -> Self {
        Self {
            kind: value.kind,
            schema_version: value.schema_version,
            ref_id: value.ref_id,
            source_kind: value.source_kind,
            locator: value.locator,
            owner: value.owner,
            scope: value.scope,
            created_by: value.created_by,
            created_at_ms: value.created_at_ms,
            locator_digest: value.locator_digest,
            staleness_token: value.staleness_token,
            safe_summary: value.safe_summary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretRefKind {
    SecretRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedactionProfile {
    #[serde(rename = "shacs-redaction-v1")]
    ShacsRedactionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionEvidence {
    pub kind: RedactionEvidenceKind,
    pub schema_version: u8,
    pub evidence_id: RedactionEvidenceRef,
    pub input_ref: SecretRefId,
    pub projection_surface: String,
    pub redaction_profile: RedactionProfile,
    pub classified_kinds: Vec<String>,
    pub safe_summary_digest: String,
    pub raw_value_persisted: bool,
    pub best_effort: bool,
    pub limits: Vec<String>,
}

impl RedactionEvidence {
    pub fn for_secret_ref(
        evidence_id: RedactionEvidenceRef,
        input_ref: SecretRefId,
        projection_surface: impl Into<String>,
        safe_summary_digest: impl Into<String>,
    ) -> Self {
        Self {
            kind: RedactionEvidenceKind::RedactionEvidence,
            schema_version: REDACTION_EVIDENCE_SCHEMA_VERSION,
            evidence_id,
            input_ref,
            projection_surface: projection_surface.into(),
            redaction_profile: RedactionProfile::ShacsRedactionV1,
            classified_kinds: Vec::new(),
            safe_summary_digest: safe_summary_digest.into(),
            raw_value_persisted: false,
            best_effort: true,
            limits: DEFAULT_LIMITS
                .iter()
                .map(|limit| (*limit).to_owned())
                .collect(),
        }
    }

    pub fn from_value(value: Value) -> Result<Self, SecretRefError> {
        reject_illegal_raw_fields(&value)?;
        let evidence = serde_json::from_value::<Self>(value)
            .map_err(|error| SecretRefError::Serde(error.to_string()))?;
        if evidence.schema_version != REDACTION_EVIDENCE_SCHEMA_VERSION {
            return Err(SecretRefError::UnsupportedSchemaVersion {
                kind: "redaction_evidence",
                version: evidence.schema_version,
            });
        }
        if evidence.raw_value_persisted {
            return Err(SecretRefError::RawValuePersisted);
        }
        Ok(evidence)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionEvidenceKind {
    RedactionEvidence,
}

fn reject_illegal_raw_fields(value: &Value) -> Result<(), SecretRefError> {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                if ILLEGAL_RAW_FIELDS.contains(&key.as_str()) {
                    return Err(SecretRefError::IllegalRawField(key.clone()));
                }
                reject_illegal_raw_fields(item)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                reject_illegal_raw_fields(item)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}
