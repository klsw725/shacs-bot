use super::{
    ActivationMutationReceipt, ActivationReason, ActivationRecord, ActivationRecordInput,
    ActivationSource, ActivationStatus, ActivationStoreError, WorkspaceTrustRef,
    ACTIVATION_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ActivationDocument {
    pub(super) schema_version: u32,
    pub(super) records: Vec<ActivationRecord>,
    pub(super) receipts: Vec<ActivationMutationReceipt>,
}

impl Default for ActivationDocument {
    fn default() -> Self {
        Self {
            schema_version: ACTIVATION_SCHEMA_VERSION,
            records: Vec::new(),
            receipts: Vec::new(),
        }
    }
}

pub(super) fn parse_document(
    value: serde_json::Value,
) -> Result<(ActivationDocument, bool), ActivationStoreError> {
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ActivationStoreError::Malformed("schemaVersion is required".to_owned()))?;
    let version = u32::try_from(version)
        .map_err(|_| ActivationStoreError::Malformed("schemaVersion is invalid".to_owned()))?;
    match version {
        0 => {
            let legacy: LegacyDocument = serde_json::from_value(value)
                .map_err(|error| ActivationStoreError::Malformed(error.to_string()))?;
            Ok((legacy.migrate(), true))
        }
        ACTIVATION_SCHEMA_VERSION => serde_json::from_value(value)
            .map(|document| (document, false))
            .map_err(|error| ActivationStoreError::Malformed(error.to_string())),
        other => Err(ActivationStoreError::UnknownSchema(other)),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyDocument {
    #[serde(rename = "schemaVersion")]
    _schema_version: u32,
    records: Vec<LegacyRecord>,
    receipts: Vec<ActivationMutationReceipt>,
}

impl LegacyDocument {
    fn migrate(self) -> ActivationDocument {
        ActivationDocument {
            schema_version: ACTIVATION_SCHEMA_VERSION,
            records: self
                .records
                .into_iter()
                .map(LegacyRecord::migrate)
                .collect(),
            receipts: self.receipts,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRecord {
    activation_ref: String,
    source: ActivationSource,
    workspace_trust_ref: WorkspaceTrustRef,
    resource_ref: String,
    source_identity: String,
    content_digest: String,
    dependency_manifest_digest: String,
    status: ActivationStatus,
    reason: ActivationReason,
    recorded_at_unix_ms: u64,
}

impl LegacyRecord {
    fn migrate(self) -> ActivationRecord {
        ActivationRecord::new(ActivationRecordInput {
            activation_ref: self.activation_ref,
            source: self.source,
            workspace_trust_ref: self.workspace_trust_ref,
            resource_ref: self.resource_ref,
            source_identity: self.source_identity,
            content_digest: self.content_digest,
            dependency_manifest_digest: self.dependency_manifest_digest,
            status: self.status,
            reason: self.reason,
            recorded_at_unix_ms: self.recorded_at_unix_ms,
        })
    }
}
