//! Atomic writes of a config file that commands patch as JSON (Spec 048
//! PRD 050 scope 4): an owner-only temporary file beside it, synced, then
//! renamed over it.

use serde_json::Value;
use std::path::Path;
use std::{error, fmt, fs, io};

#[derive(Debug)]
pub enum ConfigWriteError {
    /// The file is no longer what the command read first, so nothing was
    /// written.
    Changed,
    Io(io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for ConfigWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed => formatter.write_str("the config file changed meanwhile"),
            Self::Io(error) => write!(formatter, "config write failed: {error}"),
            Self::Json(error) => write!(formatter, "config JSON failed: {error}"),
        }
    }
}

impl error::Error for ConfigWriteError {}

impl From<io::Error> for ConfigWriteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ConfigWriteError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn write_config_value_atomically(path: &Path, value: &Value) -> Result<(), ConfigWriteError> {
    let temp_path = write_temp(path, value)?;
    rename_or_remove(&temp_path, path)
}

/// Replaces the file only if it still holds `expected`, the bytes the
/// command read first; `None` means it did not exist and must still not.
pub fn write_config_value_if_unchanged(
    path: &Path,
    expected: Option<&[u8]>,
    value: &Value,
) -> Result<(), ConfigWriteError> {
    write_if_unchanged_with_hook(path, expected, value, || {})
}

/// `before_compare` runs after the temporary file is synced and before the
/// file is read again, where a test changes the file.
pub(crate) fn write_if_unchanged_with_hook(
    path: &Path,
    expected: Option<&[u8]>,
    value: &Value,
    before_compare: impl FnOnce(),
) -> Result<(), ConfigWriteError> {
    let temp_path = write_temp(path, value)?;
    before_compare();
    let current = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
    };
    if current.as_deref() != expected {
        let _ = fs::remove_file(&temp_path);
        return Err(ConfigWriteError::Changed);
    }
    rename_or_remove(&temp_path, path)
}

fn write_temp(path: &Path, value: &Value) -> Result<std::path::PathBuf, ConfigWriteError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    Ok(crate::write_secret_temp_file(
        path,
        format!("{text}\n").as_bytes(),
    )?)
}

fn rename_or_remove(temp_path: &Path, path: &Path) -> Result<(), ConfigWriteError> {
    fs::rename(temp_path, path).map_err(|error| {
        let _ = fs::remove_file(temp_path);
        ConfigWriteError::Io(error)
    })
}

#[cfg(test)]
#[path = "config_write_tests.rs"]
mod tests;
