use super::artifacts::digest_file;
use super::model::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};
use std::process::Command;

const MAX_SOURCE_FILES: usize = 4_096;
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

pub fn collect(repo_root: &Path) -> Result<SourceManifest, Spec034ReleaseArtifactError> {
    let canonical = repo_root
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let head_oid = git(&canonical, &["rev-parse", "HEAD"])?;
    let tracked = nul_paths(&git_bytes(&canonical, &["ls-files", "-z"])?);
    let untracked = nul_paths(&git_bytes(
        &canonical,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
    let modified = nul_paths(&git_bytes(&canonical, &["diff", "--name-only", "-z"])?);
    let staged = nul_paths(&git_bytes(
        &canonical,
        &["diff", "--cached", "--name-only", "-z"],
    )?);
    let modified = modified.union(&staged).cloned().collect::<BTreeSet<_>>();
    let mut locators = tracked.union(&untracked).cloned().collect::<Vec<_>>();
    locators.retain(|locator| is_source(locator));
    locators.sort();
    if locators.len() > MAX_SOURCE_FILES {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let mut files = Vec::with_capacity(locators.len());
    let mut total_bytes = 0_u64;
    for locator in locators {
        validate_locator(&locator)?;
        let path = canonical.join(&locator);
        let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        if total_bytes > MAX_SOURCE_BYTES {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        files.push(SourceFile {
            digest: digest_file(&path)?,
            tracked: tracked.contains(&locator),
            modified: modified.contains(&locator) || untracked.contains(&locator),
            locator,
        });
    }
    let mut digest = Sha256::new();
    for file in &files {
        digest.update(file.locator.as_bytes());
        digest.update([0]);
        digest.update(file.digest.as_bytes());
        digest.update([u8::from(file.tracked), u8::from(file.modified)]);
        digest.update(b"\n");
    }
    Ok(SourceManifest {
        repo_root: canonical.display().to_string(),
        head_oid: head_oid.trim().to_owned(),
        worktree_dirty: files.iter().any(|file| file.modified),
        digest: format!("sha256:{:x}", digest.finalize()),
        files,
    })
}

fn git(repo: &Path, args: &[&str]) -> Result<String, Spec034ReleaseArtifactError> {
    let bytes = git_bytes(repo, args)?;
    String::from_utf8(bytes).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

fn git_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    let output = Command::new("git")
        .env("GIT_MASTER", "1")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if !output.status.success() {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    Ok(output.stdout)
}

fn nul_paths(bytes: &[u8]) -> BTreeSet<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

fn is_source(locator: &str) -> bool {
    let path = Path::new(locator);
    let rooted = locator == "README.md"
        || locator == "Cargo.toml"
        || locator == "crates/Cargo.lock"
        || locator.starts_with("crates/")
        || locator.starts_with("docs/");
    rooted
        && !locator.contains("/target/")
        && matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "toml" | "lock" | "md" | "json" | "yml" | "yaml" | "sh")
        )
}

pub fn validate_locator(locator: &str) -> Result<(), Spec034ReleaseArtifactError> {
    if locator.is_empty()
        || Path::new(locator)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    Ok(())
}
