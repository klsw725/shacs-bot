use super::{
    PolicySafetyProvenanceKind, PolicySafetyProvenanceRef, PolicySafetySourceKind,
    PolicySafetySourceRef,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(super) fn source_ref_key(reference: &PolicySafetySourceRef) -> (String, String, String) {
    (
        source_kind_label(&reference.kind).to_owned(),
        reference.ref_id.clone(),
        reference.digest.clone().unwrap_or_default(),
    )
}

pub(super) fn provenance_ref_key(
    reference: &PolicySafetyProvenanceRef,
) -> (String, String, String) {
    (
        provenance_kind_label(&reference.kind).to_owned(),
        reference.ref_id.clone(),
        reference.digest.clone().unwrap_or_default(),
    )
}

pub(super) fn digest_json(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonicalize_value(value).to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let mut canonical = Map::new();
            for key in keys {
                if let Some(value) = object.get(key) {
                    canonical.insert(key.clone(), canonicalize_value(value));
                }
            }
            Value::Object(canonical)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        other => other.clone(),
    }
}

fn source_kind_label(kind: &PolicySafetySourceKind) -> &'static str {
    match kind {
        PolicySafetySourceKind::PermissionConfig => "permission_config",
        PolicySafetySourceKind::SessionOption => "session_option",
        PolicySafetySourceKind::InheritedContext => "inherited_context",
        PolicySafetySourceKind::ContainmentEvidence => "containment_evidence",
        PolicySafetySourceKind::RuntimePolicy => "runtime_policy",
        PolicySafetySourceKind::ExternalExecutionSnapshotRef => "external_execution_snapshot_ref",
    }
}

fn provenance_kind_label(kind: &PolicySafetyProvenanceKind) -> &'static str {
    match kind {
        PolicySafetyProvenanceKind::ConfigProfileRef => "config_profile_ref",
        PolicySafetyProvenanceKind::ContextSnapshotRef => "context_snapshot_ref",
        PolicySafetyProvenanceKind::ProviderExecutionSnapshotRef => {
            "provider_execution_snapshot_ref"
        }
        PolicySafetyProvenanceKind::TrustRecordRef => "trust_record_ref",
        PolicySafetyProvenanceKind::RuntimeEventRef => "runtime_event_ref",
        PolicySafetyProvenanceKind::DiagnosticsRef => "diagnostics_ref",
    }
}
