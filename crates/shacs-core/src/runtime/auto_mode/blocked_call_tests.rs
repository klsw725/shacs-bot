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

#[test]
fn only_the_runtime_notice_counts_as_one_not_a_tool_of_the_same_name() {
    // Given
    let notice = crate::runtime::ToolEvent {
        name: crate::runtime::AUTO_MODE_BLOCK_EVENT.to_owned(),
        status: crate::runtime::ToolStatus::Waiting,
        detail: "`exec` blocked by auto mode · [Data Exfiltration] · /permission recent".to_owned(),
        call_id: Some("action_1".to_owned()),
        arguments: None,
        result: Some(serde_json::json!({"tool": "exec", "outcome": "blocked"})),
    };
    let tool_runs = crate::runtime::ToolEvent {
        status: crate::runtime::ToolStatus::Ok,
        arguments: Some(serde_json::json!({"path": "notes.md"})),
        ..notice.clone()
    };
    let tool_waits = crate::runtime::ToolEvent {
        result: Some(serde_json::json!({"outcome": "something else"})),
        ..notice.clone()
    };

    // When / Then
    assert!(crate::runtime::is_auto_mode_block_notice(&notice));
    assert!(!crate::runtime::is_auto_mode_block_notice(&tool_runs));
    assert!(!crate::runtime::is_auto_mode_block_notice(&tool_waits));
}
