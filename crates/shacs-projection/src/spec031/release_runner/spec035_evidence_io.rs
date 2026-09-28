use super::model::Spec031ReleaseArtifactError;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) fn read_json<T: serde::de::DeserializeOwned>(
    root: &Path,
    relative: &str,
) -> Result<T, Spec031ReleaseArtifactError> {
    let bytes = fs::read(safe_file(root, relative)?)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| evidence_error(format!("invalid JSON evidence: {relative}")))
}

pub(super) fn read_text(
    root: &Path,
    relative: &str,
) -> Result<String, Spec031ReleaseArtifactError> {
    fs::read_to_string(safe_file(root, relative)?)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)
}

pub(super) fn safe_file(
    root: &Path,
    relative: &str,
) -> Result<PathBuf, Spec031ReleaseArtifactError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    let full = root.join(path);
    let metadata = fs::symlink_metadata(&full)
        .map_err(|_| Spec031ReleaseArtifactError::MissingRequiredArtifact)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Spec031ReleaseArtifactError::InvalidArtifactPath);
    }
    Ok(full)
}

pub(super) fn require_inventory(
    inventory: &HashMap<String, String>,
    relative: &str,
) -> Result<(), Spec031ReleaseArtifactError> {
    if inventory.contains_key(relative) {
        Ok(())
    } else {
        Err(evidence_error(format!(
            "SHA-256 inventory is missing {relative}"
        )))
    }
}

pub(super) fn evidence_error(detail: impl Into<String>) -> Spec031ReleaseArtifactError {
    Spec031ReleaseArtifactError::Spec035Evidence(detail.into())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
