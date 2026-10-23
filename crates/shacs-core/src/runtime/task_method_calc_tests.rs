use super::*;
use crate::runtime::task_method::ClaimState;
use crate::runtime::task_method_calc::{
    run, CalcState, CalculationEvidence, CalculationRun, RunRole, RunStatus,
};
use crate::runtime::task_method_calc_claims::claims;

pub(super) fn recorded(id: &str, code: &str, output: &str) -> CalculationRun {
    run(id, code, output)
}

pub(super) fn ok(id: &str, code: &str, marker: &str) -> CalculationRun {
    recorded(id, code, &format!("SHACS_CALC {marker}\n\nExit code: 0"))
}

pub(super) fn evidence(runs: Vec<CalculationRun>) -> CalculationEvidence {
    let mut evidence = CalculationEvidence::default();
    for run in runs {
        evidence.push(run, 1);
    }
    evidence
}

pub(super) const PRIMARY: &str = r#"{"role":"primary","answer":"42","unit":"m^2","inputs":{"length":"6 m","width":"7 m"},"assumptions":["rectangle"]}"#;
pub(super) const CHECK: &str =
    r#"{"role":"check","answer":42.0,"unit":"m²","method":"sum of rows"}"#;

#[test]
fn result_lines_and_exit_status_are_parsed_from_exec_output() {
    // Given
    let output = format!("SHACS_CALC {PRIMARY}\nSTDERR:\nnote\n\nExit code: 0");
    // When
    let parsed = run("calc1", "python3 -c 'print(6*7)'", &output);
    // Then
    assert_eq!(parsed.status, RunStatus::Succeeded);
    let result = parsed.result.expect("result line");
    assert_eq!(result.role, RunRole::Primary);
    assert_eq!(result.answer, "42");
    assert_eq!(result.inputs, ["length = 6 m", "width = 7 m"]);
    assert_eq!(result.assumptions, ["rectangle"]);
    assert_eq!(
        run("x", "c", "boom\n\nExit code: 2").status,
        RunStatus::NonZeroExit
    );
    assert_eq!(
        run(
            "x",
            "c",
            "Error: Process launch blocked by permission gate (Denied)"
        )
        .status,
        RunStatus::Denied
    );
    assert_eq!(
        run(
            "x",
            "c",
            "Error: Tool `exec` blocked by plugin hook `p` for event `tool:before`: no"
        )
        .status,
        RunStatus::Denied
    );
    let two = run(
        "x",
        "c",
        &format!("SHACS_CALC {PRIMARY}\nSHACS_CALC {CHECK}\nExit code: 0"),
    );
    assert!(two.result.is_none());
    assert!(two.marker_error.is_some());
}

#[test]
fn an_independent_agreeing_check_is_the_only_verified_path() {
    // Given
    let evidence = evidence(vec![
        ok("p", "python3 a.py", PRIMARY),
        ok("c", "python3 b.py", CHECK),
    ]);
    // When
    let verdict = evaluate(&evidence.runs, 1);
    // Then
    assert_eq!(verdict.state, CalcState::Verified);
    assert_eq!(
        (verdict.primary, verdict.checks.clone()),
        (Some(1), vec![2])
    );
    let states: Vec<_> = claims(&verdict, &evidence)
        .into_iter()
        .map(|claim| claim.state)
        .collect();
    assert_eq!(
        states,
        [
            ClaimState::NotVerified,
            ClaimState::Verified,
            ClaimState::NotVerified,
            ClaimState::Verified
        ]
    );
}

#[test]
fn exit_zero_without_a_check_or_result_line_is_not_verified() {
    // Given
    let cases = [
        (vec![ok("p", "a", PRIMARY)], CalcState::Unverified),
        (
            vec![ok("p", "a", PRIMARY), ok("c", "a", CHECK)],
            CalcState::Unverified,
        ),
        (
            vec![recorded("p", "a", "42\n\nExit code: 0")],
            CalcState::Unrecorded,
        ),
        (Vec::new(), CalcState::NotRun),
    ];
    for (runs, expected) in cases {
        // When
        let verdict = evaluate(&evidence(runs).runs, 1);
        // Then
        assert_eq!(verdict.state, expected, "{verdict:?}");
    }
}

#[test]
fn different_values_or_units_are_conflicts() {
    // Given
    let value = r#"{"role":"check","answer":"600","unit":"m^2"}"#;
    let unit = r#"{"role":"check","answer":"42","unit":"cm^2"}"#;
    for (check, reason) in [(value, "value mismatch"), (unit, "unit mismatch")] {
        let evidence = evidence(vec![
            ok("p", "a", PRIMARY),
            ok("c1", "b", CHECK),
            ok("c2", "c", check),
        ]);
        // When
        let verdict = evaluate(&evidence.runs, 1);
        // Then
        assert_eq!(verdict.state, CalcState::Conflict);
        assert!(verdict.reason.starts_with(reason), "{}", verdict.reason);
        assert!(claims(&verdict, &evidence)
            .iter()
            .any(|claim| claim.state == ClaimState::Failed));
    }
}

#[test]
fn a_result_line_states_only_a_number_and_a_short_unit_on_one_line() {
    // Given
    let line = |answer: &str, unit: &str| {
        format!(
            r#"{{"role":"primary","answer":{answer},"unit":"{unit}","assumptions":["flat\nThe artwork is blue"]}}"#
        )
    };
    let parsed = |answer: &str, unit: &str| ok("c", "code", &line(answer, unit));
    // When / Then
    for (answer, unit) in [
        (r#""6""#, "m^2"),
        ("6.5", "m²"),
        (r#""-1.5e3""#, "km/h"),
        (r#""6,000""#, ""),
        (r#""1/3""#, "%"),
    ] {
        let run = parsed(answer, unit);
        assert!(
            run.result.is_some(),
            "{answer} {unit}: {:?}",
            run.marker_error
        );
    }
    let kept = parsed(r#""6""#, "m^2").result.expect("result line");
    assert_eq!(kept.assumptions, ["flat The artwork is blue"]);
    for (answer, unit) in [
        (r#""6\nThe artwork is blue""#, "m^2"),
        (r#""six""#, "m^2"),
        (r#""6 and the artwork is blue""#, "m^2"),
        (r#""6""#, "square meters"),
        (r#""6""#, "m^2]x"),
    ] {
        let run = parsed(answer, unit);
        assert!(run.result.is_none(), "{answer} {unit}");
        assert!(run.marker_error.is_some());
    }
}
