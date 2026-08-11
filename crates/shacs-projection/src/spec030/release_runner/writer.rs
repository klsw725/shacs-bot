use super::artifact_manifest::{build_spec030_artifact_manifest, ARTIFACT_MANIFEST_PATH};
use super::model::{
    Spec030ReleaseArtifactError, Spec030ReleaseRunArtifacts, Spec030ReleaseVerdict,
};
use crate::release_evidence::EvidenceWriter;
use serde::Serialize;
use std::path::Path;

pub(super) fn write_json(
    writer: &EvidenceWriter,
    path: impl AsRef<Path>,
    value: &impl Serialize,
) -> Result<(), Spec030ReleaseArtifactError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new(path, &bytes)
        .map_err(|_| Spec030ReleaseArtifactError::Io)
}

pub(super) fn write_artifacts(
    writer: &EvidenceWriter,
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    write_json(writer, "manifest.json", artifacts)?;
    write_json(writer, "source-manifest.json", &artifacts.source_manifest)?;
    write_json(writer, "coverage-matrix.json", &artifacts.coverage)?;
    write_json(writer, "owner-audits.json", &artifacts.owner_audits)?;
    write_json(writer, "facts.json", &artifacts.facts)?;
    write_json(writer, "surfaces.json", &artifacts.surfaces)?;
    write_json(writer, "surface-owner.json", &artifacts.surface_owner)?;
    write_json(
        writer,
        "surface-assertions.json",
        &artifacts.surface_assertions,
    )?;
    write_json(
        writer,
        "external-evidence.json",
        &artifacts.external_evidence,
    )?;
    write_json(writer, "results.json", &artifacts.commands)?;
    write_json(writer, "failure-triage.json", &artifacts.blockers)?;
    writer
        .write_new("summary.md", render_summary(artifacts).as_bytes())
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let manifest = build_spec030_artifact_manifest(
        Path::new(&artifacts.evidence_root),
        &artifacts.source_manifest,
    )
    .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    write_json(writer, ARTIFACT_MANIFEST_PATH, &manifest)
}

pub(super) fn render_summary(artifacts: &Spec030ReleaseRunArtifacts) -> String {
    let verdict = match artifacts.verdict {
        Spec030ReleaseVerdict::Pass => "PASS",
        Spec030ReleaseVerdict::Blocked => "BLOCKED",
    };
    let mut output = format!(
        "# Spec030 Release Runner Summary\n\n- schema: {}\n- run_id: {}\n- mode: {:?}\n- verdict: {verdict}\n- semantic closure evidence: {}\n- coverage rows: {}\n- owner audits: {}\n- commands: {}\n\n## Blockers\n",
        artifacts.schema,
        artifacts.run_id.as_str(),
        artifacts.mode,
        artifacts.mode == super::model::Spec030ReleaseRunnerMode::CurrentWorktree,
        artifacts.coverage.len(),
        artifacts.owner_audits.len(),
        artifacts.commands.len()
    );
    for blocker in &artifacts.blockers {
        output.push_str(&format!("- {}: {}\n", blocker.code, blocker.detail));
    }
    output.push_str("\n## Commands\n");
    for command in &artifacts.commands {
        output.push_str(&format!(
            "- {}: {:?}; tests={}; stdout={}; stderr={}\n",
            command.id,
            command.status,
            command.tests.as_ref().map_or(0, |tests| tests.tests_run),
            command.stdout_path,
            command.stderr_path
        ));
    }
    output
}
