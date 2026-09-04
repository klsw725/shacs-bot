use super::*;
use rustix::fs::{open, Mode, OFlags};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::File;
use std::path::Component;

const MAX_SOURCE_FILES: usize = 4_096;

pub(super) fn capture_context(
    context: &SourceRootContext,
) -> Result<SourceSnapshot, Spec034ReleaseArtifactError> {
    capture_after_enumeration(context, || {})
}

fn capture_after_enumeration(
    context: &SourceRootContext,
    after_enumeration: impl FnOnce(),
) -> Result<SourceSnapshot, Spec034ReleaseArtifactError> {
    context.verify()?;
    let reader = ConfinedSourceReader::open(context.root())?;
    let index = snapshot_index(context)?;
    let head_oid = git(context, &index, &["rev-parse", "HEAD"])?;
    let head_tracked = nul_paths(&git_bytes(
        context,
        &index,
        &["ls-tree", "-r", "--name-only", "-z", "HEAD"],
    )?)?;
    let tracked = nul_paths(&git_bytes(context, &index, &["ls-files", "-z"])?)?;
    let untracked = nul_paths(&git_bytes(
        context,
        &index,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?)?;
    let modified = nul_paths(&git_bytes(
        context,
        &index,
        &["diff", "--no-renames", "--name-only", "-z"],
    )?)?;
    let staged = nul_paths(&git_bytes(
        context,
        &index,
        &["diff", "--cached", "--no-renames", "--name-only", "-z"],
    )?)?;
    after_enumeration();
    context.verify()?;
    let modified = modified.union(&staged).cloned().collect::<BTreeSet<_>>();
    let worktree_dirty = !modified.is_empty() || !untracked.is_empty();
    let mut locators = head_tracked
        .union(&tracked)
        .chain(untracked.iter())
        .chain(modified.iter())
        .filter(|locator| is_source(locator))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    locators.sort();
    if locators.len() > MAX_SOURCE_FILES {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    let mut files = Vec::with_capacity(locators.len());
    let mut snapshot_files = Vec::with_capacity(locators.len());
    let mut total_bytes = 0_u64;
    for locator in locators {
        validate_locator(&locator)?;
        let remaining = MAX_SOURCE_BYTES - total_bytes;
        let (state, file_digest) = match reader.read(&locator, remaining)? {
            Some(bytes) => {
                total_bytes = total_bytes
                    .checked_add(bytes.len() as u64)
                    .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
                let file_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
                snapshot_files.push((locator.clone(), bytes));
                (SourceFileState::Present, Some(file_digest))
            }
            None if modified.contains(&locator) && head_tracked.contains(&locator) => {
                (SourceFileState::Deleted, None)
            }
            None => return Err(Spec034ReleaseArtifactError::InvalidConfig),
        };
        files.push(SourceFile {
            digest: file_digest,
            tracked: tracked.contains(&locator) || head_tracked.contains(&locator),
            modified: modified.contains(&locator) || untracked.contains(&locator),
            state,
            locator,
        });
    }
    context.verify_identity()?;
    let digest = manifest_digest(&files);
    Ok(SourceSnapshot {
        manifest: SourceManifest {
            repo_root: ".".to_owned(),
            head_oid: head_oid.trim().to_owned(),
            worktree_dirty,
            digest,
            files,
        },
        files: snapshot_files,
    })
}

#[cfg(test)]
pub(super) fn capture_after_enumeration_for_test(
    context: &SourceRootContext,
    after_enumeration: impl FnOnce(),
) -> Result<SourceSnapshot, Spec034ReleaseArtifactError> {
    capture_after_enumeration(context, after_enumeration)
}

fn manifest_digest(files: &[SourceFile]) -> String {
    let mut digest = Sha256::new();
    for file in files {
        digest.update(file.locator.as_bytes());
        digest.update([0]);
        digest.update(match file.state {
            SourceFileState::Present => b"present".as_slice(),
            SourceFileState::Deleted => b"deleted".as_slice(),
        });
        digest.update([0]);
        if let Some(file_digest) = &file.digest {
            digest.update(file_digest.as_bytes());
        }
        digest.update([u8::from(file.tracked), u8::from(file.modified)]);
        digest.update(b"\n");
    }
    format!("sha256:{:x}", digest.finalize())
}

fn git(
    context: &SourceRootContext,
    index: &tempfile::NamedTempFile,
    args: &[&str],
) -> Result<String, Spec034ReleaseArtifactError> {
    let bytes = git_bytes(context, index, args)?;
    String::from_utf8(bytes).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

fn git_bytes(
    context: &SourceRootContext,
    index: &tempfile::NamedTempFile,
    args: &[&str],
) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    context.verify_identity()?;
    let output = context
        .git()
        .command(context.root())
        .arg("--no-pager")
        .arg("--git-dir")
        .arg(context.git_dir())
        .arg("--work-tree")
        .arg(context.root())
        .arg("-c")
        .arg(format!("core.worktree={}", context.root().display()))
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .env("GIT_INDEX_FILE", index.path())
        .args(args)
        .output()
        .map_err(Spec034ReleaseArtifactError::Io);
    context.verify()?;
    let output = output?;
    if !output.status.success() {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    Ok(output.stdout)
}

fn snapshot_index(
    context: &SourceRootContext,
) -> Result<tempfile::NamedTempFile, Spec034ReleaseArtifactError> {
    let descriptor = open(
        context.git_dir().join("index"),
        OFlags::RDONLY.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC),
        Mode::empty(),
    )
    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut source: File = descriptor.into();
    let mut snapshot = tempfile::NamedTempFile::new().map_err(Spec034ReleaseArtifactError::Io)?;
    std::io::copy(&mut source, &mut snapshot).map_err(Spec034ReleaseArtifactError::Io)?;
    snapshot
        .as_file()
        .sync_all()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(snapshot)
}

fn nul_paths(bytes: &[u8]) -> Result<BTreeSet<String>, Spec034ReleaseArtifactError> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| {
            std::str::from_utf8(part)
                .map(str::to_owned)
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
        })
        .collect()
}

fn is_source(locator: &str) -> bool {
    let path = Path::new(locator);
    !locator.starts_with(".omo/")
        && !path
            .components()
            .any(|component| matches!(component, Component::Normal(value) if value == "target"))
}

#[cfg(test)]
pub(super) fn nul_paths_for_test(
    bytes: &[u8],
) -> Result<BTreeSet<String>, Spec034ReleaseArtifactError> {
    nul_paths(bytes)
}
