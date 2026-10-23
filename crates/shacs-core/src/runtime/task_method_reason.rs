use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ExplicitOverride,
    OneShotOverride,
    CalculationIntent,
    SourceGroundedIntent,
    ImageGenerationIntent,
    ComposedIntent,
    NoSpecializedIntent,
    ReportRevision,
    ImageRevision,
}

impl Reason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ExplicitOverride => "explicit_override",
            Self::OneShotOverride => "one_shot_override",
            Self::CalculationIntent => "calculation_intent",
            Self::SourceGroundedIntent => "source_grounded_intent",
            Self::ImageGenerationIntent => "image_generation_intent",
            Self::ComposedIntent => "composed_intent",
            Self::NoSpecializedIntent => "no_specialized_intent",
            Self::ReportRevision => "report_revision",
            Self::ImageRevision => "image_revision",
        }
    }
    pub fn explanation(self) -> &'static str {
        match self {
            Self::ExplicitOverride => "Selected by the user for this request.",
            Self::OneShotOverride => "Selected by the user for the next request only.",
            Self::CalculationIntent => "The request explicitly asks for a calculation.",
            Self::SourceGroundedIntent => "The request asks for a report grounded in sources.",
            Self::ImageGenerationIntent => "The request asks to generate an image.",
            Self::ComposedIntent => {
                "The request combines several specialized methods; each part is checked on its own."
            }
            Self::NoSpecializedIntent => "No unambiguous specialized intent detected.",
            Self::ReportRevision => {
                "Revising an earlier report; its checks are re-run, not inherited."
            }
            Self::ImageRevision => "Revising an earlier image; checks are re-run, not inherited.",
        }
    }
}
