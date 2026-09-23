use super::coverage::{Spec031CoverageStatus, Spec031ExternalOwnerId};
use super::coverage_validate::validate_coverage_matrix;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts, Spec031ReleaseRunId,
    Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode,
};

fn fixture() -> (tempfile::TempDir, Spec031ReleaseRunArtifacts) {
    let root = tempfile::tempdir().expect("owned fixture");
    let artifacts = super::runner::run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("spec035-catalog-gate").expect("run id"),
        evidence_root: root
            .path()
            .canonicalize()
            .expect("canonical fixture root")
            .join("run"),
        repo_root: repo_root(),
        mode: Spec031ReleaseRunnerMode::SuccessFixture,
        command_timeout: std::time::Duration::from_secs(30),
    })
    .expect("successful runner fixture");
    (root, artifacts)
}

#[test]
fn spec035_catalog_rejects_omission_even_when_all_new_rows_are_removed() {
    let (_root, mut artifacts) = fixture();
    artifacts
        .coverage_matrix
        .retain(|row| !row.requirement_id.starts_with("spec035:"));

    assert_eq!(
        validate_coverage_matrix(&artifacts, &repo_root()),
        Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)
    );
}

#[test]
fn spec035_catalog_blocked_rows_prevent_success_from_legacy_audit() {
    let (root, mut artifacts) = fixture();
    let audit = artifacts
        .external_audits
        .iter_mut()
        .find(|audit| audit.owner == Spec031ExternalOwnerId::Spec035)
        .expect("Spec035 audit");
    audit.implementation_artifacts = vec!["historical-v1-manifest.json".to_owned()];
    artifacts.coverage_matrix = super::coverage_matrix::coverage_entries(
        &root.path().join("run"),
        &repo_root(),
        Spec031CoverageStatus::Pass,
        &artifacts.command_registry,
        &artifacts.external_audits,
    )
    .expect("catalog rows");

    assert_eq!(
        validate_coverage_matrix(&artifacts, &repo_root()),
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repo")
        .to_owned()
}
