use super::model::Spec030ReleaseArtifactError;
use super::source_manifest::sha256_bytes;
use crate::Spec031ReleaseCommandRecord;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SPEC030_CLEANUP_SCHEMA: &str = "spec030.cleanup.v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030CleanupReceipt {
    pub schema: String,
    pub processes_started: u64,
    pub processes_remaining: u64,
    pub temporary_artifacts_removed: u64,
    pub temporary_artifacts_remaining: u64,
    pub command_receipts_sha256: String,
}

pub(super) fn cleanup_receipt(
    commands: &[Spec031ReleaseCommandRecord],
    evidence_root: &Path,
    extra_removed: u64,
) -> Result<Spec030CleanupReceipt, Spec030ReleaseArtifactError> {
    let mut processes_remaining = 0_u64;
    let mut temporary_artifacts_removed = extra_removed;
    let mut temporary_artifacts_remaining = 0_u64;
    for command in commands {
        let receipt = command
            .process_receipt
            .as_ref()
            .ok_or(Spec030ReleaseArtifactError::InvalidCleanupRecord)?;
        processes_remaining += u64::from(!receipt.reaped);
        for (path, suffix) in [
            (&receipt.stdout_temp_path, "stdout"),
            (&receipt.stderr_temp_path, "stderr"),
        ] {
            if command_temp_path(evidence_root, &command.id, path, suffix)?.exists() {
                temporary_artifacts_remaining += 1;
            } else {
                temporary_artifacts_removed += 1;
            }
        }
    }
    Ok(Spec030CleanupReceipt {
        schema: SPEC030_CLEANUP_SCHEMA.to_owned(),
        processes_started: u64::try_from(commands.len())
            .map_err(|_| Spec030ReleaseArtifactError::InvalidCleanupRecord)?,
        processes_remaining,
        temporary_artifacts_removed,
        temporary_artifacts_remaining,
        command_receipts_sha256: command_receipts_hash(commands)?,
    })
}

pub fn validate_spec030_cleanup_receipt(
    path: &Path,
) -> Result<Spec030CleanupReceipt, Spec030ReleaseArtifactError> {
    let bytes =
        std::fs::read(path).map_err(|_| Spec030ReleaseArtifactError::InvalidCleanupRecord)?;
    let receipt = serde_json::from_slice::<Spec030CleanupReceipt>(&bytes)
        .map_err(|_| Spec030ReleaseArtifactError::InvalidCleanupRecord)?;
    if receipt.schema != SPEC030_CLEANUP_SCHEMA
        || receipt.processes_started == 0
        || receipt.processes_remaining != 0
        || receipt.temporary_artifacts_remaining != 0
        || !valid_sha256(&receipt.command_receipts_sha256)
    {
        return Err(Spec030ReleaseArtifactError::InvalidCleanupRecord);
    }
    Ok(receipt)
}

pub(super) fn command_temp_path(
    root: &Path,
    command_id: &str,
    relative: &str,
    suffix: &str,
) -> Result<std::path::PathBuf, Spec030ReleaseArtifactError> {
    let path = Path::new(relative);
    let prefix = format!(".{command_id}.{suffix}.tmp.");
    if path.is_absolute()
        || path.parent() != Some(Path::new("commands"))
        || !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&prefix))
    {
        return Err(Spec030ReleaseArtifactError::InvalidCleanupRecord);
    }
    Ok(root.join(path))
}

pub(super) fn process_is_live(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(target_os = "linux")]
    return Path::new("/proc").join(pid.to_string()).exists();
    #[cfg(not(target_os = "linux"))]
    false
}

fn command_receipts_hash(
    commands: &[Spec031ReleaseCommandRecord],
) -> Result<String, Spec030ReleaseArtifactError> {
    let values = commands
        .iter()
        .map(|command| serde_json::json!({"id":command.id,"receipt":command.process_receipt}))
        .collect::<Vec<_>>();
    serde_json::to_vec(&values)
        .map(|bytes| sha256_bytes(&bytes))
        .map_err(|_| Spec030ReleaseArtifactError::InvalidCleanupRecord)
}

fn valid_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
