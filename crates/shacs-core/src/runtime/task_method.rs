use super::task_method_calc::CalculationEvidence;
use super::task_method_image::ImageEvidence;
pub use super::task_method_reason::Reason;
use super::task_method_report::ReportEvidence;
use serde::{Deserialize, Serialize};
use shacs_command::MethodName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodId {
    General,
    ReproducibleCalculation,
    SourceGroundedReport,
    ImageGenerationReview,
    Composed,
}

impl From<MethodName> for MethodId {
    fn from(name: MethodName) -> Self {
        match name {
            MethodName::General => Self::General,
            MethodName::ReproducibleCalculation => Self::ReproducibleCalculation,
            MethodName::SourceGroundedReport => Self::SourceGroundedReport,
            MethodName::ImageGenerationReview => Self::ImageGenerationReview,
        }
    }
}

impl MethodId {
    pub fn canonical(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::ReproducibleCalculation => "reproducible_calculation",
            Self::SourceGroundedReport => "source_grounded_report",
            Self::ImageGenerationReview => "image_generation_review",
            Self::Composed => "composed",
        }
    }

    pub fn checks(self) -> &'static [&'static str] {
        match self {
            Self::General | Self::Composed => &[],
            Self::ReproducibleCalculation => {
                &["inputs", "units", "assumptions", "independent check"]
            }
            Self::SourceGroundedReport => &[
                "sources actually read",
                "claim-source linkage",
                "unresolved claims",
            ],
            Self::ImageGenerationReview => {
                &["generated artifact", "stated requirements", "user revision"]
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSource {
    Automatic,
    Explicit,
    OneShot,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    Supported,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    Available,
    RequiresRuntimeCheck,
    Unavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    NotEvaluated,
    WaitingApproval,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Availability {
    pub support: Support,
    pub readiness: Readiness,
    pub missing_required_tools: Vec<String>,
    pub missing_optional_tools: Vec<String>,
}

impl Availability {
    pub fn blocked(&self) -> bool {
        self.support == Support::Unsupported || self.readiness == Readiness::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimState {
    NotVerified,
    Verified,
    Failed,
    SupersededNotInherited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationClaim {
    pub check: String,
    pub state: ClaimState,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub revision: u64,
    pub method: MethodId,
    pub source: SelectionSource,
    pub reason: Reason,
    pub availability: Availability,
    pub gate: Gate,
    pub superseded: bool,
    pub validation_claims: Vec<ValidationClaim>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<MethodId>,
}

impl Selection {
    /// The methods whose checks apply: each part of a composed request, or the
    /// selected specialized method itself.
    pub fn parts(&self) -> Vec<MethodId> {
        match self.method {
            MethodId::Composed => self.components.clone(),
            MethodId::General => Vec::new(),
            method => vec![method],
        }
    }

    pub fn visible(&self) -> bool {
        self.method != MethodId::General
            || self.source != SelectionSource::Automatic
            || self.revision > 1
    }

    pub fn supersede(&mut self) {
        self.superseded = true;
        for claim in &mut self.validation_claims {
            claim.state = ClaimState::SupersededNotInherited;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestState {
    InProgress,
    AwaitingContinuation,
    Blocked,
    Answered,
    Failed,
    Cancelled,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectOutcome {
    Completed,
    Failed,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalEffectFact {
    pub tool_name: String,
    pub outcome: EffectOutcome,
    pub argument_digest: String,
    pub selection_revision: u64,
    pub turn_id: String,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceFact {
    GeneratedArtifact { reference: String },
    WrittenFile { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserDecision {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserAcceptance {
    pub selection_revision: u64,
    pub verification: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MethodRequest {
    pub request_id: String,
    pub goal_preview: String,
    pub goal_digest: String,
    pub automatic_method: MethodId,
    pub automatic_reason: Reason,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automatic_components: Vec<MethodId>,
    pub state: RequestState,
    pub selection: Selection,
    pub superseded_selections: Vec<Selection>,
    pub evidence_facts: Vec<EvidenceFact>,
    pub user_decisions: Vec<UserDecision>,
    pub remaining_questions: Vec<String>,
    pub external_effect_facts: Vec<ExternalEffectFact>,
    pub omitted_observation_count: usize,
    #[serde(default, skip_serializing_if = "CalculationEvidence::is_empty")]
    pub calculation: CalculationEvidence,
    #[serde(default, skip_serializing_if = "ReportEvidence::is_empty")]
    pub report: ReportEvidence,
    #[serde(default, skip_serializing_if = "ImageEvidence::is_empty")]
    pub image: ImageEvidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_acceptance: Option<UserAcceptance>,
    /// When the request began (RFC 3339): a file changed since then is never
    /// shown as a quote (see `task_method_compose_quote`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
}
