use super::super::model::Spec034ReleaseArtifactError;
use std::path::Path;

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
mod platform {
    use super::*;
    use cap_primitives::fs::remove_dir_all;
    use rustix::fd::OwnedFd;
    use rustix::fs::{
        fstat, fsync, mkdirat, openat, renameat_with, statat, AtFlags, Mode, OFlags, RenameFlags,
        CWD,
    };
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::path::Component;

    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    struct CreatedComponent {
        parent_index: usize,
        name: OsString,
    }

    pub struct EvidenceDestination {
        absolute: bool,
        components: Vec<OsString>,
        handles: Vec<File>,
        created: Vec<CreatedComponent>,
        leaf: OsString,
        published: bool,
    }

    impl EvidenceDestination {
        pub fn prepare(path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
            let (absolute, mut components) = parse(path)?;
            let leaf = components
                .pop()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            let anchor = open_anchor(absolute)?;
            let mut destination = Self {
                absolute,
                components,
                handles: vec![anchor.into()],
                created: Vec::new(),
                leaf,
                published: false,
            };
            destination.open_components()?;
            destination.require_leaf_missing()?;
            Ok(destination)
        }

        pub fn publish(&mut self, staging: &Path) -> Result<(), Spec034ReleaseArtifactError> {
            self.publish_with(staging, || {})
        }

        fn publish_with(
            &mut self,
            staging: &Path,
            before_rename: impl FnOnce(),
        ) -> Result<(), Spec034ReleaseArtifactError> {
            self.verify_chain()?;
            let source_parent = staging
                .parent()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            let source_name = staging
                .file_name()
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            let source_parent =
                File::open(source_parent).map_err(Spec034ReleaseArtifactError::Io)?;
            before_rename();
            self.verify_chain()?;
            self.require_leaf_missing()?;
            renameat_with(
                &source_parent,
                source_name,
                self.parent(),
                &self.leaf,
                RenameFlags::NOREPLACE,
            )
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
            if fsync(self.parent()).is_err() {
                let _ = remove_dir_all(self.parent(), Path::new(&self.leaf));
                return Err(Spec034ReleaseArtifactError::InvalidConfig);
            }
            if let Err(error) = self.verify_chain() {
                let _ = remove_dir_all(self.parent(), Path::new(&self.leaf));
                return Err(error);
            }
            self.published = true;
            Ok(())
        }

        fn open_components(&mut self) -> Result<(), Spec034ReleaseArtifactError> {
            for name in &self.components {
                let parent_index = self.handles.len() - 1;
                let child = match open_child(&self.handles[parent_index], name) {
                    Ok(child) => child,
                    Err(error) if error == rustix::io::Errno::NOENT => {
                        mkdirat(
                            &self.handles[parent_index],
                            name,
                            Mode::from_raw_mode(0o700),
                        )
                        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
                        self.created.push(CreatedComponent {
                            parent_index,
                            name: name.clone(),
                        });
                        open_child(&self.handles[parent_index], name)
                            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?
                    }
                    Err(_) => return Err(Spec034ReleaseArtifactError::InvalidConfig),
                };
                self.handles.push(child.into());
            }
            Ok(())
        }

        fn verify_chain(&self) -> Result<(), Spec034ReleaseArtifactError> {
            let anchor = open_anchor(self.absolute)?;
            same_handle(&anchor, &self.handles[0])?;
            let mut current: File = anchor.into();
            for (index, name) in self.components.iter().enumerate() {
                let child = open_child(&current, name)
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
                same_handle(&child, &self.handles[index + 1])?;
                current = child.into();
            }
            Ok(())
        }

        fn require_leaf_missing(&self) -> Result<(), Spec034ReleaseArtifactError> {
            match statat(self.parent(), &self.leaf, AtFlags::SYMLINK_NOFOLLOW) {
                Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
                _ => Err(Spec034ReleaseArtifactError::InvalidConfig),
            }
        }

        fn parent(&self) -> &File {
            &self.handles[self.handles.len() - 1]
        }
    }

    impl Drop for EvidenceDestination {
        fn drop(&mut self) {
            if self.published {
                return;
            }
            for created in self.created.iter().rev() {
                let parent = &self.handles[created.parent_index];
                if same_handle_path(
                    parent,
                    &created.name,
                    &self.handles[created.parent_index + 1],
                ) {
                    let _ = remove_dir_all(parent, Path::new(&created.name));
                }
            }
        }
    }

    fn parse(path: &Path) -> Result<(bool, Vec<OsString>), Spec034ReleaseArtifactError> {
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

    fn open_anchor(absolute: bool) -> Result<OwnedFd, Spec034ReleaseArtifactError> {
        let path = if absolute {
            Path::new("/")
        } else {
            Path::new(".")
        };
        openat(CWD, path, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)
    }

    fn open_child(parent: &File, name: &OsStr) -> rustix::io::Result<OwnedFd> {
        openat(parent, name, DIRECTORY_FLAGS, Mode::empty())
    }

    fn same_handle(left: &OwnedFd, right: &File) -> Result<(), Spec034ReleaseArtifactError> {
        let left = fstat(left).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        let right = fstat(right).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
        if left.st_dev == right.st_dev && left.st_ino == right.st_ino {
            Ok(())
        } else {
            Err(Spec034ReleaseArtifactError::InvalidConfig)
        }
    }

    fn same_handle_path(parent: &File, name: &OsStr, handle: &File) -> bool {
        let Ok(path) = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
            return false;
        };
        let Ok(opened) = fstat(handle) else {
            return false;
        };
        path.st_dev == opened.st_dev && path.st_ino == opened.st_ino
    }

    #[cfg(all(test, unix))]
    mod tests;
}

#[cfg(any(target_os = "linux", target_vendor = "apple"))]
pub use platform::EvidenceDestination;

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
pub struct EvidenceDestination;

#[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
impl EvidenceDestination {
    pub fn prepare(_path: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }

    pub fn publish(&mut self, _staging: &Path) -> Result<(), Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}
