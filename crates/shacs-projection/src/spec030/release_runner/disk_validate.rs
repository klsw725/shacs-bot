use super::artifact_manifest::{
    build_spec030_artifact_manifest, Spec030ArtifactManifest, ARTIFACT_MANIFEST_PATH,
};
use super::model::*;
use super::source_manifest::{build_spec030_source_manifest, Spec030SourceManifest};
use super::writer::render_summary;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;

pub(super) fn validate_disk(
    artifacts: &Spec030ReleaseRunArtifacts,
) -> Result<(), Spec030ReleaseArtifactError> {
    let root = Path::new(&artifacts.evidence_root);
    let source = build_spec030_source_manifest(Path::new(&artifacts.repo_root))
        .map_err(|_| Spec030ReleaseArtifactError::SourceMismatch)?;
    if source != artifacts.source_manifest {
        return Err(Spec030ReleaseArtifactError::SourceMismatch);
    }
    read_expected(root, "source-manifest.json", &artifacts.source_manifest)?;
    read_expected(root, "manifest.json", artifacts)?;
    read_expected(root, "coverage-matrix.json", &artifacts.coverage)?;
    read_expected(root, "owner-audits.json", &artifacts.owner_audits)?;
    read_expected(root, "facts.json", &artifacts.facts)?;
    read_expected(root, "surfaces.json", &artifacts.surfaces)?;
    read_expected(root, "surface-owner.json", &artifacts.surface_owner)?;
    read_expected(
        root,
        "surface-assertions.json",
        &artifacts.surface_assertions,
    )?;
    read_expected(root, "external-evidence.json", &artifacts.external_evidence)?;
    read_expected(root, "results.json", &artifacts.commands)?;
    read_expected(root, "failure-triage.json", &artifacts.blockers)?;
    let summary = std::fs::read_to_string(root.join("summary.md"))
        .map_err(|_| Spec030ReleaseArtifactError::ArtifactMismatch)?;
    if summary != render_summary(artifacts) {
        return Err(Spec030ReleaseArtifactError::ArtifactMismatch);
    }
    validate_artifact_manifest(root, &source)
}

fn read_expected<T>(
    root: &Path,
    relative: &str,
    expected: &T,
) -> Result<(), Spec030ReleaseArtifactError>
where
    T: DeserializeOwned + Serialize + PartialEq,
{
    let bytes = std::fs::read(root.join(relative))
        .map_err(|_| Spec030ReleaseArtifactError::ArtifactMismatch)?;
    let parsed = serde_json::from_slice::<T>(&bytes)
        .map_err(|_| Spec030ReleaseArtifactError::ArtifactMismatch)?;
    let canonical = serde_json::to_vec_pretty(&parsed)
        .map_err(|_| Spec030ReleaseArtifactError::ArtifactMismatch)?;
    if &parsed != expected || canonical != bytes {
        return Err(Spec030ReleaseArtifactError::ArtifactMismatch);
    }
    Ok(())
}

fn validate_artifact_manifest(
    root: &Path,
    source: &Spec030SourceManifest,
) -> Result<(), Spec030ReleaseArtifactError> {
    let bytes = std::fs::read(root.join(ARTIFACT_MANIFEST_PATH))
        .map_err(|_| Spec030ReleaseArtifactError::ManifestMismatch)?;
    let parsed = serde_json::from_slice::<Spec030ArtifactManifest>(&bytes)
        .map_err(|_| Spec030ReleaseArtifactError::ManifestMismatch)?;
    let canonical = serde_json::to_vec_pretty(&parsed)
        .map_err(|_| Spec030ReleaseArtifactError::ManifestMismatch)?;
    if canonical != bytes {
        return Err(Spec030ReleaseArtifactError::ManifestMismatch);
    }
    let rebuilt = build_spec030_artifact_manifest(root, source)
        .map_err(|_| Spec030ReleaseArtifactError::ManifestMismatch)?;
    let parsed_paths = parsed
        .files
        .iter()
        .map(|file| (&file.path, file.bytes))
        .collect::<Vec<_>>();
    let rebuilt_paths = rebuilt
        .files
        .iter()
        .map(|file| (&file.path, file.bytes))
        .collect::<Vec<_>>();
    if parsed.schema != rebuilt.schema
        || parsed.git_head != rebuilt.git_head
        || parsed.source_digest != rebuilt.source_digest
        || parsed_paths != rebuilt_paths
    {
        return Err(Spec030ReleaseArtifactError::ManifestMismatch);
    }
    if parsed != rebuilt {
        return Err(Spec030ReleaseArtifactError::ArtifactMismatch);
    }
    for file in &parsed.files {
        if file.path.ends_with(".json") {
            let bytes = std::fs::read(root.join(&file.path))
                .map_err(|_| Spec030ReleaseArtifactError::ArtifactMismatch)?;
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|_| Spec030ReleaseArtifactError::ArtifactMismatch)?;
        }
    }
    Ok(())
}
