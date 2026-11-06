//! Reads below one folder through open descriptors only (Spec 048 PRD 050
//! scope 7). Every step opens with `O_NOFOLLOW`, so a symlink, or a path
//! swapped for one after a check, never leads out of the folder. Files open
//! with `O_NONBLOCK`, so a FIFO or a device never blocks, and only regular
//! files with a single link are read: a hard link made or swapped in after a
//! check by path could be a file the check would have refused.

use rustix::fd::OwnedFd;
use rustix::fs::{openat, statat, AtFlags, Dir, FileType, Mode, OFlags, Stat, CWD};
use rustix::io::Errno;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FOLDER_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

/// Why a path was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfinedSkip {
    Missing,
    Symlink,
    HardLink,
    NotAFolder,
    NotRegularFile,
    Unreadable,
}

impl ConfinedSkip {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Symlink => "symlink",
            Self::HardLink => "hard link",
            Self::NotAFolder => "not a folder",
            Self::NotRegularFile => "not a regular file",
            Self::Unreadable => "unreadable",
        }
    }
}

/// A regular file directly in a folder, as the folder lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinedEntry {
    pub name: String,
    pub modified: SystemTime,
}

#[derive(Debug)]
pub struct ConfinedFolder {
    folder: OwnedFd,
}

impl ConfinedFolder {
    /// Opens an absolute path from `/` down, every step a folder that is not
    /// a symlink.
    pub fn open(path: &Path) -> Result<Self, ConfinedSkip> {
        if !path.is_absolute() {
            return Err(ConfinedSkip::Unreadable);
        }
        let root = openat(CWD, "/", FOLDER_FLAGS, Mode::empty()).map_err(skip)?;
        Self { folder: root }.folder(
            path.strip_prefix("/")
                .map_err(|_| ConfinedSkip::Unreadable)?,
        )
    }

    /// The folder at `relative` below this one.
    pub fn folder(&self, relative: &Path) -> Result<Self, ConfinedSkip> {
        let mut current = self
            .folder
            .try_clone()
            .map_err(|_| ConfinedSkip::Unreadable)?;
        for name in names(relative)? {
            current = open_folder(&current, name)?;
        }
        Ok(Self { folder: current })
    }

    /// The first `max_bytes` of the regular file at `relative`.
    pub fn read_prefix(&self, relative: &Path, max_bytes: usize) -> Result<Vec<u8>, ConfinedSkip> {
        let names = names(relative)?;
        let Some((file_name, folders)) = names.split_last() else {
            return Err(ConfinedSkip::Unreadable);
        };
        let mut current = self
            .folder
            .try_clone()
            .map_err(|_| ConfinedSkip::Unreadable)?;
        for name in folders {
            current = open_folder(&current, name)?;
        }
        let file: File = openat(&current, *file_name, FILE_FLAGS, Mode::empty())
            .map_err(skip)?
            .into();
        let metadata = file.metadata().map_err(|_| ConfinedSkip::Unreadable)?;
        if !metadata.is_file() {
            return Err(ConfinedSkip::NotRegularFile);
        }
        if metadata.nlink() > 1 {
            return Err(ConfinedSkip::HardLink);
        }
        let limit = u64::try_from(max_bytes).map_err(|_| ConfinedSkip::Unreadable)?;
        let mut bytes = Vec::new();
        file.take(limit)
            .read_to_end(&mut bytes)
            .map_err(|_| ConfinedSkip::Unreadable)?;
        Ok(bytes)
    }

    /// The regular files directly in this folder. Symlinks and anything else
    /// are left out, and so are names that are not UTF-8.
    pub fn regular_files(&self) -> Result<Vec<ConfinedEntry>, ConfinedSkip> {
        let listing = Dir::read_from(&self.folder).map_err(skip)?;
        let mut entries = Vec::new();
        for entry in listing {
            let entry = entry.map_err(skip)?;
            let Ok(name) = entry.file_name().to_str() else {
                continue;
            };
            if name == "." || name == ".." {
                continue;
            }
            let Ok(stat) = statat(&self.folder, name, AtFlags::SYMLINK_NOFOLLOW) else {
                continue;
            };
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
                continue;
            }
            entries.push(ConfinedEntry {
                name: name.to_owned(),
                modified: modified(&stat),
            });
        }
        Ok(entries)
    }
}

fn names(relative: &Path) -> Result<Vec<&OsStr>, ConfinedSkip> {
    relative
        .components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name),
            _ => Err(ConfinedSkip::Unreadable),
        })
        .collect()
}

/// Some systems refuse a symlink opened as a folder as not being a folder;
/// the entry itself tells which it is.
fn open_folder(parent: &OwnedFd, name: &OsStr) -> Result<OwnedFd, ConfinedSkip> {
    openat(parent, name, FOLDER_FLAGS, Mode::empty()).map_err(|error| match skip(error) {
        ConfinedSkip::NotAFolder | ConfinedSkip::Symlink => {
            match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink => {
                    ConfinedSkip::Symlink
                }
                _ => ConfinedSkip::NotAFolder,
            }
        }
        other => other,
    })
}

fn skip(error: Errno) -> ConfinedSkip {
    match error {
        Errno::NOENT => ConfinedSkip::Missing,
        Errno::LOOP | Errno::MLINK => ConfinedSkip::Symlink,
        Errno::NOTDIR => ConfinedSkip::NotAFolder,
        Errno::NXIO | Errno::OPNOTSUPP => ConfinedSkip::NotRegularFile,
        _ => ConfinedSkip::Unreadable,
    }
}

fn modified(stat: &Stat) -> SystemTime {
    let seconds = u64::try_from(stat.st_mtime).unwrap_or(0);
    let nanoseconds = u32::try_from(stat.st_mtime_nsec).unwrap_or(0);
    UNIX_EPOCH + Duration::new(seconds, nanoseconds)
}

#[cfg(test)]
#[path = "confined_read_tests.rs"]
mod tests;
