use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031CoverageEvidenceKind,
    Spec031CoverageRequirementKind, Spec031CoverageStatus, Spec031ExternalAuditRow,
    Spec031ExternalAuditStatus, Spec031ReleaseCoverageEntry, Spec031TypedEvidenceClass,
};
use super::coverage_ids::required_command_ids;
use super::coverage_provenance::REQUIRED_ARTIFACT_PROVENANCE;
use super::coverage_requirement_rows::{artifact_spec, push_requirement_rows};
use super::model::{
    Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
};
use std::path::Path;

pub(super) fn coverage_entries(
    root: &Path,
    repo_root: &Path,
    schema: &str,
    external_status: Spec031CoverageStatus,
    commands: &[Spec031ReleaseCommandRecord],
    audits: &[Spec031ExternalAuditRow],
) -> Result<Vec<Spec031ReleaseCoverageEntry>, Spec031ReleaseArtifactError> {
    let mut entries = Vec::new();
    push_requirement_rows(root, &mut entries, commands, audits)?;
    push_command_rows(root, &mut entries, commands)?;
    push_artifact_rows(root, &mut entries, commands)?;
    push_external_rows(root, &mut entries, schema, external_status, audits)?;
    if super::model::is_current_layout(schema) {
        push_archived_source_row(root, &mut entries, commands)?;
    } else if schema == super::model::SPEC031_RELEASE_RUNNER_SCHEMA_V2 {
        entries.push(legacy_spec035_external_row(root, audits)?);
        let g7 = super::archive_integrity::has_g7_binding(root);
        entries.extend(super::spec035_coverage::coverage_rows(
            root, audits, repo_root, g7,
        )?);
    }
    Ok(entries)
}

fn push_archived_source_row(
    root: &Path,
    entries: &mut Vec<Spec031ReleaseCoverageEntry>,
    _commands: &[Spec031ReleaseCommandRecord],
) -> Result<(), Spec031ReleaseArtifactError> {
    let artifact = super::archive_integrity::RECEIPT.to_owned();
    entries.push(Spec031ReleaseCoverageEntry {
        requirement_id: "spec031:archive:spec035".to_owned(),
        kind: Spec031CoverageRequirementKind::RequiredArtifact,
        source_locator: "docs/archive/spec035/README.md:3".to_owned(),
        owner: "spec031".to_owned(),
        status: Spec031CoverageStatus::Pass,
        evidence_kind: Spec031CoverageEvidenceKind::ImplementedArtifact,
        evidence_class: Spec031TypedEvidenceClass::CommandResultsJson,
        artifact_media_type: Spec031ArtifactMediaType::Json,
        artifact_hash: artifact_hash(root, &artifact)?,
        artifact,
        command_result_id: None,
        reason: "archive integrity receipt binds SHA256SUMS and all twelve retired Spec035 files"
            .to_owned(),
    });
    Ok(())
}

fn push_command_rows(
    root: &Path,
    entries: &mut Vec<Spec031ReleaseCoverageEntry>,
    commands: &[Spec031ReleaseCommandRecord],
) -> Result<(), Spec031ReleaseArtifactError> {
    for &(name, command_id) in required_command_ids() {
        let command = commands.iter().find(|record| record.id == command_id);
        let (status, artifact, artifact_hash, command_result_id) = match command {
            Some(record) if record.status == Spec031ReleaseCommandStatus::Passed => (
                Spec031CoverageStatus::Pass,
                record.stdout_path.clone(),
                artifact_hash(root, &record.stdout_path)?,
                Some(record.id.clone()),
            ),
            Some(record) => (
                Spec031CoverageStatus::Blocked,
                record.stdout_path.clone(),
                artifact_hash(root, &record.stdout_path)?,
                Some(record.id.clone()),
            ),
            None => {
                let artifact = "triage/blocked-external-evidence.json";
                (
                    Spec031CoverageStatus::Blocked,
                    artifact.to_owned(),
                    artifact_hash(root, artifact)?,
                    None,
                )
            }
        };
        entries.push(Spec031ReleaseCoverageEntry {
            requirement_id: format!("spec031:command:{name}"),
            kind: Spec031CoverageRequirementKind::RequiredCommand,
            source_locator: command_source(command_id),
            owner: "spec031".to_owned(),
            status,
            evidence_kind: Spec031CoverageEvidenceKind::CommandTranscript,
            evidence_class: Spec031TypedEvidenceClass::CommandStdout,
            artifact_media_type: Spec031ArtifactMediaType::Text,
            artifact,
            artifact_hash,
            command_result_id,
            reason: "required Spec031 command result is mapped to its stdout transcript".to_owned(),
        });
    }
    Ok(())
}

fn push_artifact_rows(
    root: &Path,
    entries: &mut Vec<Spec031ReleaseCoverageEntry>,
    _commands: &[Spec031ReleaseCommandRecord],
) -> Result<(), Spec031ReleaseArtifactError> {
    for provenance in &REQUIRED_ARTIFACT_PROVENANCE {
        entries.push(artifact_spec(provenance).into_entry(root)?);
    }
    Ok(())
}

fn push_external_rows(
    root: &Path,
    entries: &mut Vec<Spec031ReleaseCoverageEntry>,
    schema: &str,
    external_status: Spec031CoverageStatus,
    audits: &[Spec031ExternalAuditRow],
) -> Result<(), Spec031ReleaseArtifactError> {
    for descriptor in super::external_audit_facts::external_owner_facts_for(schema) {
        let name = descriptor.slug;
        let audit = audits.iter().find(|audit| owner_slug(audit.owner) == name);
        let artifact = audit
            .map(|audit| audit.artifact.clone())
            .unwrap_or_else(|| format!("external/{name}-read-audit.md"));
        let status = audit
            .map(|audit| match audit.status {
                Spec031ExternalAuditStatus::Pass => Spec031CoverageStatus::Pass,
                Spec031ExternalAuditStatus::Blocked => Spec031CoverageStatus::Blocked,
            })
            .unwrap_or(external_status);
        entries.push(Spec031ReleaseCoverageEntry {
            requirement_id: format!("spec031:external:{name}"),
            kind: Spec031CoverageRequirementKind::ExternalOwner,
            source_locator: audit
                .map(|audit| audit.source_status_locator.clone())
                .unwrap_or_else(|| {
                    "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:3"
                        .to_owned()
                }),
            owner: "spec031".to_owned(),
            status,
            evidence_kind: Spec031CoverageEvidenceKind::ExternalAudit,
            evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
            artifact_media_type: Spec031ArtifactMediaType::Markdown,
            artifact_hash: artifact_hash(root, &artifact)?,
            artifact,
            command_result_id: None,
            reason: "external owner verdict is derived from observed spec status and audit inputs"
                .to_owned(),
        });
    }
    Ok(())
}

pub(super) fn legacy_spec035_external_row(
    root: &Path,
    audits: &[Spec031ExternalAuditRow],
) -> Result<Spec031ReleaseCoverageEntry, Spec031ReleaseArtifactError> {
    let audit = audits
        .iter()
        .find(|audit| owner_slug(audit.owner) == "spec035")
        .ok_or(Spec031ReleaseArtifactError::UnmappedCoverageRequirement)?;
    let status = match audit.status {
        Spec031ExternalAuditStatus::Pass => Spec031CoverageStatus::Pass,
        Spec031ExternalAuditStatus::Blocked => Spec031CoverageStatus::Blocked,
    };
    Ok(Spec031ReleaseCoverageEntry {
        requirement_id: "spec031:external:spec035".to_owned(),
        kind: Spec031CoverageRequirementKind::ExternalOwner,
        source_locator: audit.source_status_locator.clone(),
        owner: "spec031".to_owned(),
        status,
        evidence_kind: Spec031CoverageEvidenceKind::ExternalAudit,
        evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
        artifact_media_type: Spec031ArtifactMediaType::Markdown,
        artifact_hash: artifact_hash(root, &audit.artifact)?,
        artifact: audit.artifact.clone(),
        command_result_id: None,
        reason: "external owner verdict is derived from observed spec status and audit inputs"
            .to_owned(),
    })
}

fn command_source(command_id: &str) -> String {
    let line = match command_id {
        "spec031-test-lifecycle" => 59,
        "spec031-test-projection-parity" => 60,
        "spec031-test-surface-smoke" => 61,
        "spec031-test-failure-injection" => 62,
        "spec031-fmt"
        | "spec031-clippy-workspace"
        | "spec031-test-workspace"
        | "spec031-test-release-runner"
        | "spec031-build-cli"
        | "spec031-build-tui" => 62,
        _ => 62,
    };
    format!("docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md:{line}")
}

fn owner_slug(owner: super::coverage::Spec031ExternalOwnerId) -> &'static str {
    match owner {
        super::coverage::Spec031ExternalOwnerId::Spec029 => "spec029",
        super::coverage::Spec031ExternalOwnerId::Spec030 => "spec030",
        super::coverage::Spec031ExternalOwnerId::Spec032 => "spec032",
        super::coverage::Spec031ExternalOwnerId::Spec033 => "spec033",
        super::coverage::Spec031ExternalOwnerId::Spec034 => "spec034",
        super::coverage::Spec031ExternalOwnerId::Spec035 => "spec035",
    }
}
