use crate::runtime::{
    permission_action::secret_ref_correlation_material, PermissionSecretRefEvidence,
    PermissionSecretRefStatus, PolicySafetySnapshotRef,
};
use serde::{Deserialize, Serialize};
use shacs_config::{RememberedPermissionEffect, RememberedPermissionMatcher};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approval_request_id: String,
    pub action_digest: String,
    pub snapshot_digest: String,
    pub requested_scope: String,
    pub risk_summary: String,
    pub allowed_decisions: Vec<ApprovalDecisionKind>,
    pub expires_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_ref_evidence: Vec<PermissionSecretRefEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approval_request_id: String,
    pub action_digest: String,
    pub snapshot_digest: String,
    pub decision: ApprovalDecisionKind,
    pub approved_scope: String,
    pub actor: ApprovalActor,
    pub decided_at_unix_ms: u64,
    pub consumed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_safety_snapshot_ref: Option<PolicySafetySnapshotRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secret_ref_evidence: Vec<PermissionSecretRefEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    Approved,
    ApprovedForSession,
    ApprovedForProject,
    Denied,
    DeniedForSession,
    DeniedForProject,
    InspectOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionEffect {
    Allow,
    Deny,
    Inspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionScope {
    Once,
    Session,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ApprovalDecisionOption {
    pub value: &'static str,
    pub decision: ApprovalDecisionKind,
    pub effect: ApprovalDecisionEffect,
    pub scope: ApprovalDecisionScope,
}

impl ApprovalDecisionKind {
    pub const fn effect(self) -> ApprovalDecisionEffect {
        match self {
            Self::Approved | Self::ApprovedForSession | Self::ApprovedForProject => {
                ApprovalDecisionEffect::Allow
            }
            Self::Denied | Self::DeniedForSession | Self::DeniedForProject => {
                ApprovalDecisionEffect::Deny
            }
            Self::InspectOnly => ApprovalDecisionEffect::Inspect,
        }
    }

    pub const fn scope(self) -> Option<ApprovalDecisionScope> {
        match self {
            Self::Approved | Self::Denied => Some(ApprovalDecisionScope::Once),
            Self::ApprovedForSession | Self::DeniedForSession => {
                Some(ApprovalDecisionScope::Session)
            }
            Self::ApprovedForProject | Self::DeniedForProject => {
                Some(ApprovalDecisionScope::Project)
            }
            Self::InspectOnly => None,
        }
    }

    pub const fn option_value(self) -> &'static str {
        match self {
            Self::Approved => "approve",
            Self::ApprovedForSession => "approve_session",
            Self::ApprovedForProject => "approve_project",
            Self::Denied => "deny",
            Self::DeniedForSession => "deny_session",
            Self::DeniedForProject => "deny_project",
            Self::InspectOnly => "inspect_only",
        }
    }
}

pub const fn approval_decision_options() -> [ApprovalDecisionOption; 6] {
    [
        ApprovalDecisionOption {
            value: ApprovalDecisionKind::Approved.option_value(),
            decision: ApprovalDecisionKind::Approved,
            effect: ApprovalDecisionEffect::Allow,
            scope: ApprovalDecisionScope::Once,
        },
        ApprovalDecisionOption {
            value: ApprovalDecisionKind::Denied.option_value(),
            decision: ApprovalDecisionKind::Denied,
            effect: ApprovalDecisionEffect::Deny,
            scope: ApprovalDecisionScope::Once,
        },
        ApprovalDecisionOption {
            value: ApprovalDecisionKind::ApprovedForSession.option_value(),
            decision: ApprovalDecisionKind::ApprovedForSession,
            effect: ApprovalDecisionEffect::Allow,
            scope: ApprovalDecisionScope::Session,
        },
        ApprovalDecisionOption {
            value: ApprovalDecisionKind::ApprovedForProject.option_value(),
            decision: ApprovalDecisionKind::ApprovedForProject,
            effect: ApprovalDecisionEffect::Allow,
            scope: ApprovalDecisionScope::Project,
        },
        ApprovalDecisionOption {
            value: ApprovalDecisionKind::DeniedForSession.option_value(),
            decision: ApprovalDecisionKind::DeniedForSession,
            effect: ApprovalDecisionEffect::Deny,
            scope: ApprovalDecisionScope::Session,
        },
        ApprovalDecisionOption {
            value: ApprovalDecisionKind::DeniedForProject.option_value(),
            decision: ApprovalDecisionKind::DeniedForProject,
            effect: ApprovalDecisionEffect::Deny,
            scope: ApprovalDecisionScope::Project,
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalActor {
    LocalUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalCacheEntry {
    pub request: ApprovalRequest,
    pub decision: ApprovalDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionApprovalCacheEntry {
    pub session_key: String,
    pub approval_context_digest: String,
    #[serde(default)]
    pub reuse_match: SessionApprovalReuseMatch,
    pub approval: ApprovalCacheEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRememberedPermissionRule {
    pub session_key: String,
    pub approval_context_digest: String,
    pub effect: RememberedPermissionEffect,
    pub matcher: RememberedPermissionMatcher,
    pub created_unix_ms: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub legacy_imported: bool,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRememberedPermissionRules {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<SessionRememberedPermissionRule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SessionRememberedPermissionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRememberedPermissionDiagnostic {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SessionApprovalReuseMatch {
    #[default]
    ExactAction,
    ExecCommandPattern {
        pattern: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalCorrelationError {
    RequestMismatch,
    ActionMismatch,
    SnapshotMismatch,
    ScopeMismatch,
    Expired,
    Consumed,
    InspectOnly,
    Denied,
    DecisionNotAllowed,
    PolicySafetySnapshotMismatch,
    PolicySafetySnapshotMalformed,
    PolicySafetySnapshotStale,
    SecretRefEvidenceMismatch,
    SecretRefEvidenceStale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalCorrelation {
    pub approval_ref: Option<String>,
    pub error: Option<ApprovalCorrelationError>,
}

impl ApprovalCorrelation {
    pub fn approved(request_id: String) -> Self {
        Self {
            approval_ref: Some(request_id),
            error: None,
        }
    }

    pub fn rejected(error: ApprovalCorrelationError) -> Self {
        Self {
            approval_ref: None,
            error: Some(error),
        }
    }

    pub fn is_approved(&self) -> bool {
        self.error.is_none() && self.approval_ref.is_some()
    }
}

pub fn correlate_approval(
    request: &ApprovalRequest,
    decision: &ApprovalDecision,
    now_unix_ms: u64,
) -> ApprovalCorrelation {
    if request.approval_request_id != decision.approval_request_id {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::RequestMismatch);
    }
    if request.action_digest != decision.action_digest {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::ActionMismatch);
    }
    if request.snapshot_digest != decision.snapshot_digest {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::SnapshotMismatch);
    }
    if let Err(error) = correlate_policy_safety_snapshot_ref(
        request.policy_safety_snapshot_ref.as_ref(),
        decision.policy_safety_snapshot_ref.as_ref(),
        now_unix_ms,
    ) {
        return ApprovalCorrelation::rejected(error);
    }
    if secret_ref_correlation_material(&request.secret_ref_evidence)
        != secret_ref_correlation_material(&decision.secret_ref_evidence)
    {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::SecretRefEvidenceMismatch);
    }
    if request
        .secret_ref_evidence
        .iter()
        .chain(decision.secret_ref_evidence.iter())
        .any(|evidence| evidence.status == PermissionSecretRefStatus::Stale)
    {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::SecretRefEvidenceStale);
    }
    if request.requested_scope != decision.approved_scope {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::ScopeMismatch);
    }
    if now_unix_ms > request.expires_at_unix_ms
        || decision.decided_at_unix_ms > request.expires_at_unix_ms
    {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::Expired);
    }
    if decision.consumed {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::Consumed);
    }
    if !request.allowed_decisions.contains(&decision.decision) {
        return ApprovalCorrelation::rejected(ApprovalCorrelationError::DecisionNotAllowed);
    }
    match decision.decision {
        ApprovalDecisionKind::Approved
        | ApprovalDecisionKind::ApprovedForSession
        | ApprovalDecisionKind::ApprovedForProject => {
            ApprovalCorrelation::approved(request.approval_request_id.clone())
        }
        ApprovalDecisionKind::Denied
        | ApprovalDecisionKind::DeniedForSession
        | ApprovalDecisionKind::DeniedForProject => {
            ApprovalCorrelation::rejected(ApprovalCorrelationError::Denied)
        }
        ApprovalDecisionKind::InspectOnly => {
            ApprovalCorrelation::rejected(ApprovalCorrelationError::InspectOnly)
        }
    }
}

pub fn correlate_policy_safety_snapshot_ref(
    expected: Option<&PolicySafetySnapshotRef>,
    actual: Option<&PolicySafetySnapshotRef>,
    now_unix_ms: u64,
) -> Result<(), ApprovalCorrelationError> {
    validate_policy_safety_snapshot_ref(expected, now_unix_ms)?;
    validate_policy_safety_snapshot_ref(actual, now_unix_ms)?;
    match (expected, actual) {
        (None, None) => Ok(()),
        (Some(expected), Some(actual)) if expected == actual => Ok(()),
        (Some(_), Some(_)) | (Some(_), None) | (None, Some(_)) => {
            Err(ApprovalCorrelationError::PolicySafetySnapshotMismatch)
        }
    }
}

fn validate_policy_safety_snapshot_ref(
    reference: Option<&PolicySafetySnapshotRef>,
    now_unix_ms: u64,
) -> Result<(), ApprovalCorrelationError> {
    let Some(reference) = reference else {
        return Ok(());
    };
    if reference.snapshot_id.0.trim().is_empty()
        || !is_sha256_hex(&reference.policy_safety_digest.0)
    {
        return Err(ApprovalCorrelationError::PolicySafetySnapshotMalformed);
    }
    if let Some(expires_at_unix_ms) = reference.expires_at_unix_ms {
        if now_unix_ms > expires_at_unix_ms {
            return Err(ApprovalCorrelationError::PolicySafetySnapshotStale);
        }
    }
    Ok(())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval_request(allowed_decisions: Vec<ApprovalDecisionKind>) -> ApprovalRequest {
        ApprovalRequest {
            approval_request_id: "approval_test".to_owned(),
            action_digest: "action_digest".to_owned(),
            snapshot_digest: "snapshot_digest".to_owned(),
            requested_scope: "session:test".to_owned(),
            risk_summary: "Run tool `exec`".to_owned(),
            allowed_decisions,
            expires_at_unix_ms: 2_000,
            policy_safety_snapshot_ref: None,
            secret_ref_evidence: Vec::new(),
        }
    }

    fn approval_decision(
        decision: ApprovalDecisionKind,
        decided_at_unix_ms: u64,
    ) -> ApprovalDecision {
        ApprovalDecision {
            approval_request_id: "approval_test".to_owned(),
            action_digest: "action_digest".to_owned(),
            snapshot_digest: "snapshot_digest".to_owned(),
            decision,
            approved_scope: "session:test".to_owned(),
            actor: ApprovalActor::LocalUser,
            decided_at_unix_ms,
            consumed: false,
            policy_safety_snapshot_ref: None,
            secret_ref_evidence: Vec::new(),
        }
    }

    #[test]
    fn permission_approval_scope_legacy_serialized_decisions_keep_current_semantics() {
        let fixtures = [
            ("\"approved\"", ApprovalDecisionKind::Approved, None),
            (
                "\"approved_for_session\"",
                ApprovalDecisionKind::ApprovedForSession,
                None,
            ),
            (
                "\"denied\"",
                ApprovalDecisionKind::Denied,
                Some(ApprovalCorrelationError::Denied),
            ),
        ];

        for (serialized, decision_kind, expected_error) in fixtures {
            let parsed: ApprovalDecisionKind = serde_json::from_str(serialized)
                .expect("legacy approval decision kind should deserialize");
            assert_eq!(parsed, decision_kind);
            assert_eq!(
                serde_json::to_string(&parsed).expect("serialize decision kind"),
                serialized
            );

            let request = approval_request(vec![decision_kind]);
            let decision = approval_decision(decision_kind, 1_500);
            let correlation = correlate_approval(&request, &decision, 1_500);
            match expected_error {
                None => assert!(correlation.is_approved()),
                Some(error) => assert_eq!(correlation.error, Some(error)),
            }
        }
    }

    #[test]
    fn permission_approval_scope_maps_all_decisions_to_effect_scope_and_options() {
        let fixtures = [
            (
                ApprovalDecisionKind::Approved,
                ApprovalDecisionEffect::Allow,
                Some(ApprovalDecisionScope::Once),
                "approve",
            ),
            (
                ApprovalDecisionKind::ApprovedForSession,
                ApprovalDecisionEffect::Allow,
                Some(ApprovalDecisionScope::Session),
                "approve_session",
            ),
            (
                ApprovalDecisionKind::Denied,
                ApprovalDecisionEffect::Deny,
                Some(ApprovalDecisionScope::Once),
                "deny",
            ),
            (
                ApprovalDecisionKind::ApprovedForProject,
                ApprovalDecisionEffect::Allow,
                Some(ApprovalDecisionScope::Project),
                "approve_project",
            ),
            (
                ApprovalDecisionKind::DeniedForSession,
                ApprovalDecisionEffect::Deny,
                Some(ApprovalDecisionScope::Session),
                "deny_session",
            ),
            (
                ApprovalDecisionKind::DeniedForProject,
                ApprovalDecisionEffect::Deny,
                Some(ApprovalDecisionScope::Project),
                "deny_project",
            ),
            (
                ApprovalDecisionKind::InspectOnly,
                ApprovalDecisionEffect::Inspect,
                None,
                "inspect_only",
            ),
        ];

        for (decision, effect, scope, value) in fixtures {
            assert_eq!(decision.effect(), effect);
            assert_eq!(decision.scope(), scope);
            assert_eq!(decision.option_value(), value);
        }

        let option_values = approval_decision_options()
            .into_iter()
            .map(|option| option.value)
            .collect::<Vec<_>>();
        assert_eq!(
            option_values,
            vec![
                "approve",
                "deny",
                "approve_session",
                "approve_project",
                "deny_session",
                "deny_project",
            ]
        );
    }

    #[test]
    fn permission_approval_scope_correlates_project_and_deny_decisions_without_rule_expiry() {
        let allowed_decisions = vec![
            ApprovalDecisionKind::ApprovedForProject,
            ApprovalDecisionKind::DeniedForSession,
            ApprovalDecisionKind::DeniedForProject,
        ];
        let request = approval_request(allowed_decisions);

        let approved = correlate_approval(
            &request,
            &approval_decision(ApprovalDecisionKind::ApprovedForProject, 1_500),
            1_500,
        );
        assert!(approved.is_approved());

        let denied_session = correlate_approval(
            &request,
            &approval_decision(ApprovalDecisionKind::DeniedForSession, 1_500),
            1_500,
        );
        assert_eq!(denied_session.error, Some(ApprovalCorrelationError::Denied));

        let denied_project = correlate_approval(
            &request,
            &approval_decision(ApprovalDecisionKind::DeniedForProject, 1_500),
            1_500,
        );
        assert_eq!(denied_project.error, Some(ApprovalCorrelationError::Denied));
    }

    #[test]
    fn permission_approval_scope_rejects_disallowed_expired_consumed_and_inspect_only() {
        let request = approval_request(vec![ApprovalDecisionKind::Approved]);
        let disallowed_project = correlate_approval(
            &request,
            &approval_decision(ApprovalDecisionKind::ApprovedForProject, 1_500),
            1_500,
        );
        assert_eq!(
            disallowed_project.error,
            Some(ApprovalCorrelationError::DecisionNotAllowed)
        );

        let expired = correlate_approval(
            &request,
            &approval_decision(ApprovalDecisionKind::Approved, 2_001),
            1_500,
        );
        assert_eq!(expired.error, Some(ApprovalCorrelationError::Expired));

        let mut consumed_decision = approval_decision(ApprovalDecisionKind::Approved, 1_500);
        consumed_decision.consumed = true;
        let consumed = correlate_approval(&request, &consumed_decision, 1_500);
        assert_eq!(consumed.error, Some(ApprovalCorrelationError::Consumed));

        let inspect_request = approval_request(vec![ApprovalDecisionKind::InspectOnly]);
        let inspect = correlate_approval(
            &inspect_request,
            &approval_decision(ApprovalDecisionKind::InspectOnly, 1_500),
            1_500,
        );
        assert_eq!(inspect.error, Some(ApprovalCorrelationError::InspectOnly));
    }
}
