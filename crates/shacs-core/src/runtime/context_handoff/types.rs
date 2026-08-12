use super::estimator::{select_token_estimator, TokenEstimatorSelection};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_HANDOFF_BUDGET_TOKENS: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBudgetInput {
    pub active_user_message: String,
    pub required_runtime_instructions: String,
    pub max_context_tokens: Option<usize>,
    pub estimator: TokenEstimatorSelection,
}

impl Default for ContextBudgetInput {
    fn default() -> Self {
        Self {
            active_user_message: String::new(),
            required_runtime_instructions: String::new(),
            max_context_tokens: Some(DEFAULT_CONTEXT_HANDOFF_BUDGET_TOKENS),
            estimator: select_token_estimator("unknown", "unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextProviderHandoff {
    pub blocks: Vec<ProviderContextBlock>,
    pub evidence: Vec<ContextBudgetEvidence>,
    pub used_context_tokens: usize,
    pub budget_tokens: usize,
    pub estimator: TokenEstimatorSelection,
    pub required: Vec<RequiredBudgetEvidence>,
    pub required_overflow_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderContextBlock {
    pub source_label: String,
    pub trust_label: String,
    pub truncation_label: Option<String>,
    pub content: String,
    pub digest: Option<String>,
    pub byte_count: usize,
    pub token_estimate: Option<usize>,
    pub included_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBudgetEvidence {
    pub source_label: String,
    pub priority: ContextArtifactPriority,
    pub decision: ContextBudgetDecision,
    pub reason: Option<String>,
    pub digest: Option<String>,
    pub estimated_tokens: Option<usize>,
    pub included_tokens: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequiredContextKind {
    ActiveUserMessage,
    RuntimeInstructions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredBudgetEvidence {
    pub kind: RequiredContextKind,
    pub estimated_tokens: usize,
    pub overflow_tokens: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextArtifactPriority {
    ExplicitInline,
    ConfiguredExtra,
    NearestContextFile,
    AncestorContextFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBudgetDecision {
    Included,
    Truncated,
    SkippedBudget,
    SkippedSafety,
    SkippedDuplicate,
}
