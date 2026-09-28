use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_evidence_io::{evidence_error, sha256};
use super::spec035_execution_io::{decode, source_paths};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourceSnapshot {
    pub(super) head: String,
    pub(super) inventory_digest: String,
    pub(super) lockfile_digest: String,
    pub(super) files: Vec<SourceFile>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct SourceFile {
    locator: String,
    digest: String,
}

#[derive(Debug, Serialize)]
pub struct SourceCorrespondence {
    pub original_head: String,
    pub target_commit: String,
    pub inventory_digest: String,
    pub files_verified: usize,
    pub current_deltas: Vec<SourceDelta>,
}

#[derive(Debug, Serialize)]
pub struct SourceDelta {
    pub path: String,
    pub recorded_sha256: Option<String>,
    pub current_sha256: Option<String>,
}

pub(super) fn snapshot(bytes: &[u8]) -> Result<SourceSnapshot, Error> {
    let source: SourceSnapshot = decode(bytes)?;
    let serialized = serde_json::to_vec(&source.files).map_err(|_| Error::Io)?;
    if format!("sha256:{}", sha256(&serialized)) != source.inventory_digest
        || !source.files.iter().any(|file| {
            file.locator == "crates/Cargo.lock" && file.digest == source.lockfile_digest
        })
        || !source
            .files
            .iter()
            .any(|file| file.locator == "crates/Cargo.toml")
    {
        return Err(evidence_error("historical source digest mismatch"));
    }
    Ok(source)
}

pub(super) fn correspondence(
    repo: &Path,
    source: SourceSnapshot,
    commit: &str,
) -> Result<SourceCorrespondence, Error> {
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidCommandEvidence);
    }
    let tree = git(repo, &["ls-tree", "-r", "--name-only", "-z", commit])?;
    let committed: std::collections::BTreeSet<_> = tree
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_owned)
                .map_err(|_| Error::InvalidArtifactPath)
        })
        .collect::<Result<_, _>>()?;
    let mut recorded = BTreeMap::new();
    for file in &source.files {
        if file.locator.is_empty()
            || file.locator.contains('\\')
            || !Path::new(&file.locator)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || recorded
                .insert(file.locator.clone(), file.digest.clone())
                .is_some()
        {
            return Err(Error::InvalidArtifactPath);
        }
    }
    if committed != recorded.keys().cloned().collect() {
        return Err(evidence_error("committed source path set mismatch"));
    }
    for (path, digest) in &recorded {
        if format!(
            "sha256:{}",
            sha256(&git(repo, &["show", &format!("{commit}:{path}")])?)
        ) != *digest
        {
            return Err(evidence_error(format!(
                "committed source byte mismatch: {path}"
            )));
        }
    }
    let current = source_paths(repo)?;
    let mut deltas = Vec::new();
    for path in current.union(&committed) {
        let current_sha256 = if current.contains(path) {
            let safe = super::validate::require_safe_file(repo, path)?;
            Some(format!(
                "sha256:{}",
                sha256(&std::fs::read(safe).map_err(|_| Error::Io)?)
            ))
        } else {
            None
        };
        if recorded.get(path) != current_sha256.as_ref() {
            deltas.push(SourceDelta {
                path: path.clone(),
                recorded_sha256: recorded.get(path).cloned(),
                current_sha256,
            });
        }
    }
    Ok(SourceCorrespondence {
        original_head: source.head,
        target_commit: commit.to_owned(),
        inventory_digest: source.inventory_digest,
        files_verified: source.files.len(),
        current_deltas: deltas,
    })
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let output = std::process::Command::new("git")
        .env("GIT_MASTER", "1")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|_| Error::Io)?;
    if !output.status.success() {
        return Err(evidence_error("committed source lookup failed"));
    }
    Ok(output.stdout)
}
