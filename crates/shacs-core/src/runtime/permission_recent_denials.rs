use crate::runtime::{
    AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, EvaluatorConfidence, EvaluatorScopeMatch,
    PermissionMode, PermissionPolicyDecision, PermissionPolicyDecisionKind, PermissionPolicyReason,
    PermissionSecretRefEvidence, PermissionedAction, PolicySafetySnapshotRef, RuntimeToolCall,
    SafetyCapability, ToolExecutionContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

pub const RECENT_AUTO_MODE_DENIAL_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentAutoModeDenial {
    pub denial_id: String,
    pub created_at_unix_ms: u64,
    pub session_digest: String,
    pub turn_digest: String,
    pub tool_name: String,
    pub capabilities: Vec<SafetyCapability>,
    pub target_summary: Vec<String>,
    pub action_digest: String,
    pub argument_digest: String,
    pub snapshot_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_ref_evidence: Vec<PermissionSecretRefEvidence>,
    pub decision_reason: PermissionPolicyReason,
    pub classifier_verdict: AutoEvaluatorVerdictKind,
    pub classifier_confidence: EvaluatorConfidence,
    pub classifier_scope_match: EvaluatorScopeMatch,
    pub retryable: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentAutoModeDenialStore {
    denials: Vec<RecentAutoModeDenial>,
}

#[derive(Clone, PartialEq)]
pub struct RecentAutoModeRetryToken {
    denial_id: String,
    action_digest: String,
    argument_digest: String,
    snapshot_digest: String,
    policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    secret_ref_evidence: Vec<PermissionSecretRefEvidence>,
    tool_call: RuntimeToolCall,
    tool_context: ToolExecutionContext,
    expires_at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct RecentAutoModeRetryTokenMatch<'a> {
    pub action_digest: &'a str,
    pub argument_digest: &'a str,
    pub snapshot_digest: &'a str,
    pub policy_safety_snapshot_ref: Option<&'a PolicySafetySnapshotRef>,
    pub secret_ref_evidence: &'a [PermissionSecretRefEvidence],
}

impl<'a> RecentAutoModeRetryTokenMatch<'a> {
    pub fn from_denial(denial: &'a RecentAutoModeDenial) -> Self {
        Self {
            action_digest: &denial.action_digest,
            argument_digest: &denial.argument_digest,
            snapshot_digest: &denial.snapshot_digest,
            policy_safety_snapshot_ref: denial.policy_safety_snapshot_ref.as_ref(),
            secret_ref_evidence: &denial.secret_ref_evidence,
        }
    }
}

impl RecentAutoModeRetryToken {
    pub fn new(
        denial: &RecentAutoModeDenial,
        tool_call: RuntimeToolCall,
        tool_context: ToolExecutionContext,
        expires_at_unix_ms: u64,
    ) -> Self {
        Self {
            denial_id: denial.denial_id.clone(),
            action_digest: denial.action_digest.clone(),
            argument_digest: denial.argument_digest.clone(),
            snapshot_digest: denial.snapshot_digest.clone(),
            policy_safety_snapshot_ref: denial.policy_safety_snapshot_ref.clone(),
            secret_ref_evidence: denial.secret_ref_evidence.clone(),
            tool_call,
            tool_context,
            expires_at_unix_ms,
        }
    }

    pub fn denial_id(&self) -> &str {
        &self.denial_id
    }

    pub fn action_digest(&self) -> &str {
        &self.action_digest
    }

    pub fn argument_digest(&self) -> &str {
        &self.argument_digest
    }

    pub fn snapshot_digest(&self) -> &str {
        &self.snapshot_digest
    }

    pub fn policy_safety_snapshot_ref(&self) -> Option<&PolicySafetySnapshotRef> {
        self.policy_safety_snapshot_ref.as_ref()
    }

    pub fn secret_ref_evidence(&self) -> &[PermissionSecretRefEvidence] {
        &self.secret_ref_evidence
    }

    pub fn tool_call(&self) -> &RuntimeToolCall {
        &self.tool_call
    }

    pub fn tool_context(&self) -> &ToolExecutionContext {
        &self.tool_context
    }

    pub fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}

impl fmt::Debug for RecentAutoModeRetryToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RecentAutoModeRetryToken")
            .field("denial_id", &self.denial_id)
            .field("action_digest", &self.action_digest)
            .field("argument_digest", &self.argument_digest)
            .field("snapshot_digest", &self.snapshot_digest)
            .field(
                "policy_safety_snapshot_ref",
                &self.policy_safety_snapshot_ref,
            )
            .field("secret_ref_count", &self.secret_ref_evidence.len())
            .field("tool_name", &self.tool_call.name)
            .field("tool_call", &"<redacted>")
            .field("tool_context", &"<redacted>")
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecentAutoModeRetryTokenConsumeError {
    Missing,
    Expired,
    Consumed,
    Mismatched,
}

#[derive(Debug, Clone, Default)]
pub struct RecentAutoModeRetryTokenStore {
    tokens: BTreeMap<String, RecentAutoModeRetryTokenEntry>,
}

#[derive(Debug, Clone)]
struct RecentAutoModeRetryTokenEntry {
    token: RecentAutoModeRetryToken,
    consumed: bool,
}

impl RecentAutoModeRetryTokenStore {
    pub fn insert(&mut self, token: RecentAutoModeRetryToken) {
        self.tokens.insert(
            token.denial_id.clone(),
            RecentAutoModeRetryTokenEntry {
                token,
                consumed: false,
            },
        );
    }

    pub fn extend<I>(&mut self, tokens: I)
    where
        I: IntoIterator<Item = RecentAutoModeRetryToken>,
    {
        for token in tokens {
            self.insert(token);
        }
    }

    pub fn is_available(&self, denial_id: &str, now_unix_ms: u64) -> bool {
        self.tokens
            .get(denial_id)
            .is_some_and(|entry| !entry.consumed && now_unix_ms <= entry.token.expires_at_unix_ms)
    }

    pub fn peek(
        &self,
        denial_id: &str,
        now_unix_ms: u64,
    ) -> Result<&RecentAutoModeRetryToken, RecentAutoModeRetryTokenConsumeError> {
        let Some(entry) = self.tokens.get(denial_id) else {
            return Err(RecentAutoModeRetryTokenConsumeError::Missing);
        };
        if entry.consumed {
            return Err(RecentAutoModeRetryTokenConsumeError::Consumed);
        }
        if now_unix_ms > entry.token.expires_at_unix_ms {
            return Err(RecentAutoModeRetryTokenConsumeError::Expired);
        }
        Ok(&entry.token)
    }

    pub fn consume(
        &mut self,
        denial_id: &str,
        expected: RecentAutoModeRetryTokenMatch<'_>,
        now_unix_ms: u64,
    ) -> Result<RecentAutoModeRetryToken, RecentAutoModeRetryTokenConsumeError> {
        let Some(entry) = self.tokens.get_mut(denial_id) else {
            return Err(RecentAutoModeRetryTokenConsumeError::Missing);
        };
        if entry.consumed {
            return Err(RecentAutoModeRetryTokenConsumeError::Consumed);
        }
        if now_unix_ms > entry.token.expires_at_unix_ms {
            entry.consumed = true;
            return Err(RecentAutoModeRetryTokenConsumeError::Expired);
        }
        if entry.token.action_digest != expected.action_digest
            || entry.token.argument_digest != expected.argument_digest
            || entry.token.snapshot_digest != expected.snapshot_digest
            || entry.token.policy_safety_snapshot_ref.as_ref()
                != expected.policy_safety_snapshot_ref
            || entry.token.secret_ref_evidence.as_slice() != expected.secret_ref_evidence
        {
            entry.consumed = true;
            return Err(RecentAutoModeRetryTokenConsumeError::Mismatched);
        }
        entry.consumed = true;
        Ok(entry.token.clone())
    }

    pub fn invalidate(&mut self, denial_id: &str) {
        if let Some(entry) = self.tokens.get_mut(denial_id) {
            entry.consumed = true;
        }
    }
}

impl RecentAutoModeDenialStore {
    pub fn from_denials(denials: Vec<RecentAutoModeDenial>) -> Self {
        let mut store = Self { denials };
        store.truncate();
        store
    }

    pub fn push_front(&mut self, denial: RecentAutoModeDenial) {
        self.denials
            .retain(|existing| existing.denial_id != denial.denial_id);
        self.denials.insert(0, denial);
        self.truncate();
    }

    pub fn extend_newest_first<I>(&mut self, denials: I)
    where
        I: IntoIterator<Item = RecentAutoModeDenial>,
    {
        let denials = denials.into_iter().collect::<Vec<_>>();
        for denial in denials.into_iter().rev() {
            self.push_front(denial);
        }
    }

    pub fn as_slice(&self) -> &[RecentAutoModeDenial] {
        &self.denials
    }

    pub fn into_vec(self) -> Vec<RecentAutoModeDenial> {
        self.denials
    }

    fn truncate(&mut self) {
        self.denials.truncate(RECENT_AUTO_MODE_DENIAL_LIMIT);
    }
}

pub fn recent_auto_mode_denial_from_classifier_decision(
    action: &PermissionedAction,
    decision: &PermissionPolicyDecision,
    evaluator: &AutoEvaluatorVerdict,
    created_at_unix_ms: u64,
) -> Option<RecentAutoModeDenial> {
    if action.permission_mode_snapshot.mode != PermissionMode::Auto
        || !matches!(
            decision.kind,
            PermissionPolicyDecisionKind::Ask | PermissionPolicyDecisionKind::Deny
        )
        || decision.reason != PermissionPolicyReason::EvaluatorUncertain
        || evaluator.verdict != AutoEvaluatorVerdictKind::DenyCandidate
        || evaluator.evaluator_ref.as_deref() != Some("auto-mode-classifier")
    {
        return None;
    }

    let retryable = evaluator.confidence == EvaluatorConfidence::High
        && evaluator.scope_match == EvaluatorScopeMatch::Requested;

    Some(RecentAutoModeDenial {
        denial_id: denial_id(action),
        created_at_unix_ms,
        session_digest: action_id_digest("session", &action.session_id),
        turn_digest: action_id_digest("turn", &action.turn_id),
        tool_name: action.tool_name.clone(),
        capabilities: action.capabilities.clone(),
        target_summary: sanitized_target_summary(action),
        action_digest: action.action_digest.clone(),
        argument_digest: action.argument_digest.clone(),
        snapshot_digest: action.snapshot_digest.clone(),
        policy_safety_snapshot_ref: action.policy_safety_snapshot_ref.clone(),
        secret_ref_evidence: action.secret_ref_evidence.clone(),
        decision_reason: decision.reason.clone(),
        classifier_verdict: evaluator.verdict,
        classifier_confidence: evaluator.confidence,
        classifier_scope_match: evaluator.scope_match,
        retryable,
    })
}

fn sanitized_target_summary(action: &PermissionedAction) -> Vec<String> {
    action
        .target_refs
        .iter()
        .map(|target| {
            let digest_prefix = target.digest.chars().take(12).collect::<String>();
            format!("target:{digest_prefix}")
        })
        .collect()
}

fn denial_id(action: &PermissionedAction) -> String {
    let digest = digest_json(&json!({
        "action_digest": &action.action_digest,
        "argument_digest": &action.argument_digest,
        "snapshot_digest": &action.snapshot_digest,
        "policy_safety_snapshot_ref": &action.policy_safety_snapshot_ref,
    }));
    format!(
        "auto_denial_{}",
        digest.chars().take(16).collect::<String>()
    )
}

fn action_id_digest(kind: &str, value: &str) -> String {
    digest_json(&json!({
        "kind": kind,
        "value": value,
    }))
}

fn digest_json(value: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}
