use super::super::coverage::Spec031ExternalOwnerId;
use super::super::model::{
    Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus, Spec031ReleaseGateKind,
    Spec031ReleaseRunArtifacts, Spec031ReleaseRunId, Spec031ReleaseRunnerConfig,
    Spec031ReleaseRunnerMode, Spec031ReleaseTestCounts, SPEC031_RELEASE_RUNNER_SCHEMA,
};
use super::*;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn external_audit_remains_blocked_when_source_status_is_open() {
    let root = temp_path("source-open-external-audit");
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let spec = external_owner_facts()
        .iter()
        .find(|descriptor| descriptor.slug == "spec030")
        .expect("spec030 descriptor exists");
    write_file(
        repo.join(spec.source_locator),
        "# Spec030\n\nStatus: Open\n\nclosure remains external\n",
    );
    write_file(
        repo.join(".omo/evidence/spec030/closure-owner-facts.json"),
        "{\"approval_policy_redaction_containment\":true}\n",
    );
    let writer = EvidenceWriter::open_new_run(&evidence).expect("evidence writer opens");
    let mut artifacts = Spec031ReleaseRunArtifacts {
        schema: SPEC031_RELEASE_RUNNER_SCHEMA.to_owned(),
        run_id: Spec031ReleaseRunId::try_new("source-open-external-audit").expect("safe run id"),
        evidence_root: evidence.display().to_string(),
        fixture_registry: Vec::new(),
        command_registry: vec![passed_command("spec031-test-projection-parity")],
        cleanup_registry: Vec::new(),
        manifest_files: Vec::new(),
        coverage_matrix: Vec::new(),
        external_audits: Vec::new(),
        failure_triage: Vec::new(),
    };
    let config = Spec031ReleaseRunnerConfig {
        run_id: artifacts.run_id.clone(),
        evidence_root: evidence,
        repo_root: repo,
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::from_secs(1),
    };

    add_external_audits(&config, &writer, &mut artifacts, false)
        .expect("external audits are generated");

    let spec030 = artifacts
        .external_audits
        .iter()
        .find(|row| row.owner == Spec031ExternalOwnerId::Spec030)
        .expect("spec030 audit exists");
    assert_eq!(spec030.status, Spec031ExternalAuditStatus::Blocked);
    assert!(spec030.command_result_ids.is_empty());
}

fn passed_command(id: &str) -> Spec031ReleaseCommandRecord {
    Spec031ReleaseCommandRecord {
        id: id.to_owned(),
        gate: Spec031ReleaseGateKind::FocusedCargoTest,
        package: Some("shacs-projection".to_owned()),
        filter: Some("spec031_release_runner".to_owned()),
        argv: vec!["cargo".to_owned(), "test".to_owned()],
        cwd: ".".to_owned(),
        status: Spec031ReleaseCommandStatus::Passed,
        exit_code: Some(0),
        duration_ms: 1,
        stdout_path: format!("commands/{id}.stdout"),
        stderr_path: format!("commands/{id}.stderr"),
        tests: Some(Spec031ReleaseTestCounts {
            tests_run: 1,
            tests_failed: 0,
        }),
    }
}

fn write_file(path: PathBuf, content: &str) {
    std::fs::create_dir_all(path.parent().expect("test path has parent"))
        .expect("test parent directory writes");
    std::fs::write(path, content).expect("test file writes");
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(format!(
            "shacs-spec031-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ))
}
