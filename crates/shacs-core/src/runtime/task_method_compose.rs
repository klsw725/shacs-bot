use super::task_method::{MethodId, MethodRequest, Selection};
use super::task_method_apply::part_label;
use super::task_method_calc::CalcState;
use super::task_method_calc_render::PROTOCOL;
use super::task_method_classify::required_tool;
use super::task_method_image::ImageState;
use super::task_method_image_render::{revision_context, IMAGE_PROTOCOL};
use super::task_method_report::ReportState;
use super::task_method_report_render::{
    revision_context as report_revision_context, REPORT_PROTOCOL,
};

#[cfg(test)]
#[path = "task_method_compose_tests.rs"]
mod tests;

const COMPOSED_PROTOCOL: &str = "Composed request: each part below is checked on its own, and one part's result never verifies another. To place the calculation result in the report, put [calculation] on a line of its own: the runtime replaces that line with the checked result and its state, so never write the value yourself. To place the generated image, put [image] on a line of its own: the runtime replaces that line with the saved image's reference and state, so never describe what the image shows. If a part is unavailable, say so instead of producing it another way.";

/// "composed [calculation + report]" for a composed selection, else the id.
pub(super) fn method_name(selection: &Selection) -> String {
    if selection.method != MethodId::Composed || selection.components.is_empty() {
        return selection.method.canonical().to_owned();
    }
    let parts: Vec<&str> = selection
        .components
        .iter()
        .map(|part| part_label(*part))
        .collect();
    format!("composed [{}]", parts.join(" + "))
}

/// Parts whose required tool is not registered.
pub(super) fn unavailable_parts(selection: &Selection) -> Vec<MethodId> {
    let missing = &selection.availability.missing_required_tools;
    selection
        .parts()
        .into_iter()
        .filter(|part| required_tool(*part).is_some_and(|tool| missing.iter().any(|m| m == tool)))
        .collect()
}

pub(super) fn availability_note(selection: &Selection) -> String {
    let unavailable = unavailable_parts(selection);
    if selection.method != MethodId::Composed || unavailable.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = unavailable.iter().map(|part| part_label(*part)).collect();
    format!("; unavailable parts: {}", parts.join(", "))
}

pub(super) fn planned_checks(selection: &Selection) -> Vec<String> {
    let composed = selection.method == MethodId::Composed;
    selection
        .parts()
        .into_iter()
        .flat_map(|part| {
            part.checks().iter().map(move |check| {
                if composed {
                    format!("{}: {check}: not verified", part_label(part))
                } else {
                    format!("{check}: not verified")
                }
            })
        })
        .collect()
}

/// Each part's state label and whether it is confirmed: a verified
/// calculation, a source-linked report or a generated image.
pub(super) fn part_states(request: &MethodRequest) -> Vec<(MethodId, &'static str, bool)> {
    let revision = request.selection.revision;
    request
        .selection
        .parts()
        .into_iter()
        .map(|part| {
            let state = match part {
                MethodId::ReproducibleCalculation => request
                    .calculation
                    .verdict
                    .as_ref()
                    .filter(|verdict| verdict.selection_revision == revision)
                    .map(|verdict| (verdict.state.label(), verdict.state == CalcState::Verified)),
                MethodId::SourceGroundedReport => request
                    .report
                    .verdict
                    .as_ref()
                    .filter(|verdict| verdict.selection_revision == revision)
                    .map(|verdict| (verdict.state.label(), verdict.state == ReportState::Linked)),
                MethodId::ImageGenerationReview => request
                    .image
                    .verdict
                    .as_ref()
                    .filter(|verdict| verdict.selection_revision == revision)
                    .map(|verdict| {
                        (
                            verdict.state.label(),
                            verdict.state == ImageState::Generated,
                        )
                    }),
                MethodId::General | MethodId::Composed => None,
            };
            let (label, confirmed) = state.unwrap_or(("not evaluated", false));
            (part, label, confirmed)
        })
        .collect()
}

/// One line with each part's state, shown first for a composed request.
pub(super) fn parts_line(request: &MethodRequest) -> String {
    if request.selection.method != MethodId::Composed {
        return String::new();
    }
    let states: Vec<String> = part_states(request)
        .into_iter()
        .map(|(part, label, _)| format!("{} — {label}", part_label(part)))
        .collect();
    format!("\nParts (each checked on its own): {}", states.join("; "))
}

/// The protocols of every part, plus how parts may cite each other.
pub(super) fn protocol(request: &MethodRequest) -> String {
    let parts = request.selection.parts();
    let mut text = String::new();
    if request.selection.method == MethodId::Composed {
        text.push_str(&format!("\n{COMPOSED_PROTOCOL}"));
        if parts.contains(&MethodId::ImageGenerationReview) {
            text.push_str(" This request includes an image, so the user sees only report lines that copy whole sentences or lines of a file you read word for word, with its [source: …] marker (a file changed during this request does not count); write the report as such quotes.");
        }
    }
    for part in parts {
        match part {
            MethodId::ReproducibleCalculation => text.push_str(&format!("\n{PROTOCOL}")),
            MethodId::SourceGroundedReport => text.push_str(&format!(
                "\n{REPORT_PROTOCOL}{}",
                report_revision_context(request)
            )),
            MethodId::ImageGenerationReview => {
                text.push_str(&format!("\n{IMAGE_PROTOCOL}{}", revision_context(request)))
            }
            MethodId::General | MethodId::Composed => {}
        }
    }
    text
}

/// A line holding the `[image]` placeholder of a composed report.
pub(crate) fn is_image_line(line: &str) -> bool {
    has_marker(line, "image")
}

/// A line holding the `[calculation]` placeholder of a composed report.
pub(crate) fn is_calc_line(line: &str) -> bool {
    has_marker(line, "calculation")
}

fn has_marker(line: &str, name: &str) -> bool {
    line.split('[').skip(1).any(|part| {
        part.split_once(']').is_some_and(|(inner, _)| {
            inner
                .split(':')
                .next()
                .is_some_and(|key| key.trim().eq_ignore_ascii_case(name))
        })
    })
}
