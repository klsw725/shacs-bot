use std::fs;
use std::path::{Path, PathBuf};

pub mod media;
pub mod protocol;
pub mod sessions;
pub mod static_files;
pub mod tokens;

pub const WEB_DIST_DIR_NAME: &str = "dist";

pub const EMBEDDED_WEB_UI_ASSETS_NOTE: &str = "Embedded web UI assets live in dist/. The directory is populated by running the webui build and can be empty in source checkouts.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAssets {
    root: PathBuf,
}

impl WebAssets {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dist_dir(&self) -> PathBuf {
        self.root.join(WEB_DIST_DIR_NAME)
    }

    pub fn dist_is_populated(&self) -> bool {
        dist_is_populated(self.dist_dir())
    }

    pub fn serve_static(&self, request_path: &str) -> static_files::StaticFileResult {
        static_files::serve_static(self.dist_dir(), request_path)
    }
}

pub fn dist_dir(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join(WEB_DIST_DIR_NAME)
}

pub fn dist_is_populated(path: impl AsRef<Path>) -> bool {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .any(|entry| entry.is_ok())
}

pub fn manifest_dist_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(WEB_DIST_DIR_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_assets_points_at_dist_and_allows_empty_source_checkout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let assets = WebAssets::new(temp.path());

        assert_eq!(assets.root(), temp.path());
        assert_eq!(assets.dist_dir(), temp.path().join(WEB_DIST_DIR_NAME));
        assert!(!assets.dist_is_populated());
        assert!(manifest_dist_dir().ends_with(WEB_DIST_DIR_NAME));
        assert!(EMBEDDED_WEB_UI_ASSETS_NOTE.contains("dist/"));
    }

    #[test]
    fn populated_dist_is_detected() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dist = dist_dir(temp.path());
        fs::create_dir_all(&dist).expect("create dist");
        assert!(!dist_is_populated(&dist));

        fs::write(dist.join("index.html"), "<html></html>").expect("write asset");
        assert!(dist_is_populated(&dist));
    }
}
