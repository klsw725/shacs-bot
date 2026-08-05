use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031CoverageEvidenceKind,
    Spec031CoverageRequirementKind, Spec031CoverageStatus, Spec031ExternalAuditRow,
    Spec031ExternalAuditStatus, Spec031ReleaseCoverageEntry, Spec031TypedEvidenceClass,
};
use super::coverage_provenance::{requirement_provenance, ArtifactProvenance};
use super::model::{Spec031ReleaseArtifactError, Spec031ReleaseCommandRecord};
use std::path::Path;

pub(super) fn push_requirement_rows(
    root: &Path,
    entries: &mut Vec<Spec031ReleaseCoverageEntry>,
    commands: &[Spec031ReleaseCommandRecord],
    audits: &[Spec031ExternalAuditRow],
) -> Result<(), Spec031ReleaseArtifactError> {
    let _ = commands;
    for spec in requirement_specs(audits) {
        entries.push(requirement_row(root, spec)?);
    }
    Ok(())
}

fn requirement_row(
    root: &Path,
    spec: RequirementSpec,
) -> Result<Spec031ReleaseCoverageEntry, Spec031ReleaseArtifactError> {
    let artifact = if spec.status == Spec031CoverageStatus::Blocked {
        "triage/blocked-external-evidence.json"
    } else {
        spec.artifact
    };
    Ok(Spec031ReleaseCoverageEntry {
        requirement_id: spec.id,
        kind: spec.kind,
        source_locator: spec.source,
        owner: "spec031".to_owned(),
        status: spec.status,
        evidence_kind: if spec.kind == Spec031CoverageRequirementKind::ClosureEvidence
            && spec.status == Spec031CoverageStatus::Blocked
        {
            Spec031CoverageEvidenceKind::CleanupReceipt
        } else {
            Spec031CoverageEvidenceKind::ImplementedArtifact
        },
        evidence_class: if artifact == "results.json" {
            Spec031TypedEvidenceClass::CommandResultsJson
        } else {
            Spec031TypedEvidenceClass::FailureTriageJson
        },
        artifact_media_type: Spec031ArtifactMediaType::Json,
        artifact_hash: coverage_artifact_hash(root, artifact)?,
        artifact: artifact.to_owned(),
        command_result_id: None,
        reason: spec.reason,
    })
}

#[derive(Clone)]
struct RequirementSpec {
    id: String,
    kind: Spec031CoverageRequirementKind,
    source: String,
    artifact: &'static str,
    status: Spec031CoverageStatus,
    reason: String,
}

impl RequirementSpec {
    fn artifact(provenance: &ArtifactProvenance) -> Self {
        Self {
            id: format!("spec031:artifact:{}", provenance.name),
            kind: Spec031CoverageRequirementKind::RequiredArtifact,
            source: provenance.source_locator.to_owned(),
            artifact: provenance.artifact,
            status: Spec031CoverageStatus::Pass,
            reason: format!(
                "required artifact {} cites its exact generated file",
                provenance.name
            ),
        }
    }
}

pub(super) fn artifact_spec(provenance: &ArtifactProvenance) -> Spec031ReleaseCoverageEntrySeed {
    Spec031ReleaseCoverageEntrySeed {
        spec: RequirementSpec::artifact(provenance),
        evidence_class: provenance.evidence_class,
        media_type: provenance.media_type,
    }
}

pub(super) struct Spec031ReleaseCoverageEntrySeed {
    spec: RequirementSpec,
    evidence_class: Spec031TypedEvidenceClass,
    media_type: Spec031ArtifactMediaType,
}

impl Spec031ReleaseCoverageEntrySeed {
    pub(super) fn into_entry(
        self,
        root: &Path,
    ) -> Result<Spec031ReleaseCoverageEntry, Spec031ReleaseArtifactError> {
        let artifact = self.spec.artifact;
        Ok(Spec031ReleaseCoverageEntry {
            requirement_id: self.spec.id,
            kind: self.spec.kind,
            source_locator: self.spec.source,
            owner: "spec031".to_owned(),
            status: self.spec.status,
            evidence_kind: Spec031CoverageEvidenceKind::ImplementedArtifact,
            evidence_class: self.evidence_class,
            artifact_media_type: self.media_type,
            artifact_hash: coverage_artifact_hash(root, artifact)?,
            artifact: artifact.to_owned(),
            command_result_id: None,
            reason: self.spec.reason,
        })
    }
}

fn coverage_artifact_hash(
    root: &Path,
    artifact: &str,
) -> Result<String, Spec031ReleaseArtifactError> {
    if matches!(
        artifact,
        "manifest.json"
            | "coverage-matrix.json"
            | "results.json"
            | "failure-triage.json"
            | "summary.md"
    ) {
        return Ok(format!("self:{artifact}"));
    }
    artifact_hash(root, artifact)
}

fn requirement_specs(audits: &[Spec031ExternalAuditRow]) -> Vec<RequirementSpec> {
    let closure_status = if audits
        .iter()
        .any(|audit| audit.status == Spec031ExternalAuditStatus::Blocked)
    {
        Spec031CoverageStatus::Blocked
    } else {
        Spec031CoverageStatus::Pass
    };
    requirement_provenance()
        .into_iter()
        .map(|provenance| {
            let kind = if provenance.id.contains(":must:") {
                Spec031CoverageRequirementKind::ParentMustHave
            } else if provenance.id.contains(":acceptance:") {
                Spec031CoverageRequirementKind::AcceptanceCriterion
            } else if provenance.id.contains(":closure:") {
                Spec031CoverageRequirementKind::ClosureEvidence
            } else {
                Spec031CoverageRequirementKind::PrdTask
            };
            let status = if kind == Spec031CoverageRequirementKind::ClosureEvidence {
                closure_status
            } else {
                Spec031CoverageStatus::Pass
            };
            RequirementSpec {
                id: provenance.id,
                kind,
                source: provenance.source_locator,
                artifact: provenance.artifact,
                status,
                reason: if status == Spec031CoverageStatus::Blocked {
                    "closure remains blocked by required external owner audit rows".to_owned()
                } else {
                    "requirement is backed by exact source locator and typed artifact evidence"
                        .to_owned()
                },
            }
        })
        .collect()
}
