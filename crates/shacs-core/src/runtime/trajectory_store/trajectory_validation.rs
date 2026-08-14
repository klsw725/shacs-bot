use super::{RecordedTrajectoryRecord, RecordedTrajectoryStoreError};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path};

pub(super) fn read_verified(
    root: &Path,
    locator: &str,
    expected: &str,
) -> Result<Vec<u8>, RecordedTrajectoryStoreError> {
    let relative = safe_relative(locator)?;
    let canonical = root
        .join(relative)
        .canonicalize()
        .map_err(RecordedTrajectoryStoreError::Io)?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(RecordedTrajectoryStoreError::InvalidRecord);
    }
    let bytes = fs::read(canonical).map_err(RecordedTrajectoryStoreError::Io)?;
    if digest(&bytes) != expected {
        return Err(RecordedTrajectoryStoreError::DigestMismatch);
    }
    Ok(bytes)
}

pub(super) fn safe_relative(locator: &str) -> Result<&Path, RecordedTrajectoryStoreError> {
    let path = Path::new(locator);
    if locator.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(RecordedTrajectoryStoreError::InvalidRecord);
    }
    Ok(path)
}

pub(super) fn validate_id(value: &str) -> Result<(), RecordedTrajectoryStoreError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(RecordedTrajectoryStoreError::InvalidId);
    }
    Ok(())
}

pub(super) fn locator_to_string(path: &Path) -> Result<String, RecordedTrajectoryStoreError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(RecordedTrajectoryStoreError::InvalidRecord)
}

pub(super) fn record_digest(
    record: &RecordedTrajectoryRecord,
) -> Result<String, RecordedTrajectoryStoreError> {
    let mut unsigned = record.clone();
    unsigned.record_digest.clear();
    serde_json::to_vec(&unsigned)
        .map(|bytes| digest(&bytes))
        .map_err(RecordedTrajectoryStoreError::Json)
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
