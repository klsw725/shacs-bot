use super::model::Spec031ReleaseArtifactError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const ARCHIVE_ROOT: &str = "docs/archive/spec035/current";
const ARCHIVE_MANIFEST: &str = "docs/archive/spec035/current/SHA256SUMS";
pub(super) const RECEIPT: &str = "archive-integrity.json";
const ARCHIVE_MANIFEST_SHA256: &str =
    "ae1fd0b594377ce1073ce8ce02e9d979623936cdd2648ffda5a5d5f4e3475571";
const ARCHIVED_FILES: [(&str, &str); 12] = [
    (
        "CLOSURE.md",
        "76035a39fced8770c7c1cc90f62a1483a66e3dcbc430180505c6e94e71003ecb",
    ),
    (
        "SPEC.md",
        "25c7aa4750510dc9c9c641ea6b8775caa0c9bd861dc180c7c59f8bf35555ec0c",
    ),
    (
        "prds/000-shared-projection-model-and-vocabulary.md",
        "4428e8a4a68b99d88b4845c305c77a520190389edd30014f5eccc13a68daaebd",
    ),
    (
        "prds/001-surface-adapter-parity-cli-api-channel.md",
        "e7b7a788a1a5b606332a2822d869d90ea89f31171211e14668f2ecac2f41097c",
    ),
    (
        "prds/002-approval-progress-and-recovery-parity.md",
        "f02099df0fef1c58825ebd679e1e19bea9e5442f1478dc2ac949bd6984c7f071",
    ),
    (
        "prds/003-readiness-degraded-health-and-diagnostics.md",
        "c0b202fa01df0b0ea50ee4a12f3159890930effa6f3f24e86c27ba84ca559daf",
    ),
    (
        "prds/004-context-extension-app-and-media-projection.md",
        "323c8acc10c16a5bdfa318f39fed71bd866c9b33d93cf9d66acaa7735744d5a1",
    ),
    (
        "prds/005-interactive-tui-repl-and-onboard-flows.md",
        "8f85a874154eceb90cd6221656431d490726dcb8c9e2a3213409a551fa8b5a54",
    ),
    (
        "prds/006-reconnect-backpressure-and-drop-accounting.md",
        "d772f02f99381245b96ee14359fa39de9d7f0667d3738f732b11c55140492a5e",
    ),
    (
        "prds/007-release-runner-and-spec035-closure.md",
        "760b43e4ea2f92a799814aab240690a446a154716e6f339f13322f01fa5bab15",
    ),
    (
        "prds/008-transport-capability-and-snapshot-first-reconnect.md",
        "b687e706db13f82c11a27c6ea3671eb6b92458bb623edd8caf7f8b11ee969460",
    ),
    (
        "prds/009-goal-and-read-only-tasks-projection.md",
        "c83fe741136453166908642013279246be2d02c9170e9e1d5b90b29c87502233",
    ),
];
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ArchiveIntegrityReceipt {
    schema: String,
    manifest: ArchiveFileDigest,
    files: Vec<ArchiveFileDigest>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ArchiveFileDigest {
    path: String,
    sha256: String,
}

pub(super) fn receipt() -> ArchiveIntegrityReceipt {
    ArchiveIntegrityReceipt {
        schema: "spec031.archive_integrity.v1".to_owned(),
        manifest: ArchiveFileDigest {
            path: ARCHIVE_MANIFEST.to_owned(),
            sha256: ARCHIVE_MANIFEST_SHA256.to_owned(),
        },
        files: ARCHIVED_FILES
            .iter()
            .map(|(relative, sha256)| ArchiveFileDigest {
                path: format!("{ARCHIVE_ROOT}/{relative}"),
                sha256: (*sha256).to_owned(),
            })
            .collect(),
    }
}

pub(super) fn validate_spec035_archive(repo: &Path) -> Result<(), Spec031ReleaseArtifactError> {
    validate_digest(repo, ARCHIVE_MANIFEST, ARCHIVE_MANIFEST_SHA256)?;
    for (relative, expected) in ARCHIVED_FILES {
        let path = format!("{ARCHIVE_ROOT}/{relative}");
        validate_digest(repo, &path, expected)?;
    }
    Ok(())
}

pub(super) fn validate_receipt(
    evidence_root: &Path,
    repo: &Path,
) -> Result<(), Spec031ReleaseArtifactError> {
    let recorded: ArchiveIntegrityReceipt = super::validate::read_json(evidence_root, RECEIPT)?;
    if recorded != receipt() {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    validate_spec035_archive(repo)
}

fn validate_digest(
    repo: &Path,
    path: &str,
    expected: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    let bytes = std::fs::read(super::validate::require_safe_file(repo, path)?)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err(Spec031ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_spec035_archive, ARCHIVED_FILES, ARCHIVE_MANIFEST, ARCHIVE_ROOT};
    use crate::Spec031ReleaseArtifactError;

    fn archive_fixture() -> tempfile::TempDir {
        let fixture = tempfile::tempdir().expect("fixture root");
        for path in std::iter::once(ARCHIVE_MANIFEST.to_owned()).chain(
            ARCHIVED_FILES
                .iter()
                .map(|(relative, _)| format!("{ARCHIVE_ROOT}/{relative}")),
        ) {
            let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(&path);
            let target = fixture.path().join(path);
            std::fs::create_dir_all(target.parent().expect("archive parent"))
                .expect("archive parent writes");
            std::fs::copy(source, target).expect("archived source copies");
        }
        fixture
    }

    #[test]
    fn spec035_archive_accepts_preserved_source_bytes() {
        let fixture = archive_fixture();

        validate_spec035_archive(fixture.path()).expect("preserved archive passes");
    }

    #[test]
    fn spec035_archive_rejects_missing_prd() {
        let fixture = archive_fixture();
        std::fs::remove_file(
            fixture
                .path()
                .join(format!("{ARCHIVE_ROOT}/{}", ARCHIVED_FILES[2].0)),
        )
        .expect("archived PRD removes");

        assert_eq!(
            validate_spec035_archive(fixture.path()),
            Err(Spec031ReleaseArtifactError::MissingRequiredArtifact)
        );
    }

    #[test]
    fn spec035_archive_rejects_tampered_prd() {
        let fixture = archive_fixture();
        std::fs::write(
            fixture
                .path()
                .join(format!("{ARCHIVE_ROOT}/{}", ARCHIVED_FILES[11].0)),
            "tampered",
        )
        .expect("archived PRD mutates");

        assert_eq!(
            validate_spec035_archive(fixture.path()),
            Err(Spec031ReleaseArtifactError::ArtifactMismatch)
        );
    }

    #[test]
    fn spec035_archive_rejects_missing_digest_manifest() {
        let fixture = archive_fixture();
        std::fs::remove_file(fixture.path().join(ARCHIVE_MANIFEST))
            .expect("digest manifest removes");

        assert_eq!(
            validate_spec035_archive(fixture.path()),
            Err(Spec031ReleaseArtifactError::MissingRequiredArtifact)
        );
    }

    #[test]
    fn spec035_archive_rejects_tampered_digest_manifest() {
        let fixture = archive_fixture();
        std::fs::write(fixture.path().join(ARCHIVE_MANIFEST), "tampered")
            .expect("digest manifest mutates");

        assert_eq!(
            validate_spec035_archive(fixture.path()),
            Err(Spec031ReleaseArtifactError::ArtifactMismatch)
        );
    }
}
