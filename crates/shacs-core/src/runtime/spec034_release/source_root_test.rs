use super::*;

#[test]
fn malicious_git_worktree_and_include_config_are_rejected(
) -> Result<(), Box<dyn std::error::Error>> {
    for config in [
        "[core]\n\tworktree = /tmp/elsewhere\n",
        "[include]\n\tpath = /tmp/forged-config\n",
    ] {
        let repo = tempfile::tempdir()?;
        git(repo.path(), &["init"])?;
        std::fs::write(repo.path().join(".git/config"), config)?;
        let repo = repo.path().canonicalize()?;

        let result = SourceRootContext::resolve(&repo);

        assert!(result.is_err());
    }
    Ok(())
}
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn source_root_a_b_a_during_collection_never_returns_manifest(
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let repo = root.join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    commit(&repo)?;
    let displaced = root.join("repo-a");
    let replacement = root.join("repo-b");
    let git = super::super::tools::ResolvedTool::git()?;

    let result = capture_with_git_after_enumeration_for_test(&repo, git, || {
        assert!(std::fs::rename(&repo, &displaced).is_ok());
        assert!(std::fs::create_dir(&replacement).is_ok());
        assert!(std::fs::rename(&replacement, &repo).is_ok());
        assert!(std::fs::remove_dir(&repo).is_ok());
        assert!(std::fs::rename(&displaced, &repo).is_ok());
    });

    assert!(result.is_err());
    Ok(())
}

#[test]
fn source_root_swap_after_descriptor_open_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let repo = root.join("repo");
    let displaced = root.join("repo-a");
    let replacement = root.join("repo-b");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    commit(&repo)?;
    std::fs::create_dir(&replacement)?;
    std::fs::write(replacement.join("source.txt"), b"replacement")?;
    commit(&replacement)?;
    let git = super::super::tools::ResolvedTool::git()?;

    let result = SourceRootContext::with_git_hook_for_test(&repo, git, || {
        assert!(std::fs::rename(&repo, &displaced).is_ok());
        assert!(std::fs::rename(&replacement, &repo).is_ok());
    });

    assert!(result.is_err());
    Ok(())
}

#[test]
fn repository_grandparent_swap_after_root_open_preserves_descriptor_bytes(
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let temporary_root = temporary.path().canonicalize()?;
    let parent = temporary_root.join("parent");
    let repo = parent.join("repo");
    let displaced = temporary_root.join("parent-a");
    let replacement = temporary_root.join("parent-b");
    std::fs::create_dir_all(&repo)?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    commit(&repo)?;
    std::fs::create_dir_all(replacement.join("repo"))?;
    std::fs::write(replacement.join("repo/source.txt"), b"replacement")?;
    commit(&replacement.join("repo"))?;
    let git = super::super::tools::ResolvedTool::git()?;

    let context = SourceRootContext::with_git_hook_for_test(&repo, git, || {
        assert!(std::fs::rename(&parent, &displaced).is_ok());
        assert!(std::fs::rename(&replacement, &parent).is_ok());
    })?;
    let snapshot = capture_context(&context)?;

    assert_eq!(snapshot.bytes("source.txt"), Some(b"approved".as_slice()));
    Ok(())
}

#[test]
fn git_subtree_swap_after_root_open_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let repo = root.join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    commit(&repo)?;
    let displaced = root.join("git-a");
    let replacement = root.join("git-b");
    std::fs::create_dir(&replacement)?;
    std::fs::write(replacement.join("HEAD"), b"ref: refs/heads/forged\n")?;
    let git = super::super::tools::ResolvedTool::git()?;

    let result = SourceRootContext::with_git_hook_for_test(&repo, git, || {
        assert!(std::fs::rename(repo.join(".git"), &displaced).is_ok());
        assert!(std::fs::rename(&replacement, repo.join(".git")).is_ok());
    });

    assert!(result.is_err());
    Ok(())
}

#[test]
fn late_worktree_config_creation_breaks_source_seal() -> Result<(), Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    std::fs::write(repo.path().join("source.txt"), b"approved")?;
    commit(repo.path())?;
    let seal = super::super::source_git_config::GitConfigSeal::capture(repo.path())?;

    std::fs::write(repo.path().join(".git/config.worktree"), b"[core]\n")?;

    assert!(matches!(seal.verify(), Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}

#[test]
fn absent_git_config_parent_a_b_a_breaks_source_seal() -> Result<(), Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    std::fs::write(repo.path().join("source.txt"), b"approved")?;
    commit(repo.path())?;
    let seal = super::super::source_git_config::GitConfigSeal::capture(repo.path())?;
    let marker = repo.path().join(".git/marker");

    std::fs::write(&marker, b"mutation")?;
    std::fs::remove_file(&marker)?;

    assert!(matches!(
        seal.verify(),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}

#[test]
fn config_rename_after_read_cannot_establish_reopened_baseline(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let config = root.path().join("config");
    let displaced = root.path().join("config-approved");
    std::fs::write(&config, b"[core]\n")?;

    let result = super::super::source_git_config::read_seal_hook_for_test(&config, || {
        assert!(std::fs::rename(&config, &displaced).is_ok());
        assert!(std::fs::write(&config, b"[alias]\n").is_ok());
    });

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    Ok(())
}

#[test]
fn repository_parent_a_b_a_does_not_rebind_retained_descriptor(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let parent = root.path().join("parent");
    let repo = parent.join("repo");
    std::fs::create_dir_all(&repo)?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    commit(&repo)?;
    let repo = repo.canonicalize()?;
    let context = SourceRootContext::resolve(&repo)?;
    let marker = parent.join("marker");

    std::fs::write(&marker, b"mutation")?;
    std::fs::remove_file(&marker)?;

    context.verify()?;
    let snapshot = capture_context(&context)?;
    assert_eq!(snapshot.bytes("source.txt"), Some(b"approved".as_slice()));
    Ok(())
}

#[test]
fn git_replace_restore_after_enumeration_never_returns_manifest(
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let repo = root.join("repo");
    std::fs::create_dir(&repo)?;
    std::fs::write(repo.join("source.txt"), b"approved")?;
    commit(&repo)?;
    let git_path = root.join("git");
    let displaced = root.join("git-approved");
    std::fs::write(
        &git_path,
        b"#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'git version fixture'; exit 0; fi\nexec /usr/bin/git \"$@\"\n",
    )?;
    std::fs::set_permissions(&git_path, std::fs::Permissions::from_mode(0o700))?;
    let git = super::super::tools::ResolvedTool::resolve_for_test("git", vec![git_path.clone()])?;
    let controlled_git = git.path_for_test().to_path_buf();

    let result = capture_with_git_after_enumeration_for_test(&repo, git, || {
        assert!(std::fs::rename(&controlled_git, &displaced).is_ok());
        assert!(std::fs::copy(&displaced, &controlled_git).is_ok());
        assert!(std::fs::remove_file(&controlled_git).is_ok());
        assert!(std::fs::rename(&displaced, &controlled_git).is_ok());
    });

    assert!(result.is_err());
    Ok(())
}

fn commit(repo: &Path) -> Result<(), Box<dyn std::error::Error>> {
    git(repo, &["init", "--quiet"])?;
    git(repo, &["add", "."])?;
    git(
        repo,
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

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .status()?;
    status.success().then_some(()).ok_or_else(|| "git failed".into())
}
