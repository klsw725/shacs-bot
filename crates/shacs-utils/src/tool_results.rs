use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::text::{safe_filename, stringify_text_blocks, truncate_text};
use shacs_redaction::{redact_string, redact_value};

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
    let payload = content_text_payload(content)?;
    if payload.text.chars().count() <= max_chars {
        return Some(content.clone());
    }
    let original_size = payload.text.chars().count();
    let Some(workspace) = workspace else {
        return Some(Value::String(payload.fallback_text(max_chars)));
    };
    match persist_tool_result(
        workspace,
        session_key.unwrap_or("default"),
        tool_call_id,
        &payload.redacted_file,
        &payload.redacted_preview,
        payload.suffix,
        original_size,
    ) {
        Ok(persisted) => Some(Value::String(persisted.reference_text())),
        Err(_) => Some(Value::String(payload.fallback_text(max_chars))),
    }
}

struct ToolResultPayload {
    text: String,
    redacted_file: String,
    redacted_preview: String,
    suffix: &'static str,
}

impl ToolResultPayload {
    fn fallback_text(&self, max_chars: usize) -> String {
        truncate_text(&self.redacted_preview, max_chars)
    }
}

fn content_text_payload(content: &Value) -> Option<ToolResultPayload> {
    match content {
        Value::String(text) => Some(ToolResultPayload {
            text: text.clone(),
            redacted_file: redact_string(text),
            redacted_preview: redact_string(text),
            suffix: "txt",
        }),
        Value::Array(blocks) => stringify_text_blocks(blocks).map(|text| {
            let redacted = redact_value(content);
            let redacted_file =
                serde_json::to_string_pretty(&redacted).unwrap_or_else(|_| redacted.to_string());
            let redacted_preview = match &redacted {
                Value::Array(redacted_blocks) => {
                    stringify_text_blocks(redacted_blocks).unwrap_or_else(|| redact_string(&text))
                }
                _ => redact_string(&text),
            };
            ToolResultPayload {
                text,
                redacted_file,
                redacted_preview,
                suffix: "json",
            }
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
    original_size: usize,
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
        original_size,
        truncated_preview: original_size > TOOL_RESULT_PREVIEW_CHARS,
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
    fn redacts_oversized_string_tool_result_in_file_and_reference() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-redacted-string-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let secret = "OPENAI_API_KEY=sk-secret-token visible text";
        let reference = maybe_persist_text_tool_result(Some(&root), None, "call", secret, 3)
            .ok_or_else(|| "missing reference".to_owned())?;
        let stored =
            fs::read_to_string(root.join(TOOL_RESULTS_DIR).join("default").join("call.txt"))
                .map_err(|error| error.to_string())?;
        assert!(stored.contains(shacs_redaction::REDACTED));
        assert!(reference.contains(shacs_redaction::REDACTED));
        assert!(!stored.contains("sk-secret-token"));
        assert!(!reference.contains("sk-secret-token"));
        Ok(())
    }

    #[test]
    fn redacts_json_like_oversized_string_tool_result_in_file_and_reference() -> Result<(), String>
    {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-redacted-json-like-string-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let secret = r#"before {"api_key":"plain-secret"} after with enough output to persist"#;
        let reference = maybe_persist_text_tool_result(Some(&root), None, "call", secret, 24)
            .ok_or_else(|| "missing reference".to_owned())?;
        let stored =
            fs::read_to_string(root.join(TOOL_RESULTS_DIR).join("default").join("call.txt"))
                .map_err(|error| error.to_string())?;
        assert!(stored.contains("before"));
        assert!(stored.contains(shacs_redaction::REDACTED));
        assert!(reference.contains(shacs_redaction::REDACTED));
        assert!(!stored.contains("plain-secret"));
        assert!(!reference.contains("plain-secret"));
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

    #[test]
    fn redacts_oversized_text_block_json_in_file_and_reference() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-redacted-json-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let content = json!([
            {"type": "text", "text": "safe output"},
            {"type": "text", "text": "Authorization: Bearer ghp_secret_token"}
        ]);
        let reference = maybe_persist_tool_result(Some(&root), None, "call", &content, 3)
            .ok_or_else(|| "missing reference".to_owned())?;
        let reference = reference
            .as_str()
            .ok_or_else(|| "reference was not string".to_owned())?;
        let stored = fs::read_to_string(
            root.join(TOOL_RESULTS_DIR)
                .join("default")
                .join("call.json"),
        )
        .map_err(|error| error.to_string())?;
        assert!(stored.contains(shacs_redaction::REDACTED));
        assert!(reference.contains(shacs_redaction::REDACTED));
        assert!(!stored.contains("ghp_secret_token"));
        assert!(!reference.contains("ghp_secret_token"));
        Ok(())
    }

    #[test]
    fn redacts_json_like_text_block_payload_in_file_and_reference() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-redacted-json-like-block-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let content = json!([
            {"type": "text", "text": "safe output"},
            {"type": "text", "text": "client_secret: \"plain-secret\" after"}
        ]);
        let reference = maybe_persist_tool_result(Some(&root), None, "call", &content, 3)
            .ok_or_else(|| "missing reference".to_owned())?;
        let reference = reference
            .as_str()
            .ok_or_else(|| "reference was not string".to_owned())?;
        let stored = fs::read_to_string(
            root.join(TOOL_RESULTS_DIR)
                .join("default")
                .join("call.json"),
        )
        .map_err(|error| error.to_string())?;
        assert!(stored.contains("safe output"));
        assert!(stored.contains(shacs_redaction::REDACTED));
        assert!(reference.contains(shacs_redaction::REDACTED));
        assert!(!stored.contains("plain-secret"));
        assert!(!reference.contains("plain-secret"));
        Ok(())
    }

    #[test]
    fn redacts_oversized_result_without_workspace_instead_of_falling_back_to_raw(
    ) -> Result<(), String> {
        let secret =
            r#"before {"api_key":"plain-secret"} after with enough extra output to persist"#;
        let result = maybe_persist_text_tool_result(None, None, "call", secret, 64)
            .ok_or_else(|| "missing fallback".to_owned())?;
        assert!(result.contains("api_key"));
        assert!(result.contains(shacs_redaction::REDACTED));
        assert!(result.contains("after"));
        assert!(!result.contains("plain-secret"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn redacts_json_like_text_block_payload_when_persistence_fails() -> Result<(), String> {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-json-like-fail-closed-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        let bucket = root.join(TOOL_RESULTS_DIR).join("default");
        fs::create_dir_all(&bucket).map_err(|error| error.to_string())?;
        let outside = root.join("outside.json");
        fs::write(&outside, "outside").map_err(|error| error.to_string())?;
        symlink(&outside, bucket.join("call.json")).map_err(|error| error.to_string())?;

        let content = json!([
            {"type": "text", "text": "safe output"},
            {"type": "text", "text": "client_secret: \"plain-secret\" after with enough output"}
        ]);
        let result = maybe_persist_tool_result(Some(&root), None, "call", &content, 64)
            .ok_or_else(|| "missing fail-closed fallback".to_owned())?;
        let result = result
            .as_str()
            .ok_or_else(|| "fallback was not string".to_owned())?;
        assert!(result.contains("safe output"));
        assert!(result.contains(shacs_redaction::REDACTED));
        assert!(!result.contains("plain-secret"));
        assert_eq!(
            fs::read_to_string(outside).map_err(|error| error.to_string())?,
            "outside"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn redacts_oversized_result_when_persistence_fails() -> Result<(), String> {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-fail-closed-{}",
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

        let result = maybe_persist_text_tool_result(
            Some(&root),
            None,
            "call",
            "before Authorization: Bearer ghp_secret_token after with enough extra output to persist",
            64,
        )
        .ok_or_else(|| "missing fail-closed fallback".to_owned())?;
        assert!(result.contains("before Authorization: Bearer"));
        assert!(result.contains(shacs_redaction::REDACTED));
        assert!(result.contains("after"));
        assert!(!result.contains("ghp_secret_token"));
        assert_eq!(
            fs::read_to_string(outside).map_err(|error| error.to_string())?,
            "outside"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_leaf_symlink_and_returns_safe_fallback() -> Result<(), String> {
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
        let result = maybe_persist_text_tool_result(Some(&root), None, "call", "abcdef", 3)
            .ok_or_else(|| "missing fail-closed fallback".to_owned())?;
        assert_eq!(result, "abc\n... (truncated)");
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
