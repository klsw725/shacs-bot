use super::evaluate;
use super::tests::{evidence, ok, recorded, CHECK, PRIMARY};
use crate::runtime::task_method_calc::{run, CalcState, CalculationEvidence};

#[test]
fn a_rerun_supersedes_earlier_checks_and_other_revisions_do_not_count() {
    // Given
    let mut runs = evidence(vec![
        ok("p1", "a", PRIMARY),
        ok("c1", "b", CHECK),
        ok("p2", "fixed", PRIMARY),
    ])
    .runs;
    // When
    let rerun = evaluate(&runs, 1);
    runs.iter_mut().for_each(|run| run.selection_revision = 2);
    let other_revision = evaluate(&runs, 3);
    // Then
    assert_eq!(rerun.state, CalcState::Unverified);
    assert_eq!(rerun.primary, Some(3));
    assert_eq!(other_revision.state, CalcState::NotRun);
}

#[test]
fn a_failure_after_a_verified_pair_withdraws_verification_until_both_rerun() {
    // Given
    let denied = "Error: Process launch blocked by permission gate (Denied)";
    let cancelled = "Error: tool call cancelled before it ran (the method changed).";
    for (failure, expected, reason) in [
        (denied, CalcState::Unavailable, "execution was denied"),
        (
            "ModuleNotFoundError: No module named 'numpy'\n\nExit code: 1",
            CalcState::Unavailable,
            "`numpy` is not installed",
        ),
        (
            "Traceback\n\nExit code: 1",
            CalcState::Unverified,
            "a run after the latest primary result failed",
        ),
        (
            cancelled,
            CalcState::Unverified,
            "a run after the latest primary result was cancelled",
        ),
    ] {
        let mut runs = vec![
            ok("p", "a", PRIMARY),
            ok("c", "b", CHECK),
            recorded("again", "c", failure),
        ];
        // When
        let withdrawn = evaluate(&evidence(runs.clone()).runs, 1);
        let check_only = evaluate(
            &evidence([runs.clone(), vec![ok("c2", "b2", CHECK)]].concat()).runs,
            1,
        );
        runs.extend([ok("p2", "a2", PRIMARY), ok("c2", "b2", CHECK)]);
        let restored = evaluate(&evidence(runs).runs, 1);
        // Then
        assert_eq!(withdrawn.state, expected, "{}", withdrawn.reason);
        assert!(withdrawn.reason.contains(reason), "{}", withdrawn.reason);
        assert_eq!(check_only.state, expected, "{}", check_only.reason);
        assert_eq!(restored.state, CalcState::Verified);
    }
}

#[test]
fn duplicate_call_ids_resolve_by_sequence_and_revision() {
    // Given
    let wrong = r#"{"role":"primary","answer":"600","unit":"m^2"}"#;
    let mut evidence = CalculationEvidence::default();
    evidence.push(ok("call", "naive", wrong), 1);
    evidence.push(ok("call", "fixed", PRIMARY), 2);
    evidence.push(ok("call", "check", CHECK), 2);
    // When
    let verdict = evaluate(&evidence.runs, 2);
    // Then
    assert_eq!(verdict.state, CalcState::Verified);
    let primary = verdict
        .primary
        .and_then(|seq| evidence.find(seq))
        .expect("primary");
    assert_eq!(
        primary.result.as_ref().map(|result| result.answer.as_str()),
        Some("42")
    );
}

#[test]
fn failed_denied_and_missing_python_runs_never_produce_a_result() {
    // Given
    let cases = [
        (
            "Error: Process launch blocked by permission gate (Denied)",
            CalcState::Unavailable,
            "denied",
        ),
        (
            "sh: 1: python3: not found\n\nExit code: 127",
            CalcState::Unavailable,
            "Python",
        ),
        (
            "ModuleNotFoundError: No module named 'numpy'\n\nExit code: 1",
            CalcState::Unavailable,
            "numpy",
        ),
        (
            "Traceback\nZeroDivisionError\n\nExit code: 1",
            CalcState::Failed,
            "failed",
        ),
    ];
    for (output, expected, needle) in cases {
        let runs = evidence(vec![recorded("p", "python3 -c 'x'", output)]).runs;
        // When
        let verdict = evaluate(&runs, 1);
        // Then
        assert_eq!(verdict.state, expected, "{output}");
        assert!(verdict.reason.contains(needle), "{}", verdict.reason);
    }
}

#[test]
fn a_later_run_without_a_valid_result_line_withdraws_verification() {
    // Given
    for later in [
        recorded("probe", "ls", "notes.txt\n\nExit code: 0"),
        recorded(
            "bad",
            "python3 x.py",
            "SHACS_CALC {not json}\n\nExit code: 0",
        ),
    ] {
        let runs = evidence(vec![ok("p", "a", PRIMARY), ok("c", "b", CHECK), later]).runs;
        // When
        let verdict = evaluate(&runs, 1);
        // Then
        assert_eq!(verdict.state, CalcState::Unverified, "{}", verdict.reason);
        assert!(verdict.reason.contains("no valid result line"));
    }
}

#[test]
fn stored_code_and_output_stay_within_their_limits_after_redaction() {
    // Given
    let secret = "api_key=sk-secret-sensitive-value";
    let command = format!("python3 -c 'print(1)' # {secret} {}", "x".repeat(5000));
    let output = format!("{secret}\n{}\n\nExit code: 0", "y".repeat(5000));
    // When
    let stored = run("call", &command, &output);
    // Then
    assert!(stored.code.chars().count() <= 2048);
    assert!(stored.output.chars().count() <= 1024);
    assert!(stored.code.ends_with("…[truncated]"));
    assert!(!stored.code.contains("sk-secret-sensitive-value"));
    assert!(!stored.output.contains("sk-secret-sensitive-value"));
}

#[test]
fn a_result_line_inside_a_failed_or_denied_run_is_ignored() {
    // Given
    for output in [
        format!("Error: Tool `exec` blocked by plugin hook `p` for event `tool:before`:\nSHACS_CALC {PRIMARY}"),
        format!("SHACS_CALC {PRIMARY}\nTraceback\n\nExit code: 1"),
    ] {
        let failed = recorded("p", "python3 -c 'x'", &output);
        // When
        let verdict = evaluate(&evidence(vec![failed.clone()]).runs, 1);
        // Then
        assert!(failed.result.is_none());
        assert!(failed.marker_error.is_some());
        assert_eq!(verdict.primary, None);
        assert!(matches!(verdict.state, CalcState::Unavailable | CalcState::Failed));
    }
}
