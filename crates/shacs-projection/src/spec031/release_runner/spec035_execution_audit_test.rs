use super::coverage::{Spec031CoverageStatus, Spec031ExternalAuditStatus, Spec031ExternalOwnerId};
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
    Spec031ReleaseGateKind, Spec031ReleaseRunArtifacts, Spec031ReleaseRunId,
    Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode, Spec031ReleaseTestCounts,
    SPEC031_RELEASE_RUNNER_SCHEMA,
};
use super::spec035_execution::{admit, BINDING, MANIFEST};
use super::spec035_execution_fixture::Fixture;
use crate::release_evidence::EvidenceWriter;
use std::fs;

#[test]
fn spec035_execution_production_audit_and_coverage_use_current_proof_not_fixture_bypass() {
    let fixture = Fixture::new();
    let execution = admit(fixture.repo.path()).expect("constructed evidence validated");
    let root = fixture
        .repo
        .path()
        .canonicalize()
        .expect("canonical root")
        .join(".omo/runner");
    let writer = EvidenceWriter::open_new_run(&root).expect("owned output");
    super::writer::write_json(&writer, BINDING, &execution.binding).expect("immutable binding");
    let mut artifacts = Spec031ReleaseRunArtifacts {
        schema: SPEC031_RELEASE_RUNNER_SCHEMA.to_owned(),
        run_id: Spec031ReleaseRunId::try_new("constructed-audit").expect("run id"),
        evidence_root: root.display().to_string(),
        fixture_registry: vec!["fixtures/current-worktree.json".to_owned()],
        command_registry: vec![Spec031ReleaseCommandRecord {
            id: "spec031-owner-spec035".to_owned(),
            gate: Spec031ReleaseGateKind::FocusedCargoTest,
            package: Some("probe".to_owned()),
            filter: Some("evidence_probe".to_owned()),
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            cwd: fixture.repo.path().display().to_string(),
            status: Spec031ReleaseCommandStatus::Passed,
            exit_code: Some(0),
            duration_ms: 1,
            stdout_path: "test.stdout".to_owned(),
            stderr_path: "empty.stderr".to_owned(),
            tests: Some(Spec031ReleaseTestCounts {
                tests_run: 1,
                tests_failed: 0,
            }),
            process_receipt: None,
        }],
        cleanup_registry: vec![],
        manifest_files: vec![BINDING.to_owned()],
        coverage_matrix: vec![],
        external_audits: vec![],
        failure_triage: vec![],
        reproducibility_observations: vec![],
    };
    let config = Spec031ReleaseRunnerConfig {
        run_id: artifacts.run_id.clone(),
        evidence_root: root.clone(),
        repo_root: fixture.repo.path().to_owned(),
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: std::time::Duration::ZERO,
    };

    super::audit::add_external_audits(&config, &writer, &mut artifacts, false, None)
        .expect("production audit assembly");

    let audit = artifacts
        .external_audits
        .iter()
        .find(|audit| audit.owner == Spec031ExternalOwnerId::Spec035)
        .expect("audit");
    assert_eq!(audit.status, Spec031ExternalAuditStatus::Pass);
    assert_eq!(audit.implementation_artifacts, [MANIFEST]);
    super::coverage_audit_validate::validate_external_audits(&artifacts, fixture.repo.path())
        .expect("production audit validation");
    let rows = super::spec035_coverage::coverage_rows(
        &root,
        &artifacts.external_audits,
        fixture.repo.path(),
    )
    .expect("rows");
    assert_eq!(rows.len(), 80);
    assert!(rows
        .iter()
        .all(|row| row.status == Spec031CoverageStatus::Pass));

    fs::write(
        fixture.repo.path().join("crates/probe.rs"),
        "changed after admission\n",
    )
    .expect("source drift");
    assert_eq!(
        super::coverage_audit_validate::validate_external_audits(&artifacts, fixture.repo.path()),
        Err(Spec031ReleaseArtifactError::BlockedAsPass)
    );
    let rows = super::spec035_coverage::coverage_rows(
        &root,
        &artifacts.external_audits,
        fixture.repo.path(),
    )
    .expect("blocked rows");
    assert!(rows
        .iter()
        .all(|row| row.status == Spec031CoverageStatus::Blocked));
}
