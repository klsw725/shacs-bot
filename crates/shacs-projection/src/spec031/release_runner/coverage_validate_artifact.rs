use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031CoverageRequirementKind, Spec031CoverageStatus,
    Spec031ReleaseCoverageEntry, Spec031TypedEvidenceClass,
};
use super::model::{Spec031ReleaseArtifactError, Spec031ReleaseRunArtifacts};
use std::path::PathBuf;

pub(super) fn validate_coverage_artifact(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    validate_coverage_artifact_shape(entry)?;
    validate_coverage_hash(artifacts, entry)
}

fn validate_coverage_artifact_shape(
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if entry.artifact_media_type != media_type_for_path(&entry.artifact) {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    if entry.evidence_class != expected_class_for_entry(entry)? {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    Ok(())
}

fn validate_coverage_hash(
    artifacts: &Spec031ReleaseRunArtifacts,
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<(), Spec031ReleaseArtifactError> {
    if entry.artifact_hash == format!("self:{}", entry.artifact)
        && matches!(
            entry.kind,
            Spec031CoverageRequirementKind::RequiredArtifact
                | Spec031CoverageRequirementKind::ParentMustHave
                | Spec031CoverageRequirementKind::AcceptanceCriterion
                | Spec031CoverageRequirementKind::ClosureEvidence
                | Spec031CoverageRequirementKind::PrdTask
        )
        && matches!(
            entry.artifact.as_str(),
            "manifest.json"
                | "coverage-matrix.json"
                | "results.json"
                | "failure-triage.json"
                | "summary.md"
        )
    {
        return Ok(());
    }
    if entry.artifact_hash == "self-referential" {
        return Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence);
    }
    let root = PathBuf::from(&artifacts.evidence_root);
    if artifact_hash(&root, &entry.artifact)? != entry.artifact_hash {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

fn expected_class_for_entry(
    entry: &Spec031ReleaseCoverageEntry,
) -> Result<Spec031TypedEvidenceClass, Spec031ReleaseArtifactError> {
    if entry.kind == Spec031CoverageRequirementKind::RequiredCommand
        && entry.status == Spec031CoverageStatus::Blocked
        && entry.command_result_id.is_none()
    {
        return Ok(Spec031TypedEvidenceClass::FailureTriageJson);
    }
    if entry.kind == Spec031CoverageRequirementKind::RequiredCommand {
        return Ok(Spec031TypedEvidenceClass::CommandStdout);
    }
    if entry.kind == Spec031CoverageRequirementKind::ExternalOwner {
        return Ok(Spec031TypedEvidenceClass::ExternalAuditMarkdown);
    }
    match entry.artifact.as_str() {
        "manifest.json" => Ok(Spec031TypedEvidenceClass::ManifestJson),
        "coverage-matrix.json" => Ok(Spec031TypedEvidenceClass::CoverageMatrixJson),
        "results.json" => Ok(Spec031TypedEvidenceClass::CommandResultsJson),
        "failure-triage.json" => Ok(Spec031TypedEvidenceClass::FailureTriageJson),
        "summary.md" => Ok(Spec031TypedEvidenceClass::SummaryMarkdown),
        "evidence-index.json" => Ok(Spec031TypedEvidenceClass::CommandResultsJson),
        "triage/blocked-external-evidence.json" => Ok(Spec031TypedEvidenceClass::FailureTriageJson),
        _ => Err(Spec031ReleaseArtifactError::InvalidCoverageEvidence),
    }
}

fn media_type_for_path(path: &str) -> Spec031ArtifactMediaType {
    if path.ends_with(".md") {
        Spec031ArtifactMediaType::Markdown
    } else if path.ends_with(".stdout") || path.ends_with(".stderr") {
        Spec031ArtifactMediaType::Text
    } else {
        Spec031ArtifactMediaType::Json
    }
}
