use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigMigrationFileState {
    Missing,
    Original,
    Result,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigMigrationOperation {
    Apply,
    Recover,
}

pub(crate) fn classify_file(
    path: &Path,
    original_digest: &str,
    result_digest: &str,
) -> Result<ConfigMigrationFileState, std::io::Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(ConfigMigrationFileState::Missing);
        }
        Err(error) => return Err(error),
    };
    let current_digest = digest(&bytes);
    Ok(if current_digest == original_digest {
        ConfigMigrationFileState::Original
    } else if current_digest == result_digest {
        ConfigMigrationFileState::Result
    } else {
        ConfigMigrationFileState::Unknown
    })
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
