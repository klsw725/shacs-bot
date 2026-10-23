use shacs_projection::{
    run_spec031_release_runner, validate_spec031_release_artifacts,
    validate_spec031_release_artifacts_with_repo_root, Spec031ReleaseArtifactError,
    Spec031ReleaseRunArtifacts, Spec031ReleaseRunId, Spec031ReleaseRunnerConfig,
    Spec031ReleaseRunnerMode,
};
use std::collections::BTreeSet;
use std::fs;
use std::time::Duration;

#[test]
fn rejects_v2_artifact_when_legacy_binding_is_absent() -> Result<(), Box<dyn std::error::Error>> {
    let (_evidence, mut artifacts) = current_artifacts("v2-without-legacy-binding")?;
    artifacts.schema = "spec031.release_runner.v2".to_owned();

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("v2 is reserved for bound legacy artifacts");

    assert_eq!(error, Spec031ReleaseArtifactError::UnsupportedSchema);
    Ok(())
}

#[test]
fn rejects_v3_artifact_when_legacy_binding_is_present() -> Result<(), Box<dyn std::error::Error>> {
    let (_evidence, mut artifacts) = current_artifacts("v3-with-legacy-binding")?;
    artifacts
        .manifest_files
        .push("spec035-execution-binding.json".to_owned());

    let error = validate_spec031_release_artifacts(&artifacts)
        .expect_err("v3 cannot select the legacy validation contract");

    assert_eq!(error, Spec031ReleaseArtifactError::UnsupportedSchema);
    Ok(())
}

#[test]
fn v3_index_is_sixty_six_rows_five_owners_and_no_spec035_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_evidence, artifacts) = current_artifacts("v3-index-shape")?;
    let index: serde_json::Value = serde_json::from_slice(&fs::read(
        std::path::Path::new(&artifacts.evidence_root).join("evidence-index.json"),
    )?)?;
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(
        std::path::Path::new(&artifacts.evidence_root).join("archive-integrity.json"),
    )?)?;

    assert_eq!(artifacts.coverage_matrix.len(), 66);
    assert_eq!(artifacts.external_audits.len(), 5);
    assert!(index.get("spec035_catalog").is_none());
    assert_eq!(
        index["external_audits"].as_array().ok_or("owners")?.len(),
        5
    );
    assert_eq!(
        receipt["files"].as_array().ok_or("archive files")?.len(),
        12
    );
    assert_eq!(
        receipt["manifest"]["path"],
        "docs/archive/spec035/current/SHA256SUMS"
    );
    Ok(())
}

#[test]
fn v3_revalidation_rejects_tampered_archived_prd() -> Result<(), Box<dyn std::error::Error>> {
    let (_evidence, artifacts) = current_artifacts("v3-archive-tamper")?;
    let repo = copied_source_repo(&artifacts)?;
    validate_spec031_release_artifacts_with_repo_root(&artifacts, repo.path())?;
    fs::write(
        repo.path()
            .join("docs/archive/spec035/current/prds/009-goal-and-read-only-tasks-projection.md"),
        "tampered\n",
    )?;

    assert_eq!(
        validate_spec031_release_artifacts_with_repo_root(&artifacts, repo.path()),
        Err(Spec031ReleaseArtifactError::ArtifactMismatch)
    );
    Ok(())
}

#[test]
fn v3_revalidation_rejects_injected_spec035_catalog() -> Result<(), Box<dyn std::error::Error>> {
    let (_evidence, artifacts) = current_artifacts("v3-index-catalog")?;
    let path = std::path::Path::new(&artifacts.evidence_root).join("evidence-index.json");
    let mut index: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    index["spec035_catalog"] = serde_json::json!([{"id":"spec035:must:01"}]);
    fs::write(&path, serde_json::to_vec_pretty(&index)?)?;

    assert_eq!(
        validate_spec031_release_artifacts(&artifacts),
        Err(Spec031ReleaseArtifactError::ArtifactMismatch)
    );
    Ok(())
}

fn current_artifacts(
    run_id: &str,
) -> Result<(tempfile::TempDir, Spec031ReleaseRunArtifacts), Box<dyn std::error::Error>> {
    let evidence = tempfile::tempdir()?;
    let evidence_root = evidence.path().canonicalize()?.join("evidence");
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let artifacts = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new(run_id)?,
        evidence_root,
        repo_root,
        mode: Spec031ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
    })?;
    Ok((evidence, artifacts))
}

fn copied_source_repo(
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = tempfile::tempdir()?;
    let mut paths = BTreeSet::new();
    for row in &artifacts.coverage_matrix {
        paths.insert(
            row.source_locator
                .rsplit_once(':')
                .map_or(row.source_locator.as_str(), |(path, _)| path),
        );
    }
    for audit in &artifacts.external_audits {
        paths.insert(audit.source_locator.as_str());
        paths.insert(
            audit
                .source_status_locator
                .rsplit_once(':')
                .map_or(audit.source_status_locator.as_str(), |(path, _)| path),
        );
    }
    for relative in paths {
        copy_file(&source, target.path(), relative)?;
    }
    for relative in [
        "docs/archive/spec035/current/SHA256SUMS",
        "docs/archive/spec035/current/CLOSURE.md",
        "docs/archive/spec035/current/SPEC.md",
        "docs/archive/spec035/current/prds/000-shared-projection-model-and-vocabulary.md",
        "docs/archive/spec035/current/prds/001-surface-adapter-parity-cli-api-channel.md",
        "docs/archive/spec035/current/prds/002-approval-progress-and-recovery-parity.md",
        "docs/archive/spec035/current/prds/003-readiness-degraded-health-and-diagnostics.md",
        "docs/archive/spec035/current/prds/004-context-extension-app-and-media-projection.md",
        "docs/archive/spec035/current/prds/005-interactive-tui-repl-and-onboard-flows.md",
        "docs/archive/spec035/current/prds/006-reconnect-backpressure-and-drop-accounting.md",
        "docs/archive/spec035/current/prds/007-release-runner-and-spec035-closure.md",
        "docs/archive/spec035/current/prds/008-transport-capability-and-snapshot-first-reconnect.md",
        "docs/archive/spec035/current/prds/009-goal-and-read-only-tasks-projection.md",
    ] {
        copy_file(&source, target.path(), relative)?;
    }
    Ok(target)
}

fn copy_file(
    source_root: &std::path::Path,
    target_root: &std::path::Path,
    relative: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let target = target_root.join(relative);
    fs::create_dir_all(target.parent().ok_or("copied source parent")?)?;
    fs::copy(source_root.join(relative), target)?;
    Ok(())
}
