use super::task_method_calc::{
    CalcResult, CalcState, CalcVerdict, CalculationRun, RunRole, RunStatus,
};

#[cfg(test)]
#[path = "task_method_calc_rerun_tests.rs"]
mod rerun_tests;
#[cfg(test)]
#[path = "task_method_calc_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn evaluate(runs: &[CalculationRun], revision: u64) -> CalcVerdict {
    evaluate_since(runs, revision, revision)
}

/// Judges the runs of method revisions `since..=revision` for `revision`.
pub(crate) fn evaluate_since(runs: &[CalculationRun], since: u64, revision: u64) -> CalcVerdict {
    let runs: Vec<&CalculationRun> = runs
        .iter()
        .filter(|run| (since..=revision).contains(&run.selection_revision))
        .collect();
    let verdict =
        |state, reason: String, primary: Option<&CalculationRun>, checks: Vec<u64>| CalcVerdict {
            selection_revision: revision,
            state,
            reason,
            primary: primary.map(|run| run.seq),
            checks,
        };
    if runs.is_empty() {
        return verdict(
            CalcState::NotRun,
            "no calculation run has been recorded, so no answer is backed by executed code yet"
                .to_owned(),
            None,
            Vec::new(),
        );
    }
    let Some(index) = runs
        .iter()
        .rposition(|run| role(run) == Some(RunRole::Primary))
    else {
        let state = if let Some(cause) = unavailable(&runs) {
            let reason = format!("{cause}; no calculation result was produced");
            return verdict(CalcState::Unavailable, reason, None, Vec::new());
        } else if runs.iter().any(|run| run.status != RunStatus::Succeeded) {
            CalcState::Failed
        } else {
            CalcState::Unrecorded
        };
        let reason = if state == CalcState::Failed {
            "the calculation run failed before reporting a result".to_owned()
        } else {
            "no run printed a SHACS_CALC primary result line, so the answer is not linked to executed code".to_owned()
        };
        return verdict(state, reason, None, Vec::new());
    };
    let primary = runs[index];
    if primary.status != RunStatus::Succeeded {
        return verdict(
            CalcState::Failed,
            format!("the primary run did not succeed ({})", status_text(primary)),
            Some(primary),
            Vec::new(),
        );
    }
    let later = &runs[index + 1..];
    let checks: Vec<&CalculationRun> = later
        .iter()
        .copied()
        .filter(|run| role(run) == Some(RunRole::Check) && run.status == RunStatus::Succeeded)
        .collect();
    let ids = checks.iter().map(|run| run.seq).collect::<Vec<_>>();
    let Some(primary_result) = primary.result.as_ref() else {
        return verdict(
            CalcState::Unrecorded,
            "primary result is missing".to_owned(),
            Some(primary),
            ids,
        );
    };
    for check in &checks {
        let Some(check_result) = check.result.as_ref() else {
            continue;
        };
        if unit(&check_result.unit) != unit(&primary_result.unit) {
            return verdict(
                CalcState::Conflict,
                format!(
                    "unit mismatch: primary {} vs check {}",
                    value_text(primary_result),
                    value_text(check_result)
                ),
                Some(primary),
                ids,
            );
        }
        if !same_value(&primary_result.answer, &check_result.answer) {
            return verdict(
                CalcState::Conflict,
                format!(
                    "value mismatch: primary {} vs check {}",
                    value_text(primary_result),
                    value_text(check_result)
                ),
                Some(primary),
                ids,
            );
        }
    }
    let failed_later: Vec<&CalculationRun> = later
        .iter()
        .copied()
        .filter(|run| run.status != RunStatus::Succeeded)
        .collect();
    if let Some(cause) = unavailable(&failed_later) {
        return verdict(
            CalcState::Unavailable,
            format!("a later run could not complete because {cause}; the earlier result is not verified"),
            Some(primary),
            ids,
        );
    }
    if !failed_later.is_empty() {
        let cancelled = failed_later
            .iter()
            .all(|run| run.status == RunStatus::Cancelled);
        let outcome = if cancelled { "was cancelled" } else { "failed" };
        return verdict(
            CalcState::Unverified,
            format!(
                "a run after the latest primary result {outcome}; rerun the primary and the check"
            ),
            Some(primary),
            ids,
        );
    }
    if later.iter().any(|run| run.result.is_none()) {
        return verdict(
            CalcState::Unverified,
            "a run after the latest primary result printed no valid result line; rerun the primary and the check".to_owned(),
            Some(primary),
            ids,
        );
    }
    let independent = checks
        .iter()
        .any(|check| check.code_digest != primary.code_digest);
    if independent {
        return verdict(
            CalcState::Verified,
            "a separate check run with different code reproduced the same value and unit"
                .to_owned(),
            Some(primary),
            ids,
        );
    }
    let reason = if checks.is_empty() {
        "no independent check was run after the latest primary result"
    } else {
        "the check run reused the primary code, so it is not independent"
    };
    verdict(CalcState::Unverified, reason.to_owned(), Some(primary), ids)
}

fn role(run: &CalculationRun) -> Option<RunRole> {
    run.result.as_ref().map(|result| result.role)
}

pub(crate) fn status_text(run: &CalculationRun) -> String {
    match (run.status, run.exit_code) {
        (RunStatus::Succeeded, _) => "exit 0".to_owned(),
        (RunStatus::NonZeroExit, Some(code)) => format!("exit {code}"),
        (RunStatus::Denied, _) => "execution denied".to_owned(),
        (RunStatus::Cancelled, _) => "cancelled".to_owned(),
        (RunStatus::NonZeroExit | RunStatus::Failed, _) => "execution error".to_owned(),
    }
}

fn unavailable(runs: &[&CalculationRun]) -> Option<String> {
    if runs.iter().any(|run| run.status == RunStatus::Denied) {
        return Some("execution was denied by the permission or process gate".to_owned());
    }
    if let Some(module) = runs.iter().find_map(|run| missing_module(&run.output)) {
        return Some(format!("the Python package `{module}` is not installed"));
    }
    runs.iter()
        .any(|run| {
            run.exit_code == Some(127)
                || run.output.contains("command not found") && run.code.contains("python")
        })
        .then(|| "Python is not available in this environment".to_owned())
}

fn missing_module(output: &str) -> Option<String> {
    let rest = output
        .split("ModuleNotFoundError: No module named ")
        .nth(1)?;
    let name: String = rest
        .trim_start_matches(['\'', '"'])
        .chars()
        .take_while(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.' | '-'))
        .take(64)
        .collect();
    (!name.is_empty()).then_some(name)
}

fn value_text(result: &CalcResult) -> String {
    if result.unit.is_empty() {
        result.answer.clone()
    } else {
        format!("{} {}", result.answer, result.unit)
    }
}

fn unit(text: &str) -> String {
    text.replace('²', "^2")
        .replace('³', "^3")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn same_value(left: &str, right: &str) -> bool {
    let number = |text: &str| {
        text.chars()
            .filter(|ch| !matches!(ch, ',' | '_' | ' '))
            .collect::<String>()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    };
    match (number(left), number(right)) {
        (Some(left), Some(right)) => {
            (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
        }
        _ => left.trim().eq_ignore_ascii_case(right.trim()),
    }
}
