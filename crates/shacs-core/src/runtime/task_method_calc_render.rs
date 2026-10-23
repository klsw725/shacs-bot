use super::task_method::{MethodId, MethodRequest, UserAcceptance};
use super::task_method_calc::{CalcResult, CalcState, CalculationRun, RunRole};
use super::task_method_calc_eval::status_text;
use super::task_method_compose_quote::withholds;

pub(super) const PROTOCOL: &str = "Calculation protocol: run the computation with the exec tool (for example `python3 -c ...`). Each run must print exactly one line `SHACS_CALC <json>`. The primary run prints {\"role\":\"primary\",\"answer\":\"<value>\",\"unit\":\"<unit>\",\"inputs\":{\"<name>\":\"<value and unit>\"},\"assumptions\":[\"<assumption>\"]}. Then run a separate check with a different formula or method that prints {\"role\":\"check\",\"answer\":\"<value>\",\"unit\":\"<unit>\",\"method\":\"<how it differs>\"}. The runtime compares the recorded runs and decides verification; never call the result verified yourself. If execution is denied or Python or a package is unavailable, say so plainly instead of presenting a number as a checked result. After fixing an error, rerun both the primary run and the check.";

pub(super) fn summary(request: &MethodRequest) -> String {
    let evidence = &request.calculation;
    let Some(verdict) = evidence.verdict.as_ref() else {
        return String::new();
    };
    if !request
        .selection
        .parts()
        .contains(&MethodId::ReproducibleCalculation)
        || verdict.selection_revision != request.selection.revision
    {
        return format!(
            "\nCalculation evidence from revision {}: superseded_not_inherited (was {}).",
            verdict.selection_revision,
            verdict.state.label()
        );
    }
    let reason = if withholds(request) {
        without_package(&verdict.reason)
    } else {
        verdict.reason.clone()
    };
    let mut text = format!("\nCalculation: {} — {reason}", verdict.state.label());
    if let Some(primary) = verdict.primary.and_then(|seq| evidence.find(seq)) {
        if let Some(result) = &primary.result {
            let label = if verdict.state == CalcState::Verified {
                "Verified answer"
            } else {
                "Recorded primary result (not verified)"
            };
            text.push_str(&format!("\n{label}: {}", value(result)));
            let stated = !result.inputs.is_empty() || !result.assumptions.is_empty();
            if withholds(request) && stated {
                // The model wrote these; with an image part, only checked values are shown.
                text.push_str("\nInputs and assumptions: recorded, shown in /method status");
            } else {
                if !result.inputs.is_empty() {
                    text.push_str(&format!("\nInputs: {}", result.inputs.join("; ")));
                }
                if !result.assumptions.is_empty() {
                    text.push_str(&format!("\nAssumptions: {}", result.assumptions.join("; ")));
                }
            }
        }
        // With an image part a run shows as its number: the model's call id
        // stays in /method status.
        let name = |run: &CalculationRun| {
            if withholds(request) {
                format!("#{}", run.seq)
            } else {
                label(run)
            }
        };
        let mut runs = vec![format!(
            "primary {} {}",
            name(primary),
            status_text(primary)
        )];
        runs.extend(
            verdict
                .checks
                .iter()
                .filter_map(|seq| evidence.find(*seq))
                .map(|run| format!("check {} {}", name(run), status_text(run))),
        );
        text.push_str(&format!("\nExecution: {}", runs.join("; ")));
    }
    if request.selection.method != MethodId::Composed {
        text.push_str(&format!(
            "\nUser acceptance: {}",
            acceptance(request.user_acceptance.as_ref())
        ));
    }
    text
}

/// The runtime's line in place of a `[calculation]` line of a composed report:
/// the checked result, or the calculation's state when there is none. The
/// model's words on that line are never shown.
pub(super) fn calc_line(request: &MethodRequest) -> String {
    if !request
        .selection
        .parts()
        .contains(&MethodId::ReproducibleCalculation)
    {
        return "[Calculation: none — this request has no calculation part]".to_owned();
    }
    let evidence = &request.calculation;
    let verdict = evidence
        .verdict
        .as_ref()
        .filter(|verdict| verdict.selection_revision == request.selection.revision);
    let checked = verdict
        .filter(|verdict| verdict.state == CalcState::Verified)
        .and_then(|verdict| verdict.primary.and_then(|seq| evidence.find(seq)))
        .and_then(|primary| primary.result.as_ref());
    match (checked, verdict) {
        (Some(result), _) => format!(
            "[Calculation: {} — verified by a separate check run]",
            value(result)
        ),
        (None, Some(verdict)) if verdict.state == CalcState::Conflict => {
            "[Calculation: none — the calculation part is in conflict]".to_owned()
        }
        (None, Some(verdict)) => format!(
            "[Calculation: none — the calculation part is {}]",
            verdict.state.label()
        ),
        (None, None) => "[Calculation: none — the calculation part is not run]".to_owned(),
    }
}

pub(super) fn details(request: &MethodRequest) -> String {
    let evidence = &request.calculation;
    if evidence.runs.is_empty() && evidence.omitted_runs == 0 {
        return String::new();
    }
    let revision = request.selection.revision;
    let counts =
        |run: &CalculationRun| (evidence.since..=revision).contains(&run.selection_revision);
    let latest_primary = evidence.runs.iter().rposition(|run| {
        counts(run) && run.result.as_ref().map(|result| result.role) == Some(RunRole::Primary)
    });
    let mut text = "\nCalculation runs:".to_owned();
    for (index, run) in evidence.runs.iter().enumerate() {
        let note = if !counts(run) {
            format!(
                " [revision {}; superseded_not_inherited]",
                run.selection_revision
            )
        } else if run.selection_revision != revision {
            format!(
                " [revision {}; kept across the method change and judged again]",
                run.selection_revision
            )
        } else if latest_primary.is_some_and(|latest| index < latest) {
            " [superseded by a later primary run]".to_owned()
        } else {
            String::new()
        };
        text.push_str(&run_text(run, &note));
    }
    if evidence.omitted_runs > 0 {
        text.push_str(&format!(
            "\nRuns omitted by retention limit: {}",
            evidence.omitted_runs
        ));
    }
    text
}

fn run_text(run: &CalculationRun, note: &str) -> String {
    let role = match run.result.as_ref().map(|result| result.role) {
        Some(RunRole::Primary) => "primary",
        Some(RunRole::Check) => "check",
        None => "no result line",
    };
    let mut text = format!(
        "\n- {} [{role}] {}{note}\n  code: {}\n  output: {}",
        label(run),
        status_text(run),
        run.code,
        run.output.replace('\n', "\n          ")
    );
    if let Some(result) = &run.result {
        text.push_str(&format!("\n  result: {}", value(result)));
        if !result.method.is_empty() {
            text.push_str(&format!(" (method: {})", result.method));
        }
    }
    if let Some(error) = &run.marker_error {
        text.push_str(&format!("\n  result line problem: {error}"));
    }
    text
}

/// `reason` without the package name a run printed (the model's code wrote
/// that output), as an image part's summary shows only checked words.
fn without_package(reason: &str) -> String {
    let Some((before, rest)) = reason.split_once("the Python package `") else {
        return reason.to_owned();
    };
    let Some((_, after)) = rest.split_once("` is not installed") else {
        return reason.to_owned();
    };
    format!("{before}a required Python package is not installed (see /method status){after}")
}

pub(super) fn acceptance(accepted: Option<&UserAcceptance>) -> String {
    match accepted {
        Some(accepted) => format!(
            "accepted at {} for revision {} while verification was {} (acceptance does not change verification)",
            accepted.recorded_at, accepted.selection_revision, accepted.verification
        ),
        None => "not recorded (use /method accept; acceptance does not change verification)"
            .to_owned(),
    }
}

fn label(run: &CalculationRun) -> String {
    format!("#{} {}", run.seq, run.call_id)
}

fn value(result: &CalcResult) -> String {
    if result.unit.is_empty() {
        result.answer.clone()
    } else {
        format!("{} {}", result.answer, result.unit)
    }
}
