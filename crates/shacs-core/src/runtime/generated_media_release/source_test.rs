use super::*;
use std::process::Command;

#[test]
fn source_manifest_binds_tracked_sse_changes() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo_path = root.path().join("repo");
    std::fs::create_dir(&repo_path)?;
    let repo = repo_path.canonicalize()?;
    std::fs::create_dir_all(repo.join("crates/shacs-providers/tests/fixtures"))?;
    std::fs::write(repo.join("crates/Cargo.toml"), b"[workspace]\n")?;
    let fixture = repo.join("crates/shacs-providers/tests/fixtures/input.sse");
    std::fs::write(&fixture, b"data: original\n")?;
    commit_fixture(&repo)?;
    let before = collect(&repo)?;

    std::fs::write(&fixture, b"data: changed\n")?;
    let after = collect(&repo)?;

    assert_ne!(after.digest, before.digest);
    assert!(after.worktree_dirty);
    assert!(after.files.iter().any(|file| {
        file.locator == "crates/shacs-providers/tests/fixtures/input.sse" && file.modified
    }));
    Ok(())
}

#[test]
fn source_manifest_binds_all_git_visible_files_and_full_dirty_state(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo)?;
    let repo = repo.canonicalize()?;
    for (locator, content) in [
        ("Dockerfile", "FROM scratch\n"),
        ("AGENTS.md", "rules\n"),
        (".env.example", "KEY=value\n"),
        ("compose.yaml", "services: {}\n"),
        ("config.toml", "enabled = true\n"),
        (".omo/evidence.json", "{}\n"),
        ("nested/target/generated.txt", "generated\n"),
    ] {
        let path = repo.join(locator);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }
    commit_fixture(&repo)?;
    let baseline = collect(&repo)?;
    for locator in ["Dockerfile", "AGENTS.md", ".env.example", "compose.yaml", "config.toml"] {
        assert!(baseline.files.iter().any(|file| file.locator == locator));
    }
    assert!(!baseline.files.iter().any(|file| file.locator.starts_with(".omo/")));
    assert!(!baseline.files.iter().any(|file| file.locator.contains("/target/")));

    std::fs::write(repo.join(".omo/evidence.json"), b"changed\n")?;
    assert!(collect(&repo)?.worktree_dirty);
    git(&repo, &["restore", ".omo/evidence.json"])?;
    std::fs::write(repo.join("nested/target/generated.txt"), b"changed\n")?;
    assert!(collect(&repo)?.worktree_dirty);
    git(&repo, &["restore", "nested/target/generated.txt"])?;
    for locator in ["Dockerfile", "AGENTS.md", ".env.example"] {
        std::fs::write(repo.join(locator), format!("changed {locator}\n"))?;
    }
    std::fs::write(repo.join("local.override"), b"untracked\n")?;
    let changed = collect(&repo)?;
    assert!(changed.worktree_dirty);
    for locator in ["Dockerfile", "AGENTS.md", ".env.example", "local.override"] {
        assert!(changed.files.iter().any(|file| file.locator == locator && file.modified));
    }
    Ok(())
}

#[test]
fn nul_paths_reject_invalid_utf8() {
    let paths = nul_paths(b"crates/good.rs\0crates/\xff.rs\0");
    assert!(matches!(paths, Err(Spec034ReleaseArtifactError::InvalidConfig)));
}

#[test]
fn source_manifest_records_unstaged_and_staged_deletions() -> Result<(), Box<dyn std::error::Error>> {
    for staged in [false, true] {
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo)?;
        let repo = repo.canonicalize()?;
        std::fs::write(repo.join("tracked.txt"), b"present")?;
        commit_fixture(&repo)?;
        let clean = collect(&repo)?;
        assert!(clean.files.iter().all(|file| file.state == SourceFileState::Present));
        std::fs::remove_file(repo.join("tracked.txt"))?;
        if staged {
            git(&repo, &["add", "-u"])?;
        }

        let deleted = collect(&repo)?;

        let row = deleted.files.iter().find(|file| file.locator == "tracked.txt").ok_or("missing tombstone")?;
        assert_eq!(row.state, SourceFileState::Deleted);
        assert!(row.digest.is_none());
        assert!(row.modified);
        assert!(deleted.worktree_dirty);
        assert_ne!(deleted.digest, clean.digest);
    }
    Ok(())
}

#[test]
fn source_manifest_records_rename_and_recreation() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo)?;
    let repo = repo.canonicalize()?;
    std::fs::write(repo.join("old.txt"), b"old")?;
    commit_fixture(&repo)?;
    git(&repo, &["mv", "old.txt", "new.txt"])?;
    let renamed = collect(&repo)?;
    assert_eq!(
        renamed.files.iter().find(|file| file.locator == "old.txt").ok_or("old missing")?.state,
        SourceFileState::Deleted
    );
    assert_eq!(
        renamed.files.iter().find(|file| file.locator == "new.txt").ok_or("new missing")?.state,
        SourceFileState::Present
    );

    std::fs::write(repo.join("old.txt"), b"recreated")?;
    let recreated = collect(&repo)?;

    assert_eq!(
        recreated.files.iter().find(|file| file.locator == "old.txt").ok_or("recreated missing")?.state,
        SourceFileState::Present
    );
    assert!(recreated.files.iter().find(|file| file.locator == "old.txt").and_then(|file| file.digest.as_ref()).is_some());
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_manifest_rejects_symlink_ancestor_outside_repository(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo_path = root.path().join("repo");
    std::fs::create_dir(&repo_path)?;
    let repo = repo_path.canonicalize()?;
    let outside = tempfile::tempdir()?;
    std::fs::create_dir(repo.join("nested"))?;
    std::fs::write(repo.join("nested/source.rs"), b"inside")?;
    commit_fixture(&repo)?;
    std::fs::write(outside.path().join("source.rs"), b"outside")?;
    std::fs::rename(repo.join("nested"), repo.join("displaced"))?;
    std::os::unix::fs::symlink(outside.path(), repo.join("nested"))?;

    assert!(collect(&repo).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_reader_stays_bound_when_opened_ancestor_is_replaced(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo_path = root.path().join("repo");
    std::fs::create_dir(&repo_path)?;
    let repo = repo_path.canonicalize()?;
    let outside = tempfile::tempdir()?;
    std::fs::create_dir(repo.join("nested"))?;
    std::fs::write(repo.join("nested/source.rs"), b"inside")?;
    std::fs::write(outside.path().join("source.rs"), b"outside")?;
    let expected = format!("sha256:{:x}", Sha256::digest(b"inside"));

    let actual = digest_source_for_test(&repo, "nested/source.rs", || {
        assert!(std::fs::rename(repo.join("nested"), repo.join("displaced")).is_ok());
        assert!(std::os::unix::fs::symlink(outside.path(), repo.join("nested")).is_ok());
    })?;

    assert_eq!(actual, expected);
    Ok(())
}

#[test]
fn materialized_execution_snapshot_survives_live_s_b_s_replacement(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo)?;
    let repo = repo.canonicalize()?;
    std::fs::write(repo.join("Cargo.toml"), b"[workspace]\n")?;
    std::fs::write(repo.join("tested.txt"), b"S")?;
    commit_fixture(&repo)?;
    let snapshot = capture(&repo)?;
    let execution = snapshot.materialize()?;

    std::fs::write(repo.join("tested.txt"), b"B")?;
    let tested = Command::new("/bin/cat")
        .arg(execution.path().join("tested.txt"))
        .output()?;
    std::fs::write(repo.join("tested.txt"), b"S")?;

    assert!(tested.status.success());
    assert_eq!(tested.stdout, b"S");
    assert_eq!(collect(&repo)?, snapshot.manifest);
    assert_eq!(std::fs::read(execution.path().join("tested.txt"))?, b"S");
    Ok(())
}

#[cfg(unix)]
#[test]
fn materialized_execution_seal_rejects_mutate_and_restore(
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo)?;
    let repo = repo.canonicalize()?;
    std::fs::write(repo.join("Cargo.toml"), b"[workspace]\n")?;
    std::fs::write(repo.join("tested.txt"), b"S")?;
    commit_fixture(&repo)?;
    let execution = capture(&repo)?.materialize()?;
    let tested = execution.path().join("tested.txt");
    std::fs::set_permissions(&tested, std::fs::Permissions::from_mode(0o600))?;
    std::fs::write(&tested, b"B")?;
    std::fs::write(&tested, b"S")?;

    assert!(matches!(execution.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}

fn commit_fixture(repo: &Path) -> Result<(), Box<dyn std::error::Error>> {
    git(repo, &["init", "--quiet"])?;
    git(repo, &["add", "."])?;
    git(repo, &[
        "-c", "user.name=Spec034 Test", "-c", "user.email=spec034@example.invalid",
        "commit", "--quiet", "-m", "fixture",
    ])
}

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git").arg("-C").arg(repo).args(args).status()?;
    if !status.success() {
        return Err("git fixture command failed".into());
    }
    Ok(())
}
