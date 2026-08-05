use super::SurfaceActionError;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RUNTIME_MARKER_MAX_BYTES: u64 = 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeOwnershipMarker {
    pub(super) owner_id: String,
    pub(super) pid: u32,
    pub(super) expires_at_ms: u64,
    pub(super) process_evidence: RuntimeOwnerProcessEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RuntimeOwnerProcessEvidence {
    pub(super) pid_alive: bool,
    pub(super) process_started_after_marker: bool,
}

pub(super) fn runtime_stop_request_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("stop-request.json")
}

pub(super) fn runtime_ownership_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("ownership-marker.json")
}

pub(super) fn read_runtime_ownership_marker(
    path: &Path,
) -> Result<Option<RuntimeOwnershipMarker>, SurfaceActionError> {
    let Some(value) = read_runtime_marker_json(path)? else {
        return Ok(None);
    };
    let schema_version = required_marker_u64(&value, "schema_version")? as u32;
    if schema_version != 1 {
        return Err(SurfaceActionError::InvalidMarker(format!(
            "runtime ownership marker has unsupported schema_version {schema_version}"
        )));
    }
    let owner_id = required_marker_string(&value, "owner_id")?;
    let pid = required_marker_u32(&value, "pid")?;
    let started_at_ms = required_marker_u64(&value, "acquired_at_ms")?;
    let updated_at_ms = required_marker_u64(&value, "renewed_at_ms")?;
    let expires_at_ms = required_marker_u64(&value, "expires_at_ms")?;
    if expires_at_ms <= updated_at_ms || owner_id != runtime_owner_id(pid, started_at_ms) {
        return Err(SurfaceActionError::InvalidMarker(
            "runtime ownership marker has invalid owner lease identity".to_owned(),
        ));
    }
    Ok(Some(RuntimeOwnershipMarker {
        owner_id,
        pid,
        expires_at_ms,
        process_evidence: RuntimeOwnerProcessEvidence {
            pid_alive: pid_is_alive(pid),
            process_started_after_marker: false,
        },
    }))
}

pub(super) fn runtime_stop_request_marker_value(
    request: &str,
    request_id: &str,
    owner_pid: Option<u32>,
    target_owner_id: Option<&str>,
    event_sequence: u64,
    requested_at_ms: u64,
) -> Value {
    json!({
        "schema_version": 1,
        "request": request,
        "request_id": request_id,
        "requested_at_ms": requested_at_ms,
        "owner_pid": owner_pid,
        "target_owner_id": target_owner_id,
        "event_sequence": event_sequence,
    })
}

pub(super) fn write_runtime_marker_atomically(
    path: &Path,
    value: &Value,
) -> Result<(), SurfaceActionError> {
    let parent = path.parent().ok_or_else(|| {
        SurfaceActionError::InvalidMarker("runtime marker path has no parent directory".to_owned())
    })?;
    fs::create_dir_all(parent)?;
    let temp_path = path.with_extension(format!(
        "json.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        Ok::<_, SurfaceActionError>(())
    })();
    if let Err(error) = write_result {
        let _cleanup_result = fs::remove_file(&temp_path);
        return Err(error);
    }
    Ok(())
}

pub(super) fn read_runtime_marker_json(path: &Path) -> Result<Option<Value>, SurfaceActionError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SurfaceActionError::Io(error)),
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() || metadata.len() > RUNTIME_MARKER_MAX_BYTES {
        return Err(SurfaceActionError::InvalidMarker(format!(
            "runtime marker is not readable: {}",
            path.display()
        )));
    }
    let mut raw = String::new();
    file.take(RUNTIME_MARKER_MAX_BYTES + 1)
        .read_to_string(&mut raw)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

fn required_marker_string(value: &Value, key: &str) -> Result<String, SurfaceActionError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| SurfaceActionError::InvalidMarker(format!("runtime marker missing `{key}`")))
}

fn required_marker_u64(value: &Value, key: &str) -> Result<u64, SurfaceActionError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| SurfaceActionError::InvalidMarker(format!("runtime marker missing `{key}`")))
}

fn required_marker_u32(value: &Value, key: &str) -> Result<u32, SurfaceActionError> {
    required_marker_u64(value, key)?.try_into().map_err(|_| {
        SurfaceActionError::InvalidMarker(format!("runtime marker `{key}` is too large"))
    })
}

fn runtime_owner_id(pid: u32, acquired_at_ms: u64) -> String {
    format!("owner-{pid}-{acquired_at_ms}")
}

fn pid_is_alive(pid: u32) -> bool {
    let Ok(raw_pid) = i32::try_from(pid) else {
        return false;
    };
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(raw_pid), None).is_ok()
}
