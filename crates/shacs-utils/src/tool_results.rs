use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::text::{safe_filename, stringify_text_blocks, truncate_text};

pub const TOOL_RESULTS_DIR: &str = ".nanobot/tool-results";
pub const TOOL_RESULT_PREVIEW_CHARS: usize = 1_200;
pub const TOOL_RESULT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const TOOL_RESULT_MAX_BUCKETS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedToolResult {
    pub path: PathBuf,
    pub original_size: usize,
    pub preview: String,
    pub truncated_preview: bool,
}

impl PersistedToolResult {
    pub fn reference_text(&self) -> String {
        let mut text = format!(
            "[tool output persisted]\nFull output saved to: {}\nOriginal size: {} chars\nPreview:\n{}",
            self.path.display(),
            self.original_size,
            self.preview
        );
        if self.truncated_preview {
            text.push_str("\n...\n(Read the saved file if you need the full output.)");
        }
        text
    }
}

pub fn maybe_persist_text_tool_result(
    workspace: Option<&Path>,
    session_key: Option<&str>,
    tool_call_id: &str,
    content: &str,
    max_chars: usize,
) -> Option<String> {
    maybe_persist_tool_result(
        workspace,
        session_key,
        tool_call_id,
        &Value::String(content.to_owned()),
        max_chars,
    )
    .and_then(|value| value.as_str().map(str::to_owned))
}

pub fn maybe_persist_tool_result(
    workspace: Option<&Path>,
    session_key: Option<&str>,
    tool_call_id: &str,
    content: &Value,
    max_chars: usize,
) -> Option<Value> {
    if max_chars == 0 {
        return Some(content.clone());
    }
    let (text_payload, file_payload, suffix) = content_text_payload(content)?;
    if text_payload.chars().count() <= max_chars {
        return Some(content.clone());
    }
    let workspace = workspace?;
    let persisted = persist_tool_result(
        workspace,
        session_key.unwrap_or("default"),
        tool_call_id,
        &file_payload,
        &text_payload,
        suffix,
    )
    .ok()?;
    Some(Value::String(persisted.reference_text()))
}

fn content_text_payload(content: &Value) -> Option<(String, String, &'static str)> {
    match content {
        Value::String(text) => Some((text.clone(), text.clone(), "txt")),
        Value::Array(blocks) => stringify_text_blocks(blocks).map(|text| {
            let json_payload =
                serde_json::to_string_pretty(content).unwrap_or_else(|_| content.to_string());
            (text, json_payload, "json")
        }),
        _ => None,
    }
}

fn persist_tool_result(
    workspace: &Path,
    session_key: &str,
    tool_call_id: &str,
    file_payload: &str,
    preview_payload: &str,
    suffix: &str,
) -> std::io::Result<PersistedToolResult> {
    let root = workspace.join(TOOL_RESULTS_DIR);
    let bucket = root.join(non_empty_safe_filename(session_key));
    reject_existing_tool_result_symlinks(workspace, &bucket)?;
    fs::create_dir_all(&bucket)?;
    reject_tool_result_symlinks(workspace, &bucket)?;
    cleanup_tool_result_buckets(&root, &bucket).ok();

    let path = bucket.join(format!(
        "{}.{}",
        non_empty_safe_filename(tool_call_id),
        suffix
    ));
    reject_tool_result_leaf_symlink(&path)?;
    if !path.exists() {
        write_text_atomic(&path, file_payload)?;
    }
    let preview = truncate_text(preview_payload, TOOL_RESULT_PREVIEW_CHARS);
    Ok(PersistedToolResult {
        path,
        original_size: preview_payload.chars().count(),
        truncated_preview: preview_payload.chars().count() > TOOL_RESULT_PREVIEW_CHARS,
        preview,
    })
}

fn cleanup_tool_result_buckets(root: &Path, current_bucket: &Path) -> std::io::Result<()> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(());
    };
    let cutoff = SystemTime::now()
        .checked_sub(TOOL_RESULT_RETENTION)
        .unwrap_or(UNIX_EPOCH);
    let mut siblings = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current_bucket || !path.is_dir() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        if modified < cutoff {
            let _ = fs::remove_dir_all(&path);
        } else if path.exists() {
            siblings.push((path, modified));
        }
    }
    let keep = TOOL_RESULT_MAX_BUCKETS.saturating_sub(1);
    if siblings.len() <= keep {
        return Ok(());
    }
    siblings.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    for (path, _) in siblings.into_iter().skip(keep) {
        let _ = fs::remove_dir_all(path);
    }
    Ok(())
}

fn write_text_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let tmp_path = unique_tmp_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }
    Ok(())
}

fn unique_tmp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let process_id = std::process::id();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("tool-result.txt");
    path.with_file_name(format!(".{file_name}.{process_id}.{nanos}.tmp"))
}

fn reject_tool_result_symlinks(workspace: &Path, dir: &Path) -> std::io::Result<()> {
    let workspace = workspace.canonicalize()?;
    let dir = dir.canonicalize()?;
    if !dir.starts_with(&workspace) {
        return Err(permission_denied("tool result path escapes workspace"));
    }
    let relative = dir
        .strip_prefix(&workspace)
        .map_err(|_| permission_denied("tool result path escapes workspace"))?;
    let mut current = workspace;
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(permission_denied(
                "symlink paths are not allowed for tool results",
            ));
        }
    }
    Ok(())
}

fn reject_existing_tool_result_symlinks(workspace: &Path, dir: &Path) -> std::io::Result<()> {
    let relative = dir
        .strip_prefix(workspace)
        .map_err(|_| permission_denied("tool result path escapes workspace"))?;
    let mut current = workspace.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(permission_denied(
                    "symlink paths are not allowed for tool results",
                ));
            }
        }
    }
    Ok(())
}

fn reject_tool_result_leaf_symlink(path: &Path) -> std::io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(permission_denied(
                "symlink files are not allowed for tool results",
            ));
        }
    }
    Ok(())
}

fn permission_denied(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, message)
}

fn non_empty_safe_filename(value: &str) -> String {
    match safe_filename(value).as_str() {
        "" | "." | ".." => "tool-result".to_owned(),
        safe => safe.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn persists_oversized_string_tool_result() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let reference =
            maybe_persist_text_tool_result(Some(&root), Some("session/one"), "call:1", "abcdef", 3)
                .ok_or_else(|| "missing reference".to_owned())?;
        assert!(reference.contains("[tool output persisted]"));
        assert!(reference.contains("Original size: 6 chars"));
        assert!(root
            .join(TOOL_RESULTS_DIR)
            .join("session_one")
            .join("call_1.txt")
            .is_file());
        Ok(())
    }

    #[test]
    fn persists_text_block_arrays_as_json_reference() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-json-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let content = json!([
            {"type": "text", "text": "hello"},
            {"type": "text", "text": "world"}
        ]);
        let reference = maybe_persist_tool_result(Some(&root), None, "call", &content, 3)
            .ok_or_else(|| "missing reference".to_owned())?;
        assert!(reference
            .as_str()
            .unwrap_or_default()
            .contains("hello\nworld"));
        assert!(root
            .join(TOOL_RESULTS_DIR)
            .join("default")
            .join("call.json")
            .is_file());
        let stored = fs::read_to_string(
            root.join(TOOL_RESULTS_DIR)
                .join("default")
                .join("call.json"),
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            serde_json::from_str::<Value>(&stored).map_err(|error| error.to_string())?,
            content
        );
        assert_ne!(stored, "hello\nworld");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_leaf_symlink() -> Result<(), String> {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-symlink-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        let bucket = root.join(TOOL_RESULTS_DIR).join("default");
        fs::create_dir_all(&bucket).map_err(|error| error.to_string())?;
        let outside = root.join("outside.txt");
        fs::write(&outside, "outside").map_err(|error| error.to_string())?;
        symlink(&outside, bucket.join("call.txt")).map_err(|error| error.to_string())?;
        assert_eq!(
            maybe_persist_text_tool_result(Some(&root), None, "call", "abcdef", 3),
            None
        );
        assert_eq!(
            fs::read_to_string(outside).map_err(|error| error.to_string())?,
            "outside"
        );
        Ok(())
    }

    #[test]
    fn ignores_non_text_arrays() {
        let value = json!([{ "type": "image_url", "image_url": {"url": "data:"} }]);
        assert_eq!(
            maybe_persist_tool_result(None, None, "call", &value, 1),
            None
        );
    }
}
