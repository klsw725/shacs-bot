use serde::{Deserialize, Serialize};

use super::{
    Spec033AutomationFact, Spec033DiagnosticsReceipt, Spec033EvaluatorFact,
    Spec033HookConfirmationFact, Spec033OwnerFact, Spec033ReplayFact, Spec033RollbackCandidateFact,
    Spec033SelfImprovementFact, Spec033VerifyFact,
};

pub const SPEC033_SNAPSHOT_SCHEMA: &str = "spec033.projection_snapshot.v5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033Availability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033Owner {
    Goal,
    Evaluator,
    Automation,
    HookConfirmation,
    SelfImprovement,
    Verify,
    RollbackCandidate,
    Replay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033EvidenceSource {
    SessionMetadata,
    DurableStore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033EvidenceLineage {
    pub owner: Spec033Owner,
    pub source: Spec033EvidenceSource,
    pub evidence_refs: Vec<String>,
}

impl Spec033EvidenceLineage {
    pub fn new(
        owner: Spec033Owner,
        source: Spec033EvidenceSource,
        evidence_refs: Vec<String>,
    ) -> Self {
        Self {
            owner,
            source,
            evidence_refs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033GoalStatus {
    Unavailable,
    Active,
    Paused,
    Blocked,
    Done,
    Cleared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec033GoalVerdict {
    Done,
    Continue,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033GoalFact {
    pub goal_id: String,
    pub session_id: String,
    pub status: Spec033GoalStatus,
    pub turn_budget: u32,
    pub turns_used: u32,
    pub last_verdict: Option<Spec033GoalVerdict>,
    pub blocked: bool,
    pub stop_reason: Option<String>,
    pub budget: Spec033GoalBudgetFact,
    pub usage: Spec033GoalUsageSummary,
    pub user_interrupted: bool,
    pub latest_transition: Option<Spec033GoalTransitionFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033GoalUsageSummary {
    pub turn_limit: u32,
    pub turns_used: u32,
    pub remaining_turns: u32,
    pub exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033GoalBudgetFact {
    pub turn_budget: u32,
    pub turns_used: u32,
    pub remaining_turns: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033GoalTransitionFact {
    pub goal_id: String,
    pub prior_state: Spec033GoalStatus,
    pub current_state: Spec033GoalStatus,
    pub stop_reason: String,
    pub budget: Spec033GoalBudgetFact,
    pub user_interrupted: bool,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033GoalOwner {
    pub availability: Spec033Availability,
    pub fact: Option<Spec033GoalFact>,
    pub lineage: Spec033EvidenceLineage,
}

impl Spec033GoalOwner {
    pub fn unavailable() -> Self {
        Self {
            availability: Spec033Availability::Unavailable,
            fact: None,
            lineage: Spec033EvidenceLineage::new(
                Spec033Owner::Goal,
                Spec033EvidenceSource::SessionMetadata,
                Vec::new(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033CapabilityOwner {
    pub availability: Spec033Availability,
    pub lineage: Spec033EvidenceLineage,
}

impl Spec033CapabilityOwner {
    pub fn unavailable(owner: Spec033Owner, source: Spec033EvidenceSource) -> Self {
        Self {
            availability: Spec033Availability::Unavailable,
            lineage: Spec033EvidenceLineage::new(owner, source, Vec::new()),
        }
    }

    pub fn available(
        owner: Spec033Owner,
        source: Spec033EvidenceSource,
        evidence_refs: Vec<String>,
    ) -> Self {
        Self {
            availability: Spec033Availability::Available,
            lineage: Spec033EvidenceLineage::new(owner, source, evidence_refs),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec033Snapshot {
    pub schema: String,
    pub session_id: String,
    pub goal: Spec033GoalOwner,
    pub evaluator: Spec033OwnerFact<Spec033EvaluatorFact>,
    pub automation: Spec033OwnerFact<Spec033AutomationFact>,
    pub hook_confirmation: Spec033OwnerFact<Spec033HookConfirmationFact>,
    pub self_improvement: Spec033OwnerFact<Spec033SelfImprovementFact>,
    pub verify: Spec033OwnerFact<Spec033VerifyFact>,
    pub rollback_candidate: Spec033OwnerFact<Spec033RollbackCandidateFact>,
    pub replay: Spec033OwnerFact<Spec033ReplayFact>,
    pub diagnostics: Spec033DiagnosticsReceipt,
}

impl Spec033Snapshot {
    pub fn unavailable(session_id: impl Into<String>) -> Self {
        Self {
            schema: SPEC033_SNAPSHOT_SCHEMA.to_owned(),
            session_id: session_id.into(),
            goal: Spec033GoalOwner::unavailable(),
            evaluator: unavailable(
                Spec033Owner::Evaluator,
                Spec033EvidenceSource::SessionMetadata,
            ),
            automation: unavailable(
                Spec033Owner::Automation,
                Spec033EvidenceSource::DurableStore,
            ),
            hook_confirmation: unavailable(
                Spec033Owner::HookConfirmation,
                Spec033EvidenceSource::DurableStore,
            ),
            self_improvement: unavailable(
                Spec033Owner::SelfImprovement,
                Spec033EvidenceSource::DurableStore,
            ),
            verify: unavailable(Spec033Owner::Verify, Spec033EvidenceSource::DurableStore),
            rollback_candidate: unavailable(
                Spec033Owner::RollbackCandidate,
                Spec033EvidenceSource::DurableStore,
            ),
            replay: unavailable(Spec033Owner::Replay, Spec033EvidenceSource::DurableStore),
            diagnostics: Spec033DiagnosticsReceipt::unavailable(),
        }
    }
}

fn unavailable<T>(owner: Spec033Owner, source: Spec033EvidenceSource) -> Spec033OwnerFact<T> {
    Spec033OwnerFact::unavailable(owner, source)
}
