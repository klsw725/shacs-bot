use shacs_core::runtime::{
    audit_spec034_release_artifacts_against, audit_spec034_release_artifacts_against_expected,
    run_spec034_release_runner, Spec034ReleaseConfig, Spec034ReleaseMode,
};
use std::error::Error;
use std::path::Path;
use std::time::Duration;

#[path = "spec034_release_runner_security/mod.rs"]
mod security;
#[path = "spec034_release_runner_security/support.rs"]
mod support;
#[cfg(unix)]
#[path = "spec034_release_runner/symlink.rs"]
mod symlink;

#[test]
fn runner_publishes_runner_only_source_bound_evidence() -> Result<(), Box<dyn Error>> {
    // Given
    let baseline = support::release_evidence()?;
    let evidence = &baseline.evidence;
    let repo = &baseline.repo;
    let publication = &baseline.publication;
    let root_path = evidence.parent().ok_or("release evidence parent missing")?;

    // When
    let manifest = &publication.manifest;

    // Then
    assert_eq!(support::baseline_generation_count(), 1);
    assert_eq!(manifest.run_id, "spec034-success-fixture");
    assert_eq!(manifest.requirement_count, 22);
    assert_eq!(manifest.blocker_count, 8);
    assert!(manifest.runner_passed);
    assert!(manifest.runner_only);
    assert!(!manifest.closure_eligible);
    assert_eq!(manifest.repo_root, ".");
    assert_eq!(manifest.source.repo_root, ".");
    assert!(!manifest.source.files.is_empty());
    assert_eq!(manifest.fixture_digests.len(), 2);
    let results: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.join("results.json"))?)?;
    for command in results["commands"].as_array().ok_or("commands missing")? {
        assert_eq!(command["command"]["cwd"], ".");
        assert_eq!(command["source_digest"], manifest.source.digest);
        assert_eq!(command["tool"].as_object().ok_or("tool")?.len(), 3);
        assert!(command["tool"].get("path").is_none());
        assert!(command["command"].get("duration_ms").is_none());
        assert!(command["command"].get("process_receipt").is_none());
        assert!(command["portable_process_receipt"]
            .get("stdout_temp_locator")
            .is_none());
        assert!(command["portable_process_receipt"]
            .get("stderr_temp_locator")
            .is_none());
        assert_eq!(
            command["portable_process_receipt"],
            serde_json::json!({
                "reaped": true,
                "temp_paths_published": true
            })
        );
        let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(
            evidence.join(
                command["command"]["stdout_path"]
                    .as_str()
                    .ok_or("stdout path")?,
            ),
        )?)?;
        assert_eq!(summary.as_object().ok_or("summary")?.len(), 3);
    }
    let serialized_results = serde_json::to_string(&results)?;
    for forbidden in [
        "pid",
        "duration_ms",
        "process_receipt",
        "stdout_temp_locator",
        "stderr_temp_locator",
    ] {
        assert!(!contains_key(&results, forbidden));
    }
    assert!(!serialized_results.contains(".tmp."));
    assert!(!serialized_results.contains(root_path.to_string_lossy().as_ref()));
    for locator in [
        "manifest.json",
        "results.json",
        "coverage-matrix.json",
        "failure-triage.json",
        "reproducibility-observations.json",
        "review-records.json",
        "owner-audits.json",
        "cleanup-receipt.json",
        "publication-status.json",
        "summary.json",
    ] {
        assert!(evidence.join(locator).is_file(), "missing {locator}");
    }
    let final_manifest =
        audit_spec034_release_artifacts_against_expected(evidence, repo, publication)?;
    assert_eq!(&final_manifest.manifest, manifest);
    assert_eq!(
        final_manifest.content_digest,
        publication.identity.content_digest
    );
    assert_eq!(
        final_manifest.content_digest,
        publication.identity.content_digest
    );
    let mut wrong_expected = publication.clone();
    wrong_expected.identity.content_digest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned();
    assert!(
        audit_spec034_release_artifacts_against_expected(evidence, repo, &wrong_expected).is_err()
    );
    assert!(!final_manifest.execution_attested);
    assert!(final_manifest.structural_only);

    // When / Then: every semantic mutation is rejected even after rebinding its artifact digest.
    assert_rejects_json_mutation(evidence, repo, "coverage-matrix.json", |value| {
        value["requirements"].as_array_mut().map(Vec::pop);
    })?;
    assert_rejects_json_mutation(evidence, repo, "coverage-matrix.json", |value| {
        let first = value["requirements"][0].clone();
        if let Some(rows) = value["requirements"].as_array_mut() {
            rows.push(first);
        }
    })?;
    assert_rejects_json_mutation(evidence, repo, "coverage-matrix.json", |value| {
        value["requirements"][0]["requirement_id"] = serde_json::json!("034-MH999");
    })?;
    assert_rejects_json_mutation(evidence, repo, "coverage-matrix.json", |value| {
        value["requirements"][0]["evidence"]["locator"] = serde_json::json!("../outside");
    })?;
    assert_rejects_json_mutation(evidence, repo, "review-records.json", |value| {
        value["records"].as_array_mut().map(Vec::pop);
    })?;
    assert_rejects_json_mutation(evidence, repo, "review-records.json", |value| {
        value["records"][0]["kind"] = serde_json::json!("forged");
    })?;
    assert_rejects_json_mutation(evidence, repo, "cleanup-receipt.json", |value| {
        value["raw_evidence_cleaned"] = serde_json::json!(false);
    })?;
    assert_rejects_json_mutation(evidence, repo, "cleanup-receipt.json", |value| {
        value["leak_count"] = serde_json::json!(1);
    })?;
    assert_rejects_json_mutation(evidence, repo, "cleanup-receipt.json", |value| {
        value["cleanup_binding_digest"] = serde_json::json!("sha256:forged");
    })?;
    assert_rejects_json_mutation(evidence, repo, "summary.json", |value| {
        value["non_guarantees"].as_array_mut().map(Vec::pop);
    })?;
    assert_rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["closure_eligible"] = serde_json::json!(true);
    })?;
    assert_rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["command"]["cwd"] = serde_json::json!(repo.display().to_string());
    })?;
    assert_rejects_json_mutation(evidence, repo, "results.json", |value| {
        value["commands"][0]["command"]["stdout_path"] = serde_json::json!("forged.stdout");
    })?;
    assert_rejects_file_mutation(evidence, repo, "spec034-schema-contract.stdout")?;

    Ok(())
}

fn contains_key(value: &serde_json::Value, key: &str) -> bool {
    match value {
        serde_json::Value::Object(object) => {
            object.contains_key(key) || object.values().any(|value| contains_key(value, key))
        }
        serde_json::Value::Array(values) => values.iter().any(|value| contains_key(value, key)),
        _ => false,
    }
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
    assert!(audit_spec034_release_artifacts_against(&copy, repo).is_err());
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
    assert!(audit_spec034_release_artifacts_against(&copy, repo).is_err());
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
