use super::source_manifest::{sha256_bytes, Spec030SourceManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

pub const SPEC030_ARTIFACT_MANIFEST_SCHEMA: &str = "spec030.artifact_manifest.v1";
pub const ARTIFACT_MANIFEST_PATH: &str = "artifact-manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ArtifactManifest {
    pub schema: String,
    pub git_head: String,
    pub source_digest: String,
    pub root_digest: String,
    pub files: Vec<Spec030ArtifactFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030ArtifactFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030ArtifactManifestError {
    Io,
    UnsafePath,
}

impl Display for Spec030ArtifactManifestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec030ArtifactManifestError {}

pub fn build_spec030_artifact_manifest(
    evidence_root: &Path,
    source: &Spec030SourceManifest,
) -> Result<Spec030ArtifactManifest, Spec030ArtifactManifestError> {
    let mut paths = Vec::new();
    collect_files(evidence_root, evidence_root, &mut paths)?;
    paths.sort();
    let files = paths
        .into_iter()
        .map(|relative| artifact_file(evidence_root, relative))
        .collect::<Result<Vec<_>, _>>()?;
    let mut root = Sha256::new();
    root.update(source.git_head.as_bytes());
    root.update([0]);
    root.update(source.source_digest.as_bytes());
    root.update([0]);
    for file in &files {
        root.update(file.path.as_bytes());
        root.update([0]);
        root.update(file.sha256.as_bytes());
        root.update([0]);
        root.update(file.bytes.to_string().as_bytes());
        root.update(*b"\n");
    }
    Ok(Spec030ArtifactManifest {
        schema: SPEC030_ARTIFACT_MANIFEST_SCHEMA.to_owned(),
        git_head: source.git_head.clone(),
        source_digest: source.source_digest.clone(),
        root_digest: sha256_bytes(&root.finalize()),
        files,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), Spec030ArtifactManifestError> {
    for entry in std::fs::read_dir(directory).map_err(|_| Spec030ArtifactManifestError::Io)? {
        let entry = entry.map_err(|_| Spec030ArtifactManifestError::Io)?;
        let file_type = entry
            .file_type()
            .map_err(|_| Spec030ArtifactManifestError::Io)?;
        if file_type.is_symlink() {
            return Err(Spec030ArtifactManifestError::UnsafePath);
        }
        if file_type.is_dir() {
            collect_files(root, &entry.path(), paths)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| Spec030ArtifactManifestError::UnsafePath)?
                .to_path_buf();
            if relative != Path::new(ARTIFACT_MANIFEST_PATH) {
                paths.push(relative);
            }
        } else {
            return Err(Spec030ArtifactManifestError::UnsafePath);
        }
    }
    Ok(())
}

fn artifact_file(
    root: &Path,
    relative: PathBuf,
) -> Result<Spec030ArtifactFile, Spec030ArtifactManifestError> {
    let path = relative
        .to_str()
        .ok_or(Spec030ArtifactManifestError::UnsafePath)?
        .replace('\\', "/");
    let bytes =
        std::fs::read(root.join(&relative)).map_err(|_| Spec030ArtifactManifestError::Io)?;
    let byte_count = u64::try_from(bytes.len()).map_err(|_| Spec030ArtifactManifestError::Io)?;
    Ok(Spec030ArtifactFile {
        path,
        sha256: sha256_bytes(&bytes),
        bytes: byte_count,
    })
}
