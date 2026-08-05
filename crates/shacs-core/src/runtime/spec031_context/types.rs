use super::super::context_files::ContextFileProjection;
use super::super::context_handoff::{ContextBudgetDecision, ContextProviderHandoff};
use super::super::context_refs::ResolvedContextArtifact;
use serde::Serialize;
use shacs_projection::{
    Spec031ConstructionError, Spec031Envelope, Spec031Freshness, Spec031InclusionReason,
    Spec031SafeSummary, Spec031SubjectRef,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ContextEvidenceProjection {
    pub rows: Vec<Spec031ContextEvidenceRow>,
    pub envelopes: Vec<Spec031Envelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031ContextEvidenceRow {
    pub opaque_ref: Spec031ContextOwnerRef,
    pub kind: Spec031ContextEvidenceRowKind,
    pub order: usize,
    pub reason: Spec031InclusionReason,
    pub evidence_reason: Spec031ContextEvidenceReason,
    pub included: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_decision: Option<ContextBudgetDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_estimated_tokens: Option<usize>,
    pub result_summary: Spec031SafeSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ContextEvidenceRowKind {
    ContextFile,
    InlineReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ContextEvidenceReason {
    Included,
    Skipped,
    Blocked,
    Missing,
    Unsupported,
    ExtractionFailed,
    PromptAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Spec031ContextOwnerRef(Spec031SubjectRef);

impl Spec031ContextOwnerRef {
    pub fn try_new(value: &str) -> Result<Self, Spec031ConstructionError> {
        Spec031SubjectRef::try_new(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub struct Spec031ContextEvidenceInput<'a> {
    pub batch_ref: Option<Spec031ContextOwnerRef>,
    pub owner_freshness: Spec031Freshness,
    pub inline_artifacts: &'a [ResolvedContextArtifact],
    pub context_files: &'a [ContextFileProjection],
    pub provider_handoff: Option<&'a ContextProviderHandoff>,
}
