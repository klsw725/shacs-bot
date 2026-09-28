use super::spec035_preflight_test::pending_fixture;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "manual CLI QA with an explicitly supplied compiled runner and new evidence directory"]
fn spec035_helper_surface_records_actual_preflight_outputs_on_constructed_input() {
    let binary = PathBuf::from(std::env::var("SPEC035_RUNNER_BIN").expect("compiled runner"));
    let output =
        PathBuf::from(std::env::var("SPEC035_HELPER_EVIDENCE").expect("owned evidence directory"));
    std::fs::create_dir(&output).expect("new evidence directory");
    let fixture = pending_fixture();
    let repo = fixture.repo.path().display().to_string();
    let root = fixture
        .repo
        .path()
        .join(".omo/never-executed")
        .display()
        .to_string();
    let common = [
        "--run-id",
        "constructed-execution",
        "--repo-root",
        &repo,
        "--evidence-root",
        &root,
        "--mode",
        "current-worktree",
    ];
    let mut observations = Vec::new();
    for (name, extra, success) in [
        ("help", vec!["--help"], true),
        ("invalid", vec!["--phase", "invalid"], false),
        ("preflight", vec!["--phase", "preflight"], true),
        ("missing-postrun", vec!["--phase", "finalize"], false),
    ] {
        let result = Command::new(&binary)
            .args(common)
            .args(&extra)
            .output()
            .expect("actual helper");
        assert_eq!(
            result.status.success(),
            success,
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        if name == "preflight" {
            let report: Value = serde_json::from_slice(&result.stdout).expect("report");
            assert_eq!(report["status"], "pending-final-audit");
            assert_eq!(report["requirements"], 80);
            assert_eq!(report["passed"], 74);
            assert_eq!(report["blocked"].as_array().expect("blocked").len(), 6);
            assert!(!PathBuf::from(&root).exists());
        }
        for (extension, bytes) in [("stdout", &result.stdout), ("stderr", &result.stderr)] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output.join(format!("{name}.{extension}")))
                .expect("new capture");
            file.write_all(bytes).expect("capture");
        }
        observations.push(json!({"name":name,"argv":extra,"exit_code":result.status.code(),"constructed_input":true}));
    }
    std::fs::write(fixture.root.join("observation.json"), "tampered")
        .expect("tamper synthetic input");
    let result = Command::new(&binary)
        .args(common)
        .args(["--phase", "preflight"])
        .output()
        .expect("actual rejection");
    assert!(!result.status.success());
    std::fs::write(output.join("tampered.stdout"), &result.stdout).expect("stdout");
    std::fs::write(output.join("tampered.stderr"), &result.stderr).expect("stderr");
    observations
        .push(json!({"name":"tampered","exit_code":result.status.code(),"constructed_input":true}));
    std::fs::write(
        output.join("observations.json"),
        serde_json::to_vec_pretty(&observations).expect("observations"),
    )
    .expect("observations");
    println!(
        "actual helper QA retained at {}; constructed inputs only, not G7 closure",
        output.display()
    );
}
