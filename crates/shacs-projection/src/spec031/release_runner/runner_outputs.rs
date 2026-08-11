use super::coverage::{
    Spec031ExternalAuditRow, Spec031ExternalAuditStatus, Spec031ExternalOwnerId,
};
use super::coverage_provenance::{requirement_provenance, REQUIRED_ARTIFACT_PROVENANCE};
use super::external_audit_facts::external_owner_facts;
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts, Spec031ReleaseRunnerConfig,
    SPEC031_RELEASE_RUNNER_SCHEMA,
};
use super::writer::write_json;
use crate::release_evidence::EvidenceWriter;

pub(super) struct CleanupReceiptSpec<'a> {
    pub(super) file_name: &'a str,
    pub(super) status: &'a str,
    pub(super) resource_id: &'a str,
    pub(super) check_artifact: &'a str,
}

pub(super) fn push_cleanup(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
    spec: CleanupReceiptSpec<'_>,
) -> Result<(), Spec031ReleaseArtifactError> {
    let cleanup = format!("cleanup/{}", spec.file_name);
    write_json(
        writer,
        &cleanup,
        &serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": config.run_id.as_str(),
            "status": spec.status,
            "success": true,
            "resource_id": spec.resource_id,
            "check_artifact": spec.check_artifact
        }),
    )?;
    artifacts.cleanup_registry.push(cleanup);
    Ok(())
}

pub(super) fn push_triage(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
    file_name: &str,
    code: &str,
    message: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    let triage = format!("triage/{file_name}");
    write_json(
        writer,
        &triage,
        &serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": config.run_id.as_str(),
            "code": code,
            "message": message
        }),
    )?;
    artifacts.failure_triage.push(triage);
    Ok(())
}

pub(super) fn push_blocked_external_triage(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let blockers: Vec<serde_json::Value> = artifacts
        .external_audits
        .iter()
        .filter(|audit| audit.status == Spec031ExternalAuditStatus::Blocked)
        .map(blocked_external_value)
        .collect();
    let triage = "triage/blocked-external-evidence.json";
    write_json(
        writer,
        triage,
        &serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": config.run_id.as_str(),
            "code": "blocked_external_evidence",
            "message": "required external read audits are not PASS",
            "blocked_external_audits": blockers
        }),
    )?;
    artifacts.failure_triage.push(triage.to_owned());
    Ok(())
}

pub(super) fn write_evidence_index(
    config: &Spec031ReleaseRunnerConfig,
    writer: &EvidenceWriter,
    artifacts: &mut Spec031ReleaseRunArtifacts,
) -> Result<(), Spec031ReleaseArtifactError> {
    let path = "evidence-index.json";
    write_json(
        writer,
        path,
        &serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": config.run_id.as_str(),
            "command_result_ids": artifacts.command_registry.iter().map(|record| record.id.as_str()).collect::<Vec<_>>(),
            "external_audits": artifacts.external_audits.iter().map(indexed_audit).collect::<Vec<_>>(),
            "fixtures": artifacts.fixture_registry,
            "cleanup": artifacts.cleanup_registry,
            "failure_triage": artifacts.failure_triage,
            "authoritative_sources": authoritative_sources()
        }),
    )?;
    if !artifacts.manifest_files.iter().any(|file| file == path) {
        artifacts.manifest_files.push(path.to_owned());
    }
    Ok(())
}

fn authoritative_sources() -> Vec<String> {
    let mut sources: Vec<String> = requirement_provenance()
        .into_iter()
        .map(|row| row.source_locator)
        .collect();
    sources.extend(
        REQUIRED_ARTIFACT_PROVENANCE
            .iter()
            .map(|row| row.source_locator.to_owned()),
    );
    sources.extend(external_owner_facts().iter().flat_map(|row| {
        [
            row.source_locator.to_owned(),
            row.source_status_locator.to_owned(),
        ]
    }));
    sources.sort();
    sources.dedup();
    sources
}

fn blocked_external_value(audit: &Spec031ExternalAuditRow) -> serde_json::Value {
    serde_json::json!({
        "owner": owner_slug(audit.owner),
        "artifact": audit.artifact,
        "source_status_locator": audit.source_status_locator,
        "reason": audit.reason,
        "artifact_hash": audit.artifact_hash
    })
}

fn indexed_audit(audit: &Spec031ExternalAuditRow) -> serde_json::Value {
    serde_json::json!({
        "owner": owner_slug(audit.owner),
        "status": audit.status,
        "source_locator": audit.source_locator,
        "source_status_locator": audit.source_status_locator,
        "artifact": audit.artifact,
        "artifact_hash": audit.artifact_hash,
        "reason": audit.reason,
        "command_result_ids": audit.command_result_ids
    })
}

fn owner_slug(owner: Spec031ExternalOwnerId) -> &'static str {
    match owner {
        Spec031ExternalOwnerId::Spec029 => "spec029",
        Spec031ExternalOwnerId::Spec030 => "spec030",
        Spec031ExternalOwnerId::Spec032 => "spec032",
        Spec031ExternalOwnerId::Spec033 => "spec033",
        Spec031ExternalOwnerId::Spec034 => "spec034",
        Spec031ExternalOwnerId::Spec035 => "spec035",
    }
}
