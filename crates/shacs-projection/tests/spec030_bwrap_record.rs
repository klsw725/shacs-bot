#[cfg(not(target_os = "linux"))]
use shacs_projection::{
    build_spec030_source_manifest, run_spec030_release_runner, Spec030ReleaseRunId,
    Spec030ReleaseRunnerConfig, Spec030ReleaseRunnerMode,
};
use shacs_projection::{
    validate_spec030_bwrap_record, Spec030BwrapRecordError, SPEC030_BWRAP_RECORD_SCHEMA,
};
use std::fs;
use std::path::PathBuf;
#[cfg(not(target_os = "linux"))]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock follows epoch")
        .as_nanos();
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!(
            "shacs-spec030-bwrap-{label}-{}-{nonce}",
            std::process::id()
        ))
}

fn valid_record() -> serde_json::Value {
    serde_json::json!({
        "schema": SPEC030_BWRAP_RECORD_SCHEMA,
        "source_digest": format!("sha256:{}", "0".repeat(64)),
        "platform": "linux",
        "environment": {"SHACS_REQUIRE_BWRAP": "1"},
        "test_name": "real_bwrap_lane_runs_only_when_required",
        "status": "passed",
        "tests": {"tests_run": 1, "tests_failed": 0},
        "cleanup": {
            "descendants_cleaned": true,
            "temporary_artifacts_removed": true
        },
        "containment": {
            "adapter_scoped": true,
            "universal_containment": false,
            "kernel_isolation": false
        }
    })
}

fn write_record(
    label: &str,
    value: &serde_json::Value,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = temp_path(label);
    fs::write(&path, serde_json::to_vec_pretty(value)?)?;
    Ok(path)
}

#[cfg(not(target_os = "linux"))]
fn write_manual(
    label: &str,
    repo: &std::path::Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let digest = build_spec030_source_manifest(repo)?.source_digest;
    write_record(
        label,
        &serde_json::json!({
            "schema":"spec030.manual_qa.v1",
            "source_digest":digest,
            "observed_commands":[
                {"id":"cli-json","status":"passed"},
                {"id":"cli-human","status":"passed"},
                {"id":"tui-no-session","status":"passed"},
                {"id":"api-schema-1","status":"passed"},
                {"id":"api-schema-2","status":"passed"}
            ],
            "non_guarantees":[
                "current_os_user_authority",
                "not_kernel_isolation",
                "optional_adapter_scoped_sandbox"
            ]
        }),
    )
}

#[cfg(not(target_os = "linux"))]
fn current_config(
    label: &str,
    repo_root: PathBuf,
    manual_record: PathBuf,
    bwrap_record: PathBuf,
) -> Spec030ReleaseRunnerConfig {
    Spec030ReleaseRunnerConfig {
        run_id: Spec030ReleaseRunId::try_new(label).expect("test run id is safe"),
        evidence_root: temp_path(&format!("{label}-evidence")),
        repo_root,
        mode: Spec030ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::from_secs(1),
        manual_records: vec![manual_record],
        bwrap_record: Some(bwrap_record),
    }
}

#[cfg(not(target_os = "linux"))]
fn dirty_repo(label: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let repo = temp_path(label);
    fs::create_dir_all(&repo)?;
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&repo)
        .output()?;
    fs::write(repo.join("tracked"), "tracked")?;
    std::process::Command::new("git")
        .args(["add", "tracked"])
        .current_dir(&repo)
        .output()?;
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Spec030",
            "-c",
            "user.email=spec030@example.invalid",
            "commit",
            "-m",
            "fixture",
        ])
        .current_dir(&repo)
        .output()?;
    fs::write(repo.join("dirty"), "dirty")?;
    Ok(repo)
}

#[test]
fn spec030_release_runner_bwrap_record_rejects_boolean_only_self_attestation(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let path = write_record("self-attested", &valid_record())?;

    // When
    let error = validate_spec030_bwrap_record(&path).expect_err("producer transcript is required");

    // Then
    assert_eq!(error, Spec030BwrapRecordError::Malformed);
    Ok(())
}

#[test]
fn spec030_release_runner_bwrap_record_rejects_malformed_or_unknown_fields(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let mut wrong_platform = valid_record();
    wrong_platform["platform"] = serde_json::json!("darwin");
    let platform_path = write_record("wrong-platform", &wrong_platform)?;
    let mut unknown = valid_record();
    unknown["prose"] = serde_json::json!("trust me");
    let unknown_path = write_record("unknown-field", &unknown)?;

    // When / Then
    assert_eq!(
        validate_spec030_bwrap_record(&platform_path).expect_err("platform is exact"),
        Spec030BwrapRecordError::Malformed
    );
    assert_eq!(
        validate_spec030_bwrap_record(&unknown_path).expect_err("unknown fields fail"),
        Spec030BwrapRecordError::Malformed
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn spec030_release_runner_bwrap_record_rejects_symlink() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    // Given
    let target = write_record("symlink-target", &valid_record())?;
    let link = temp_path("symlink-link");
    symlink(target, &link)?;

    // When
    let error = validate_spec030_bwrap_record(&link).expect_err("symlink is untrusted");

    // Then
    assert_eq!(error, Spec030BwrapRecordError::InvalidPath);
    Ok(())
}

#[test]
#[cfg(not(target_os = "linux"))]
fn spec030_release_runner_rejects_external_record_without_trusted_linux_producer(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let repo = dirty_repo("valid-current")?;
    let manual = write_manual("valid-current-manual", &repo)?;
    let bwrap = write_record("valid-current-bwrap", &valid_record())?;

    // When
    let artifacts =
        run_spec030_release_runner(&current_config("valid-current", repo, manual, bwrap))?;

    // Then
    assert!(artifacts
        .blockers
        .iter()
        .any(|blocker| blocker.code == "bwrap_untrusted_producer"));
    assert!(artifacts.external_evidence.is_empty());
    assert!(artifacts
        .commands
        .iter()
        .all(|command| command.id != "spec030-bwrap-active"));
    Ok(())
}

#[test]
#[cfg(not(target_os = "linux"))]
fn spec030_release_runner_bwrap_record_failures_remain_blockers(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let cases = [
        (
            serde_json::json!({"schema": "wrong"}),
            "bwrap_untrusted_producer",
        ),
        (
            serde_json::json!({"producer_case":"failed"}),
            "bwrap_untrusted_producer",
        ),
        (
            serde_json::json!({"producer_case":"zero"}),
            "bwrap_untrusted_producer",
        ),
    ];

    // When / Then
    for (index, (record, expected)) in cases.into_iter().enumerate() {
        let label = format!("blocked-{index}");
        let repo = dirty_repo(&label)?;
        let manual = write_manual(&format!("{label}-manual"), &repo)?;
        let bwrap = write_record(&format!("{label}-bwrap"), &record)?;
        let artifacts = run_spec030_release_runner(&current_config(&label, repo, manual, bwrap))?;
        assert!(artifacts
            .blockers
            .iter()
            .any(|blocker| blocker.code == expected));
        assert!(artifacts
            .commands
            .iter()
            .all(|command| command.id != "spec030-bwrap-active"));
    }
    Ok(())
}
