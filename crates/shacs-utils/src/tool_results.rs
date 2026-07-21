use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::text::{stringify_text_blocks, truncate_text};
use shacs_redaction::{redact_string, redact_value};

pub const TOOL_RESULTS_DIR: &str = ".nanobot/tool-results";
pub const TOOL_RESULT_PREVIEW_CHARS: usize = 1_200;
pub const TOOL_RESULT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
pub const TOOL_RESULT_MAX_BUCKETS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultPersistenceDisposition {
    Inline,
    Persisted,
    TruncatedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultArtifactContentKind {
    Text,
    JsonTextBlocks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultArtifactRef {
    pub locator: String,
    pub digest: String,
    pub content_kind: ToolResultArtifactContentKind,
    pub original_size: usize,
    pub preview_truncated: bool,
    pub redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultPersistenceOutcome {
    pub content: Value,
    pub disposition: ToolResultPersistenceDisposition,
    pub artifact: Option<ToolResultArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedToolResult {
    pub path: PathBuf,
    pub original_size: usize,
    pub preview: String,
    pub truncated_preview: bool,
    pub artifact: ToolResultArtifactRef,
}

impl PersistedToolResult {
    pub fn reference_text(&self) -> String {
        let mut text = format!(
            "[tool output persisted]\nFull output saved to: {}\nOriginal size: {} chars\nPreview:\n{}",
            self.artifact.locator,
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
    maybe_persist_tool_result_with_artifact(
        workspace,
        session_key,
        tool_call_id,
        &Value::String(content.to_owned()),
        max_chars,
    )
    .map(|outcome| outcome.content)
    .and_then(|value| value.as_str().map(str::to_owned))
}

pub fn maybe_persist_text_tool_result_with_artifact(
    workspace: Option<&Path>,
    session_key: Option<&str>,
    tool_call_id: &str,
    content: &str,
    max_chars: usize,
) -> Option<ToolResultPersistenceOutcome> {
    maybe_persist_tool_result_with_artifact(
        workspace,
        session_key,
        tool_call_id,
        &Value::String(content.to_owned()),
        max_chars,
    )
}

pub fn maybe_persist_tool_result(
    workspace: Option<&Path>,
    session_key: Option<&str>,
    tool_call_id: &str,
    content: &Value,
    max_chars: usize,
) -> Option<Value> {
    maybe_persist_tool_result_with_artifact(
        workspace,
        session_key,
        tool_call_id,
        content,
        max_chars,
    )
    .map(|outcome| outcome.content)
}

pub fn maybe_persist_tool_result_with_artifact(
    workspace: Option<&Path>,
    session_key: Option<&str>,
    tool_call_id: &str,
    content: &Value,
    max_chars: usize,
) -> Option<ToolResultPersistenceOutcome> {
    if max_chars == 0 {
        return Some(ToolResultPersistenceOutcome {
            content: content.clone(),
            disposition: ToolResultPersistenceDisposition::Inline,
            artifact: None,
        });
    }
    let payload = content_text_payload(content)?;
    if payload.text.chars().count() <= max_chars {
        return Some(ToolResultPersistenceOutcome {
            content: content.clone(),
            disposition: ToolResultPersistenceDisposition::Inline,
            artifact: None,
        });
    }
    let original_size = payload.text.chars().count();
    let Some(workspace) = workspace else {
        return Some(ToolResultPersistenceOutcome {
            content: Value::String(payload.fallback_text(max_chars)),
            disposition: ToolResultPersistenceDisposition::TruncatedFallback,
            artifact: None,
        });
    };
    match persist_tool_result(
        workspace,
        session_key.unwrap_or("default"),
        tool_call_id,
        &payload,
        original_size,
    ) {
        Ok(persisted) => Some(ToolResultPersistenceOutcome {
            content: Value::String(persisted.reference_text()),
            disposition: ToolResultPersistenceDisposition::Persisted,
            artifact: Some(persisted.artifact),
        }),
        Err(_) => Some(ToolResultPersistenceOutcome {
            content: Value::String(payload.fallback_text(max_chars)),
            disposition: ToolResultPersistenceDisposition::TruncatedFallback,
            artifact: None,
        }),
    }
}

struct ToolResultPayload {
    text: String,
    redacted_file: String,
    redacted_preview: String,
    suffix: &'static str,
    content_kind: ToolResultArtifactContentKind,
    redacted: bool,
}

impl ToolResultPayload {
    fn fallback_text(&self, max_chars: usize) -> String {
        truncate_text(&self.redacted_preview, max_chars)
    }
}

fn content_text_payload(content: &Value) -> Option<ToolResultPayload> {
    match content {
        Value::String(text) => {
            let redacted = redact_string(text);
            Some(ToolResultPayload {
                text: text.clone(),
                redacted_file: redacted.clone(),
                redacted_preview: redacted.clone(),
                suffix: "txt",
                content_kind: ToolResultArtifactContentKind::Text,
                redacted: redacted != *text,
            })
        }
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
                content_kind: ToolResultArtifactContentKind::JsonTextBlocks,
                redacted: redacted != *content,
            }
        }),
        _ => None,
    }
}

fn persist_tool_result(
    workspace: &Path,
    session_key: &str,
    tool_call_id: &str,
    payload: &ToolResultPayload,
    original_size: usize,
) -> std::io::Result<PersistedToolResult> {
    let root = workspace.join(TOOL_RESULTS_DIR);
    let relative_path = tool_result_relative_path(session_key, tool_call_id, payload);
    let path = workspace.join(&relative_path);
    let bucket = path
        .parent()
        .ok_or_else(|| permission_denied("tool result path has no parent"))?
        .to_path_buf();
    reject_existing_tool_result_symlinks(workspace, &bucket)?;
    fs::create_dir_all(&bucket)?;
    reject_tool_result_symlinks(workspace, &bucket)?;
    cleanup_tool_result_buckets(&root, &bucket).ok();

    reject_tool_result_leaf_symlink(&path)?;
    if !path.exists() {
        write_text_atomic(&path, &payload.redacted_file)?;
    }
    let persisted_bytes = fs::read(&path)?;
    if persisted_bytes != payload.redacted_file.as_bytes() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "tool result artifact collision",
        ));
    }
    let preview = truncate_text(&payload.redacted_preview, TOOL_RESULT_PREVIEW_CHARS);
    let locator = workspace_relative_locator(workspace, &path)?;
    let truncated_preview = original_size > TOOL_RESULT_PREVIEW_CHARS;
    Ok(PersistedToolResult {
        path,
        original_size,
        truncated_preview,
        preview,
        artifact: ToolResultArtifactRef {
            locator,
            digest: sha256_hex(&persisted_bytes),
            content_kind: payload.content_kind.clone(),
            original_size,
            preview_truncated: truncated_preview,
            redacted: payload.redacted,
        },
    })
}

fn workspace_relative_locator(workspace: &Path, path: &Path) -> std::io::Result<String> {
    let relative = path
        .strip_prefix(workspace)
        .map_err(|_| permission_denied("tool result path escapes workspace"))?;
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(permission_denied("tool result path escapes workspace"));
    }
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn opaque_name(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{prefix}-{digest:x}")
}

fn tool_result_relative_path(
    session_key: &str,
    tool_call_id: &str,
    payload: &ToolResultPayload,
) -> PathBuf {
    let payload_digest = sha256_hex(payload.redacted_file.as_bytes());
    Path::new(TOOL_RESULTS_DIR)
        .join(opaque_name("session", session_key))
        .join(format!(
            "{}-{}.{}",
            opaque_name("call", tool_call_id),
            payload_digest.trim_start_matches("sha256:"),
            payload.suffix
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_root(label: &str) -> Result<PathBuf, String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-tool-results-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        Ok(root)
    }

    fn expected_artifact_path(
        root: &Path,
        session_key: &str,
        tool_call_id: &str,
        content: &Value,
    ) -> Result<PathBuf, String> {
        let payload = content_text_payload(content).ok_or_else(|| "missing payload".to_owned())?;
        Ok(root.join(tool_result_relative_path(
            session_key,
            tool_call_id,
            &payload,
        )))
    }

    #[test]
    fn returns_inline_outcome_for_small_text_tool_result() -> Result<(), String> {
        let outcome = maybe_persist_text_tool_result_with_artifact(
            Some(&temp_root("inline")?),
            None,
            "call",
            "short",
            64,
        )
        .ok_or_else(|| "missing inline outcome".to_owned())?;
        assert_eq!(outcome.content, Value::String("short".to_owned()));
        assert_eq!(
            outcome.disposition,
            ToolResultPersistenceDisposition::Inline
        );
        assert_eq!(outcome.artifact, None);
        Ok(())
    }

    #[test]
    fn persists_oversized_text_with_safe_relative_artifact_ref() -> Result<(), String> {
        let root = temp_root("structured-text")?;
        let outcome = maybe_persist_text_tool_result_with_artifact(
            Some(&root),
            Some("session/one"),
            "call:1",
            "abcdef",
            3,
        )
        .ok_or_else(|| "missing persisted outcome".to_owned())?;
        let artifact = outcome
            .artifact
            .ok_or_else(|| "missing artifact ref".to_owned())?;
        assert_eq!(
            outcome.disposition,
            ToolResultPersistenceDisposition::Persisted
        );
        let expected_path = expected_artifact_path(
            &root,
            "session/one",
            "call:1",
            &Value::String("abcdef".to_owned()),
        )?;
        assert_eq!(root.join(&artifact.locator), expected_path);
        assert!(Path::new(&artifact.locator).is_relative());
        assert!(!artifact
            .locator
            .contains(&root.to_string_lossy().to_string()));
        assert_eq!(artifact.content_kind, ToolResultArtifactContentKind::Text);
        assert_eq!(artifact.original_size, 6);
        assert!(!artifact.preview_truncated);
        assert!(!artifact.redacted);
        let stored =
            fs::read_to_string(root.join(&artifact.locator)).map_err(|error| error.to_string())?;
        assert_eq!(stored, "abcdef");
        assert_eq!(artifact.digest, sha256_hex(stored.as_bytes()));
        Ok(())
    }

    #[test]
    fn persists_oversized_json_with_artifact_ref() -> Result<(), String> {
        let root = temp_root("structured-json")?;
        let content = json!([
            {"type": "text", "text": "hello"},
            {"type": "text", "text": "world"}
        ]);
        let outcome =
            maybe_persist_tool_result_with_artifact(Some(&root), None, "call", &content, 3)
                .ok_or_else(|| "missing persisted json outcome".to_owned())?;
        let artifact = outcome
            .artifact
            .ok_or_else(|| "missing artifact ref".to_owned())?;
        assert_eq!(
            outcome.disposition,
            ToolResultPersistenceDisposition::Persisted
        );
        let expected_path = expected_artifact_path(&root, "default", "call", &content)?;
        assert_eq!(root.join(&artifact.locator), expected_path);
        assert_eq!(
            artifact.content_kind,
            ToolResultArtifactContentKind::JsonTextBlocks
        );
        assert_eq!(artifact.original_size, "hello\nworld".chars().count());
        assert!(!artifact.redacted);
        let stored =
            fs::read_to_string(root.join(&artifact.locator)).map_err(|error| error.to_string())?;
        assert_eq!(artifact.digest, sha256_hex(stored.as_bytes()));
        assert_eq!(
            serde_json::from_str::<Value>(&stored).map_err(|error| error.to_string())?,
            content
        );
        Ok(())
    }

    #[test]
    fn repeated_call_id_uses_payload_specific_artifact() -> Result<(), String> {
        let root = temp_root("repeated-call-id")?;
        let first = maybe_persist_text_tool_result_with_artifact(
            Some(&root),
            Some("session"),
            "call",
            "first oversized result",
            3,
        )
        .and_then(|outcome| outcome.artifact)
        .ok_or_else(|| "missing first artifact".to_owned())?;
        let second = maybe_persist_text_tool_result_with_artifact(
            Some(&root),
            Some("session"),
            "call",
            "second oversized result",
            3,
        )
        .and_then(|outcome| outcome.artifact)
        .ok_or_else(|| "missing second artifact".to_owned())?;

        assert_ne!(first.locator, second.locator);
        assert_ne!(first.digest, second.digest);
        assert_eq!(
            fs::read_to_string(root.join(first.locator)).map_err(|error| error.to_string())?,
            "first oversized result"
        );
        assert_eq!(
            fs::read_to_string(root.join(second.locator)).map_err(|error| error.to_string())?,
            "second oversized result"
        );
        Ok(())
    }

    #[test]
    fn marks_structured_artifact_redacted_and_digests_redacted_payload() -> Result<(), String> {
        let root = temp_root("structured-redacted")?;
        let secret = "OPENAI_API_KEY=sk-secret-token visible text";
        let outcome =
            maybe_persist_text_tool_result_with_artifact(Some(&root), None, "call", secret, 3)
                .ok_or_else(|| "missing redacted outcome".to_owned())?;
        let artifact = outcome
            .artifact
            .ok_or_else(|| "missing artifact ref".to_owned())?;
        let stored =
            fs::read_to_string(root.join(&artifact.locator)).map_err(|error| error.to_string())?;
        let rendered = outcome
            .content
            .as_str()
            .ok_or_else(|| "content was not string".to_owned())?;
        assert!(artifact.redacted);
        assert_eq!(artifact.digest, sha256_hex(stored.as_bytes()));
        assert!(stored.contains(shacs_redaction::REDACTED));
        assert!(rendered.contains(shacs_redaction::REDACTED));
        assert!(!stored.contains("sk-secret-token"));
        assert!(!rendered.contains("sk-secret-token"));
        Ok(())
    }

    #[test]
    fn returns_truncated_fallback_outcome_without_workspace() -> Result<(), String> {
        let outcome = maybe_persist_text_tool_result_with_artifact(
            None,
            None,
            "call",
            r#"before {"api_key":"plain-secret"} after with enough extra output to persist"#,
            64,
        )
        .ok_or_else(|| "missing fallback outcome".to_owned())?;
        let fallback = outcome
            .content
            .as_str()
            .ok_or_else(|| "fallback was not string".to_owned())?;
        assert_eq!(
            outcome.disposition,
            ToolResultPersistenceDisposition::TruncatedFallback
        );
        assert_eq!(outcome.artifact, None);
        assert!(fallback.contains(shacs_redaction::REDACTED));
        assert!(!fallback.contains("plain-secret"));
        Ok(())
    }

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
        assert!(expected_artifact_path(
            &root,
            "session/one",
            "call:1",
            &Value::String("abcdef".to_owned()),
        )?
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
        let stored = fs::read_to_string(expected_artifact_path(
            &root,
            "default",
            "call",
            &Value::String(secret.to_owned()),
        )?)
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
        let stored = fs::read_to_string(expected_artifact_path(
            &root,
            "default",
            "call",
            &Value::String(secret.to_owned()),
        )?)
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
        let path = expected_artifact_path(&root, "default", "call", &content)?;
        assert!(path.is_file());
        let stored = fs::read_to_string(path).map_err(|error| error.to_string())?;
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
        let stored =
            fs::read_to_string(expected_artifact_path(&root, "default", "call", &content)?)
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
        let stored =
            fs::read_to_string(expected_artifact_path(&root, "default", "call", &content)?)
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
        let content = json!([
            {"type": "text", "text": "safe output"},
            {"type": "text", "text": "client_secret: \"plain-secret\" after with enough output"}
        ]);
        let artifact_path = expected_artifact_path(&root, "default", "call", &content)?;
        fs::create_dir_all(
            artifact_path
                .parent()
                .ok_or_else(|| "missing artifact parent".to_owned())?,
        )
        .map_err(|error| error.to_string())?;
        let outside = root.join("outside.json");
        fs::write(&outside, "outside").map_err(|error| error.to_string())?;
        symlink(&outside, artifact_path).map_err(|error| error.to_string())?;
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
        let content = "before Authorization: Bearer ghp_secret_token after with enough extra output to persist";
        let artifact_path =
            expected_artifact_path(&root, "default", "call", &Value::String(content.to_owned()))?;
        fs::create_dir_all(
            artifact_path
                .parent()
                .ok_or_else(|| "missing artifact parent".to_owned())?,
        )
        .map_err(|error| error.to_string())?;
        let outside = root.join("outside.txt");
        fs::write(&outside, "outside").map_err(|error| error.to_string())?;
        symlink(&outside, artifact_path).map_err(|error| error.to_string())?;

        let result = maybe_persist_text_tool_result(Some(&root), None, "call", content, 64)
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
        let artifact_path = expected_artifact_path(
            &root,
            "default",
            "call",
            &Value::String("abcdef".to_owned()),
        )?;
        fs::create_dir_all(
            artifact_path
                .parent()
                .ok_or_else(|| "missing artifact parent".to_owned())?,
        )
        .map_err(|error| error.to_string())?;
        let outside = root.join("outside.txt");
        fs::write(&outside, "outside").map_err(|error| error.to_string())?;
        symlink(&outside, artifact_path).map_err(|error| error.to_string())?;
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
