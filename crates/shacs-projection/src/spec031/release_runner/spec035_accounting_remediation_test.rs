use super::{write, CommandFixture};
use crate::spec031::release_runner::spec035_test_counts::{workspace_counts, TargetSummary};
use serde_json::{json, Value};

const ONE: &str = "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
const SHIFTED: &str = concat!(
    "running 1 test\n",
    "running 1 test\n",
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "running 2 tests\n",
    "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "running 3 tests\n",
    "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "running 100 tests\n",
    "test result: ok. 100 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "running 3 tests\n",
    "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
);

fn fixture(stdout: &str, stderr: &str) -> CommandFixture {
    let mut fixture = CommandFixture::new();
    fixture.stdout = write(
        fixture.evidence.path(),
        "workspace.stdout",
        stdout.as_bytes(),
    );
    fixture.stderr = write(
        fixture.evidence.path(),
        "workspace.stderr",
        stderr.as_bytes(),
    );
    fixture.command["stdout"] = json!(fixture.stdout);
    fixture.command["stderr"] = json!(fixture.stderr);
    fixture.accounting["stdout"] = json!(fixture.stdout);
    fixture.accounting["stderr"] = json!(fixture.stderr);
    fixture
}

#[test]
fn spec035_admission_ra_r01_rejects_skipped_target_without_accounting() {
    let mut fixture = fixture(ONE, "Running target-a\nRunning target-b\n");
    fixture.command["test_accounting"] = Value::Null;
    fixture.command["tests"]["tests_run"] = json!(1);

    let result = fixture.validate();

    assert!(result.is_err(), "incomplete target-b admitted: {result:?}");
}

#[test]
fn spec035_admission_ra_r01_rejects_inner_only_completion_without_accounting() {
    let mut fixture = fixture(&format!("running 1 test\n{ONE}"), "Running target-a\n");
    fixture.command["test_accounting"] = Value::Null;
    fixture.command["tests"]["tests_run"] = json!(1);

    let result = fixture.validate();

    assert!(result.is_err(), "incomplete outer admitted: {result:?}");
}

#[test]
fn spec035_admission_ra_r02_rejects_reviewers_shifted_offsets() {
    let mut fixture = fixture(
        SHIFTED,
        "Running target-a\nRunning target-b\nRunning target-c\n",
    );
    fixture.command["tests"]["tests_run"] = json!(6);
    fixture.accounting["targets"] = json!([
        {"header":"Running target-a","summary_line":3},
        {"header":"Running target-b","summary_line":5},
        {"header":"Running target-c","summary_line":12}
    ]);

    let result = fixture.validate();

    assert!(
        result.is_err(),
        "104 top-level tests relabeled as 6: {result:?}"
    );
}

#[test]
fn spec035_admission_ra_r02_rejects_top_level_mislabeled_as_nested() {
    let text = ONE.repeat(3);
    let targets = [2, 6].map(|summary_line| TargetSummary {
        header: "Running target".to_owned(),
        summary_line,
    });

    let result = workspace_counts(&text, "Running target\nRunning target\n", &targets);

    assert!(
        result.is_err(),
        "unowned middle target admitted: {result:?}"
    );
}

#[test]
fn spec035_admission_ra_r02_rejects_ambiguous_parent_child_completion() {
    let text = format!(
        "running 1 test\n{ONE}{}",
        ONE.lines().nth(1).expect("summary")
    );
    let targets = [TargetSummary {
        header: "Running target".to_owned(),
        summary_line: 4,
    }];

    let result = workspace_counts(&text, "Running target\n", &targets);

    assert!(result.is_err(), "ambiguous ownership admitted: {result:?}");
}

#[test]
fn spec035_admission_ra_r01_accepts_complete_flat_command_without_accounting() {
    let mut fixture = fixture(ONE, "Running target-a\n");
    fixture.command["test_accounting"] = Value::Null;
    fixture.command["tests"]["tests_run"] = json!(1);

    let result = fixture.validate();

    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn spec035_admission_ra_r02_accepts_unambiguous_overlapping_children() {
    let text = concat!(
        "running 5 tests\nrunning 1 test\nrunning 2 tests\n",
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n",
        "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\n",
        "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    );
    let targets = [TargetSummary {
        header: "Running target".to_owned(),
        summary_line: 6,
    }];

    let result = workspace_counts(text, "Running target\n", &targets).expect("unambiguous");

    assert_eq!(result.top_level.passed, 5);
    assert_eq!(result.top_level_summary_lines, [6]);
    assert_eq!(result.nested.passed, 3);
    assert_eq!(result.nested_summary_lines, [4, 5]);
}

#[test]
fn spec035_admission_ra_r02_accepts_distinct_parent_after_unfinished_child() {
    let text = "running 2 tests\nrunning 1 test\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    let targets = [TargetSummary {
        header: "Running target".to_owned(),
        summary_line: 3,
    }];

    let result = workspace_counts(text, "Running target\n", &targets).expect("distinct parent");

    assert_eq!(result.top_level.passed, 2);
    assert_eq!(result.top_level_summary_lines, [3]);
    assert_eq!(result.nested.targets, 0);
    assert_eq!(result.nested.passed, 0);
}

#[test]
fn spec035_admission_ra_r01_rejects_unfinished_child_in_flat_fallback() {
    let text = "running 2 tests\nrunning 1 test\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";

    let result = crate::spec031::release_runner::spec035_test_counts::flat_workspace_counts(
        text,
        "Running target\n",
    );

    assert!(result.is_err(), "nested announcement admitted: {result:?}");
}
