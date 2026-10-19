use std::path::Path;
use std::process::Command;

#[test]
fn current_worktree_stderr_reports_missing_spec035_archive(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let evidence_root = temp.path().canonicalize()?.join("current-evidence");
    let repo_root = temp.path().canonicalize()?.join("repo");
    std::fs::create_dir_all(&repo_root)?;
    assert!(Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&repo_root)
        .status()?
        .success());

    let output = Command::new(env!("CARGO_BIN_EXE_spec031-release-runner"))
        .args([
            "--run-id",
            "spec031-cli-missing-archive",
            "--evidence-root",
            evidence_root.to_str().ok_or("non-UTF-8 evidence path")?,
            "--repo-root",
            repo_root.to_str().ok_or("non-UTF-8 repository path")?,
            "--mode",
            "current-worktree",
        ])
        .output()?;

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.starts_with("spec031 release runner failed: MissingRequiredArtifact"));

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
            "spec031-cli-success",
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

#[test]
fn retired_spec035_phases_are_not_available_from_current_cli(
) -> Result<(), Box<dyn std::error::Error>> {
    for phase in ["preflight", "finalize"] {
        let output = Command::new(env!("CARGO_BIN_EXE_spec031-release-runner"))
            .args(["--phase", phase])
            .output()?;

        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr)?.contains("unknown argument: --phase"));
    }

    Ok(())
}
