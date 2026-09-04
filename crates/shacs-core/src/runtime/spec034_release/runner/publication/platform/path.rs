use super::{Spec034ReleaseArtifactError, DIRECTORY_FLAGS};
use rustix::fd::OwnedFd;
use rustix::fs::{openat, Mode, CWD};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::path::{Component, Path};

pub(super) fn parse(
    path: &Path,
) -> Result<(bool, Vec<OsString>), Spec034ReleaseArtifactError> {
    let mut absolute = false;
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir if components.is_empty() => absolute = true,
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::CurDir if components.is_empty() => {}
            Component::RootDir
            | Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_) => {
                return Err(Spec034ReleaseArtifactError::InvalidConfig);
            }
        }
    }
    Ok((absolute, components))
}

pub(super) fn open_anchor(
    absolute: bool,
) -> Result<OwnedFd, Spec034ReleaseArtifactError> {
    let path = if absolute {
        Path::new("/")
    } else {
        Path::new(".")
    };
    openat(CWD, path, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
}

pub(super) fn open_child(parent: &File, name: &OsStr) -> rustix::io::Result<OwnedFd> {
    openat(parent, name, DIRECTORY_FLAGS, Mode::empty())
}
