use super::*;
use std::process::Command;

#[cfg(unix)]
#[test]
fn cargo_clean_preserves_sealed_external_cache() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir()?;
    let canonical = root.path().canonicalize()?;
    let repo = canonical.join("repo");
    let workspace = repo.join("crates");
    let cache = canonical.join("external-cache");
    std::fs::create_dir_all(workspace.join("target/debug"))?;
    std::fs::write(
        workspace.join("Cargo.toml"),
        b"[workspace]\nresolver = \"2\"\nmembers = []\n",
    )?;
    assert!(matches!(
        RunnerIsolation::prepare(
            &repo,
            &canonical.join("target-evidence"),
            Some(&workspace.join("target/cache")),
        ),
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    ));
    std::fs::create_dir(&cache)?;
    let linked_cache = canonical.join("linked-cache");
    std::os::unix::fs::symlink(&cache, &linked_cache)?;
    assert!(matches!(
        RunnerIsolation::prepare(
            &repo,
            &canonical.join("linked-evidence"),
            Some(&linked_cache),
        ),
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    ));
    let isolation = RunnerIsolation::prepare(&repo, &canonical.join("evidence"), Some(&cache))?;
    let cache_root = isolation
        .cache_tools()
        .ancestors()
        .nth(4)
        .ok_or("cache root missing")?
        .to_path_buf();
    let marker = cache_root.join("sealed-marker");
    std::fs::write(&marker, b"immutable-cache")?;
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o400))?;
    std::fs::set_permissions(&cache_root, std::fs::Permissions::from_mode(0o500))?;

    let output = Command::new("cargo")
        .args(["clean", "--manifest-path"])
        .arg(workspace.join("Cargo.toml"))
        .output();

    std::fs::set_permissions(&cache_root, std::fs::Permissions::from_mode(0o700))?;
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600))?;
    let output = output?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(std::fs::read(marker)?, b"immutable-cache");
    isolation.cleanup()?;
    Ok(())
}
