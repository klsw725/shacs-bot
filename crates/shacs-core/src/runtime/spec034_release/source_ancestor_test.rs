use super::*;

#[test]
fn materialized_snapshot_ancestor_a_b_a_breaks_execution_seal(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo)?;
    let repo = repo.canonicalize()?;
    std::fs::write(repo.join("Cargo.toml"), b"[workspace]\n")?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    let status = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&repo)
        .args(["init", "--quiet"])
        .status()?;
    assert!(status.success());
    let status = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&repo)
        .args(["add", "."])
        .status()?;
    assert!(status.success());
    let status = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&repo)
        .args([
            "-c",
            "user.name=Spec034 Test",
            "-c",
            "user.email=spec034@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ])
        .status()?;
    assert!(status.success());
    let execution = capture(&repo)?.materialize()?;
    let ancestor = execution.task_parent_path().to_path_buf();
    let parent = ancestor.parent().ok_or("task parent ancestor")?;
    let displaced = parent.join("source-parent-a");

    std::fs::rename(&ancestor, &displaced)?;
    std::fs::create_dir(&ancestor)?;
    std::fs::remove_dir(&ancestor)?;
    std::fs::rename(&displaced, &ancestor)?;

    assert!(matches!(execution.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}
