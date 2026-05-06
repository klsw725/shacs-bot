use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::protocol::MediaUrl;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedMedia {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFetchResponse {
    pub body: Vec<u8>,
    pub content_type: String,
    pub cache_control: String,
    pub nosniff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaError {
    InvalidSignature,
    InvalidPayload,
    NotFound,
    ReadError,
}

pub fn sign_media_path(
    media_root: impl AsRef<Path>,
    secret: &[u8],
    abs_path: impl AsRef<Path>,
) -> Option<SignedMedia> {
    let media_root = media_root.as_ref().canonicalize().ok()?;
    let abs_path = abs_path.as_ref().canonicalize().ok()?;
    let rel = abs_path.strip_prefix(&media_root).ok()?;
    let rel_posix = path_to_posix(rel)?;
    let payload = URL_SAFE_NO_PAD.encode(rel_posix.as_bytes());
    let sig = sign_payload(secret, &payload);
    Some(SignedMedia {
        url: format!("/api/media/{sig}/{payload}"),
        name: abs_path.file_name()?.to_str()?.to_owned(),
    })
}

pub fn fetch_signed_media(
    media_root: impl AsRef<Path>,
    secret: &[u8],
    sig: &str,
    payload: &str,
) -> Result<MediaFetchResponse, MediaError> {
    let expected = sign_payload(secret, payload);
    if !constant_time_eq(expected.as_bytes(), sig.as_bytes()) {
        return Err(MediaError::InvalidSignature);
    }
    let rel_bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| MediaError::InvalidPayload)?;
    let rel = String::from_utf8(rel_bytes).map_err(|_| MediaError::InvalidPayload)?;
    let rel_path = safe_relative_path(&rel).ok_or(MediaError::InvalidPayload)?;
    let media_root = media_root
        .as_ref()
        .canonicalize()
        .map_err(|_| MediaError::NotFound)?;
    let candidate = media_root.join(rel_path);
    let resolved = candidate.canonicalize().map_err(|_| MediaError::NotFound)?;
    if !resolved.starts_with(&media_root) || !resolved.is_file() {
        return Err(MediaError::NotFound);
    }
    let body = fs::read(&resolved).map_err(|_| MediaError::ReadError)?;
    Ok(MediaFetchResponse {
        body,
        content_type: allowed_media_mime(&resolved).to_owned(),
        cache_control: "private, max-age=31536000, immutable".to_owned(),
        nosniff: true,
    })
}

pub fn augment_media_urls(payload: &mut Value, media_root: impl AsRef<Path>, secret: &[u8]) {
    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        let media = object
            .get("media")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if media.is_empty() {
            continue;
        }
        let mut urls = Vec::new();
        for entry in media {
            let Some(path) = entry.as_str().filter(|value| !value.is_empty()) else {
                continue;
            };
            if let Some(signed) = sign_media_path(&media_root, secret, path) {
                urls.push(Value::Object(serde_json::Map::from_iter([
                    ("url".to_owned(), Value::String(signed.url)),
                    ("name".to_owned(), Value::String(signed.name)),
                ])));
            }
        }
        if !urls.is_empty() {
            object.insert("media_urls".to_owned(), Value::Array(urls));
        }
        object.remove("media");
    }
}

pub fn media_url_from_signed(signed: SignedMedia) -> MediaUrl {
    MediaUrl {
        url: signed.url,
        name: Some(signed.name),
    }
}

fn sign_payload(secret: &[u8], payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let digest = mac.finalize().into_bytes();
    URL_SAFE_NO_PAD.encode(&digest[..16])
}

fn path_to_posix(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return None;
        };
        parts.push(value.to_str()?);
    }
    Some(parts.join("/"))
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn allowed_media_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signs_fetches_and_augments_media_without_raw_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let media = root.path().join("websocket");
        fs::create_dir_all(&media)?;
        let image = media.join("pic.png");
        fs::write(&image, b"png")?;
        let secret = b"secret";

        let signed = sign_media_path(root.path(), secret, &image).expect("signed");
        let mut parts = signed.url.split('/').rev();
        let payload = parts.next().unwrap();
        let sig = parts.next().unwrap();
        let fetched = fetch_signed_media(root.path(), secret, sig, payload)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(fetched.body, b"png");
        assert_eq!(fetched.content_type, "image/png");
        assert!(fetched.nosniff);
        assert_eq!(
            fetch_signed_media(root.path(), b"wrong", sig, payload),
            Err(MediaError::InvalidSignature)
        );

        let mut payload =
            json!({"messages": [{"role": "user", "media": [image.to_string_lossy()]}]});
        augment_media_urls(&mut payload, root.path(), secret);
        assert!(payload["messages"][0].get("media").is_none());
        assert!(payload["messages"][0]["media_urls"][0]["url"]
            .as_str()
            .unwrap_or_default()
            .starts_with("/api/media/"));
        Ok(())
    }
}
