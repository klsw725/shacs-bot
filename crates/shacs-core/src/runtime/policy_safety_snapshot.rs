#[path = "policy_safety_snapshot_digest.rs"]
mod policy_safety_snapshot_digest;
#[path = "policy_safety_snapshot_types.rs"]
mod policy_safety_snapshot_types;

pub use policy_safety_snapshot_types::{
    CapabilityCeilingRef, PolicySafetyDigest, PolicySafetyProvenanceKind,
    PolicySafetyProvenanceRef, PolicySafetySnapshot, PolicySafetySnapshotCreationReason,
    PolicySafetySnapshotError, PolicySafetySnapshotId, PolicySafetySnapshotInput,
    PolicySafetySnapshotRef, PolicySafetySnapshotSchemaId, PolicySafetySourceKind,
    PolicySafetySourceRef, RedactedPolicySafetySummary, POLICY_SAFETY_SNAPSHOT_SCHEMA_V1,
};

use policy_safety_snapshot_digest::{digest_json, provenance_ref_key, source_ref_key};
use serde_json::Value;
use shacs_config::SafetyCapability;

impl PolicySafetySnapshot {
    pub fn create(input: PolicySafetySnapshotInput) -> Result<Self, PolicySafetySnapshotError> {
        reject_raw_refs(&input.source_refs, &input.provenance_refs)?;
        let mut capability_ceiling = input.capability_ceiling.clone();
        capability_ceiling
            .capabilities
            .sort_by_key(|capability| capability_label(*capability));
        capability_ceiling.capabilities.dedup();
        let mut source_refs = input.source_refs.clone();
        let mut provenance_refs = input.provenance_refs.clone();
        source_refs.sort_by_key(source_ref_key);
        provenance_refs.sort_by_key(provenance_ref_key);
        let redacted_summary = summary(&input, &capability_ceiling, &source_refs, &provenance_refs);
        let policy_safety_digest = digest_material(
            &input,
            &capability_ceiling,
            &source_refs,
            &provenance_refs,
            &redacted_summary,
        );
        Ok(Self {
            schema_id: PolicySafetySnapshotSchemaId::V1,
            snapshot_id: PolicySafetySnapshotId(input.snapshot_id),
            created_at_unix_ms: input.created_at_unix_ms,
            expires_at_unix_ms: input.expires_at_unix_ms,
            permission_mode: input.permission_mode,
            capability_ceiling,
            containment: input.containment,
            source_refs,
            provenance_refs,
            creation_reason: input.creation_reason,
            redacted_summary,
            policy_safety_digest,
        })
    }

    pub fn reference(&self) -> PolicySafetySnapshotRef {
        PolicySafetySnapshotRef {
            schema_id: self.schema_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            policy_safety_digest: self.policy_safety_digest.clone(),
            created_at_unix_ms: self.created_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            redacted_summary: self.redacted_summary.clone(),
        }
    }

    pub fn parse_ref(value: Value) -> Result<PolicySafetySnapshotRef, PolicySafetySnapshotError> {
        reject_unknown_schema(&value)?;
        serde_json::from_value(value).map_err(|error| PolicySafetySnapshotError::Malformed {
            detail: error.to_string(),
        })
    }

    pub fn require_ref(
        reference: Option<&PolicySafetySnapshotRef>,
    ) -> Result<&PolicySafetySnapshotRef, PolicySafetySnapshotError> {
        reference.ok_or(PolicySafetySnapshotError::MissingRef)
    }

    pub fn validate_ref(
        &self,
        reference: &PolicySafetySnapshotRef,
        now_unix_ms: u64,
    ) -> Result<(), PolicySafetySnapshotError> {
        if self.snapshot_id != reference.snapshot_id {
            return Err(PolicySafetySnapshotError::SnapshotIdMismatch {
                expected: self.snapshot_id.0.clone(),
                actual: reference.snapshot_id.0.clone(),
            });
        }
        if self.policy_safety_digest != reference.policy_safety_digest {
            return Err(PolicySafetySnapshotError::DigestMismatch {
                expected: self.policy_safety_digest.0.clone(),
                actual: reference.policy_safety_digest.0.clone(),
            });
        }
        if let Some(expired_at_unix_ms) = self.expires_at_unix_ms {
            if now_unix_ms > expired_at_unix_ms {
                return Err(PolicySafetySnapshotError::StaleSnapshot {
                    expired_at_unix_ms,
                    now_unix_ms,
                });
            }
        }
        Ok(())
    }
}

fn summary(
    input: &PolicySafetySnapshotInput,
    ceiling: &CapabilityCeilingRef,
    sources: &[PolicySafetySourceRef],
    provenances: &[PolicySafetyProvenanceRef],
) -> RedactedPolicySafetySummary {
    RedactedPolicySafetySummary {
        permission_mode: input.permission_mode.mode.as_str().to_owned(),
        capability_count: ceiling.capabilities.len(),
        containment_digest: input
            .containment
            .as_ref()
            .and_then(|containment| containment.digest.clone()),
        source_ref_count: sources.len(),
        provenance_ref_count: provenances.len(),
    }
}

fn digest_material(
    input: &PolicySafetySnapshotInput,
    ceiling: &CapabilityCeilingRef,
    sources: &[PolicySafetySourceRef],
    provenances: &[PolicySafetyProvenanceRef],
    summary: &RedactedPolicySafetySummary,
) -> PolicySafetyDigest {
    PolicySafetyDigest(digest_json(&serde_json::json!({
        "schema_id": POLICY_SAFETY_SNAPSHOT_SCHEMA_V1,
        "snapshot_id": input.snapshot_id,
        "created_at_unix_ms": input.created_at_unix_ms,
        "expires_at_unix_ms": input.expires_at_unix_ms,
        "permission_mode": input.permission_mode,
        "capability_ceiling": ceiling,
        "containment": input.containment,
        "source_refs": sources,
        "provenance_refs": provenances,
        "creation_reason": input.creation_reason,
        "redacted_summary": summary,
    })))
}

fn reject_unknown_schema(value: &Value) -> Result<(), PolicySafetySnapshotError> {
    match value.get("schema_id").and_then(Value::as_str) {
        Some(POLICY_SAFETY_SNAPSHOT_SCHEMA_V1) => Ok(()),
        Some(schema_id) => Err(PolicySafetySnapshotError::UnknownSchema {
            schema_id: schema_id.to_owned(),
        }),
        None => Err(PolicySafetySnapshotError::MissingField {
            field: "schema_id".to_owned(),
        }),
    }
}

fn reject_raw_refs(
    sources: &[PolicySafetySourceRef],
    provenances: &[PolicySafetyProvenanceRef],
) -> Result<(), PolicySafetySnapshotError> {
    for source in sources {
        reject_empty_text("source_refs.ref_id", &source.ref_id)?;
        reject_raw_text("source_refs.ref_id", &source.ref_id)?;
    }
    for provenance in provenances {
        reject_empty_text("provenance_refs.ref_id", &provenance.ref_id)?;
        reject_raw_text("provenance_refs.ref_id", &provenance.ref_id)?;
    }
    Ok(())
}

fn reject_empty_text(field: &str, text: &str) -> Result<(), PolicySafetySnapshotError> {
    if text.trim().is_empty() {
        Err(PolicySafetySnapshotError::MissingField {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

fn reject_raw_text(field: &str, text: &str) -> Result<(), PolicySafetySnapshotError> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("sk-") || text.starts_with("/Users/") || text.starts_with("/home/") {
        Err(PolicySafetySnapshotError::RawMaterialRejected {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

fn capability_label(capability: SafetyCapability) -> &'static str {
    match capability {
        SafetyCapability::FsRead => "fs_read",
        SafetyCapability::FsWrite => "fs_write",
        SafetyCapability::ProcExec => "proc_exec",
        SafetyCapability::NetOutbound => "net_outbound",
        SafetyCapability::SecretRead => "secret_read",
        SafetyCapability::ExternalDelivery => "external_delivery",
        SafetyCapability::AutomationSchedule => "automation_schedule",
        SafetyCapability::AppInstall => "app_install",
        SafetyCapability::RuntimeConfigWrite => "runtime_config_write",
        SafetyCapability::SelfModification => "self_modification",
    }
}
