use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_evidence_io::{evidence_error, sha256};
use super::spec035_execution_model::{Execution, FileRef};
use super::validate::require_safe_file;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

pub(super) struct Evidence<'a> {
    pub(super) root: &'a Path,
    pub(super) repo: &'a Path,
    pub(super) execution: &'a Execution,
    inventory: HashMap<String, String>,
    sources: BTreeSet<String>,
}

impl<'a> Evidence<'a> {
    pub(super) fn open(
        repo: &'a Path,
        root: &'a Path,
        execution: &'a Execution,
    ) -> Result<Self, Error> {
        let files: Vec<FileRef> = decode(&read_bound(root, &execution.inventory)?)?;
        let mut inventory = HashMap::new();
        for file in files {
            read_bound(root, &file)?;
            if inventory.insert(file.path, file.sha256).is_some() {
                return Err(Error::InvalidCommandEvidence);
            }
        }
        let mut evidence = Self {
            root,
            repo,
            execution,
            inventory,
            sources: BTreeSet::new(),
        };
        let before = evidence.bytes(&execution.source.before)?;
        let after = evidence.bytes(&execution.source.after)?;
        if before != after || execution.source.before.path == execution.source.after.path {
            return Err(evidence_error(
                "execution source changed between before and after snapshots",
            ));
        }
        let sources: Vec<FileRef> = decode(&before)?;
        for file in sources {
            read_bound(repo, &file)?;
            if !evidence.sources.insert(file.path) {
                return Err(Error::InvalidCommandEvidence);
            }
        }
        if !evidence.sources.contains("crates/Cargo.toml")
            || !evidence.sources.contains("crates/Cargo.lock")
            || evidence.sources != source_paths(repo)?
        {
            return Err(evidence_error(
                "execution source inventory does not cover the current repository",
            ));
        }
        Ok(evidence)
    }

    pub(super) fn bytes(&self, file: &FileRef) -> Result<Vec<u8>, Error> {
        if self.inventory.get(&file.path) != Some(&file.sha256) {
            return Err(evidence_error(format!(
                "execution artifact is not inventory-bound: {}",
                file.path
            )));
        }
        read_bound(self.root, file)
    }

    pub(super) fn json<T: serde::de::DeserializeOwned>(&self, file: &FileRef) -> Result<T, Error> {
        decode(&self.bytes(file)?)
    }

    pub(super) fn source_locator(&self, locator: &str) -> Result<(), Error> {
        let (path, number) = locator
            .rsplit_once(':')
            .ok_or(Error::InvalidCoverageEvidence)?;
        let line = number
            .parse::<usize>()
            .map_err(|_| Error::InvalidCoverageEvidence)?;
        if line == 0 || !self.sources.contains(path) {
            return Err(Error::InvalidCoverageEvidence);
        }
        let text = std::fs::read_to_string(require_safe_file(self.repo, path)?)
            .map_err(|_| Error::MissingRequiredArtifact)?;
        if !text
            .lines()
            .nth(line - 1)
            .is_some_and(|line| !line.trim().is_empty())
        {
            return Err(Error::InvalidCoverageEvidence);
        }
        Ok(())
    }

    pub(super) fn identity(&self, run_id: &str, source_sha256: &str) -> Result<(), Error> {
        if run_id != self.execution.run_id || source_sha256 != self.execution.source.before.sha256 {
            return Err(evidence_error(
                "execution receipt run/source binding mismatch",
            ));
        }
        Ok(())
    }
}

pub(super) fn read_bound(root: &Path, file: &FileRef) -> Result<Vec<u8>, Error> {
    if file.path.contains('\\')
        || file
            .path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(Error::InvalidArtifactPath);
    }
    let path = require_safe_file(root, &file.path)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|_| Error::MissingRequiredArtifact)?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| Error::MissingRequiredArtifact)?;
    if canonical_path != canonical_root.join(&file.path) {
        return Err(Error::InvalidArtifactPath);
    }
    let bytes = std::fs::read(path).map_err(|_| Error::MissingRequiredArtifact)?;
    if file.sha256 != sha256(&bytes) {
        return Err(evidence_error(format!(
            "execution SHA-256 mismatch: {}",
            file.path
        )));
    }
    Ok(bytes)
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidCommandEvidence)
}

fn source_paths(repo: &Path) -> Result<BTreeSet<String>, Error> {
    let output = std::process::Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .current_dir(repo)
        .output()
        .map_err(|_| Error::Io)?;
    if !output.status.success() {
        return Err(evidence_error(
            "cannot enumerate execution source inventory",
        ));
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(|_| Error::InvalidArtifactPath))
        .collect()
}
