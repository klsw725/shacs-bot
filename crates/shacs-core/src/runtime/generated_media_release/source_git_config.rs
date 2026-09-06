use super::model::Spec034ReleaseArtifactError;
#[cfg(test)]
use super::path_chain::PathChainSeal;
#[cfg(test)]
use rustix::fs::{open, Mode, OFlags};
#[cfg(test)]
use std::fs::File;
#[cfg(test)]
use std::path::{Path, PathBuf};

#[path = "source_git_config/policy.rs"]
mod policy;
pub(super) use policy::reject_behavior_config;

#[cfg(test)]
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[cfg(test)]
pub(super) struct GitConfigSeal {
    seals: Vec<ConfigPathSeal>,
}

#[cfg(test)]
enum ConfigPathSeal {
    Present(ConfigFileSeal),
    Absent { path: PathBuf, parent: PathChainSeal },
}

#[cfg(test)]
struct ConfigFileSeal {
    path: PathBuf,
    file: File,
    snapshot: ConfigFileSnapshot,
}

#[cfg(test)]
#[derive(PartialEq, Eq)]
struct ConfigFileSnapshot {
    device: u64,
    inode: u64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
    mode: u32,
    links: u64,
    size: u64,
    digest: String,
}

#[cfg(test)]
impl GitConfigSeal {
    pub fn capture(root: &Path) -> Result<Self, Spec034ReleaseArtifactError> {
        let dot_git = root.join(".git");
        let metadata = std::fs::symlink_metadata(&dot_git)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        let (git_dir, mut seals) = if metadata.is_dir() {
            (dot_git.clone(), Vec::new())
        } else if metadata.is_file() {
            let (bytes, seal) = read_and_seal(&dot_git)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
            let path = text
                .trim()
                .strip_prefix("gitdir: ")
                .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
            let git_dir = if Path::new(path).is_absolute() {
                PathBuf::from(path)
            } else {
                root.join(path)
            };
            (git_dir, vec![ConfigPathSeal::Present(seal)])
        } else {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        };
        let git_dir = git_dir
            .canonicalize()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let common_config = if metadata.is_file() {
            git_dir.join("../..").join("config")
        } else {
            git_dir.join("config")
        };
        for path in [common_config, git_dir.join("config.worktree")] {
            match std::fs::symlink_metadata(&path) {
                Ok(_) => {
                    let (bytes, seal) = read_and_seal(&path)?;
                    reject_behavior_config(&bytes)?;
                    seals.push(ConfigPathSeal::Present(seal));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    seals.push(absent_seal(&path)?);
                }
                Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
            }
        }
        let alternates = git_dir.join("objects/info/alternates");
        match std::fs::symlink_metadata(&alternates) {
            Ok(_) => return Err(Spec034ReleaseArtifactError::InvalidConfig),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                seals.push(absent_seal(&alternates)?);
            }
            Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
        }
        Ok(Self { seals })
    }

    pub fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        for seal in &self.seals {
            match seal {
                ConfigPathSeal::Present(seal) => seal.verify()?,
                ConfigPathSeal::Absent { path, parent } => {
                    parent.verify()?;
                    match std::fs::symlink_metadata(path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        _ => return Err(Spec034ReleaseArtifactError::DigestMismatch),
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
fn absent_seal(path: &Path) -> Result<ConfigPathSeal, Spec034ReleaseArtifactError> {
    let mut existing = path
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    }
    let parent = existing
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(ConfigPathSeal::Absent {
        path: path.to_path_buf(),
        parent: PathChainSeal::capture_leaf(&parent)?,
    })
}

#[cfg(test)]
fn read_and_seal(
    path: &Path,
) -> Result<(Vec<u8>, ConfigFileSeal), Spec034ReleaseArtifactError> {
    read_and_seal_with(path, || {})
}

#[cfg(test)]
fn read_and_seal_with(
    path: &Path,
    after_read: impl FnOnce(),
) -> Result<(Vec<u8>, ConfigFileSeal), Spec034ReleaseArtifactError> {
    let descriptor = open(
        path,
        OFlags::RDONLY.union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC),
        Mode::empty(),
    )
    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let file: File = descriptor.into();
    let before = ConfigFileSnapshot::capture(&file, read_descriptor(&file)?)?;
    let bytes = read_descriptor(&file)?;
    let after = ConfigFileSnapshot::capture(&file, bytes.clone())?;
    if before != after {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    after_read();
    let seal = ConfigFileSeal {
        path: path.to_path_buf(),
        file,
        snapshot: after,
    };
    seal.verify_path_identity()?;
    Ok((bytes, seal))
}

#[cfg(test)]
pub(super) fn read_seal_hook_for_test(
    path: &Path,
    after_read: impl FnOnce(),
) -> Result<(), Spec034ReleaseArtifactError> {
    let (_, seal) = read_and_seal_with(path, after_read)?;
    seal.verify()
}

#[cfg(test)]
impl ConfigFileSeal {
    fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.verify_path_identity()?;
        let current = ConfigFileSnapshot::capture(&self.file, read_descriptor(&self.file)?)?;
        (current == self.snapshot)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }

    #[cfg(unix)]
    fn verify_path_identity(&self) -> Result<(), Spec034ReleaseArtifactError> {
        use std::os::unix::fs::MetadataExt;
        let path = std::fs::symlink_metadata(&self.path).map_err(Spec034ReleaseArtifactError::Io)?;
        let opened = self.file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        (path.file_type().is_file()
            && path.dev() == opened.dev()
            && path.ino() == opened.ino())
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }

    #[cfg(not(unix))]
    fn verify_path_identity(&self) -> Result<(), Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}

#[cfg(all(test, unix))]
impl ConfigFileSnapshot {
    fn capture(file: &File, bytes: Vec<u8>) -> Result<Self, Spec034ReleaseArtifactError> {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES || bytes.len() as u64 != metadata.len() {
            return Err(Spec034ReleaseArtifactError::InvalidConfig);
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            ctime_seconds: metadata.ctime(),
            ctime_nanoseconds: metadata.ctime_nsec(),
            mode: metadata.mode(),
            links: metadata.nlink(),
            size: metadata.size(),
            digest: super::artifacts::digest_bytes(&bytes),
        })
    }
}

#[cfg(all(test, not(unix)))]
impl ConfigFileSnapshot {
    fn capture(_file: &File, _bytes: Vec<u8>) -> Result<Self, Spec034ReleaseArtifactError> {
        Err(Spec034ReleaseArtifactError::InvalidConfig)
    }
}

#[cfg(all(test, unix))]
fn read_descriptor(file: &File) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::FileExt;
    let size = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?.len();
    if size > MAX_CONFIG_BYTES {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    let mut bytes = vec![0_u8; size as usize];
    file.read_exact_at(&mut bytes, 0)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(bytes)
}

#[cfg(all(test, not(unix)))]
fn read_descriptor(_file: &File) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}
