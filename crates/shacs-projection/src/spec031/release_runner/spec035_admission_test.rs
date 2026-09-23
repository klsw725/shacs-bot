use super::spec035_test_counts::workspace_counts;
use super::spec035_test_counts::TargetSummary;
use std::path::Path;

#[test]
fn spec035_admission_accepts_exact_no_fail_fast_workspace_argv() {
    let argv = [
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "--locked",
        "--workspace",
        "--no-fail-fast",
    ];

    let accepted = super::spec035_execution_commands::workspace_argv(&argv);

    assert!(accepted);
}

#[test]
#[ignore = "requires preserved workspace receipt; reads evidence only"]
fn spec035_admission_preserved_workspace_counts_exclude_nested_children() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stdout = std::fs::read_to_string(
        repo.join(".omo/evidence/spec035/workspace-no-fail-fast-20260917/workspace.stdout"),
    )
    .expect("preserved real stdout");

    let root = repo.join(".omo/evidence/spec035/workspace-no-fail-fast-20260917");
    let stderr = std::fs::read_to_string(root.join("workspace.stderr")).expect("stderr");
    let mut targets = Vec::new();
    for (file, prefix) in [
        ("top-level-test-targets.json", "Running "),
        ("doc-test-outcomes.json", "Doc-tests "),
    ] {
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(root.join(file)).expect("target index"))
                .expect("JSON");
        targets.extend(rows.iter().map(|row| TargetSummary {
            header: format!("{prefix}{}", row["target"].as_str().expect("target")),
            summary_line:
                usize::try_from(row["stdoutLine"].as_u64().expect("line")).expect("usize"),
        }));
    }
    let summary = workspace_counts(&stdout, &stderr, &targets).expect("valid transcript");
    let counts = summary.counts();

    assert_eq!(counts.tests_run, 2906);
    assert_eq!(counts.tests_failed, 0);
    assert_eq!(summary.top_level.ignored, 2);
    assert_eq!(summary.top_level.targets, 296);
    assert_eq!(summary.nested.passed, 10);
    assert_eq!(summary.nested.targets, 12);
}

#[test]
fn spec035_admission_rejects_workspace_argv_weakening() {
    let valid = [
        "cargo",
        "test",
        "--manifest-path",
        "crates/Cargo.toml",
        "--locked",
        "--workspace",
        "--no-fail-fast",
    ];
    for suffix in [
        &["-p", "shacs-projection"][..],
        &["--exclude", "shacs-core"],
        &["--", "--ignored"],
        &["--lib"],
        &["some_filter"],
        &["--no-fail-fast"],
    ] {
        let mut argv = valid.to_vec();
        argv.extend_from_slice(suffix);
        assert!(!super::spec035_execution_commands::workspace_argv(&argv));
    }
    let unlocked: Vec<_> = valid.into_iter().filter(|arg| *arg != "--locked").collect();
    assert!(!super::spec035_execution_commands::workspace_argv(
        &unlocked
    ));
}

#[test]
fn spec035_admission_rejects_incomplete_or_forged_harness_counts() {
    for text in [
        "running 2 tests\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        "running 1 test\n",
        "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
        "running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s\n",
        "running 0 tests\ntest result: ok. 18446744073709551615 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    ] {
        let targets = [TargetSummary {header:"Running test-target".to_owned(), summary_line:2}];
        assert!(workspace_counts(text, "Running test-target", &targets).is_err(), "{text}");
    }
}

#[test]
fn spec035_admission_keeps_nested_failures_visible() {
    let text = "running 2 tests\nrunning 1 test\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    let targets = [TargetSummary {
        header: "Running test-target".to_owned(),
        summary_line: 4,
    }];
    assert!(workspace_counts(text, "Running test-target", &targets).is_err());
}

#[test]
fn spec035_admission_requires_accounting_for_nested_workspace_output() {
    let text = "running 2 tests\nrunning 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\ntest result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert!(
        super::spec035_test_counts::flat_workspace_counts(text, "Running test-target").is_err()
    );
}

#[test]
fn spec035_admission_preserves_flat_workspace_counts() {
    let text = "running 2 tests\ntest result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    let counts = super::spec035_test_counts::flat_workspace_counts(text, "Running test-target")
        .expect("flat workspace");
    assert_eq!(counts.tests_run, 1);
}

#[test]
#[ignore = "requires preserved source, receipts and committed tree; read-only admission driver"]
fn spec035_admission_preserved_receipts_have_scoped_correspondence() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let adapter = ".omo/evidence/spec035/closure/receipt-admission-20260918.json";

    let report = crate::spec031::inspect_spec035_receipt_admission(
        &repo,
        adapter,
        "workspace-no-fail-fast-20260917",
    )
    .expect("authentic admission, not closure");

    assert_eq!(report.workspace.top_level.passed, 2906);
    assert_eq!(report.workspace.nested.passed, 10);
    assert_eq!(report.source.files_verified, 1854);
    assert!(!report.source.current_deltas.is_empty());
    assert!(!report.semantic_closure_evaluated);
    if let Some(path) = std::env::var_os("SHACS_SPEC035_ADMISSION_REPORT") {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("new report only");
        serde_json::to_writer_pretty(file, &report).expect("retained admission report");
    }
    println!("{}", serde_json::to_string_pretty(&report).expect("report"));
}
