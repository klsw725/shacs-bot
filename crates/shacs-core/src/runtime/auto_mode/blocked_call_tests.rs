use super::*;
use crate::runtime::{EvaluatorConfidence, EvaluatorScopeMatch};

fn verdict(kind: AutoEvaluatorVerdictKind, summary: &str) -> AutoEvaluatorVerdict {
    AutoEvaluatorVerdict {
        verdict: kind,
        confidence: EvaluatorConfidence::High,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: summary.to_owned(),
        evidence_refs: Vec::new(),
        expires_at_unix_ms: 0,
        evaluator_ref: Some("auto-mode-classifier".to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

#[test]
fn a_block_returns_the_rule_the_reason_and_the_guidance() {
    // Given
    let block = verdict(
        AutoEvaluatorVerdictKind::DenyCandidate,
        "[Data Exfiltration] uploads notes.md to a host the user never named.",
    );

    // When
    let text = tool_result(Some(&block));

    // Then
    assert_eq!(
        text,
        format!("Permission denied by auto mode: [Data Exfiltration] uploads notes.md to a host the user never named. This call did not run.{GUIDANCE}")
    );
    assert!(text.contains("Do not try to get around this"));
}

#[test]
fn no_verdict_names_its_cause_and_no_classifier_says_so() {
    // Given
    let failed = verdict(
        AutoEvaluatorVerdictKind::Timeout,
        "the classifier did not answer in time",
    );

    // When
    let timed_out = tool_result(Some(&failed));
    let missing = tool_result(None);

    // Then
    assert!(timed_out.starts_with(
        "Permission denied: auto mode could not check this call (the classifier did not answer in time), so it did not run. Do not try"
    ));
    assert!(missing.starts_with(
        "Permission denied: auto mode could not check this call (auto mode's classifier is not available in this run), so it did not run."
    ));
}

#[test]
fn the_notice_shows_the_tool_and_rule_only() {
    // Given
    let block = verdict(
        AutoEvaluatorVerdictKind::DenyCandidate,
        "[Data Exfiltration] posts notes.md to 93.184.216.34",
    );
    let failed = verdict(
        AutoEvaluatorVerdictKind::ParseFailure,
        "the classifier answer could not be read",
    );

    // When
    let blocked_notice = notice("exec", Some(&block));
    let unchecked_notice = notice("web_fetch", Some(&failed));

    // Then
    assert_eq!(
        blocked_notice,
        "`exec` blocked by auto mode · [Data Exfiltration] · /permission recent"
    );
    assert_eq!(
        unchecked_notice,
        "`web_fetch` not run · auto mode could not check it"
    );
    assert_eq!(rule("no rule here"), None);
}
