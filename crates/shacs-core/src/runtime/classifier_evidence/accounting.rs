use super::types::*;
use crate::runtime::AutoEvaluatorVerdict;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn token_accounting(
    usage: Option<&BTreeMap<String, u64>>,
    failure_reason: Option<AccountingUnavailableReason>,
) -> ClassifierTokenAccounting {
    if let Some(reason) = failure_reason {
        return ClassifierTokenAccounting {
            input: failed(reason),
            output: failed(reason),
        };
    }
    let Some(usage) = usage else {
        return unavailable_token_accounting(AccountingUnavailableReason::ProviderOmittedUsage);
    };
    ClassifierTokenAccounting {
        input: usage
            .get("prompt_tokens")
            .copied()
            .map(measured_tokens)
            .unwrap_or_else(|| unavailable(AccountingUnavailableReason::ProviderOmittedUsage)),
        output: usage
            .get("completion_tokens")
            .copied()
            .map(measured_tokens)
            .unwrap_or_else(|| unavailable(AccountingUnavailableReason::ProviderOmittedUsage)),
    }
}

pub(super) fn unavailable_token_accounting(
    reason: AccountingUnavailableReason,
) -> ClassifierTokenAccounting {
    ClassifierTokenAccounting {
        input: unavailable(reason),
        output: unavailable(reason),
    }
}

pub(super) fn latency_accounting(
    created_at_unix_ms: u64,
    completed_at_unix_ms: Option<u64>,
) -> AccountingValue {
    completed_at_unix_ms.map_or_else(
        || unavailable(AccountingUnavailableReason::ClockUnavailable),
        |completed| AccountingValue {
            state: AccountingState::Measured,
            value: Some(completed.saturating_sub(created_at_unix_ms)),
            unit: Some("ms".to_owned()),
            unavailable_reason: None,
            estimator_id: None,
            basis: None,
            confidence: None,
        },
    )
}

pub(super) fn unavailable(reason: AccountingUnavailableReason) -> AccountingValue {
    AccountingValue {
        state: AccountingState::Unavailable,
        value: None,
        unit: None,
        unavailable_reason: Some(reason),
        estimator_id: None,
        basis: None,
        confidence: None,
    }
}

pub(super) fn diagnostic_refs(verdict: &AutoEvaluatorVerdict) -> Vec<RedactedDiagnosticRef> {
    verdict
        .evidence_refs
        .iter()
        .chain(
            verdict
                .prompt_injection_signals
                .iter()
                .map(|signal| &signal.source_ref),
        )
        .map(|reference| {
            let digest = digest_text(reference);
            RedactedDiagnosticRef {
                ref_id: format!(
                    "classifier_ref_{}",
                    digest.chars().take(12).collect::<String>()
                ),
                kind: "classifier_evidence_ref".to_owned(),
                digest: Some(digest),
            }
        })
        .collect()
}

pub(super) fn refresh_evidence_id(evidence: &mut ClassifierDecisionEvidence) {
    evidence.evidence_id = ClassifierEvidenceId(format!(
        "classifier_ev_{}",
        digest_json(&serde_json::to_value(&evidence).unwrap_or_else(|_| json!({})))
            .chars()
            .take(16)
            .collect::<String>()
    ));
}

pub(super) fn digest_json(value: &Value) -> String {
    digest_text(&value.to_string())
}

fn measured_tokens(value: u64) -> AccountingValue {
    AccountingValue {
        state: AccountingState::Measured,
        value: Some(value),
        unit: Some("tokens".to_owned()),
        unavailable_reason: None,
        estimator_id: None,
        basis: None,
        confidence: None,
    }
}

fn failed(reason: AccountingUnavailableReason) -> AccountingValue {
    AccountingValue {
        state: AccountingState::Failed,
        value: None,
        unit: None,
        unavailable_reason: Some(reason),
        estimator_id: None,
        basis: None,
        confidence: None,
    }
}

fn digest_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
