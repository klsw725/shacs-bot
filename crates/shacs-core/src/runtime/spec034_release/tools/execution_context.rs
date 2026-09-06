use super::{digest_bytes, PathChainSeal, Spec034ReleaseArtifactError};
use std::fs::{File, Metadata, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

pub(super) struct ToolchainExecutionContext {
    home: PathChainSeal,
    cargo_home: PathChainSeal,
    target: PathChainSeal,
    home_cargo: PathChainSeal,
    config: ConfigFileSeal,
    config_toml: ConfigFileSeal,
    cargo_home_path: PathBuf,
    home_cargo_path: PathBuf,
}

struct ConfigFileSeal {
    path: PathBuf,
    handle: File,
    identity: FileIdentity,
    digest: String,
}

#[derive(PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
    size: u64,
}

impl ToolchainExecutionContext {
    pub(super) fn prepare(
        home: &Path,
        cargo_home: &Path,
        target: &Path,
        vendor: Option<&Path>,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        let home_cargo_path = home.join(".cargo");
        std::fs::create_dir_all(&home_cargo_path).map_err(Spec034ReleaseArtifactError::Io)?;
        prepare_cargo_home(cargo_home)?;
        let config_bytes = config_bytes(vendor)?;
        let config = create_config(cargo_home.join("config"), &config_bytes)?;
        let config_toml = create_config(cargo_home.join("config.toml"), &config_bytes)?;
        let context = Self {
            home: PathChainSeal::capture_mutable(home)?,
            cargo_home: PathChainSeal::capture_mutable(cargo_home)?,
            target: PathChainSeal::capture_mutable(target)?,
            home_cargo: PathChainSeal::capture_leaf(&home_cargo_path)?,
            config,
            config_toml,
            cargo_home_path: cargo_home.to_path_buf(),
            home_cargo_path,
        };
        context.verify()?;
        Ok(context)
    }

    pub(super) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.home.verify()?;
        self.cargo_home.verify()?;
        self.target.verify()?;
        self.home_cargo.verify()?;
        self.config.verify()?;
        self.config_toml.verify()?;
        reject_alternate_configs(&self.cargo_home_path, &self.home_cargo_path)
    }
}

fn create_config(path: PathBuf, expected: &[u8]) -> Result<ConfigFileSeal, Spec034ReleaseArtifactError> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(expected)
                .map_err(Spec034ReleaseArtifactError::Io)?;
            file.sync_all().map_err(Spec034ReleaseArtifactError::Io)?;
            set_read_only(&path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
    }
    let handle = File::open(&path).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = std::fs::symlink_metadata(&path).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut seal = ConfigFileSeal {
        path,
        handle,
        identity: file_identity(&metadata),
        digest: digest_bytes(expected),
    };
    seal.verify()?;
    seal.handle
        .rewind()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(seal)
}

impl ConfigFileSeal {
    fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        let path_metadata = std::fs::symlink_metadata(&self.path)
            .map_err(|_| Spec034ReleaseArtifactError::DigestMismatch)?;
        let handle_metadata = self
            .handle
            .metadata()
            .map_err(|_| Spec034ReleaseArtifactError::DigestMismatch)?;
        if path_metadata.file_type().is_symlink()
            || !path_metadata.is_file()
            || !path_metadata.permissions().readonly()
            || file_identity(&path_metadata) != self.identity
            || file_identity(&handle_metadata) != self.identity
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let mut handle = self
            .handle
            .try_clone()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let mut bytes = Vec::new();
        handle
            .rewind()
            .map_err(Spec034ReleaseArtifactError::Io)?;
        handle
            .read_to_end(&mut bytes)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        (digest_bytes(&bytes) == self.digest)
            .then_some(())
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)
    }
}

fn reject_alternate_configs(
    cargo_home: &Path,
    home_cargo: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    for entry in std::fs::read_dir(cargo_home).map_err(Spec034ReleaseArtifactError::Io)? {
        let name = entry
            .map_err(Spec034ReleaseArtifactError::Io)?
            .file_name();
        let name = name.to_string_lossy();
        if name.starts_with("config") && name != "config" && name != "config.toml" {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
    }
    for name in ["config", "config.toml"] {
        if std::fs::symlink_metadata(home_cargo.join(name)).is_ok() {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
    }
    Ok(())
}

fn prepare_cargo_home(cargo_home: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    for name in [".package-cache", ".package-cache-mutate", ".global-cache"] {
        let path = cargo_home.join(name);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if !file
            .metadata()
            .map_err(Spec034ReleaseArtifactError::Io)?
            .is_file()
            || std::fs::symlink_metadata(path)
                .map_err(Spec034ReleaseArtifactError::Io)?
                .file_type()
                .is_symlink()
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
    }
    Ok(())
}

fn config_bytes(vendor: Option<&Path>) -> Result<Vec<u8>, Spec034ReleaseArtifactError> {
    let mut config = String::from("[net]\noffline = true\n");
    if let Some(vendor) = vendor {
        let vendor = vendor.to_str().ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
        config.push_str("\n[source.crates-io]\nreplace-with = \"vendored-sources\"\n\n[source.vendored-sources]\ndirectory = ");
        config.push_str(&serde_json::to_string(vendor).map_err(Spec034ReleaseArtifactError::Json)?);
        config.push('\n');
    }
    Ok(config.into_bytes())
}

#[cfg(unix)]
fn set_read_only(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o400))
        .map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(not(unix))]
fn set_read_only(path: &Path) -> Result<(), Spec034ReleaseArtifactError> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions).map_err(Spec034ReleaseArtifactError::Io)
}

#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        ctime_seconds: metadata.ctime(),
        ctime_nanoseconds: metadata.ctime_nsec(),
        size: metadata.size(),
    }
}

#[cfg(not(unix))]
fn file_identity(metadata: &Metadata) -> FileIdentity {
    FileIdentity {
        device: 0,
        inode: 0,
        ctime_seconds: 0,
        ctime_nanoseconds: 0,
        size: metadata.len(),
    }
}
