use super::bwrap_provenance::Spec030BwrapRecord;
use super::model::{Spec030ExternalEvidence, Spec030ReleaseArtifactError};
use super::records::push_blocker;
use super::source_manifest::sha256_bytes;
use super::writer::write_json;
use crate::release_evidence::EvidenceWriter;
use crate::Spec031ReleaseCommandRecord;
use std::path::Path;

const RECORD_ARTIFACT: &str = "external/bwrap-linux-record.json";

impl Spec030BwrapRecord {
    pub fn artifact_hash(&self) -> Result<String, serde_json::Error> {
        let bytes = serde_json::to_vec(self)?;
        Ok(sha256_bytes(&bytes))
    }
}

pub(super) fn write_provenance(
    writer: &EvidenceWriter,
    record: &Spec030BwrapRecord,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Spec030ExternalEvidence, Spec030ReleaseArtifactError> {
    writer
        .create_dir_all("external")
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    write_json(writer, RECORD_ARTIFACT, record)?;
    writer
        .write_new("external/bwrap-linux-record.stdout", stdout)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    writer
        .write_new("external/bwrap-linux-record.stderr", stderr)
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    let artifact_hash = record
        .artifact_hash()
        .map_err(|_| Spec030ReleaseArtifactError::Io)?;
    Ok(Spec030ExternalEvidence {
        kind: "linux_bwrap_active_lane".to_owned(),
        artifact: RECORD_ARTIFACT.to_owned(),
        artifact_hash,
    })
}

pub(super) fn materialize_external(
    path: Option<&Path>,
    _source_digest: &str,
    _writer: &EvidenceWriter,
    blockers: &mut Vec<super::model::Spec030ReleaseBlocker>,
) -> Result<
    Option<(Spec031ReleaseCommandRecord, Spec030ExternalEvidence)>,
    Spec030ReleaseArtifactError,
> {
    let Some(path) = path else {
        push_blocker(
            blockers,
            "bwrap_active_unverified",
            "Linux SHACS_REQUIRE_BWRAP=1 lane was not run on this platform",
        );
        return Ok(None);
    };
    let _ = path;
    push_blocker(
        blockers,
        "bwrap_untrusted_producer",
        "only the in-process Linux runner may produce trusted bwrap evidence",
    );
    Ok(None)
}
