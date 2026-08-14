use super::{Spec033Availability, Spec033EvidenceLineage, Spec033EvidenceSource, Spec033Owner};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033EvaluatorRoute {
    Notify,
    Suppress,
    Continue,
    Escalate,
    Verify,
    RollbackCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033EvaluatorFact {
    pub verdict: String,
    pub route: Spec033EvaluatorRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033AutomationJobStatus {
    Pending,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Suppressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033DeliveryStatus {
    NotRequested,
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033AutomationFact {
    pub work_id: String,
    pub job_id: String,
    pub run_id: String,
    pub turn_id: Option<String>,
    pub snapshot_id: Option<String>,
    pub snapshot_digest: Option<String>,
    pub checkpoint_id: Option<String>,
    pub artifact_refs: Vec<String>,
    pub job_status: Spec033AutomationJobStatus,
    pub delivery_status: Spec033DeliveryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033DiagnosticUnavailableReason {
    MissingOwnerEvidence,
    IdentifierNotRecorded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033DiagnosticLink {
    pub availability: Spec033Availability,
    pub value: Option<String>,
    pub unavailable_reason: Option<Spec033DiagnosticUnavailableReason>,
}

impl Spec033DiagnosticLink {
    pub fn available(value: impl Into<String>) -> Self {
        Self {
            availability: Spec033Availability::Available,
            value: Some(value.into()),
            unavailable_reason: None,
        }
    }

    pub const fn unavailable(reason: Spec033DiagnosticUnavailableReason) -> Self {
        Self {
            availability: Spec033Availability::Unavailable,
            value: None,
            unavailable_reason: Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033DiagnosticsReceipt {
    pub goal_id: Spec033DiagnosticLink,
    pub automation_job_id: Spec033DiagnosticLink,
    pub turn_id: Spec033DiagnosticLink,
    pub evaluator_request_id: Spec033DiagnosticLink,
    pub hook_confirmation_event_id: Spec033DiagnosticLink,
    pub checkpoint_id: Spec033DiagnosticLink,
    pub trajectory_id: Spec033DiagnosticLink,
    pub execution_snapshot_id: Spec033DiagnosticLink,
    pub execution_snapshot_digest: Spec033DiagnosticLink,
    pub safe_artifact_refs: Spec033DiagnosticLinks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033DiagnosticLinks {
    pub availability: Spec033Availability,
    pub values: Vec<String>,
    pub unavailable_reason: Option<Spec033DiagnosticUnavailableReason>,
}

impl Spec033DiagnosticLinks {
    pub fn available(values: Vec<String>) -> Self {
        Self {
            availability: Spec033Availability::Available,
            values,
            unavailable_reason: None,
        }
    }

    pub const fn unavailable(reason: Spec033DiagnosticUnavailableReason) -> Self {
        Self {
            availability: Spec033Availability::Unavailable,
            values: Vec::new(),
            unavailable_reason: Some(reason),
        }
    }
}

impl Spec033DiagnosticsReceipt {
    pub const fn unavailable() -> Self {
        let missing = Spec033DiagnosticUnavailableReason::MissingOwnerEvidence;
        let unrecorded = Spec033DiagnosticUnavailableReason::IdentifierNotRecorded;
        Self {
            goal_id: Spec033DiagnosticLink::unavailable(missing),
            automation_job_id: Spec033DiagnosticLink::unavailable(missing),
            turn_id: Spec033DiagnosticLink::unavailable(missing),
            evaluator_request_id: Spec033DiagnosticLink::unavailable(missing),
            hook_confirmation_event_id: Spec033DiagnosticLink::unavailable(unrecorded),
            checkpoint_id: Spec033DiagnosticLink::unavailable(unrecorded),
            trajectory_id: Spec033DiagnosticLink::unavailable(missing),
            execution_snapshot_id: Spec033DiagnosticLink::unavailable(missing),
            execution_snapshot_digest: Spec033DiagnosticLink::unavailable(missing),
            safe_artifact_refs: Spec033DiagnosticLinks::unavailable(missing),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033HookConfirmationFact {
    NotRequired,
    Confirmed,
    Denied,
    HeadlessDenied,
    Vetoed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033SelfImprovementFact {
    pub proposal_id: String,
    pub applied: bool,
    pub rolled_back: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033VerifyFact {
    pub proposal_id: String,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033RollbackCandidateFact {
    pub proposal_id: String,
    pub verify_failure_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033ReplayStatus {
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033ReplayFact {
    pub receipt_id: String,
    pub correlation_id: String,
    pub trajectory_id: String,
    pub status: Spec033ReplayStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033OwnerFact<T> {
    pub availability: Spec033Availability,
    pub fact: Option<T>,
    pub lineage: Spec033EvidenceLineage,
}

impl<T> Spec033OwnerFact<T> {
    pub fn unavailable(owner: Spec033Owner, source: Spec033EvidenceSource) -> Self {
        Self {
            availability: Spec033Availability::Unavailable,
            fact: None,
            lineage: Spec033EvidenceLineage::new(owner, source, Vec::new()),
        }
    }

    pub fn available(
        owner: Spec033Owner,
        source: Spec033EvidenceSource,
        fact: T,
        evidence_refs: Vec<String>,
    ) -> Self {
        Self {
            availability: Spec033Availability::Available,
            fact: Some(fact),
            lineage: Spec033EvidenceLineage::new(owner, source, evidence_refs),
        }
    }
}
