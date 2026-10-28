use super::task_method::{
    Availability, Gate, MethodId, Readiness, Reason, Selection, SelectionSource, Support,
};
use crate::tools::ToolRegistry;

#[cfg(test)]
#[path = "task_method_classify_tests.rs"]
mod tests;

pub(super) fn classify(text: &str) -> (MethodId, Reason) {
    match intents(text).as_slice() {
        [] => (MethodId::General, Reason::NoSpecializedIntent),
        [MethodId::ReproducibleCalculation] => {
            (MethodId::ReproducibleCalculation, Reason::CalculationIntent)
        }
        [MethodId::SourceGroundedReport] => {
            (MethodId::SourceGroundedReport, Reason::SourceGroundedIntent)
        }
        [MethodId::ImageGenerationReview] => (
            MethodId::ImageGenerationReview,
            Reason::ImageGenerationIntent,
        ),
        _ => (MethodId::Composed, Reason::ComposedIntent),
    }
}

/// Specialized intents found in a request, in a fixed order.
pub(super) fn intents(text: &str) -> Vec<MethodId> {
    let text = text.to_lowercase();
    let contains = |words: &[&str]| words.iter().any(|word| text.contains(word));
    let calculation = contains(&[
        "calculate ",
        "compute ",
        "계산해",
        "계산해줘",
        "계산해 줘",
        "계산하",
        "계산을",
        "수치 계산",
    ]);
    let report = contains(&["report", "보고서"])
        && contains(&["source", "citation", "cite", "출처", "자료", "근거"]);
    let image = contains(&[
        "generate an image",
        "generate image",
        "create an image",
        "create image",
        "이미지 생성",
        "이미지를 생성",
        "이미지 만들어",
        "이미지를 만들어",
        "그림을 그려",
        "그림 그려",
    ]);
    [
        (calculation, MethodId::ReproducibleCalculation),
        (report, MethodId::SourceGroundedReport),
        (image, MethodId::ImageGenerationReview),
    ]
    .into_iter()
    .filter_map(|(found, method)| found.then_some(method))
    .collect()
}

/// The registered tool a part cannot run without.
pub(super) fn required_tool(method: MethodId) -> Option<&'static str> {
    match method {
        MethodId::ReproducibleCalculation => Some("exec"),
        MethodId::SourceGroundedReport => Some("read_file"),
        MethodId::ImageGenerationReview => Some("image_generate"),
        MethodId::General | MethodId::Composed => None,
    }
}

/// A composed request runs while at least one part is available; parts whose
/// required tool is missing stay unavailable instead of blocking the others.
pub(super) fn composed_availability(parts: &[MethodId], tools: &ToolRegistry) -> Availability {
    let each: Vec<Availability> = parts
        .iter()
        .map(|part| availability(*part, tools))
        .collect();
    let union = |pick: fn(&Availability) -> &Vec<String>| {
        let mut all: Vec<String> = Vec::new();
        for tool in each.iter().flat_map(pick) {
            if !all.contains(tool) {
                all.push(tool.clone());
            }
        }
        all
    };
    let usable: Vec<&Availability> = each.iter().filter(|part| !part.blocked()).collect();
    Availability {
        support: if parts.len() >= 2 {
            Support::Supported
        } else {
            Support::Unsupported
        },
        readiness: if usable.is_empty() {
            Readiness::Unavailable
        } else if usable
            .iter()
            .any(|part| part.readiness == Readiness::RequiresRuntimeCheck)
        {
            Readiness::RequiresRuntimeCheck
        } else {
            Readiness::Available
        },
        missing_required_tools: union(|part| &part.missing_required_tools),
        missing_optional_tools: union(|part| &part.missing_optional_tools),
    }
}

pub(super) fn availability(method: MethodId, tools: &ToolRegistry) -> Availability {
    let (required, optional): (&[&str], &[&str]) = match method {
        MethodId::General | MethodId::Composed => (&[], &[]),
        MethodId::ReproducibleCalculation => (&["exec"], &[]),
        MethodId::SourceGroundedReport => (&["read_file"], &["web_search", "web_fetch"]),
        MethodId::ImageGenerationReview => (&["image_generate"], &[]),
    };
    let missing = |names: &[&str]| {
        names
            .iter()
            .filter(|name| !tools.has(name))
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    let missing_required_tools = missing(required);
    let readiness = if method == MethodId::Composed || !missing_required_tools.is_empty() {
        Readiness::Unavailable
    } else if method == MethodId::ImageGenerationReview {
        Readiness::RequiresRuntimeCheck
    } else {
        Readiness::Available
    };
    Availability {
        support: if method == MethodId::Composed {
            Support::Unsupported
        } else {
            Support::Supported
        },
        readiness,
        missing_required_tools,
        missing_optional_tools: missing(optional),
    }
}

pub(super) fn selection(
    intent: (MethodId, SelectionSource, Reason),
    parts: &[MethodId],
    revision: u64,
    tools: &ToolRegistry,
) -> Selection {
    let (availability, components) = if intent.0 == MethodId::Composed {
        (composed_availability(parts, tools), parts.to_vec())
    } else {
        (availability(intent.0, tools), Vec::new())
    };
    Selection {
        revision,
        method: intent.0,
        source: intent.1,
        reason: intent.2,
        availability,
        gate: Gate::NotEvaluated,
        superseded: false,
        validation_claims: Vec::new(),
        components,
    }
}
