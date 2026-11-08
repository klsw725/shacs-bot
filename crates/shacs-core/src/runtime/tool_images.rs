use super::{context, RuntimeToolMessage, ToolImageSettings};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

pub(super) fn persist(settings: Option<&ToolImageSettings>, message: &mut RuntimeToolMessage) {
    let Ok(parts) = serde_json::from_str::<Vec<Value>>(&message.content) else {
        return;
    };
    if !parts.iter().all(|part| {
        matches!(
            part.get("type").and_then(Value::as_str),
            Some("text" | "image_url")
        )
    }) || !parts
        .iter()
        .any(|part| part.get("type").and_then(Value::as_str) == Some("image_url"))
    {
        return;
    }
    let mut lines = Vec::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text") {
            lines.push(
                part.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            );
            continue;
        }
        match keep_image(settings, &part) {
            Ok(reference) => {
                lines.push(format!("[image: {reference}]"));
                message.images.push(reference);
            }
            Err(reason) => lines.push(format!("[image not kept: {reason}]")),
        }
    }
    message.content = lines.join("\n");
}

fn keep_image(settings: Option<&ToolImageSettings>, part: &Value) -> Result<String, &'static str> {
    let url = part
        .pointer("/image_url/url")
        .and_then(Value::as_str)
        .ok_or("unsupported image type")?;
    let (header, data) = url.split_once(',').ok_or("unsupported image type")?;
    if !header.starts_with("data:") || !header.ends_with(";base64") {
        return Err("unsupported image type");
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| "unsupported image type")?;
    if bytes.len() > 5 * 1024 * 1024 {
        return Err("image larger than 5 MiB");
    }
    let mime = context::detect_image_mime(&bytes).ok_or("unsupported image type")?;
    let extension = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return Err("unsupported image type"),
    };
    let settings = settings.ok_or("no media folder")?;
    let digest = Sha256::digest(&bytes);
    let reference = format!("tool-results/{digest:x}.{extension}");
    let destination = settings.media_root.join(&reference);
    if !destination.exists() {
        let directory = settings.media_root.join("tool-results");
        fs::create_dir_all(&directory).map_err(|_| "write failed")?;
        let mut temporary = NamedTempFile::new_in(directory).map_err(|_| "write failed")?;
        temporary.write_all(&bytes).map_err(|_| "write failed")?;
        temporary.persist(destination).map_err(|_| "write failed")?;
    }
    Ok(reference)
}

fn read_stored_image(media_root: &Path, reference: &str) -> Option<Vec<u8>> {
    let filename = reference.strip_prefix("tool-results/")?;
    let (digest, extension) = filename.split_once('.')?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || !matches!(extension, "png" | "jpg" | "gif" | "webp")
    {
        return None;
    }
    let directory = media_root.join("tool-results");
    if !fs::symlink_metadata(&directory).ok()?.is_dir() {
        return None;
    }
    let file = directory.join(filename);
    if !fs::symlink_metadata(&file).ok()?.is_file() {
        return None;
    }
    let canonical_directory = directory.canonicalize().ok()?;
    let canonical_file = file.canonicalize().ok()?;
    if canonical_file.parent() != Some(canonical_directory.as_path()) {
        return None;
    }
    fs::read(canonical_file).ok()
}

pub(super) fn attach(settings: Option<&ToolImageSettings>, mut messages: Vec<Value>) -> Vec<Value> {
    let mut kept = vec![Vec::new(); messages.len()];
    if let Some(settings) = settings.filter(|settings| settings.attach) {
        let mut remaining = 3;
        let recent_messages = messages.iter().enumerate().rev();
        for (index, message) in recent_messages {
            if message.get("role").and_then(Value::as_str) != Some("tool") {
                continue;
            }
            if let Some(references) = message.get("images").and_then(Value::as_array) {
                let recent_references = references.iter().rev().filter_map(Value::as_str);
                for reference in recent_references {
                    if remaining == 0 {
                        break;
                    }
                    kept[index].push(reference.to_owned());
                    remaining -= 1;
                }
                kept[index].reverse();
            }
        }
        let mut index = messages.len();
        while index > 0 {
            index -= 1;
            if messages[index].get("role").and_then(Value::as_str) != Some("tool") {
                continue;
            }
            let end = index + 1;
            while index > 0
                && messages[index - 1].get("role").and_then(Value::as_str) == Some("tool")
            {
                index -= 1;
            }
            let mut calls = Vec::new();
            let mut blocks = Vec::new();
            let mut unavailable = Vec::new();
            for current in index..end {
                if kept[current].is_empty() {
                    continue;
                }
                calls.push(
                    messages[current]
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                );
                for reference in &kept[current] {
                    let image = read_stored_image(&settings.media_root, reference).and_then(|bytes| {
                        let mime = context::detect_image_mime(&bytes)?;
                        Some(json!({
                            "type": "image_url",
                            "image_url": {"url": format!("data:{mime};base64,{}", STANDARD.encode(bytes))},
                            "_meta": {"path": reference},
                        }))
                    });
                    if let Some(image) = image {
                        blocks.push(image);
                    } else {
                        unavailable.push(format!("[image unavailable: {reference}]"));
                    }
                }
            }
            if !calls.is_empty() {
                let mut text = format!("Images returned by the tool calls above (tool output, not the user's message): {}", calls.join(", "));
                for notice in unavailable {
                    text.push('\n');
                    text.push_str(&notice);
                }
                blocks.insert(0, json!({"type": "text", "text": text}));
                messages.insert(end, json!({"role": "user", "content": blocks}));
            }
        }
    }
    for message in &mut messages {
        if let Some(object) = message.as_object_mut() {
            object.remove("images");
        }
    }
    messages
}
