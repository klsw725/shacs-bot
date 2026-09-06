use super::artifacts::digest_bytes;
use super::model::Spec034ReleaseArtifactError;
use cap_primitives::fs::read_base_dir;
use rustix::fs::{openat, Mode, OFlags, CWD};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path};

#[path = "source_descriptor_metadata.rs"]
mod metadata;
use metadata::{mode, preserve_mode, same_snapshot};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);
const MAX_TREE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TREE_ENTRIES: usize = 32_768;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TreeKind {
    Worktree,
    LiveWorktree,
    Git,
    ControlledGit,
    Cache,
}

struct TreeBudget {
    bytes: u64,
    entries: usize,
}

pub(super) fn copy_tree(
    source: &File,
    target: &Path,
    kind: TreeKind,
) -> Result<String, Spec034ReleaseArtifactError> {
    std::fs::create_dir_all(target).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut digest = Sha256::new();
    let mut budget = TreeBudget { bytes: 0, entries: 0 };
    walk(source, Some(target), Path::new(""), kind, &mut digest, &mut budget)?;
    Ok(format!("sha256:{:x}", digest.finalize()))
}

pub(super) fn digest_tree(
    source: &File,
    kind: TreeKind,
) -> Result<String, Spec034ReleaseArtifactError> {
    let mut digest = Sha256::new();
    let mut budget = TreeBudget { bytes: 0, entries: 0 };
    walk(source, None, Path::new(""), kind, &mut digest, &mut budget)?;
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn walk(
    directory: &File,
    target: Option<&Path>,
    relative: &Path,
    kind: TreeKind,
    digest: &mut Sha256,
    budget: &mut TreeBudget,
) -> Result<(), Spec034ReleaseArtifactError> {
    let before = directory.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut names = read_base_dir(directory)
        .map_err(Spec034ReleaseArtifactError::Io)?
        .map(|entry| entry.map(|value| value.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    names.sort();
    names.dedup();
    for name in names {
        let child_relative = relative.join(&name);
        if excluded(&child_relative, kind) {
            continue;
        }
        budget.entries += 1;
        if budget.entries > MAX_TREE_ENTRIES {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        if let Ok(child) = open_directory(directory, &name) {
            let child: File = child.into();
            let child_metadata = child.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
            digest.update(b"d\0");
            digest.update(child_relative.as_os_str().as_encoded_bytes());
            digest.update([0]);
            update_live_metadata(digest, &child_metadata, kind);
            let child_target = target.map(|path| path.join(&name));
            if let Some(path) = &child_target {
                std::fs::create_dir_all(path).map_err(Spec034ReleaseArtifactError::Io)?;
            }
            walk(&child, child_target.as_deref(), &child_relative, kind, digest, budget)?;
            continue;
        }
        let descriptor = openat(directory, &name, FILE_FLAGS, Mode::empty())
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let mut file: File = descriptor.into();
        let file_before = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if !file_before.is_file() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_TREE_BYTES.saturating_sub(budget.bytes) + 1)
            .read_to_end(&mut bytes)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        budget.bytes = budget
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
        let file_after = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if budget.bytes > MAX_TREE_BYTES || !same_snapshot(&file_before, &file_after) {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        digest.update(b"f\0");
        digest.update(child_relative.as_os_str().as_encoded_bytes());
        digest.update([0]);
        digest.update(mode(&file_after).to_le_bytes());
        update_live_metadata(digest, &file_after, kind);
        digest.update(digest_bytes(&bytes).as_bytes());
        digest.update([0]);
        if let Some(path) = target.map(|value| value.join(&name)) {
            let mut output = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path)
                .map_err(Spec034ReleaseArtifactError::Io)?;
            output.write_all(&bytes).map_err(Spec034ReleaseArtifactError::Io)?;
            preserve_mode(&path, &file_after)?;
        }
    }
    let after = directory.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    same_snapshot(&before, &after)
        .then_some(())
        .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
}

fn excluded(relative: &Path, kind: TreeKind) -> bool {
    match kind {
        TreeKind::Worktree | TreeKind::LiveWorktree => relative.components().any(|component| {
            matches!(component, Component::Normal(name) if name == ".git" || name == ".omo" || name == "target")
        }),
        TreeKind::Git => relative.components().next().is_some_and(|component| {
            matches!(component, Component::Normal(name) if matches!(name.to_str(), Some("config" | "config.worktree" | "hooks" | "logs" | "worktrees" | "commondir" | "gitdir")))
        }),
        TreeKind::ControlledGit => relative.components().next().is_some_and(|component| {
            matches!(component, Component::Normal(name) if matches!(name.to_str(), Some("config.worktree" | "hooks" | "logs" | "worktrees" | "commondir" | "gitdir")))
        }),
        TreeKind::Cache => relative == Path::new("cache-manifest.json"),
    }
}

#[cfg(unix)]
fn update_live_metadata(digest: &mut Sha256, metadata: &Metadata, kind: TreeKind) {
    use std::os::unix::fs::MetadataExt;
    if kind == TreeKind::LiveWorktree {
        digest.update(metadata.dev().to_le_bytes());
        digest.update(metadata.ino().to_le_bytes());
        digest.update(metadata.ctime().to_le_bytes());
        digest.update(metadata.ctime_nsec().to_le_bytes());
        digest.update(metadata.mode().to_le_bytes());
        digest.update(metadata.size().to_le_bytes());
    }
}

#[cfg(not(unix))]
fn update_live_metadata(_digest: &mut Sha256, _metadata: &Metadata, _kind: TreeKind) {}

pub(super) fn open_directory(
    parent: &File,
    name: &OsStr,
) -> Result<rustix::fd::OwnedFd, Spec034ReleaseArtifactError> {
    openat(parent, name, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

pub(super) fn open_optional_directory(
    parent: &File,
    name: &OsStr,
) -> Result<Option<File>, Spec034ReleaseArtifactError> {
    match openat(parent, name, DIRECTORY_FLAGS, Mode::empty()) {
        Ok(descriptor) => Ok(Some(descriptor.into())),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(None),
        Err(_) => Err(Spec034ReleaseArtifactError::InvalidConfig),
    }
}

pub(super) fn open_anchored_directory(
    anchor: &File,
    path: &Path,
) -> Result<File, Spec034ReleaseArtifactError> {
    let mut current = if path.is_absolute() {
        let descriptor = openat(CWD, Path::new("/"), DIRECTORY_FLAGS, Mode::empty())
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        File::from(descriptor)
    } else {
        anchor.try_clone().map_err(Spec034ReleaseArtifactError::Io)?
    };
    for component in path.components() {
        let name: OsString = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::ParentDir => OsString::from(".."),
            Component::Normal(name) => name.to_os_string(),
            Component::Prefix(_) => return Err(Spec034ReleaseArtifactError::InvalidConfig),
        };
        current = File::from(open_directory(&current, &name)?);
    }
    Ok(current)
}

pub(super) fn read_file(
    parent: &File,
    name: &OsStr,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>, Spec034ReleaseArtifactError> {
    let descriptor = match openat(parent, name, FILE_FLAGS, Mode::empty()) {
        Ok(value) => value,
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(None),
        Err(_) => return Err(Spec034ReleaseArtifactError::InvalidConfig),
    };
    let mut file: File = descriptor.into();
    let before = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let after = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    if bytes.len() as u64 > max_bytes || !same_snapshot(&before, &after) {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    Ok(Some(bytes))
}
