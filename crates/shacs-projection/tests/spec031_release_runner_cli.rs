use shacs_projection::{Spec031ExternalOwnerId, Spec031ReleaseRunArtifacts};
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn current_worktree_stderr_matches_generated_spec035_cause(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let evidence_root = temp.path().canonicalize()?.join("current-evidence");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;

    let output = Command::new(env!("CARGO_BIN_EXE_spec031-release-runner"))
        .args([
            "--run-id",
            "spec035-cli-current",
            "--evidence-root",
            evidence_root.to_str().ok_or("non-UTF-8 evidence path")?,
            "--repo-root",
            repo_root.to_str().ok_or("non-UTF-8 repository path")?,
            "--mode",
            "current-worktree",
        ])
        .output()?;

    assert!(!output.status.success());
    let artifacts: Spec031ReleaseRunArtifacts =
        serde_json::from_slice(&fs::read(evidence_root.join("manifest.json"))?)?;
    let spec035_reason = &artifacts
        .external_audits
        .iter()
        .find(|audit| audit.owner == Spec031ExternalOwnerId::Spec035)
        .ok_or("missing Spec035 audit")?
        .reason;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(spec035_reason.contains("source binding digest mismatch for "));
    assert_eq!(
        stderr,
        format!("spec031 release runner failed: BlockedExternalEvidence: {spec035_reason}\n")
    );

    Ok(())
}

#[test]
fn success_fixture_writes_no_stderr() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let evidence_root = temp.path().canonicalize()?.join("success-evidence");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;

    let output = Command::new(env!("CARGO_BIN_EXE_spec031-release-runner"))
        .args([
            "--run-id",
            "spec035-cli-success",
            "--evidence-root",
            evidence_root.to_str().ok_or("non-UTF-8 evidence path")?,
            "--repo-root",
            repo_root.to_str().ok_or("non-UTF-8 repository path")?,
            "--mode",
            "success-fixture",
        ])
        .output()?;

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    Ok(())
}
