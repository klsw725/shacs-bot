use super::task_method_calc::bounded;
use super::task_method_image::{
    AttemptStatus, ImageArtifact, ImageAttempt, ARTIFACT_LIMIT, TEXT_LIMIT,
};
use super::task_method_image_measure::measure;
use super::task_method_report::failure;
use serde_json::Value;
use std::path::{Path, PathBuf};

const READ_LIMIT: u64 = 64 * 1024 * 1024;

/// What an `image_generate` call asked for.
#[derive(Debug, Clone)]
pub(crate) struct ImageCall {
    pub(crate) options: Vec<String>,
    pub(crate) prompt: Option<String>,
}

pub(crate) fn image_call(name: &str, arguments: &Value) -> Option<ImageCall> {
    if name != "image_generate" {
        return None;
    }
    let options = ["size", "format", "count", "quality", "background"]
        .iter()
        .filter_map(|key| {
            let value = arguments.get(*key)?;
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            Some(format!("{key}={}", bounded(&text, 64)))
        })
        .collect();
    let prompt = arguments
        .get("prompt")
        .and_then(Value::as_str)
        .map(|prompt| bounded(prompt, TEXT_LIMIT));
    Some(ImageCall { options, prompt })
}

/// Records one `image_generate` result. `not_completed` is set when the
/// runtime already knows the call did not complete (blocked or errored).
pub(crate) fn attempt(
    call_id: &str,
    call: &ImageCall,
    output: &str,
    not_completed: Option<&str>,
    media_roots: &[PathBuf],
) -> ImageAttempt {
    let (status, artifacts, error) = classify(output, not_completed, media_roots);
    ImageAttempt {
        seq: 0,
        call_id: bounded(call_id, TEXT_LIMIT),
        selection_revision: 0,
        status,
        options: call.options.clone(),
        prompt: call.prompt.clone(),
        artifacts,
        error,
        observed_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn classify(
    output: &str,
    not_completed: Option<&str>,
    media_roots: &[PathBuf],
) -> (AttemptStatus, Vec<ImageArtifact>, Option<String>) {
    if let Some(error) = failure(output) {
        return (status_of(&error), Vec::new(), Some(error));
    }
    if let Some(reason) = not_completed {
        let status = if reason.starts_with("blocked") {
            AttemptStatus::Blocked
        } else {
            AttemptStatus::Failed
        };
        return (status, Vec::new(), Some(reason.to_owned()));
    }
    let value = serde_json::from_str::<Value>(output.trim()).ok();
    let items = value
        .as_ref()
        .and_then(|value| value.get("artifacts"))
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty());
    let Some(items) = items else {
        let error = "image_generate returned no image".to_owned();
        return (AttemptStatus::Failed, Vec::new(), Some(error));
    };
    let provider = value
        .as_ref()
        .and_then(|value| value.get("provider"))
        .and_then(|provider| label(provider, "providerId", "modelId"));
    let artifacts: Vec<ImageArtifact> = items
        .iter()
        .take(ARTIFACT_LIMIT)
        .map(|item| inspect(item, provider.as_deref(), media_roots))
        .collect();
    if items.len() > ARTIFACT_LIMIT {
        // Images beyond the limit are not checked, so the set is not confirmed.
        let error = format!(
            "{} images were reported, but only the first {ARTIFACT_LIMIT} can be checked",
            items.len()
        );
        return (AttemptStatus::CommitUncertain, artifacts, Some(error));
    }
    if artifacts.iter().all(|artifact| artifact.committed) {
        (AttemptStatus::Generated, artifacts, None)
    } else {
        let error =
            "a reported image was not found under the media root or did not match its digest";
        (
            AttemptStatus::CommitUncertain,
            artifacts,
            Some(error.to_owned()),
        )
    }
}

/// Maps a failure to a state. The tool's own categories come first, so an OS
/// error inside a write failure (for example "Permission denied") stays a
/// write failure.
fn status_of(error: &str) -> AttemptStatus {
    let lower = error.to_ascii_lowercase();
    if lower.starts_with("error: image generation") {
        return if lower.contains("requires configured authentication")
            || lower.contains("was not found")
        {
            AttemptStatus::Unavailable
        } else if lower.contains("unsupported/config failure") {
            AttemptStatus::Unsupported
        } else if lower.contains("media write failure") {
            AttemptStatus::WriteFailed
        } else {
            AttemptStatus::Failed
        };
    }
    if lower.starts_with("[tool call skipped") || lower.contains("cancelled") {
        AttemptStatus::Cancelled
    } else if lower.contains("denied") {
        AttemptStatus::Denied
    } else if lower.contains("blocked") {
        AttemptStatus::Blocked
    } else {
        AttemptStatus::Failed
    }
}

fn label(value: &Value, id: &str, model: &str) -> Option<String> {
    let id = value.get(id).and_then(Value::as_str)?;
    Some(match value.get(model).and_then(Value::as_str) {
        Some(model) => bounded(&format!("{id}/{model}"), TEXT_LIMIT),
        None => bounded(id, TEXT_LIMIT),
    })
}

fn inspect(item: &Value, provider: Option<&str>, media_roots: &[PathBuf]) -> ImageArtifact {
    let text = |key: &str| item.get(key).and_then(Value::as_str);
    let relative = text("mediaRootRelativePath");
    let reference = relative.or_else(|| text("mediaRef")).unwrap_or("");
    let sha256 = text("sha256")
        .unwrap_or("")
        .trim_start_matches("sha256:")
        .to_ascii_lowercase();
    let candidates: Vec<PathBuf> = match (relative, text("path")) {
        (Some(relative), _) => media_roots.iter().map(|root| root.join(relative)).collect(),
        (None, Some(path)) => vec![PathBuf::from(path)],
        (None, None) => Vec::new(),
    };
    let bytes = candidates
        .iter()
        .find_map(|path| confined_read(path, media_roots));
    let measured = bytes.as_deref().and_then(measure);
    let committed = bytes
        .as_deref()
        .is_some_and(|bytes| !sha256.is_empty() && hex_sha256(bytes) == sha256);
    ImageArtifact {
        artifact_id: bounded(text("artifactId").unwrap_or(""), TEXT_LIMIT),
        reference: bounded(reference, TEXT_LIMIT),
        sha256: bounded(&sha256, 64),
        committed,
        format: measured.map(|(format, _, _)| format.to_owned()),
        width: measured.map(|(_, width, _)| width),
        height: measured.map(|(_, _, height)| height),
        byte_len: bytes.as_ref().map(|bytes| bytes.len() as u64),
        provider: label(item, "providerId", "modelId").or_else(|| provider.map(str::to_owned)),
    }
}

/// Reads a file only if it resolves inside one of the media roots.
fn confined_read(path: &Path, media_roots: &[PathBuf]) -> Option<Vec<u8>> {
    let resolved = std::fs::canonicalize(path).ok()?;
    let inside = media_roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| resolved.starts_with(root));
    let metadata = std::fs::metadata(&resolved).ok()?;
    if !inside || !metadata.is_file() || metadata.len() > READ_LIMIT {
        return None;
    }
    std::fs::read(&resolved).ok()
}

fn hex_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
