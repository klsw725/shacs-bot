use shacs_core::runtime::{
    run_spec034_release_runner, validate_spec034_release_artifacts_against, Spec034ReleaseConfig,
    Spec034ReleaseMode,
};
use std::error::Error;
use std::path::Path;
use std::time::Duration;

#[test]
fn runner_publishes_runner_only_source_bound_evidence() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let root_path = root.path().canonicalize()?;
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let evidence = root_path.join("release");
    let config = Spec034ReleaseConfig {
        run_id: "spec034-success-fixture".to_owned(),
        repo_root: repo.clone(),
        evidence_root: evidence.clone(),
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    };

    // When
    let manifest = run_spec034_release_runner(&config)?;

    // Then
    assert_eq!(manifest.run_id, config.run_id);
    assert_eq!(manifest.requirement_count, 22);
    assert_eq!(manifest.blocker_count, 8);
    assert!(manifest.runner_passed);
    assert!(manifest.runner_only);
    assert!(!manifest.closure_eligible);
    assert!(!manifest.source.files.is_empty());
    assert_eq!(manifest.fixture_digests.len(), 2);
    for locator in [
        "manifest.json",
        "results.json",
        "coverage-matrix.json",
        "failure-triage.json",
        "reproducibility-observations.json",
        "review-records.json",
        "owner-audits.json",
        "cleanup-receipt.json",
        "summary.json",
    ] {
        assert!(evidence.join(locator).is_file(), "missing {locator}");
    }
    validate_spec034_release_artifacts_against(&evidence, &repo)?;

    // When / Then: every semantic mutation is rejected even after rebinding its artifact digest.
    assert_rejects_json_mutation(&evidence, &repo, "coverage-matrix.json", |value| {
        value["requirements"].as_array_mut().map(Vec::pop);
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "coverage-matrix.json", |value| {
        let first = value["requirements"][0].clone();
        if let Some(rows) = value["requirements"].as_array_mut() {
            rows.push(first);
        }
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "coverage-matrix.json", |value| {
        value["requirements"][0]["requirement_id"] = serde_json::json!("034-MH999");
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "coverage-matrix.json", |value| {
        value["requirements"][0]["evidence"]["locator"] = serde_json::json!("../outside");
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "review-records.json", |value| {
        value["records"].as_array_mut().map(Vec::pop);
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "review-records.json", |value| {
        value["records"][0]["kind"] = serde_json::json!("forged");
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "cleanup-receipt.json", |value| {
        value["raw_evidence_cleaned"] = serde_json::json!(false);
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "summary.json", |value| {
        value["non_guarantees"].as_array_mut().map(Vec::pop);
    })?;
    assert_rejects_json_mutation(&evidence, &repo, "results.json", |value| {
        value["closure_eligible"] = serde_json::json!(true);
    })?;
    assert_rejects_file_mutation(&evidence, &repo, "spec034-schema-contract.stdout")?;

    let fixture_path = repo.join(&manifest.fixture_digests[0].locator);
    let fixture_bytes = std::fs::read(&fixture_path)?;
    std::fs::write(&fixture_path, b"fixture hash tamper")?;
    let fixture_result = validate_spec034_release_artifacts_against(&evidence, &repo);
    std::fs::write(&fixture_path, fixture_bytes)?;
    assert!(fixture_result.is_err());

    let source_path = repo.join(&manifest.source.files[0].locator);
    let source_bytes = std::fs::read(&source_path)?;
    std::fs::write(&source_path, b"source mutation after digest")?;
    let source_result = validate_spec034_release_artifacts_against(&evidence, &repo);
    std::fs::write(&source_path, source_bytes)?;
    assert!(source_result.is_err());
    Ok(())
}

fn assert_rejects_file_mutation(
    evidence: &Path,
    repo: &Path,
    locator: &str,
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let copy = root.path().join("release");
    copy_tree(evidence, &copy)?;
    std::fs::write(copy.join(locator), b"tampered command evidence")?;
    rebind_artifact_digest(&copy, locator)?;
    assert!(validate_spec034_release_artifacts_against(&copy, repo).is_err());
    Ok(())
}

fn assert_rejects_json_mutation(
    evidence: &Path,
    repo: &Path,
    locator: &str,
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let copy = root.path().join("release");
    copy_tree(evidence, &copy)?;
    let path = copy.join(locator);
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    mutate(&mut value);
    std::fs::write(&path, serde_json::to_vec_pretty(&value)?)?;
    rebind_artifact_digest(&copy, locator)?;
    assert!(validate_spec034_release_artifacts_against(&copy, repo).is_err());
    Ok(())
}

fn rebind_artifact_digest(root: &Path, locator: &str) -> Result<(), Box<dyn Error>> {
    use sha2::{Digest, Sha256};
    let manifest_path = root.join("manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    let digest = format!(
        "sha256:{:x}",
        Sha256::digest(std::fs::read(root.join(locator))?)
    );
    let row = manifest["artifact_digests"]
        .as_array_mut()
        .and_then(|rows| rows.iter_mut().find(|row| row["locator"] == locator))
        .ok_or("artifact digest row missing")?;
    row["digest"] = serde_json::json!(digest);
    std::fs::write(manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn validation_rejects_symlinked_artifact() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let release = root.path().join("release");
    std::fs::create_dir(&release)?;
    std::fs::write(root.path().join("outside.json"), b"{}")?;
    std::os::unix::fs::symlink(
        root.path().join("outside.json"),
        release.join("manifest.json"),
    )?;

    // When
    let result = validate_spec034_release_artifacts_against(&release, Path::new("."));

    // Then
    assert!(result.is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn runner_rejects_intermediate_ancestor_symlink_without_publication() -> Result<(), Box<dyn Error>>
{
    // Given
    let root = tempfile::tempdir()?;
    let root = root.path().canonicalize()?;
    let real = root.join("nested-real");
    let linked = root.join("nested-link");
    std::fs::create_dir(&real)?;
    std::os::unix::fs::symlink(&real, &linked)?;
    let evidence = linked.join("child/evidence");
    let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    // When
    let result = run_spec034_release_runner(&Spec034ReleaseConfig {
        run_id: "spec034-intermediate-symlink".to_owned(),
        repo_root: repo,
        evidence_root: evidence,
        mode: Spec034ReleaseMode::SuccessFixture,
        command_timeout: Duration::from_secs(600),
    });

    // Then
    assert!(result.is_err());
    assert!(!real.join("child/evidence").exists());
    Ok(())
}
