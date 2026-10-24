use super::task_method::{MethodId, MethodRequest, ValidationClaim};
use super::task_method_calc::CalcState;
use super::task_method_calc_claims::claims as calc_claims;
use super::task_method_calc_eval::evaluate_since as evaluate_calc;
use super::task_method_classify::required_tool;
use super::task_method_compose_quote::mark_quotes;
use super::task_method_image::ImageState;
use super::task_method_image_claims::image_claims;
use super::task_method_image_eval::evaluate as evaluate_image;
use super::task_method_observe::MethodObservation;
use super::task_method_report::{freshness, CitationStatus};
use super::task_method_report_claims::report_claims;
use super::task_method_report_eval::evaluate_with;
use super::task_method_report_parse::RESULT_CALCULATION;

pub(crate) fn apply_observation(request: &mut MethodRequest, observed: &mut MethodObservation) {
    let revision = request.selection.revision;
    let parts = request.selection.parts();
    if parts.contains(&MethodId::ReproducibleCalculation) {
        for run in observed.calculation_runs.drain(..) {
            request.calculation.push(run, revision);
        }
        request.calculation.bound();
    }
    if parts.contains(&MethodId::SourceGroundedReport) {
        for source in observed.report_sources.drain(..) {
            request.report.push_source(source, revision);
        }
        for output in observed.report_outputs.drain(..) {
            request.report.push_output(output, revision);
        }
    }
    if parts.contains(&MethodId::ImageGenerationReview) {
        for attempt in observed.image_attempts.drain(..) {
            request.image.push_attempt(attempt, revision);
        }
    }
    observed.calculation_runs.clear();
    observed.image_attempts.clear();
    observed.report_sources.clear();
    observed.report_outputs.clear();
    refresh(request);
}

/// Re-judges every part of the current revision. Each part keeps its own
/// verdict; a composed request lists the parts' checks under their names.
pub(crate) fn refresh(request: &mut MethodRequest) {
    let parts = request.selection.parts();
    if parts.is_empty() {
        return;
    }
    let revision = request.selection.revision;
    let missing = request
        .selection
        .availability
        .missing_required_tools
        .clone();
    let unavailable = |part: MethodId| {
        required_tool(part).is_some_and(|tool| missing.iter().any(|name| name == tool))
    };
    let mut checks: Vec<(MethodId, Vec<ValidationClaim>)> = Vec::new();
    if parts.contains(&MethodId::ReproducibleCalculation) {
        let mut verdict = evaluate_calc(
            &request.calculation.runs,
            request.calculation.since,
            revision,
        );
        if unavailable(MethodId::ReproducibleCalculation) {
            verdict.state = CalcState::Unavailable;
            verdict.reason = "exec is not registered, so this part is unavailable".to_owned();
        }
        let claims = calc_claims(&verdict, &request.calculation);
        request.calculation.verdict = Some(verdict);
        checks.push((MethodId::ReproducibleCalculation, claims));
    }
    if parts.contains(&MethodId::ImageGenerationReview) {
        let mut verdict = {
            let mut texts: Vec<&str> = request.image.purpose.iter().map(String::as_str).collect();
            texts.push(&request.goal_preview);
            evaluate_image(&request.image, revision, &texts)
        };
        if unavailable(MethodId::ImageGenerationReview) {
            verdict.state = ImageState::Unavailable;
            verdict.reason =
                "image_generate is not registered, so this part is unavailable".to_owned();
        }
        request.image.verdict = Some(verdict);
        checks.push((
            MethodId::ImageGenerationReview,
            image_claims(&request.image),
        ));
    }
    if parts.contains(&MethodId::SourceGroundedReport) {
        // `[calculation]` lines of a composed answer are the runtime's own and never
        // reach the report; one left in a report file is not checked.
        let calculation = parts.contains(&MethodId::ReproducibleCalculation);
        let results = move |key: &str, _: &str| {
            (key == RESULT_CALCULATION).then_some(if calculation {
                CitationStatus::ResultUnchecked
            } else {
                CitationStatus::ResultMissing
            })
        };
        let mut verdict = evaluate_with(&request.report, revision, &freshness, &results);
        if unavailable(MethodId::SourceGroundedReport) {
            verdict.state = super::task_method_report::ReportState::Unavailable;
            verdict.reason = "read_file is not registered, so this part is unavailable".to_owned();
        }
        let claims = report_claims(&verdict);
        request.report.verdict = Some(verdict);
        mark_quotes(request);
        checks.push((MethodId::SourceGroundedReport, claims));
    }
    let composed = request.selection.method == MethodId::Composed;
    let mut claims = Vec::new();
    for part in parts {
        for (method, mut part_claims) in checks.iter().filter(|(m, _)| *m == part).cloned() {
            for claim in &mut part_claims {
                if let Some(tool) = required_tool(method).filter(|_| unavailable(method)) {
                    // An unavailable part was never attempted: nothing to verify or fail.
                    claim.state = super::task_method::ClaimState::NotVerified;
                    claim.evidence_refs = vec![format!("unavailable: {tool} is not registered")];
                }
                if composed {
                    claim.check = format!("{}: {}", part_label(method), claim.check);
                }
            }
            claims.extend(part_claims);
        }
    }
    request.selection.validation_claims = claims;
}

pub(crate) fn part_label(method: MethodId) -> &'static str {
    match method {
        MethodId::ReproducibleCalculation => "calculation",
        MethodId::SourceGroundedReport => "report",
        MethodId::ImageGenerationReview => "image",
        MethodId::General => "general",
        MethodId::Composed => "composed",
    }
}
