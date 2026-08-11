use super::model::{Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts};
use crate::release_evidence::EvidenceWriter;
use serde::Serialize;
use std::path::Path;

pub fn write_spec031_release_artifacts(
    artifacts: &Spec031ReleaseRunArtifacts,
    evidence_root: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    let writer = EvidenceWriter::open_existing(evidence_root)
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    write_spec031_release_artifacts_with(&writer, artifacts)
}

pub(super) fn write_json(
    writer: &EvidenceWriter,
    path: impl AsRef<Path>,
    value: &impl Serialize,
) -> Result<(), Spec031ReleaseArtifactError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| Spec031ReleaseArtifactError::Io)?;
    writer
        .write_new(path, &bytes)
        .map_err(|_| Spec031ReleaseArtifactError::Io)
}

pub(super) fn write_text(
    writer: &EvidenceWriter,
    path: impl AsRef<Path>,
    text: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    writer
        .write_new(path, text.as_bytes())
        .map_err(|_| Spec031ReleaseArtifactError::Io)
}

pub(super) fn write_spec031_release_artifacts_with(
    writer: &EvidenceWriter,
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    write_json(writer, "manifest.json", artifacts)?;
    write_json(writer, "coverage-matrix.json", &artifacts.coverage_matrix)?;
    write_json(writer, "results.json", &artifacts.command_registry)?;
    write_json(writer, "failure-triage.json", &artifacts.failure_triage)?;
    write_json(writer, "fixture-registry.json", &artifacts.fixture_registry)?;
    write_json(writer, "command-registry.json", &artifacts.command_registry)?;
    write_json(writer, "cleanup-registry.json", &artifacts.cleanup_registry)?;
    write_text(writer, "summary.md", &render_summary(artifacts))
}

fn render_summary(artifacts: &Spec031ReleaseRunArtifacts) -> String {
    let status = if artifacts.failure_triage.is_empty() {
        "PASS"
    } else {
        "BLOCKED"
    };
    let mut summary = format!(
        "# Spec031 Release Runner Summary\n\n- schema: {}\n- run_id: {}\n- status: {}\n- commands: {}\n- cleanup receipts: {}\n- failures: {}\n",
        artifacts.schema,
        artifacts.run_id.as_str(),
        status,
        artifacts.command_registry.len(),
        artifacts.cleanup_registry.len(),
        artifacts.failure_triage.join("; ")
    );
    summary.push_str("\n## Commands\n");
    for command in &artifacts.command_registry {
        summary.push_str(&format!(
            "\n### {}\n- gate: {:?}\n- package: {}\n- filter: {}\n- cwd: {}\n- argv: {}\n- status: {:?}\n- exit_code: {:?}\n- tests_run: {}\n- tests_failed: {}\n- stdout: {}\n- stderr: {}\n",
            command.id,
            command.gate,
            command.package.as_deref().unwrap_or("<workspace>"),
            command.filter.as_deref().unwrap_or("<none>"),
            command.cwd,
            command.argv.join(" "),
            command.status,
            command.exit_code,
            command.tests.as_ref().map_or(0, |tests| tests.tests_run),
            command.tests.as_ref().map_or(0, |tests| tests.tests_failed),
            command.stdout_path,
            command.stderr_path
        ));
    }
    summary.push_str("\n## Cleanup Receipts\n");
    for receipt in &artifacts.cleanup_registry {
        summary.push_str(&format!("- {receipt}\n"));
    }
    summary.push_str("\n## Failure Triage\n");
    for triage in &artifacts.failure_triage {
        summary.push_str(&format!("- {triage}\n"));
    }
    summary.push_str(&format!(
        "\n## Coverage\n- rows: {}\n- pass: {}\n- blocked: {}\n",
        artifacts.coverage_matrix.len(),
        artifacts
            .coverage_matrix
            .iter()
            .filter(|entry| entry.status == super::coverage::Spec031CoverageStatus::Pass)
            .count(),
        artifacts
            .coverage_matrix
            .iter()
            .filter(|entry| entry.status == super::coverage::Spec031CoverageStatus::Blocked)
            .count()
    ));
    summary.push_str("\n## External Audits\n");
    for audit in &artifacts.external_audits {
        summary.push_str(&format!(
            "- owner: {:?}; status: {:?}; artifact: {}; source_status_locator: {}; reason: {}\n",
            audit.owner, audit.status, audit.artifact, audit.source_status_locator, audit.reason
        ));
    }
    summary
}
