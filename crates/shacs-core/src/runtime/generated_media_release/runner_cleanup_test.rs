use super::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

#[test]
fn renamed_isolation_root_with_replacement_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: cleanup will see retained A renamed away and same-owner B at A's name.
    let repo = test_repo()?;
    let output = tempfile::tempdir()?;
    let evidence = output.path().canonicalize()?.join("release");
    let paths = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&paths);
    isolation::RunnerIsolation::inject_next_cleanup_hook(move |root| {
        let displaced = root.with_extension("displaced-a");
        std::fs::rename(root, &displaced).expect("displace retained root");
        std::fs::create_dir(root).expect("install replacement root");
        std::fs::write(root.join("replacement"), b"B").expect("write replacement sentinel");
        recorded.borrow_mut().extend([root.to_path_buf(), displaced]);
    });

    // When: the runner reaches cleanup before final validation.
    let result = run_with_publication_hooks(
        &config(repo.path(), &evidence, "spec034-cleanup-root-replaced"),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    // Then: cleanup fails, B remains, and no validated destination exists.
    let replacement_preserved = paths.borrow().first().is_some_and(|root| {
        std::fs::read(root.join("replacement")).is_ok_and(|bytes| bytes == b"B")
    });
    cleanup_paths(&paths.borrow())?;
    assert_cleanup_blocked(result, &evidence, replacement_preserved);
    Ok(())
}

#[test]
#[cfg(target_vendor = "apple")]
fn isolation_root_a_b_a_swap_never_publishes() -> Result<(), Box<dyn std::error::Error>> {
    // Given: cleanup will observe A displaced and restored after B temporarily occupies its name.
    let repo = test_repo()?;
    let output = tempfile::tempdir()?;
    let evidence = output.path().canonicalize()?.join("release");
    let paths = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&paths);
    isolation::RunnerIsolation::inject_next_cleanup_hook(move |root| {
        let displaced = root.with_extension("displaced-a");
        let replacement = root.with_extension("replacement-b");
        std::fs::create_dir(&replacement).expect("create replacement root");
        std::fs::write(replacement.join("sentinel"), b"B").expect("write replacement sentinel");
        std::fs::rename(root, &displaced).expect("displace retained root");
        std::fs::rename(&replacement, root).expect("install replacement root");
        std::fs::rename(root, &replacement).expect("restore replacement root");
        std::fs::rename(&displaced, root).expect("restore retained root");
        recorded.borrow_mut().extend([root.to_path_buf(), replacement]);
    });

    // When: the runner reaches cleanup before final validation.
    let result = run_with_publication_hooks(
        &config(repo.path(), &evidence, "spec034-cleanup-root-aba"),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    // Then: vnode rename history blocks proof, preserves B, and prevents publication.
    let replacement_preserved = paths.borrow().get(1).is_some_and(|root| {
        std::fs::read(root.join("sentinel")).is_ok_and(|bytes| bytes == b"B")
    });
    cleanup_paths(&paths.borrow())?;
    assert_cleanup_blocked(result, &evidence, replacement_preserved);
    Ok(())
}

#[test]
fn uncertain_cleanup_event_history_never_publishes() -> Result<(), Box<dyn std::error::Error>> {
    // Given: the next isolation cleanup cannot establish complete vnode event history.
    let repo = test_repo()?;
    let output = tempfile::tempdir()?;
    let evidence = output.path().canonicalize()?.join("release");
    let paths = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&paths);
    isolation::RunnerIsolation::inject_next_cleanup_hook(move |root| {
        recorded.borrow_mut().push(root.to_path_buf());
    });
    isolation::RunnerIsolation::inject_next_monitor_uncertainty();

    // When: the runner reaches cleanup before final validation.
    let result = run_with_publication_hooks(
        &config(repo.path(), &evidence, "spec034-cleanup-event-uncertain"),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    // Then: uncertainty reports one bounded leak and publication remains absent.
    let root_retained = paths.borrow().first().is_some_and(|root| root.is_dir());
    cleanup_paths(&paths.borrow())?;
    assert_cleanup_blocked(result, &evidence, root_retained);
    Ok(())
}

#[test]
fn pre_unlink_root_replacement_never_publishes() -> Result<(), Box<dyn std::error::Error>> {
    // Given: the runner reaches final root verification before A is displaced and B is installed.
    let repo = test_repo()?;
    let output = tempfile::tempdir()?;
    let evidence = output.path().canonicalize()?.join("release");
    let paths = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&paths);
    isolation::RunnerIsolation::inject_next_pre_unlink_hook(move |root| {
        let displaced = root.with_extension("late-displaced-a");
        std::fs::rename(root, &displaced).expect("displace verified root");
        std::fs::create_dir(root).expect("install empty replacement root");
        recorded.borrow_mut().extend([root.to_path_buf(), displaced]);
    });

    // When: cleanup reaches its final pathname identity check.
    let result = run_with_publication_hooks(
        &config(repo.path(), &evidence, "spec034-cleanup-pre-unlink-race"),
        validation::validate_pending_with_git,
        |_| {},
        |_| {},
    );

    // Then: replacement B survives, retained A is reported, and publication remains absent.
    let replacement_preserved = paths.borrow().first().is_some_and(|root| root.is_dir());
    let original_retained = paths.borrow().get(1).is_some_and(|root| root.is_dir());
    cleanup_paths(&paths.borrow())?;
    assert_cleanup_blocked(result, &evidence, original_retained);
    assert!(replacement_preserved);
    Ok(())
}

fn assert_cleanup_blocked(
    result: Result<super::super::CommittedPublicationResult, Spec034ReleaseArtifactError>,
    evidence: &Path,
    retained: bool,
) {
    assert!(
        matches!(result, Err(Spec034ReleaseArtifactError::CleanupResidual { leak_count: 1 })),
        "{result:?}"
    );
    assert!(retained);
    assert!(!evidence.exists());
    assert!(!evidence.join("publication-status.json").exists());
}

fn cleanup_paths(paths: &[PathBuf]) -> Result<(), std::io::Error> {
    for path in paths {
        if path.is_dir() {
            restore_test_permissions(path)?;
            std::fs::remove_dir_all(path)?;
        }
    }
    Ok(())
}

fn restore_test_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            restore_test_permissions(&entry.path())?;
        }
    }
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

fn test_repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let parent = workspace.canonicalize()?.parent().ok_or("workspace parent")?.to_path_buf();
    let repo = tempfile::Builder::new().prefix(".spec034-cleanup-test-").tempdir_in(parent)?;
    std::fs::write(repo.path().join("source.txt"), b"approved")?;
    for locator in catalog::FIXTURES {
        let path = repo.path().join(locator);
        std::fs::create_dir_all(path.parent().ok_or("fixture parent")?)?;
        std::fs::write(path, b"approved fixture")?;
    }
    git(repo.path(), &["init", "--quiet"])?;
    git(repo.path(), &["add", "."])?;
    git(repo.path(), &["-c", "user.name=Spec034 Test", "-c", "user.email=spec034@example.invalid", "commit", "--quiet", "-m", "fixture"])?;
    Ok(repo)
}

fn git(repo: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    if !Command::new("git").arg("-C").arg(repo).args(args).status()?.success() {
        return Err("git fixture command failed".into());
    }
    Ok(())
}
