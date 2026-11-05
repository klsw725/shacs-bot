//! What auto mode says about a call it did not run at row 16 (Spec 048 PRD
//! 020 scopes 2 and 8): the tool result the assistant gets and the notice
//! the user sees.

use super::classifier_stage::CLASSIFIER_REF;
use crate::runtime::{
    AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, EvaluatorConfidence, EvaluatorScopeMatch,
};

const GUIDANCE: &str = " Do not try to get around this, for example by doing the same thing with another tool or command. Find a safer way to do what the user asked, keep working on parts that do not need this call, or stop and ask the user if this call is essential.";
const NO_CLASSIFIER: &str = "auto mode's classifier is not available in this run";
/// The cause of a call the turn ended before auto mode could check.
pub(crate) const TURN_ENDED: &str = "the turn ended first";
pub(crate) const UNKNOWN_RULE: &str = "[Unknown Rule]";

/// The classifier stopped the call; any other call auto mode did not run
/// had no verdict.
pub(crate) fn blocked(verdict: Option<&AutoEvaluatorVerdict>) -> bool {
    verdict.is_some_and(|verdict| verdict.verdict == AutoEvaluatorVerdictKind::DenyCandidate)
}

/// The tool result of a call row 16 refused. It starts with `Permission
/// denied` like the other refusals, so code that tells a call that did not
/// run by that start keeps working.
pub(crate) fn tool_result(verdict: Option<&AutoEvaluatorVerdict>) -> String {
    match verdict {
        Some(verdict) if blocked(Some(verdict)) => format!(
            "Permission denied by auto mode: {}. This call did not run.{GUIDANCE}",
            sentence(&verdict.risk_summary)
        ),
        Some(verdict) => format!(
            "Permission denied: auto mode could not check this call ({}), so it did not run.{GUIDANCE}",
            sentence(&verdict.risk_summary)
        ),
        None => format!(
            "Permission denied: auto mode could not check this call ({NO_CLASSIFIER}), so it did not run.{GUIDANCE}"
        ),
    }
}

/// The verdict a call gets when the turn ended before auto mode could
/// check it, so every later decision on it is the same no verdict.
pub(crate) fn turn_ended_verdict(now_unix_ms: u64) -> AutoEvaluatorVerdict {
    AutoEvaluatorVerdict {
        verdict: AutoEvaluatorVerdictKind::ParseFailure,
        confidence: EvaluatorConfidence::Unknown,
        scope_match: EvaluatorScopeMatch::Unknown,
        risk_summary: TURN_ENDED.to_owned(),
        evidence_refs: vec![CLASSIFIER_REF.to_owned()],
        expires_at_unix_ms: now_unix_ms,
        evaluator_ref: Some(CLASSIFIER_REF.to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

/// The `[Rule]` a block summary starts with.
pub(crate) fn rule(summary: &str) -> Option<&str> {
    let end = summary.strip_prefix('[')?.find(']')?;
    Some(&summary[..end + 2])
}

/// The rule of a blocked verdict as every surface shows it: `[Unknown Rule]`
/// when its summary names none.
pub(crate) fn rule_name(verdict: Option<&AutoEvaluatorVerdict>) -> Option<&str> {
    verdict
        .filter(|verdict| blocked(Some(verdict)))
        .map(|verdict| rule(&verdict.risk_summary).unwrap_or(UNKNOWN_RULE))
}

/// The notice for the user: the tool and the rule, never the arguments or
/// the reason.
pub(crate) fn notice(tool: &str, verdict: Option<&AutoEvaluatorVerdict>) -> String {
    match rule_name(verdict) {
        Some(rule) => format!("`{tool}` blocked by auto mode · {rule} · /permission recent"),
        None => format!("`{tool}` not run · auto mode could not check it"),
    }
}

fn sentence(text: &str) -> &str {
    text.trim().trim_end_matches('.')
}

#[cfg(test)]
#[path = "blocked_call_tests.rs"]
mod tests;
