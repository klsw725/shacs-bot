use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approval_request_id: String,
    pub action_digest: String,
    pub snapshot_digest: String,
    pub requested_scope: String,
    pub risk_summary: String,
    pub allowed_decisions: Vec<ApprovalDecisionKind>,
    pub expires_at_unix_ms: u64,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    Approved,
    Denied,
    InspectOnly,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalCorrelationError {
    RequestMismatch,
    ActionMismatch,
    SnapshotMismatch,
    Expired,
    Consumed,
    InspectOnly,
    Denied,
    DecisionNotAllowed,
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
        ApprovalDecisionKind::Approved => {
            ApprovalCorrelation::approved(request.approval_request_id.clone())
        }
        ApprovalDecisionKind::Denied => {
            ApprovalCorrelation::rejected(ApprovalCorrelationError::Denied)
        }
        ApprovalDecisionKind::InspectOnly => {
            ApprovalCorrelation::rejected(ApprovalCorrelationError::InspectOnly)
        }
    }
}
