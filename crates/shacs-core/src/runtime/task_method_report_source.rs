use super::task_method_calc::{bounded, bounded_text};
use super::task_method_report::{
    ReportSource, SourceKind, SourceStatus, LOCATOR_LIMIT, TEXT_LIMIT,
};
use super::task_method_report_parse::locator_key;
use serde_json::Value;
use std::path::Path;

const FAILED_PREFIXES: [&str; 4] = [
    "Error",
    "Permission denied",
    "[Tool call skipped",
    "[Tool result unavailable",
];

/// A workspace file as it was when read.
pub(crate) struct FileRead {
    pub(crate) requested: String,
    pub(crate) path: String,
    pub(crate) relative: String,
    pub(crate) digest: String,
}

/// Why a read or write result is a failure: an error-style prefix, or a
/// JSON object with a top-level `error` (how `web_fetch` reports failures).
pub(crate) fn failure(output: &str) -> Option<String> {
    let head = output.trim_start();
    if FAILED_PREFIXES
        .iter()
        .any(|prefix| head.starts_with(prefix))
    {
        return Some(bounded(
            head.lines().next().unwrap_or("").trim(),
            TEXT_LIMIT,
        ));
    }
    if !head.starts_with('{') {
        return None;
    }
    let error = match serde_json::from_str::<Value>(head).ok()?.get("error")? {
        Value::Null | Value::Bool(false) => return None,
        Value::String(text) => text.trim().to_owned(),
        other => other.to_string(),
    };
    Some(if error.is_empty() {
        "the tool reported an error".to_owned()
    } else {
        bounded(&error, TEXT_LIMIT)
    })
}

#[cfg(test)]
pub(crate) fn source(
    call_id: &str,
    kind: SourceKind,
    locator: &str,
    output: &str,
    file: Option<FileRead>,
) -> ReportSource {
    read_source(call_id, kind, locator, output, file, failure(output))
}

pub(crate) fn read_source(
    call_id: &str,
    kind: SourceKind,
    locator: &str,
    output: &str,
    file: Option<FileRead>,
    error: Option<String>,
) -> ReportSource {
    let file = file.filter(|_| error.is_none());
    let mut match_keys = vec![locator_key(locator)];
    if let Some(file) = &file {
        match_keys.push(locator_key(&file.relative));
        match_keys.push(locator_key(&file.path));
    }
    match_keys.sort();
    match_keys.dedup();
    ReportSource {
        seq: 0,
        call_id: bounded(call_id, LOCATOR_LIMIT),
        selection_revision: 0,
        version: 0,
        carried: false,
        kind,
        locator: bounded(locator.trim(), LOCATOR_LIMIT),
        match_keys,
        status: if error.is_some() {
            SourceStatus::Failed
        } else {
            SourceStatus::Read
        },
        content_digest: error.is_none().then(|| digest(output.as_bytes())),
        file_digest: file.as_ref().map(|file| file.digest.clone()),
        resolved_path: file.as_ref().map(|file| bounded_text(&file.path, 1024)),
        requested_path: file.map(|file| bounded_text(&file.requested, 1024)),
        read_at: chrono::Utc::now().to_rfc3339(),
        error,
    }
}

pub(crate) fn file_state(workspace: Option<&Path>, locator: &str) -> Option<FileRead> {
    let workspace = std::fs::canonicalize(workspace?).ok()?;
    let candidate = Path::new(locator.trim());
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    };
    let resolved = std::fs::canonicalize(&joined).ok()?;
    let relative = resolved.strip_prefix(&workspace).ok()?;
    if !resolved.is_file() {
        return None;
    }
    let bytes = std::fs::read(&resolved).ok()?;
    Some(FileRead {
        requested: joined.to_string_lossy().into_owned(),
        path: resolved.to_string_lossy().into_owned(),
        relative: relative.to_string_lossy().into_owned(),
        digest: digest(&bytes),
    })
}

pub(crate) fn freshness(source: &ReportSource) -> super::task_method_report_eval::Freshness {
    use super::task_method_report_eval::Freshness;
    let (Some(path), Some(expected)) = (&source.resolved_path, &source.file_digest) else {
        return Freshness::Unknown;
    };
    // The path as read must still resolve to the same file (a re-pointed or
    // swapped-in symlink counts as changed and is never followed).
    let requested = source.requested_path.as_deref().unwrap_or(path);
    if !std::fs::canonicalize(requested).is_ok_and(|resolved| resolved == Path::new(path)) {
        return Freshness::Changed;
    }
    match std::fs::read(path) {
        Ok(bytes) if &digest(&bytes) == expected => Freshness::Unchanged,
        Ok(_) | Err(_) => Freshness::Changed,
    }
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}
