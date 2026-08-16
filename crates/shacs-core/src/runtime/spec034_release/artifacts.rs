use super::model::*;
use super::source::validate_locator;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024;

pub fn digest_file(path: &Path) -> Result<String, Spec034ReleaseArtifactError> {
    let metadata = std::fs::metadata(path).map_err(Spec034ReleaseArtifactError::Io)?;
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let bytes = std::fs::read(path).map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub fn write_json(
    root: &Path,
    locator: &str,
    value: &impl serde::Serialize,
) -> Result<(), Spec034ReleaseArtifactError> {
    validate_locator(locator)?;
    let path = root.join(locator);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Spec034ReleaseArtifactError::Io)?;
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(Spec034ReleaseArtifactError::Json)?;
    std::fs::write(path, bytes).map_err(Spec034ReleaseArtifactError::Io)
}

pub fn read_json<T: serde::de::DeserializeOwned>(
    root: &Path,
    locator: &str,
) -> Result<T, Spec034ReleaseArtifactError> {
    let path = validated_file(root, locator)?;
    let bytes = std::fs::read(path).map_err(Spec034ReleaseArtifactError::Io)?;
    serde_json::from_slice(&bytes).map_err(Spec034ReleaseArtifactError::Json)
}

pub fn collect_digests(root: &Path) -> Result<Vec<DigestRow>, Spec034ReleaseArtifactError> {
    let mut rows = Vec::new();
    collect_dir(root, root, &mut rows)?;
    rows.sort_by(|left, right| left.locator.cmp(&right.locator));
    Ok(rows)
}

fn collect_dir(
    root: &Path,
    directory: &Path,
    rows: &mut Vec<DigestRow>,
) -> Result<(), Spec034ReleaseArtifactError> {
    for entry in std::fs::read_dir(directory).map_err(Spec034ReleaseArtifactError::Io)? {
        let entry = entry.map_err(Spec034ReleaseArtifactError::Io)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(Spec034ReleaseArtifactError::Io)?;
        if file_type.is_symlink() {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        if file_type.is_dir() {
            collect_dir(root, &path, rows)?;
        } else if file_type.is_file() && entry.file_name() != "manifest.json" {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
            let locator = relative.to_string_lossy().replace('\\', "/");
            validate_locator(&locator)?;
            rows.push(DigestRow {
                locator,
                digest: digest_file(&path)?,
            });
        } else if !file_type.is_file() {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
    }
    Ok(())
}

pub fn validate_digest_rows(
    root: &Path,
    rows: &[DigestRow],
) -> Result<(), Spec034ReleaseArtifactError> {
    let actual = collect_digests(root)?;
    if actual != rows {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    for row in rows {
        let path = validated_file(root, &row.locator)?;
        if digest_file(&path)? != row.digest {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
    }
    Ok(())
}

pub fn validated_file(root: &Path, locator: &str) -> Result<PathBuf, Spec034ReleaseArtifactError> {
    validate_locator(locator)?;
    let root_metadata = std::fs::symlink_metadata(root).map_err(Spec034ReleaseArtifactError::Io)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let canonical_root = root
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let path = root.join(locator);
    let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let canonical = path
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if !canonical.starts_with(canonical_root) {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(canonical)
}
