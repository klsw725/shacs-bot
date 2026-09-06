use super::*;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const SUBSTITUTE_CLEANUP_DIGEST: &str =
    "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

#[test]
fn cleanup_binding_digest_substitution_before_manifest_capture_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: cleanup produced a valid proof, but its serialized digest will be substituted.
    let repo = test_repo()?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    test_hooks::inject_before_manifest_capture(substitute_cleanup_digest_file);

    // When: the runner captures a manifest over the substituted receipt.
    let result = run_with_publication_hooks(
        &config(repo.path(), &evidence, "spec034-cleanup-binding-pre-manifest"),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    // Then: typed cleanup proof mismatch prevents a validated artifact.
    assert!(result.is_err(), "{result:?}");
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn cleanup_binding_digest_substitution_after_validation_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: pending validation parsed the authentic cleanup receipt.
    let repo = test_repo()?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    test_hooks::inject_after_pending_validation(|receipt| {
        receipt.cleanup_binding_digest = SUBSTITUTE_CLEANUP_DIGEST.to_owned();
    });

    // When: the validated typed receipt is substituted before publication binding.
    let result = run_with_publication_hooks(
        &config(repo.path(), &evidence, "spec034-cleanup-binding-post-validation"),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    // Then: publication binding rejects it and no validated artifact exists.
    assert!(result.is_err(), "{result:?}");
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn source_mutation_at_final_hook_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = test_repo()?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let config = config(repo.path(), &evidence, "spec034-final-source-mutation");

    let result = run_with_publication_hooks(
        &config,
        validation::validate_pending_with_git,
        |_| {},
        |_| {
            std::fs::write(repo.path().join("source.txt"), b"changed")
                .expect("mutate source at final hook");
        },
    );

    assert!(result.is_err());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn fixture_mutation_at_final_hook_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = test_repo()?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let config = config(repo.path(), &evidence, "spec034-final-fixture-mutation");

    let result = run_with_publication_hooks(
        &config,
        validation::validate_pending_with_git,
        |_| {},
        |_| {
            std::fs::write(repo.path().join(catalog::FIXTURES[0]), b"changed fixture")
                .expect("mutate fixture at final hook");
        },
    );

    assert!(result.is_err());
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn cleanup_failure_after_commands_never_publishes() -> Result<(), Box<dyn std::error::Error>> {
    let repo = test_repo()?;
    let root = tempfile::tempdir()?;
    let evidence = root.path().canonicalize()?.join("release");
    let config = config(repo.path(), &evidence, "spec034-cleanup-failure");
    isolation::RunnerIsolation::inject_next_cleanup_failure();

    let result = run_with_publication_hooks(
        &config,
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::CleanupFailed(_))));
    assert!(!evidence.exists());
    Ok(())
}

#[test]
fn source_swap_between_adoption_and_command_never_invokes_executor(
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = test_repo()?;
    let source_root = source::SourceRootContext::resolve_release(repo.path())?;
    let mut adopted = source::capture_context(&source_root)?;
    for locator in catalog::FIXTURES {
        adopted.include(&source_root, locator)?;
    }
    std::fs::write(repo.path().join("source.txt"), b"transient")?;
    let calls = AtomicUsize::new(0);

    let result = run_after_source_preflight(&source_root, &adopted, || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    std::fs::write(repo.path().join("source.txt"), b"approved")?;

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

fn config(repo: &Path, evidence: &Path, run_id: &str) -> Spec034ReleaseConfig {
    Spec034ReleaseConfig {
        run_id: run_id.to_owned(),
        repo_root: repo.to_path_buf(),
        evidence_root: evidence.to_path_buf(),
        cache_root: evidence.parent().map(|parent| parent.join("cache")),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
    }
}

fn substitute_cleanup_digest_file(root: &Path) {
    let path = root.join("cleanup-receipt.json");
    let bytes = std::fs::read(&path).expect("read cleanup receipt");
    let mut receipt: CleanupReceipt = serde_json::from_slice(&bytes).expect("parse cleanup receipt");
    receipt.cleanup_binding_digest = SUBSTITUTE_CLEANUP_DIGEST.to_owned();
    let bytes = serde_json::to_vec_pretty(&receipt).expect("serialize cleanup receipt");
    std::fs::write(path, bytes).expect("substitute cleanup digest");
}

fn test_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let parent = workspace
        .canonicalize()?
        .parent()
        .ok_or("workspace parent")?
        .to_path_buf();
    let repo = tempfile::Builder::new()
        .prefix(".spec034-binding-test-")
        .tempdir_in(&parent)?;
    std::fs::write(repo.path().join("source.txt"), b"approved")?;
    for locator in catalog::FIXTURES {
        let path = repo.path().join(locator);
        std::fs::create_dir_all(path.parent().ok_or("fixture parent")?)?;
        std::fs::write(path, b"approved fixture")?;
    }
    git(repo.path(), &["init", "--quiet"])?;
    git(repo.path(), &["add", "."])?;
    git(
        repo.path(),
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
    )?;
    Ok(repo)
}

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("git").arg("-C").arg(repo).args(args).status()?;
    if !status.success() {
        return Err("git fixture command failed".into());
    }
    Ok(())
}
