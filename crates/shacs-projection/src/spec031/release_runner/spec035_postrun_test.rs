use super::spec035_execution_fixture::{file_ref, write_json};
use super::spec035_postrun_fixture::{runner_fixture, supplement};
use serde_json::json;
use std::path::Path;

#[test]
fn spec035_postrun_seals_only_after_exit_and_read_audit_without_rewriting_originals() {
    let (fixture, artifacts) = runner_fixture();
    let root = Path::new(&artifacts.evidence_root);
    let before = std::fs::read(root.join("manifest.json")).expect("original");
    super::validate::validate_pending_artifacts(&artifacts, fixture.repo.path())
        .expect("execution validation succeeds");
    assert_eq!(
        super::writer::summary_status(&artifacts),
        "pending-final-audit"
    );
    assert!(
        super::validate::validate_spec031_release_artifacts_with_repo_root(
            &artifacts,
            fixture.repo.path()
        )
        .is_err()
    );
    supplement(&fixture, &artifacts);

    artifacts
        .finalize_spec035(fixture.repo.path(), "postrun/finalization.json")
        .expect("constructed postrun admitted");

    super::validate::validate_spec031_release_artifacts_with_repo_root(
        &artifacts,
        fixture.repo.path(),
    )
    .expect("sealed original");
    assert_eq!(
        std::fs::read(root.join("manifest.json")).expect("original unchanged"),
        before
    );
    assert!(artifacts
        .finalize_spec035(fixture.repo.path(), "postrun/finalization.json")
        .is_err());
}

#[test]
fn spec035_postrun_rejects_missing_failed_stale_and_tampered_observations() {
    for mutation in [
        "missing-review",
        "failed-review",
        "stale-run",
        "changed-source",
        "missing-original",
        "changed-original",
        "exit-failed",
        "exit-running",
        "prior-failure",
        "missing-row",
        "row-failed",
        "review-unread",
        "duplicate-exit-flag",
        "self-review",
        "preflight-replaced",
    ] {
        let (fixture, mut artifacts) = runner_fixture();
        let root = std::path::PathBuf::from(&artifacts.evidence_root);
        let mut postrun = supplement(&fixture, &artifacts);
        match mutation {
            "missing-review" => {
                std::fs::remove_file(root.join("postrun/read-audit.json")).expect("remove");
            }
            "failed-review" => postrun["review_command"]["exit_code"] = json!(1),
            "stale-run" => postrun["binding"]["run_id"] = json!("another-run"),
            "self-review" => {
                postrun["review_command"]["argv"] = json!(["spec031-release-runner", "--help"])
            }
            "preflight-replaced" => {
                std::fs::write(
                    fixture.root.join("manifest.json"),
                    serde_json::to_vec(&fixture.manifest).expect("equivalent manifest"),
                )
                .expect("replace");
            }
            "changed-source" => {
                std::fs::write(fixture.repo.path().join("crates/probe.rs"), "changed")
                    .expect("source");
            }
            "missing-original" => {
                postrun["originals"]
                    .as_array_mut()
                    .expect("originals")
                    .pop();
            }
            "changed-original" => {
                std::fs::write(root.join("results.json"), "[]").expect("tamper");
            }
            "exit-failed" | "exit-running" | "duplicate-exit-flag" => {
                let mut exit: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(root.join("postrun/exit.json")).expect("exit"),
                )
                .expect("json");
                match mutation {
                    "exit-failed" => exit["exit_code"] = json!(1),
                    "exit-running" => exit["reaped"] = json!(false),
                    "duplicate-exit-flag" => exit["argv"]
                        .as_array_mut()
                        .expect("argv")
                        .extend([json!("--run-id"), json!("another-run")]),
                    _ => unreachable!(),
                }
                write_json(&root.join("postrun/exit.json"), &exit);
                postrun["exit"] = file_ref(&root, "postrun/exit.json");
            }
            "prior-failure" => artifacts.command_registry[0].exit_code = Some(1),
            "missing-row" => {
                postrun["requirements"].as_array_mut().expect("rows").pop();
            }
            "row-failed" | "review-unread" => {
                let path = if mutation == "row-failed" {
                    "postrun/row-0.json"
                } else {
                    "postrun/read-audit.json"
                };
                let mut receipt: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join(path)).expect("receipt"))
                        .expect("json");
                if mutation == "row-failed" {
                    receipt["checks"][0]["verdict"] = json!("FAILED");
                } else {
                    receipt["checks"][0]["artifacts"] = json!([]);
                }
                write_json(&root.join(path), &receipt);
                if mutation == "row-failed" {
                    postrun["requirements"][0]["receipt"] = file_ref(&root, path);
                } else {
                    postrun["read_audit"] = file_ref(&root, path);
                }
            }
            _ => unreachable!(),
        }
        write_json(&root.join("postrun/finalization.json"), &postrun);
        assert!(
            artifacts
                .finalize_spec035(fixture.repo.path(), "postrun/finalization.json")
                .is_err(),
            "{mutation}"
        );
        assert!(
            !root.join(super::spec035_postrun::SEAL).exists(),
            "{mutation}"
        );
    }
}

#[test]
fn spec035_postrun_seal_revalidation_rejects_later_original_tampering() {
    let (fixture, artifacts) = runner_fixture();
    supplement(&fixture, &artifacts);
    artifacts
        .finalize_spec035(fixture.repo.path(), "postrun/finalization.json")
        .expect("seal");
    std::fs::write(
        Path::new(&artifacts.evidence_root).join("postrun/review.stdout"),
        "changed",
    )
    .expect("tamper");

    assert!(
        super::validate::validate_spec031_release_artifacts_with_repo_root(
            &artifacts,
            fixture.repo.path()
        )
        .is_err()
    );
}

#[test]
fn spec035_summary_never_reports_pass_for_failed_commands_without_triage() {
    let (_fixture, mut artifacts) = runner_fixture();
    artifacts.command_registry[0].exit_code = Some(1);
    assert_eq!(super::writer::summary_status(&artifacts), "BLOCKED");
}

#[test]
fn spec035_postrun_cannot_erase_catalog_to_turn_preflight_into_final_admission() {
    let (fixture, mut artifacts) = runner_fixture();
    artifacts
        .manifest_files
        .retain(|file| file != "evidence-index.json");
    artifacts
        .coverage_matrix
        .retain(|row| !row.requirement_id.starts_with("spec035:"));

    assert!(
        super::coverage_validate::validate_coverage_matrix(&artifacts, fixture.repo.path())
            .is_err()
    );
}
