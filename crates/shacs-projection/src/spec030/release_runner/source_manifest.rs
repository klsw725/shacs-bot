use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::process::Command;

pub const SPEC030_SOURCE_MANIFEST_SCHEMA: &str = "spec030.source_manifest.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030SourceManifest {
    pub schema: String,
    pub git_head: String,
    pub source_digest: String,
    pub files: Vec<Spec030SourceFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec030SourceFile {
    pub path: String,
    pub kind: Spec030SourceFileKind,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec030SourceFileKind {
    File,
    Symlink,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec030SourceManifestError {
    Git,
    InvalidPath,
    Io,
}

impl Display for Spec030SourceManifestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for Spec030SourceManifestError {}

pub fn build_spec030_source_manifest(
    repo_root: &Path,
) -> Result<Spec030SourceManifest, Spec030SourceManifestError> {
    let git_head = git_head(repo_root)?;
    let output = Command::new("git")
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .current_dir(repo_root)
        .output()
        .map_err(|_| Spec030SourceManifestError::Git)?;
    if !output.status.success() {
        return Err(Spec030SourceManifestError::Git);
    }
    let mut paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_owned)
                .map_err(|_| Spec030SourceManifestError::InvalidPath)
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    paths.dedup();
    let files = paths
        .into_iter()
        .map(|path| source_file(repo_root, path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut root = Sha256::new();
    root.update(git_head.as_bytes());
    root.update([0]);
    for file in &files {
        root.update(file.path.as_bytes());
        root.update([0]);
        root.update(match file.kind {
            Spec030SourceFileKind::File => b"file".as_slice(),
            Spec030SourceFileKind::Symlink => b"symlink".as_slice(),
            Spec030SourceFileKind::Missing => b"missing".as_slice(),
        });
        root.update([0]);
        root.update(file.sha256.as_bytes());
        root.update(*b"\n");
    }
    Ok(Spec030SourceManifest {
        schema: SPEC030_SOURCE_MANIFEST_SCHEMA.to_owned(),
        git_head,
        source_digest: sha256_digest(root.finalize()),
        files,
    })
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    sha256_digest(Sha256::digest(bytes))
}

fn source_file(
    repo_root: &Path,
    path: String,
) -> Result<Spec030SourceFile, Spec030SourceManifestError> {
    let full_path = repo_root.join(&path);
    let metadata = match std::fs::symlink_metadata(&full_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Spec030SourceFile {
                path,
                kind: Spec030SourceFileKind::Missing,
                sha256: sha256_bytes(&[]),
            });
        }
        Err(_) => return Err(Spec030SourceManifestError::Io),
    };
    let (kind, bytes) = if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(full_path).map_err(|_| Spec030SourceManifestError::Io)?;
        (
            Spec030SourceFileKind::Symlink,
            target.to_string_lossy().as_bytes().to_vec(),
        )
    } else if metadata.is_file() {
        (
            Spec030SourceFileKind::File,
            std::fs::read(full_path).map_err(|_| Spec030SourceManifestError::Io)?,
        )
    } else {
        return Err(Spec030SourceManifestError::InvalidPath);
    };
    Ok(Spec030SourceFile {
        path,
        kind,
        sha256: sha256_bytes(&bytes),
    })
}

fn git_output(repo_root: &Path, arguments: &[&str]) -> Result<String, Spec030SourceManifestError> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repo_root)
        .output()
        .map_err(|_| Spec030SourceManifestError::Git)?;
    if !output.status.success() {
        return Err(Spec030SourceManifestError::Git);
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| Spec030SourceManifestError::Git)
}

fn git_head(repo_root: &Path) -> Result<String, Spec030SourceManifestError> {
    match git_output(repo_root, &["rev-parse", "HEAD"]) {
        Ok(head) => Ok(head),
        Err(_) => {
            git_output(repo_root, &["rev-parse", "--git-dir"])?;
            Ok("unborn".to_owned())
        }
    }
}

fn sha256_digest(bytes: impl AsRef<[u8]>) -> String {
    let hex = bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}
