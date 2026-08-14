#[path = "spec033_local_improvement_support.rs"]
mod support;

use sha2::{Digest, Sha256};
use shacs_core::runtime::{
    LocalArtifactOwner, LocalGateSource, LocalImprovementBlock, LocalImprovementRuntime,
    LocalImprovementService, LocalImprovementStore,
};
use std::fs;
use std::process::Command;
use std::sync::Arc;
use support::{proposal, write_snapshot, Gates, Verifier};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[test]
fn process_apply_worker() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(root) = std::env::var("SHACS_PRD003_PROCESS_ROOT") else {
        return Ok(());
    };
    let result_path = std::env::var("SHACS_PRD003_PROCESS_RESULT")?;
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(
            std::path::Path::new(&root).join("store.json"),
        )?),
        Arc::new(LocalArtifactOwner::new(&root)?),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    let outcome = if runtime.apply("proposal:process-race").is_ok() {
        "applied"
    } else {
        "blocked"
    };
    fs::write(result_path, outcome)?;
    Ok(())
}

#[test]
fn independent_processes_cannot_both_apply_expected_digest(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    fs::write(&target, br#"{"enabled":false}"#)?;
    write_snapshot(&snapshot)?;
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(root.path().join("store.json"))?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    runtime.propose(proposal(
        "proposal:process-race",
        &digest(&fs::read(&target)?),
        &snapshot,
    )?)?;
    let executable = std::env::current_exe()?;
    let results = [
        root.path().join("first.result"),
        root.path().join("second.result"),
    ];
    let mut children = results
        .iter()
        .map(|result| {
            Command::new(&executable)
                .args(["--exact", "process_apply_worker", "--nocapture"])
                .env("SHACS_PRD003_PROCESS_ROOT", root.path())
                .env("SHACS_PRD003_PROCESS_RESULT", result)
                .spawn()
        })
        .collect::<Result<Vec<_>, _>>()?;
    for child in &mut children {
        assert!(child.wait()?.success());
    }
    let outcomes = results
        .iter()
        .map(fs::read_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| *outcome == "applied")
            .count(),
        1
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(target)?)?,
        serde_json::json!({"enabled": true})
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn preplanted_staging_symlink_does_not_modify_external_file(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let target = root.path().join("settings.json");
    let snapshot = root.path().join("snapshot.json");
    let victim = root.path().join("external.json");
    fs::write(&target, br#"{"enabled":false}"#)?;
    fs::write(&victim, b"external sentinel")?;
    write_snapshot(&snapshot)?;
    let runtime = LocalImprovementRuntime::new(
        Arc::new(LocalImprovementStore::open(root.path().join("store.json"))?),
        Arc::new(LocalArtifactOwner::new(root.path())?),
        Arc::new(Gates::new()),
        Arc::new(Verifier {
            passes: true.into(),
        }),
    );
    runtime.propose(proposal(
        "proposal:staging-symlink",
        &digest(&fs::read(&target)?),
        &snapshot,
    )?)?;
    std::os::unix::fs::symlink(
        &victim,
        root.path()
            .join(".shacs-self-improvement/replacement.stage"),
    )?;

    // When
    let _ = runtime.apply("proposal:staging-symlink");

    // Then
    assert_eq!(fs::read(&victim)?, b"external sentinel");
    Ok(())
}

#[cfg(unix)]
#[test]
fn state_directory_symlink_is_rejected_before_external_writes(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let external = tempfile::tempdir()?;
    std::os::unix::fs::symlink(external.path(), root.path().join(".shacs-self-improvement"))?;
    let gates: Arc<dyn LocalGateSource> = Arc::new(Gates::new());

    // When
    let result = LocalImprovementService::open(root.path(), gates);

    // Then
    assert!(matches!(result, Err(LocalImprovementBlock::UnsafeTarget)));
    assert!(fs::read_dir(external.path())?.next().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn state_store_symlink_is_rejected_before_external_writes() -> Result<(), Box<dyn std::error::Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let external = tempfile::NamedTempFile::new()?;
    fs::create_dir(root.path().join(".shacs-self-improvement"))?;
    std::os::unix::fs::symlink(
        external.path(),
        root.path().join(".shacs-self-improvement/store.json"),
    )?;
    let gates: Arc<dyn LocalGateSource> = Arc::new(Gates::new());

    // When
    let result = LocalImprovementService::open(root.path(), gates);

    // Then
    assert!(matches!(result, Err(LocalImprovementBlock::UnsafeTarget)));
    assert!(fs::read(external.path())?.is_empty());
    Ok(())
}
