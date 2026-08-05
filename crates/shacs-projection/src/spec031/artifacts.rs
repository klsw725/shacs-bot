use super::evidence_writer::EvidenceWriter;
use super::external_owner::{
    blocker_file_name, read_audit_file_name, ExternalOwnerSpec, Spec031ExternalOwnerArtifactSet,
    Spec031ExternalOwnerProjection, Spec031ExternalOwnerReasonCode, Spec031ReadAuditArtifact,
};
use std::fmt::{Display, Formatter};
use std::path::Path;

#[derive(Debug)]
pub enum Spec031ArtifactError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl Display for Spec031ArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "spec031 artifact io error: {error}"),
            Self::Json(error) => write!(formatter, "spec031 artifact json error: {error}"),
        }
    }
}

impl std::error::Error for Spec031ArtifactError {}

impl From<std::io::Error> for Spec031ArtifactError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Spec031ArtifactError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn spec031_prd004_external_owner_artifacts(
    projection: &Spec031ExternalOwnerProjection,
    output_dir: &Path,
) -> Result<Spec031ExternalOwnerArtifactSet, Spec031ArtifactError> {
    let writer = EvidenceWriter::open_new_run(output_dir)?;
    write_projection(&writer, projection)?;
    let read_audits = [ExternalOwnerSpec::Spec032, ExternalOwnerSpec::Spec034]
        .into_iter()
        .map(|owner| read_audit_for_owner(projection, owner))
        .collect::<Vec<_>>();
    for artifact in &read_audits {
        write_artifact(&writer, &artifact.file_name, artifact)?;
    }
    let closure_blockers = projection
        .closure_blockers
        .iter()
        .map(|blocker| Spec031ReadAuditArtifact {
            file_name: blocker_file_name(blocker.owner, blocker.capability),
            status: "BLOCKED".to_owned(),
            owner: blocker.owner,
            reason_code: blocker.reason_code,
        })
        .collect::<Vec<_>>();
    for artifact in &closure_blockers {
        write_artifact(&writer, &artifact.file_name, artifact)?;
    }
    Ok(Spec031ExternalOwnerArtifactSet {
        read_audits,
        closure_blockers,
    })
}

fn write_projection(
    writer: &EvidenceWriter,
    projection: &Spec031ExternalOwnerProjection,
) -> Result<(), Spec031ArtifactError> {
    let bytes = serde_json::to_vec_pretty(projection)?;
    writer.write_new("available-partial-projection.json", &bytes)?;
    Ok(())
}

fn read_audit_for_owner(
    projection: &Spec031ExternalOwnerProjection,
    owner: ExternalOwnerSpec,
) -> Spec031ReadAuditArtifact {
    let blocker = projection
        .closure_blockers
        .iter()
        .find(|blocker| blocker.owner == owner);
    Spec031ReadAuditArtifact {
        file_name: read_audit_file_name(owner),
        status: if blocker.is_some() { "BLOCKED" } else { "PASS" }.to_owned(),
        owner,
        reason_code: blocker.map_or(Spec031ExternalOwnerReasonCode::OwnerRecorded, |blocker| {
            blocker.reason_code
        }),
    }
}

fn write_artifact(
    writer: &EvidenceWriter,
    file_name: &str,
    artifact: &Spec031ReadAuditArtifact,
) -> Result<(), Spec031ArtifactError> {
    let bytes = serde_json::to_vec_pretty(artifact)?;
    writer.write_new(file_name, &bytes)?;
    Ok(())
}
