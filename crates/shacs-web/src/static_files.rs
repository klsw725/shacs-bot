use std::fs;
use std::path::{Component, Path, PathBuf};

pub type StaticFileResult = Result<Option<StaticFileResponse>, StaticFileError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticFileResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
    pub cache_control: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticFileError {
    Forbidden,
    ReadError,
}

pub fn serve_static(dist: impl AsRef<Path>, request_path: &str) -> StaticFileResult {
    let dist = dist.as_ref();
    let Some(mut candidate) = resolve_candidate(dist, request_path)? else {
        return Ok(None);
    };
    if !candidate.is_file() {
        let index = dist.join("index.html");
        if index.is_file() {
            candidate = index;
        } else {
            return Ok(None);
        }
    }
    let body = fs::read(&candidate).map_err(|_| StaticFileError::ReadError)?;
    let content_type = content_type_for_path(&candidate);
    let cache_control =
        if candidate.file_name().and_then(|value| value.to_str()) == Some("index.html") {
            "no-cache".to_owned()
        } else {
            "public, max-age=31536000, immutable".to_owned()
        };
    Ok(Some(StaticFileResponse {
        status: 200,
        body,
        content_type,
        cache_control,
    }))
}

fn resolve_candidate(dist: &Path, request_path: &str) -> Result<Option<PathBuf>, StaticFileError> {
    if !dist.is_dir() {
        return Ok(None);
    }
    let rel = request_path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let relative = Path::new(rel);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(StaticFileError::Forbidden);
    }
    let candidate = dist.join(relative);
    let resolved_dist = dist
        .canonicalize()
        .map_err(|_| StaticFileError::ReadError)?;
    if let Ok(resolved_candidate) = candidate.canonicalize() {
        if !resolved_candidate.starts_with(&resolved_dist) {
            return Err(StaticFileError::Forbidden);
        }
    } else if let Some(parent) = candidate
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
    {
        if !parent.starts_with(&resolved_dist) {
            return Err(StaticFileError::Forbidden);
        }
    }
    Ok(Some(candidate))
}

pub fn content_type_for_path(path: &Path) -> String {
    let base = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    };
    if base.starts_with("text/") || matches!(base, "application/javascript" | "application/json") {
        format!("{base}; charset=utf-8")
    } else {
        base.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_root_assets_fallbacks_and_blocks_traversal() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("index.html"), "<html></html>")?;
        fs::write(root.path().join("app.123.js"), "console.log(1)")?;

        let index = serve_static(root.path(), "/")
            .map_err(|error| format!("{error:?}"))?
            .expect("index");
        assert_eq!(index.cache_control, "no-cache");
        assert_eq!(index.content_type, "text/html; charset=utf-8");

        let asset = serve_static(root.path(), "/app.123.js")
            .map_err(|error| format!("{error:?}"))?
            .expect("asset");
        assert_eq!(asset.cache_control, "public, max-age=31536000, immutable");
        assert_eq!(asset.content_type, "application/javascript; charset=utf-8");

        let fallback = serve_static(root.path(), "/chat/abc")
            .map_err(|error| format!("{error:?}"))?
            .expect("fallback");
        assert_eq!(fallback.body, b"<html></html>");
        assert_eq!(
            serve_static(root.path(), "/../secret"),
            Err(StaticFileError::Forbidden)
        );
        Ok(())
    }
}
