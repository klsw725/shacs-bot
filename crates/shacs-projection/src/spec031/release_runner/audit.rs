#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;

use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031ExternalAuditRow, Spec031ExternalAuditStatus,
    Spec031TypedEvidenceClass,
};
use super::external_audit_facts::{external_owner_facts, ExternalOwnerFactDescriptor};
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandStatus, Spec031ReleaseRunArtifacts,
    Spec031ReleaseRunnerConfig,
};
use super::writer::{write_json, write_text};
use crate::release_evidence::EvidenceWriter;
use std::fs;

pub(super) fn add_external_audits(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
    all_pass: bool,
) -> Result<(), Spec031ReleaseArtifactError> {
    writer
        .create_dir_all("external")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    for spec in external_owner_facts() {
        let observed_source_status = observed_status(config, spec)?;
        let implementation_artifacts = if all_pass {
            write_success_fixture_facts(writer, spec)?
        } else {
            spec.fact_artifacts
                .iter()
                .map(|artifact| (*artifact).to_owned())
                .collect()
        };
        let status = if audit_passes(
            config,
            spec,
            artifacts,
            &implementation_artifacts,
            all_pass || source_status_is_complete(&observed_source_status),
        ) {
            Spec031ExternalAuditStatus::Pass
        } else {
            Spec031ExternalAuditStatus::Blocked
        };
        let reason = if status == Spec031ExternalAuditStatus::Pass {
            "artifact-backed exact fact audit passes"
        } else {
            spec.blocked_reason
        };
        let source_status = if all_pass {
            "Status: Complete (Success Fixture)"
        } else {
            observed_source_status.as_str()
        };
        let artifact = format!("external/{}-read-audit.md", spec.slug);
        let audit_command_ids: Vec<String> = if status == Spec031ExternalAuditStatus::Pass {
            spec.command_result_ids
                .iter()
                .map(|id| (*id).to_owned())
                .collect()
        } else {
            Vec::new()
        };
        write_text(
            writer,
            &artifact,
            &render_audit(
                spec,
                status,
                source_status,
                reason,
                &implementation_artifacts,
                &audit_command_ids,
            ),
        )?;
        artifacts.external_audits.push(Spec031ExternalAuditRow {
            owner: spec.owner,
            status,
            source_locator: spec.source_locator.to_owned(),
            source_status_locator: spec.source_status_locator.to_owned(),
            implementation_artifacts,
            command_result_ids: audit_command_ids,
            artifact: artifact.clone(),
            artifact_media_type: Spec031ArtifactMediaType::Markdown,
            evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
            artifact_hash: artifact_hash(&config.evidence_root, &artifact)?,
            reason: reason.to_owned(),
        });
    }
    Ok(())
}

fn write_success_fixture_facts(
    writer: &EvidenceWriter,
    spec: &ExternalOwnerFactDescriptor,
) -> Result<Vec<String>, Spec031ReleaseArtifactError> {
    writer
        .create_dir_all("fixtures/success-fixture/external-owner-facts")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    let artifact = format!(
        "fixtures/success-fixture/external-owner-facts/{}.json",
        spec.slug
    );
    write_json(
        writer,
        &artifact,
        &serde_json::json!({
            "schema": super::model::SPEC031_RELEASE_RUNNER_SCHEMA,
            "owner": spec.slug,
            "fact": "success-fixture-owner-fact",
            "source_locator": spec.source_locator,
            "source_status_locator": spec.source_status_locator
        }),
    )?;
    Ok(vec![artifact])
}

fn observed_status(
    config: &Spec031ReleaseRunnerConfig,
    spec: &ExternalOwnerFactDescriptor,
) -> Result<String, Spec031ReleaseArtifactError> {
    let Ok(text) = fs::read_to_string(config.repo_root.join(spec.source_locator)) else {
        return Ok("Status: Open".to_owned());
    };
    text.lines()
        .find(|line| line.trim_start().starts_with("Status:"))
        .map(|line| line.trim().to_owned())
        .ok_or(Spec031ReleaseArtifactError::InvalidCoverageEvidence)
}

fn audit_passes(
    config: &Spec031ReleaseRunnerConfig,
    spec: &ExternalOwnerFactDescriptor,
    artifacts: &Spec031ReleaseRunArtifacts,
    implementation_artifacts: &[String],
    source_complete: bool,
) -> bool {
    source_complete
        && !implementation_artifacts.is_empty()
        && implementation_artifacts
            .iter()
            .all(|artifact| fact_artifact_exists(config, artifact))
        && spec.command_result_ids.iter().all(|id| {
            artifacts.command_registry.iter().any(|record| {
                record.id == *id && record.status == Spec031ReleaseCommandStatus::Passed
            })
        })
}

fn source_status_is_complete(status: &str) -> bool {
    status.starts_with("Status: Complete")
}

fn fact_artifact_exists(config: &Spec031ReleaseRunnerConfig, artifact: &str) -> bool {
    let (path, token) = match artifact.split_once('#') {
        Some(parts) => parts,
        None => (artifact, ""),
    };
    let full_path = if path.starts_with("fixtures/") {
        config.evidence_root.join(path)
    } else {
        config.repo_root.join(path)
    };
    let Ok(text) = fs::read_to_string(full_path) else {
        return false;
    };
    token.is_empty() || text.contains(token)
}

fn render_audit(
    spec: &ExternalOwnerFactDescriptor,
    status: Spec031ExternalAuditStatus,
    source_status: &str,
    reason: &str,
    implementation_artifacts: &[String],
    command_ids: &[String],
) -> String {
    let status = match status {
        Spec031ExternalAuditStatus::Pass => "pass",
        Spec031ExternalAuditStatus::Blocked => "blocked",
    };
    let artifacts = implementation_artifacts.join(", ");
    let commands = command_ids.join(", ");
    format!(
        "# Spec031 External Owner Audit\n\nowner: {}\nstatus: {status}\nverdict: {status}\nsource_locator: {}\nsource_status_locator: {}\nsource_status: {}\nimplementation_artifacts: {artifacts}\ncommand_result_ids: {commands}\nreason: {reason}\n",
        spec.slug, spec.source_locator, spec.source_status_locator, source_status
    )
}
