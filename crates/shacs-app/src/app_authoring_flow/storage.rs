use super::ApplyError;
use serde::{de::DeserializeOwned, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn tree_digest(root: &Path) -> Result<String, ApplyError> {
    let root = root.canonicalize()?;
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    hasher.update(b"shacs-authoring-revision-v1\n");
    for (relative, path) in files {
        hasher.update(relative.as_os_str().as_encoded_bytes());
        hasher.update(b"\n");
        hasher.update(fs::read(path)?);
        hasher.update(b"\n");
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub(crate) fn public_tree_digest(root: &Path) -> Result<String, ApplyError> {
    tree_digest(root)
}

pub(crate) fn short_digest(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
        .chars()
        .take(16)
        .collect()
}

pub(crate) fn copy_tree(source: &Path, destination: &Path) -> Result<(), ApplyError> {
    if destination.exists() {
        return Err(ApplyError::UnsafeCandidate(destination.to_path_buf()));
    }
    fs::create_dir_all(destination)?;
    copy_children(source, destination)
}

pub(crate) fn replace_tree(source: &Path, destination: &Path) -> Result<(), ApplyError> {
    let staging = destination.with_extension(format!("apply-{}.tmp", std::process::id()));
    remove_tree(&staging)?;
    copy_tree(source, &staging)?;
    remove_tree(destination)?;
    fs::rename(staging, destination)?;
    sync_parent(destination)
}

pub(crate) fn remove_tree(path: &Path) -> Result<(), ApplyError> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), ApplyError> {
    let parent = path
        .parent()
        .ok_or_else(|| ApplyError::UnsafeCandidate(path.to_path_buf()))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, ApplyError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), ApplyError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_symlink() {
            return Err(ApplyError::UnsafeCandidate(path));
        }
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ApplyError::UnsafeCandidate(path.clone()))?;
            files.push((relative.to_path_buf(), path));
        }
    }
    Ok(())
}

fn copy_children(source: &Path, destination: &Path) -> Result<(), ApplyError> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_symlink() {
            return Err(ApplyError::UnsafeCandidate(entry.path()));
        }
        if file_type.is_dir() {
            fs::create_dir(&target)?;
            copy_children(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), ApplyError> {
    let parent = path
        .parent()
        .ok_or_else(|| ApplyError::UnsafeCandidate(path.to_path_buf()))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
