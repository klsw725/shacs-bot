use super::coverage::Spec031CoverageStatus;
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
fn spec031_archive_row_rejects_omission() {
    let (_root, mut artifacts) = fixture();
    artifacts
        .coverage_matrix
        .retain(|row| row.requirement_id != "spec031:archive:spec035");

    assert_eq!(
        validate_coverage_matrix(&artifacts, &repo_root()),
        Err(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)
    );
}

#[test]
fn spec031_archive_row_cannot_claim_pass_without_command_evidence() {
    let (_root, mut artifacts) = fixture();
    let row = artifacts
        .coverage_matrix
        .iter_mut()
        .find(|row| row.requirement_id == "spec031:archive:spec035")
        .expect("archive row");
    row.status = Spec031CoverageStatus::Blocked;
    row.command_result_id = None;

    assert_eq!(
        validate_coverage_matrix(&artifacts, &repo_root()),
        Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence)
    );
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repo")
        .to_owned()
}
