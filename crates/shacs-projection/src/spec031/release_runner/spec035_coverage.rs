use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031CoverageEvidenceKind, Spec031CoverageStatus,
    Spec031ExternalAuditRow, Spec031ExternalAuditStatus, Spec031ExternalOwnerId,
    Spec031ReleaseCoverageEntry, Spec031TypedEvidenceClass,
};
use super::model::{Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts};
use std::path::Path;

pub(super) fn has_catalog(
    artifacts: &Spec031ReleaseRunArtifacts,
    repo: &Path,
) -> Result<bool, Spec031ReleaseArtifactError> {
    if !artifacts
        .manifest_files
        .iter()
        .any(|file| file == "evidence-index.json")
    {
        return Ok(false);
    }
    #[derive(serde::Deserialize)]
    struct Index {
        spec035_catalog: Option<Vec<super::spec035_catalog::Requirement>>,
    }
    let index: Index =
        super::validate::read_json(Path::new(&artifacts.evidence_root), "evidence-index.json")?;
    match index.spec035_catalog {
        None => Ok(false),
        Some(catalog) if catalog == super::spec035_catalog::catalog_at(repo)? => Ok(true),
        Some(_) => Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence),
    }
}

pub(super) fn coverage_rows(
    root: &Path,
    audits: &[Spec031ExternalAuditRow],
    repo: &Path,
) -> Result<Vec<Spec031ReleaseCoverageEntry>, Spec031ReleaseArtifactError> {
    let fixture_pass = audits.iter().any(|audit| {
        audit.owner == Spec031ExternalOwnerId::Spec035
            && audit.status == Spec031ExternalAuditStatus::Pass
            && audit.implementation_artifacts
                == ["fixtures/success-fixture/external-owner-facts/spec035.json"]
    });
    let execution = if audits.iter().any(|audit| {
        audit.owner == Spec031ExternalOwnerId::Spec035
            && audit.status == Spec031ExternalAuditStatus::Pass
            && audit.implementation_artifacts == [super::spec035_execution::MANIFEST]
    }) {
        super::spec035_execution::preflight_bound(repo, root).ok()
    } else {
        None
    };
    let artifact = "external/spec035-read-audit.md";
    let hash = artifact_hash(root, artifact)?;
    let catalog = if execution.is_some()
        || fixture_pass
        || repo.join(super::spec035_catalog::SPEC_ROOT).is_dir()
    {
        super::spec035_catalog::catalog_at(repo)?
    } else {
        super::spec035_catalog::catalog()
    };
    Ok(catalog
        .into_iter()
        .map(|row| {
            let proof = execution.as_ref().and_then(|execution| execution.proof_for(&row));
            let reason = match (&execution, proof) {
                (Some(execution), Some(proof)) => format!(
                    "validated preflight {}; manifest sha256:{}; source sha256:{}; row proof {} sha256:{}; pending-final-audit",
                    execution.binding.run_id, execution.binding.manifest_sha256,
                    execution.binding.source_sha256, proof.path, proof.sha256
                ),
                _ if fixture_pass => "success-fixture mechanics only, not semantic Spec035 closure".to_owned(),
                _ => "Spec035 closure requires validated current execution evidence; v1 historical rows and v2 classification cannot discharge this catalog".to_owned(),
            };
            Spec031ReleaseCoverageEntry {
            requirement_id: row.id,
            kind: row.kind,
            source_locator: row.source_locator,
            owner: row.owner,
            status: if fixture_pass || proof.is_some() { Spec031CoverageStatus::Pass } else { Spec031CoverageStatus::Blocked },
            evidence_kind: Spec031CoverageEvidenceKind::ExternalAudit,
            evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
            artifact_media_type: Spec031ArtifactMediaType::Markdown,
            artifact: artifact.to_owned(),
            artifact_hash: hash.clone(),
            command_result_id: None,
            reason: format!(
                "{}; required authoritative closure rows: {}",
                reason,
                row.closure_ids.join(", ")
            ),
        }})
        .collect())
}
