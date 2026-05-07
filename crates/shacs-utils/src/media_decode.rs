use crate::text::safe_filename;
use base64::{engine::general_purpose::STANDARD, Engine};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_MAX_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_FILE_SIZE: usize = DEFAULT_MAX_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaDecodeError {
    Malformed,
    UnsupportedType { mime_type: String },
    FileSizeExceeded { limit: usize },
    Io(String),
}

pub fn save_base64_data_url(
    data_url: &str,
    media_dir: impl AsRef<Path>,
    max_bytes: Option<usize>,
) -> Result<Option<String>, MediaDecodeError> {
    let regex = Regex::new(r"(?s)^data:([^;]+);base64,(.+)$")
        .map_err(|error| MediaDecodeError::Io(error.to_string()))?;
    let Some(captures) = regex.captures(data_url) else {
        if data_url.starts_with("data:") {
            return Err(MediaDecodeError::Malformed);
        }
        return Ok(None);
    };
    let mime_type = captures
        .get(1)
        .map(|value| value.as_str())
        .unwrap_or_default();
    if !is_supported_data_url_mime_type(mime_type) {
        return Err(MediaDecodeError::UnsupportedType {
            mime_type: mime_type.to_owned(),
        });
    }
    let payload = captures
        .get(2)
        .map(|value| value.as_str())
        .unwrap_or_default();
    let raw = match STANDARD.decode(payload) {
        Ok(raw) => raw,
        Err(_) => return Err(MediaDecodeError::Malformed),
    };
    let limit = max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    if raw.len() > limit {
        return Err(MediaDecodeError::FileSizeExceeded { limit });
    }
    let media_dir = media_dir.as_ref();
    fs::create_dir_all(media_dir).map_err(|error| MediaDecodeError::Io(error.to_string()))?;
    let filename = safe_filename(&format!(
        "{}{}",
        unique_media_stem(),
        guess_extension(mime_type)
    ));
    let destination = media_dir.join(filename);
    fs::write(&destination, raw).map_err(|error| MediaDecodeError::Io(error.to_string()))?;
    Ok(Some(destination.to_string_lossy().to_string()))
}

fn is_supported_data_url_mime_type(mime_type: &str) -> bool {
    matches!(
        mime_type.to_ascii_lowercase().as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "text/plain"
            | "application/json"
            | "application/pdf"
    )
}

fn guess_extension(mime_type: &str) -> &'static str {
    match mime_type.to_ascii_lowercase().as_str() {
        "image/png" => ".png",
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "text/plain" => ".txt",
        "application/json" => ".json",
        "application/pdf" => ".pdf",
        _ => ".bin",
    }
}

fn unique_media_stem() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:012x}", nanos & 0xffffffffffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_data_url_and_rejects_malformed_or_oversized_payload() {
        let dir = tempfile_dir();
        let saved = save_base64_data_url("data:image/png;base64,aGk=", &dir, None)
            .expect("decode ok")
            .expect("saved path");
        assert!(saved.ends_with(".png"));
        assert_eq!(fs::read(saved).expect("read saved"), b"hi");
        assert_eq!(save_base64_data_url("nope", &dir, None), Ok(None));
        assert_eq!(
            save_base64_data_url("data:image/png;base64,%%%", &dir, None),
            Err(MediaDecodeError::Malformed)
        );
        assert_eq!(
            save_base64_data_url("data:application/x-sh;base64,aGk=", &dir, None),
            Err(MediaDecodeError::UnsupportedType {
                mime_type: "application/x-sh".to_owned()
            })
        );
        assert_eq!(
            save_base64_data_url("data:image/png;base64,aGk=", &dir, Some(1)),
            Err(MediaDecodeError::FileSizeExceeded { limit: 1 })
        );
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("shacs-utils-media-{}", unique_media_stem()));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }
}
