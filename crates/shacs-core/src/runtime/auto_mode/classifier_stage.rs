//! Runs the two-stage classifier for one pending action (Spec 048 PRD 000
//! scope 12): a one-word answer first, then a reasoned JSON verdict when the
//! first stage does not allow. Each stage has its own 60-second deadline.

use super::classifier_request::{self, STAGE_ONE, STAGE_TWO};
use crate::runtime::{
    AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, ClassifierAttemptStatus,
    ClassifierStageEvidence, EvaluatorConfidence, EvaluatorScopeMatch,
};
use serde::Deserialize;
use serde_json::{json, Value};
use shacs_providers::{
    GenerationSettings, LlmResponse, ProviderClient, ProviderError, ProviderInvocation,
    ProviderRequest,
};
use shacs_redaction::redact_string;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub(crate) const STAGE_DEADLINE: Duration = Duration::from_secs(60);
pub(crate) const CLASSIFIER_REF: &str = "auto-mode-classifier";
const VERDICT_TTL_MS: u64 = 5 * 60 * 1000;
const MAX_RULE_CHARS: usize = 80;
const MAX_REASON_CHARS: usize = 300;

pub(crate) struct ClassifierCall<'a> {
    pub(crate) client: &'a dyn ProviderClient,
    pub(crate) invocation: &'a ProviderInvocation,
    pub(crate) model: &'a str,
    pub(crate) system: &'a str,
    pub(crate) payload: &'a Value,
    pub(crate) now_unix_ms: &'a dyn Fn() -> u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ClassifierAttempt {
    pub(crate) verdict: AutoEvaluatorVerdict,
    pub(crate) status: ClassifierAttemptStatus,
    pub(crate) usage: Option<BTreeMap<String, u64>>,
    pub(crate) stages: Vec<ClassifierStageEvidence>,
    pub(crate) rule: Option<String>,
    pub(crate) started_at_unix_ms: u64,
    pub(crate) completed_at_unix_ms: u64,
}

#[derive(Debug, Deserialize)]
struct ReasonedVerdict {
    decision: String,
    #[serde(default)]
    rule: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

struct Attempt<'c, 'a> {
    call: &'c ClassifierCall<'a>,
    started_at_unix_ms: u64,
    stages: Vec<ClassifierStageEvidence>,
    usage: BTreeMap<String, u64>,
}

enum Failure {
    Stopped,
    TimedOut,
    Provider,
    Unreadable,
}

pub(crate) fn classify(call: &ClassifierCall<'_>) -> ClassifierAttempt {
    let mut attempt = Attempt {
        call,
        started_at_unix_ms: (call.now_unix_ms)(),
        stages: Vec::new(),
        usage: BTreeMap::new(),
    };
    let first = match attempt.ask(STAGE_ONE, 64, 1) {
        Ok(response) => response,
        Err(failure) => return attempt.failed(failure),
    };
    if first_word(first.content.as_deref()).as_deref() == Some("no") {
        let verdict = verdict(
            call,
            AutoEvaluatorVerdictKind::AllowCandidate,
            "allowed by the auto mode classifier",
        );
        return attempt.finish(verdict, ClassifierAttemptStatus::Success, None);
    }
    let second = match attempt.ask(STAGE_TWO, 1_024, 2) {
        Ok(response) => response,
        Err(failure) => return attempt.failed(failure),
    };
    let Some(reasoned) = second.content.as_deref().and_then(parse_reasoned) else {
        return attempt.failed(Failure::Unreadable);
    };
    let rule = clipped(reasoned.rule.as_deref().unwrap_or("none"), MAX_RULE_CHARS);
    let reason = clipped(
        reasoned.reason.as_deref().unwrap_or_default(),
        MAX_REASON_CHARS,
    );
    match reasoned.decision.trim().to_lowercase().as_str() {
        "allow" => {
            let verdict = verdict(call, AutoEvaluatorVerdictKind::AllowCandidate, &reason);
            attempt.finish(verdict, ClassifierAttemptStatus::Success, Some(rule))
        }
        "block" => {
            let summary = format!("[{}] {reason}", rule.trim_matches(['[', ']']));
            let verdict = verdict(call, AutoEvaluatorVerdictKind::DenyCandidate, &summary);
            attempt.finish(verdict, ClassifierAttemptStatus::Success, Some(rule))
        }
        _ => attempt.failed(Failure::Unreadable),
    }
}

impl Attempt<'_, '_> {
    fn ask(
        &mut self,
        instruction: &str,
        max_tokens: u32,
        stage: u8,
    ) -> Result<LlmResponse, Failure> {
        let call = self.call;
        if call.invocation.is_cancelled() {
            return Err(Failure::Stopped);
        }
        let request = ProviderRequest {
            messages: vec![
                json!({"role": "system", "content": call.system}),
                json!({
                    "role": "user",
                    "content": classifier_request::user_message(call.payload, instruction),
                }),
            ],
            tools: Vec::new(),
            model: call.model.to_owned(),
            settings: GenerationSettings {
                temperature: 0.0,
                max_tokens,
                reasoning_effort: None,
            },
            tool_choice: None,
        };
        let stage_deadline = Instant::now() + STAGE_DEADLINE;
        let deadline = call
            .invocation
            .deadline()
            .map_or(stage_deadline, |turn_deadline| {
                turn_deadline.min(stage_deadline)
            });
        let invocation = call.invocation.clone().with_deadline(deadline);
        let started = (call.now_unix_ms)();
        let result = call.client.chat_with_invocation(request, &invocation);
        let usage = result
            .as_ref()
            .map(|response| response.usage.clone())
            .unwrap_or_default();
        for (key, value) in &usage {
            *self.usage.entry(key.clone()).or_default() += value;
        }
        let result = result.map_err(|error| {
            if is_timeout(&error) {
                Failure::TimedOut
            } else {
                Failure::Provider
            }
        });
        let outcome = match &result {
            Ok(_) => "answered",
            Err(Failure::TimedOut) => "timed_out",
            Err(_) => "provider_error",
        };
        self.stages.push(ClassifierStageEvidence {
            stage,
            outcome: outcome.to_owned(),
            duration_ms: (call.now_unix_ms)().saturating_sub(started),
            usage,
        });
        result
    }

    fn failed(self, failure: Failure) -> ClassifierAttempt {
        let (kind, status, reason) = match failure {
            Failure::Stopped => (
                AutoEvaluatorVerdictKind::ParseFailure,
                ClassifierAttemptStatus::ProviderError,
                "the turn was stopped before the classifier answered",
            ),
            Failure::TimedOut => (
                AutoEvaluatorVerdictKind::Timeout,
                ClassifierAttemptStatus::ProviderTimeout,
                "the classifier did not answer in time",
            ),
            Failure::Provider => (
                AutoEvaluatorVerdictKind::ParseFailure,
                ClassifierAttemptStatus::ProviderError,
                "the classifier request failed",
            ),
            Failure::Unreadable => (
                AutoEvaluatorVerdictKind::ParseFailure,
                ClassifierAttemptStatus::ParseFailure,
                "the classifier answer could not be read",
            ),
        };
        let verdict = AutoEvaluatorVerdict {
            verdict: kind,
            confidence: EvaluatorConfidence::Unknown,
            scope_match: EvaluatorScopeMatch::Unknown,
            risk_summary: reason.to_owned(),
            evidence_refs: vec![CLASSIFIER_REF.to_owned()],
            expires_at_unix_ms: (self.call.now_unix_ms)(),
            evaluator_ref: Some(CLASSIFIER_REF.to_owned()),
            prompt_injection_signals: Vec::new(),
        };
        self.finish(verdict, status, None)
    }

    fn finish(
        self,
        verdict: AutoEvaluatorVerdict,
        status: ClassifierAttemptStatus,
        rule: Option<String>,
    ) -> ClassifierAttempt {
        ClassifierAttempt {
            verdict,
            status,
            usage: (!self.usage.is_empty()).then_some(self.usage),
            stages: self.stages,
            rule,
            started_at_unix_ms: self.started_at_unix_ms,
            completed_at_unix_ms: (self.call.now_unix_ms)(),
        }
    }
}

fn first_word(content: Option<&str>) -> Option<String> {
    content?.split_whitespace().next().map(|word| {
        word.trim_matches(|character: char| !character.is_alphanumeric())
            .to_lowercase()
    })
}

/// Reads the second stage answer when it is one JSON object or one fenced
/// block holding it, like the 022 classifier.
fn parse_reasoned(content: &str) -> Option<ReasonedVerdict> {
    let trimmed = content.trim();
    let fenced = trimmed
        .strip_prefix("```")
        .map(|rest| {
            rest.strip_prefix("json")
                .or_else(|| rest.strip_prefix("JSON"))
                .unwrap_or(rest)
        })
        .and_then(|rest| rest.trim_end().strip_suffix("```"))
        .map(str::trim);
    [Some(trimmed), fenced]
        .into_iter()
        .flatten()
        .find_map(|candidate| serde_json::from_str::<ReasonedVerdict>(candidate).ok())
}

fn verdict(
    call: &ClassifierCall<'_>,
    kind: AutoEvaluatorVerdictKind,
    summary: &str,
) -> AutoEvaluatorVerdict {
    AutoEvaluatorVerdict {
        verdict: kind,
        confidence: EvaluatorConfidence::High,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: redact_string(summary),
        evidence_refs: vec![CLASSIFIER_REF.to_owned()],
        expires_at_unix_ms: (call.now_unix_ms)().saturating_add(VERDICT_TTL_MS),
        evaluator_ref: Some(CLASSIFIER_REF.to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

fn is_timeout(error: &ProviderError) -> bool {
    let ProviderError::Api { message, .. } = error else {
        return false;
    };
    let message = message.to_ascii_lowercase();
    ["timeout", "timed out", "deadline"]
        .iter()
        .any(|needle| message.contains(needle))
}

fn clipped(text: &str, limit: usize) -> String {
    redact_string(&text.chars().take(limit).collect::<String>())
}

#[cfg(test)]
#[path = "classifier_stage_tests.rs"]
mod tests;
