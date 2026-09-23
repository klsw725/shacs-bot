use super::super::model::Spec031ReleaseArtifactError as Error;
use super::super::spec035_execution_fixture::{file_ref, write_json};
use super::super::spec035_postrun::SEAL;
use super::super::spec035_postrun_fixture::{runner_fixture, supplement};
use serde_json::json;
use std::path::Path;

#[test]
fn spec035_postrun_rejects_oracle_reordered_cargo_self_review() {
    let (fixture, artifacts) = runner_fixture();
    let root = Path::new(&artifacts.evidence_root);
    let mut postrun = supplement(&fixture, &artifacts);
    postrun["review_command"]["argv"] = json!([
        "cargo",
        "run",
        "--locked",
        "--manifest-path",
        "crates/Cargo.toml",
        "-p",
        "shacs-projection",
        "--bin",
        "spec031-release-runner",
        "--",
        "--help"
    ]);
    write_json(&root.join("postrun/finalization.json"), &postrun);

    let result = artifacts.finalize_spec035(fixture.repo.path(), "postrun/finalization.json");

    println!(
        "{}",
        json!({"case":"oracle-reordered-cargo-self-review", "postrun":file_ref(root, "postrun/finalization.json"), "argv":postrun["review_command"]["argv"], "result":format!("{result:?}"), "seal_exists":root.join(SEAL).exists()})
    );
    assert!(
        matches!(result, Err(Error::InvalidCommandEvidence)),
        "{result:?}"
    );
    assert!(!root.join(SEAL).exists());
}

#[test]
fn spec035_postrun_rejects_direct_cargo_runner_bin_variants() {
    for command in [
        "cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --help",
        "cargo run --bin=spec031-release-runner --locked --manifest-path=crates/Cargo.toml --package=shacs-projection -- --help",
        "cargo r --locked -pshacs-projection --bin spec031-release-runner --manifest-path crates/Cargo.toml -- --help",
        "cargo --locked run --bin spec031-release-runner --package shacs-projection --manifest-path crates/Cargo.toml -- --help",
        "cargo run --quiet --manifest-path crates/Cargo.toml --bin=spec031-release-runner -p shacs-projection -- --help",
        "/usr/bin/cargo run --bin spec031-release-runner --manifest-path crates/Cargo.toml -p shacs-projection -- --help",
        "cargo run --bin spec031-release-runner",
        "spec031-release-runner --help",
        "/owned/bin/spec031-release-runner --help",
    ] {
        let (fixture, artifacts) = runner_fixture();
        let root = Path::new(&artifacts.evidence_root);
        let mut postrun = supplement(&fixture, &artifacts);
        postrun["review_command"]["argv"] = json!(command.split_whitespace().collect::<Vec<_>>());
        write_json(&root.join("postrun/finalization.json"), &postrun);

        let result = artifacts.finalize_spec035(fixture.repo.path(), "postrun/finalization.json");

        assert!(matches!(result, Err(Error::InvalidCommandEvidence)), "{command}: {result:?}");
        assert!(!root.join(SEAL).exists(), "{command}");
    }
}

#[test]
fn spec035_postrun_allows_independent_review_with_bound_pass_receipts() {
    for command in [
        "independent-review-test-fixture",
        "cargo run --locked --manifest-path crates/Cargo.toml --bin independent-review -- --input spec031-release-runner",
        "cargo r --bin=independent-review -- --bin spec031-release-runner",
        "cargo run --bin independent-review -- --bin=spec031-release-runner",
    ] {
        let (fixture, artifacts) = runner_fixture();
        let root = Path::new(&artifacts.evidence_root);
        let mut postrun = supplement(&fixture, &artifacts);
        postrun["review_command"]["argv"] = json!(command.split_whitespace().collect::<Vec<_>>());
        write_json(&root.join("postrun/finalization.json"), &postrun);

        let result = artifacts.finalize_spec035(fixture.repo.path(), "postrun/finalization.json");

        assert!(result.is_ok(), "{command}: {result:?}");
        assert!(root.join(SEAL).exists());
    }
}

#[test]
fn spec035_postrun_unknown_review_semantics_cannot_seal() {
    for verdict in ["UNKNOWN", "BLOCKED", "FAILED"] {
        let (fixture, artifacts) = runner_fixture();
        let root = Path::new(&artifacts.evidence_root);
        let mut postrun = supplement(&fixture, &artifacts);
        let path = "postrun/read-audit.json";
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join(path)).expect("review receipt"))
                .expect("receipt json");
        receipt["checks"][0]["verdict"] = json!(verdict);
        write_json(&root.join(path), &receipt);
        postrun["read_audit"] = file_ref(root, path);
        write_json(&root.join("postrun/finalization.json"), &postrun);

        let result = artifacts.finalize_spec035(fixture.repo.path(), "postrun/finalization.json");

        assert!(result.is_err(), "{verdict}: {result:?}");
        assert!(!root.join(SEAL).exists());
    }
}

#[test]
fn spec035_postrun_exit_invocation_remains_strict() {
    for command in [
        "cargo run --locked --manifest-path crates/Cargo.toml -p shacs-projection --bin spec031-release-runner -- --help",
        "cargo r --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --help",
        "cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin=spec031-release-runner -- --help",
    ] {
        let argv: Vec<_> = command.split_whitespace().map(str::to_owned).collect();

        let arguments = super::runner_arguments(&argv);

        assert!(arguments.is_none(), "{command}");
    }
    for command in [
        "cargo run --manifest-path crates/Cargo.toml --locked -p shacs-projection --bin spec031-release-runner -- --help",
        "/owned/bin/spec031-release-runner --help",
    ] {
        let argv: Vec<_> = command.split_whitespace().map(str::to_owned).collect();

        let arguments = super::runner_arguments(&argv);

        assert_eq!(arguments, Some(&["--help".to_owned()][..]), "{command}");
    }
}
