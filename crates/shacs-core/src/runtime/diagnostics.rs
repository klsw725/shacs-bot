#[path = "diagnostics_types.rs"]
mod diagnostics_types;

use crate::runtime::{
    ClassifierDecisionEvidence, ContainmentPermissionProof, PermissionDiagnosticsSummary,
    PermissionSecretRefAuditSummary, ProcessExecutionReceipt, SkillTrustPermissionDecision,
    SkillTrustPermissionDecisionKind,
};
pub use diagnostics_types::*;
use serde_json::Value;

pub fn build_core_diagnostics_aggregate(
    input: CoreDiagnosticsAggregateInput<'_>,
) -> Result<CoreDiagnosticsAggregate, CoreDiagnosticsError> {
    reject_input_raw_material(&input)?;
    let aggregate = CoreDiagnosticsAggregate {
        schema_id: "spec030_core_diagnostics.v1",
        policy_safety: policy_safety(&input),
        secrets: secrets(input.permission),
        process: process(input.process_receipts),
        containment: containment(input.containment_proofs),
        classifier: classifier(input.classifier_evidence),
        trust: trust(input.trust_decisions),
    };
    reject_raw_material(&aggregate)?;
    Ok(aggregate)
}

fn reject_input_raw_material(
    input: &CoreDiagnosticsAggregateInput<'_>,
) -> Result<(), CoreDiagnosticsError> {
    for value in [
        serde_json::to_value(input.permission),
        serde_json::to_value(input.process_receipts),
        serde_json::to_value(input.containment_proofs),
        serde_json::to_value(input.classifier_evidence),
        serde_json::to_value(input.trust_decisions),
    ] {
        let value = value.map_err(|_| CoreDiagnosticsError::Serialization)?;
        if value_contains_raw_material(&value) {
            return Err(CoreDiagnosticsError::RawDiagnosticMaterial);
        }
    }
    Ok(())
}

fn policy_safety(input: &CoreDiagnosticsAggregateInput<'_>) -> PolicySafetyDiagnosticsDto {
    let source = &input.permission.policy_safety_refs;
    PolicySafetyDiagnosticsDto {
        present_count: source.present_count,
        missing_count: source.missing_count,
        stale_count: source.stale_count,
        malformed_count: source.malformed_count,
        refs: source
            .items
            .iter()
            .map(|item| PolicySafetyRefDiagnostic {
                status: item.status,
                snapshot_id: item.snapshot_id.clone(),
                policy_safety_digest: item.policy_safety_digest.clone(),
            })
            .collect(),
    }
}

fn secrets(permission: &PermissionDiagnosticsSummary) -> SecretDiagnosticsDto {
    let source = &permission.secret_refs;
    SecretDiagnosticsDto {
        resolved_count: source.resolved_count,
        unresolved_count: source.unresolved_count,
        missing_count: source.missing_count,
        stale_count: source.stale_count,
        unsupported_count: source.unsupported_count,
        malformed_count: source.malformed_count,
        refs: source.items.iter().map(secret_ref).collect(),
    }
}

fn secret_ref(item: &PermissionSecretRefAuditSummary) -> SecretRefDiagnostic {
    SecretRefDiagnostic {
        ref_id: item.ref_id.clone(),
        source_kind: item.source_kind.clone(),
        status: item.status,
        redaction_evidence_ref: item.redaction_evidence_ref.clone(),
        requested_consumer: item.requested_consumer.clone(),
    }
}

fn process(receipts: &[ProcessExecutionReceipt]) -> ProcessDiagnosticsDto {
    ProcessDiagnosticsDto {
        receipt_count: receipts.len(),
        total_dispatch_count: receipts.iter().map(|receipt| receipt.dispatch_count).sum(),
        receipts: receipts
            .iter()
            .map(|receipt| ProcessReceiptDiagnostic {
                receipt_id: receipt.receipt_id.clone(),
                adapter: receipt.adapter,
                terminal_outcome: receipt.terminal_outcome,
                dispatch_count: receipt.dispatch_count,
                policy_decision: receipt.policy_decision.kind,
                policy_reason: receipt.policy_decision.reason.clone(),
                policy_safety_snapshot_id: receipt.policy_safety_snapshot_ref.snapshot_id.0.clone(),
                policy_safety_digest: receipt
                    .policy_safety_snapshot_ref
                    .policy_safety_digest
                    .0
                    .clone(),
                secret_ref_count: receipt.secret_ref_count,
                redacted_target_count: receipt.redacted_command.redacted_targets.len(),
            })
            .collect(),
    }
}

fn containment(proofs: &[ContainmentPermissionProof]) -> ContainmentDiagnosticsDto {
    ContainmentDiagnosticsDto {
        proof_count: proofs.len(),
        proofs: proofs
            .iter()
            .map(|proof| ContainmentProofDiagnostic {
                proof_id: proof.proof_id.clone(),
                envelope_id: proof.envelope_id.clone(),
                policy_safety_digest: proof.policy_safety_digest.0.clone(),
                containment_outcome: proof.containment_outcome,
                workspace_outcome: proof.workspace_outcome,
                ceiling_outcome: proof.ceiling_outcome,
                admission: proof.admission,
                violation_count: proof.violations.len(),
                blocked_external_status: proof
                    .blocked_external_surface
                    .as_ref()
                    .map(|blocked| blocked.status.clone()),
            })
            .collect(),
    }
}

fn classifier(evidence: &[ClassifierDecisionEvidence]) -> ClassifierDiagnosticsDto {
    ClassifierDiagnosticsDto {
        evidence_count: evidence.len(),
        items: evidence
            .iter()
            .map(|item| ClassifierEvidenceDiagnostic {
                evidence_id: item.evidence_id.0.clone(),
                route_kind: item.route.kind,
                disposition: item.disposition,
                precedence: item.precedence,
                input_token_state: item.token_accounting.input.state,
                output_token_state: item.token_accounting.output.state,
                latency_state: item.latency.duration_ms.state,
                cost_state: item.cost.total.state,
                policy_safety_snapshot_id: item
                    .action
                    .policy_safety_snapshot_ref
                    .as_ref()
                    .map(|reference| reference.snapshot_id.0.clone()),
            })
            .collect(),
    }
}

fn trust(decisions: &[SkillTrustPermissionDecision]) -> TrustDiagnosticsDto {
    TrustDiagnosticsDto {
        decision_count: decisions.len(),
        validated_count: decisions
            .iter()
            .filter(|decision| decision.kind == SkillTrustPermissionDecisionKind::Validated)
            .count(),
        rejected_count: decisions
            .iter()
            .filter(|decision| decision.kind == SkillTrustPermissionDecisionKind::Rejected)
            .count(),
        blocked_external_count: decisions
            .iter()
            .filter(|decision| {
                decision.kind == SkillTrustPermissionDecisionKind::BlockedExternalSurface
            })
            .count(),
        decisions: decisions
            .iter()
            .map(|decision| TrustDecisionDiagnostic {
                kind: decision.kind,
                reason: decision.reason,
                blocked_status: decision
                    .blocked_external_surface
                    .as_ref()
                    .map(|blocked| blocked.status.clone()),
                blocked_owner: decision
                    .blocked_external_surface
                    .as_ref()
                    .map(|blocked| blocked.owner.clone()),
                dispatch_count: decision.dispatch_count,
            })
            .collect(),
    }
}

fn reject_raw_material(aggregate: &CoreDiagnosticsAggregate) -> Result<(), CoreDiagnosticsError> {
    let value = serde_json::to_value(aggregate).map_err(|_| CoreDiagnosticsError::Serialization)?;
    if value_contains_raw_material(&value) {
        Err(CoreDiagnosticsError::RawDiagnosticMaterial)
    } else {
        Ok(())
    }
}

fn value_contains_raw_material(value: &Value) -> bool {
    match value {
        Value::String(text) => raw_text(text),
        Value::Array(items) => items.iter().any(value_contains_raw_material),
        Value::Object(map) => map.values().any(value_contains_raw_material),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn raw_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let normalized: String = text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    lower.contains("sk-")
        || lower.contains("bearer ")
        || lower.contains("private key")
        || lower.contains("-----begin private key-----")
        || text.contains("RAW_")
        || text.contains("/Users/")
        || text.contains("/home/")
        || text.starts_with('/')
        || text.starts_with("\\\\")
        || contains_windows_drive_path(text)
        || lower.contains("provider-secret")
        || lower.contains("process_handle")
        || normalized.contains("processhandle")
        || normalized.contains("rawstdout")
        || normalized.contains("rawstderr")
        || normalized.contains("standardoutputraw")
        || normalized.contains("rawproviderpayload")
}

fn contains_windows_drive_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    })
}
