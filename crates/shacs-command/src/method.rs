use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodName {
    General,
    ReproducibleCalculation,
    SourceGroundedReport,
    ImageGenerationReview,
}

impl MethodName {
    pub fn canonical(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::ReproducibleCalculation => "reproducible_calculation",
            Self::SourceGroundedReport => "source_grounded_report",
            Self::ImageGenerationReview => "image_generation_review",
        }
    }

    fn parse(input: &str) -> Option<Self> {
        match input {
            "general" => Some(Self::General),
            "calculation" | "reproducible_calculation" => Some(Self::ReproducibleCalculation),
            "report" | "source_grounded_report" => Some(Self::SourceGroundedReport),
            "image" | "image_generation_review" => Some(Self::ImageGenerationReview),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MethodCommandArgs {
    Status,
    List,
    Use(MethodName),
    Auto,
    History,
    Accept,
    Revise,
    Invalid,
}

pub(super) fn parse_method_args(args: &str) -> MethodCommandArgs {
    let normalized = args.to_ascii_lowercase();
    let words: Vec<_> = normalized.split_whitespace().collect();
    match words.as_slice() {
        [] | ["status"] => MethodCommandArgs::Status,
        ["list"] => MethodCommandArgs::List,
        ["auto"] => MethodCommandArgs::Auto,
        ["history"] => MethodCommandArgs::History,
        ["accept"] => MethodCommandArgs::Accept,
        ["revise"] => MethodCommandArgs::Revise,
        ["use", name] => MethodName::parse(name)
            .map(MethodCommandArgs::Use)
            .unwrap_or(MethodCommandArgs::Invalid),
        _ => MethodCommandArgs::Invalid,
    }
}
