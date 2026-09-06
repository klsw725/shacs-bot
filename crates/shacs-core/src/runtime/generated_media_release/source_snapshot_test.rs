use super::*;
use std::process::Command;

#[test]
fn linked_worktree_uses_controlled_common_and_worktree_git_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let repository = root.join("repository");
    let linked = root.join("linked");
    std::fs::create_dir(&repository)?;
    std::fs::write(repository.join("source.txt"), b"approved")?;
    commit(&repository)?;
    git(
        &repository,
        &["worktree", "add", "--quiet", "-b", "linked", linked.to_str().ok_or("linked path")?],
    )?;

    let context = SourceRootContext::resolve(&linked)
        .map_err(|error| format!("linked context resolution failed: {error}"))?;
    let snapshot = capture_context(&context)
        .map_err(|error| format!("linked source capture failed: {error}"))?;

    assert_eq!(snapshot.bytes("source.txt"), Some(b"approved".as_slice()));
    context.verify()?;
    Ok(())
}

#[test]
fn live_config_worktree_insertion_after_snapshot_breaks_retained_git_binding(
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = tempfile::tempdir()?;
    let root = repository.path().canonicalize()?;
    std::fs::write(root.join("source.txt"), b"approved")?;
    commit(&root)?;
    let context = SourceRootContext::resolve(&root)?;

    std::fs::write(root.join(".git/config.worktree"), b"[core]\n")?;

    assert!(matches!(
        context.verify(),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn persistent_snapshot_is_writable_after_drop_and_resealed_on_reuse(
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let repository = tempfile::tempdir()?;
    let root = repository.path().canonicalize()?;
    std::fs::write(root.join("source.txt"), b"approved")?;
    commit(&root)?;
    let snapshot = capture(&root)?;
    let cache = tempfile::tempdir()?;
    let source_parent = cache.path().join("source");
    std::fs::create_dir(&source_parent)?;

    let first = snapshot.materialize_at(&source_parent)?;
    let path = first.path().to_path_buf();
    assert_eq!(std::fs::metadata(&path)?.permissions().mode() & 0o200, 0);
    drop(first);
    assert_ne!(std::fs::metadata(&path)?.permissions().mode() & 0o200, 0);

    let second = snapshot.materialize_at(&source_parent)?;
    assert_eq!(std::fs::metadata(&path)?.permissions().mode() & 0o200, 0);
    second.verify()?;
    drop(second);
    assert_ne!(std::fs::metadata(&path)?.permissions().mode() & 0o200, 0);
    Ok(())
}

#[test]
fn bounded_read_rejects_transient_content_restored_before_confirmation(
) -> Result<(), Box<dyn std::error::Error>> {
    let repository = tempfile::tempdir()?;
    let root = repository.path().canonicalize()?;
    let path = root.join("source.txt");
    std::fs::write(&path, b"approved")?;
    let reader = ConfinedSourceReader::open(&root)?;

    let result = reader.read_with_content_hook(
        "source.txt",
        64,
        || std::fs::write(&path, b"transient").expect("install transient bytes"),
        || std::fs::write(&path, b"approved").expect("restore approved bytes"),
    );

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}

fn commit(repository: &Path) -> Result<(), Box<dyn std::error::Error>> {
    git(repository, &["init", "--quiet"])?;
    git(repository, &["add", "."])?;
    git(
        repository,
        &[
            "-c",
            "user.name=Spec034 Test",
            "-c",
            "user.email=spec034@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    )
}

fn git(repository: &Path, arguments: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .status()?;
    status.success().then_some(()).ok_or_else(|| "git failed".into())
}
