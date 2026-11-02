use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use super::memory_search_paths::{self, MemorySearchPaths};
use super::model_manifest::{ModelFile, ModelManifest, CONFIG_FILE, TOKENIZER_FILE, WEIGHTS_FILE};

/// The files of one model, each read once and found to match the manifest. Loading uses these
/// bytes, so what was checked is what runs.
pub struct VerifiedModel {
    pub manifest: &'static ModelManifest,
    pub config: Vec<u8>,
    pub tokenizer: Vec<u8>,
    pub weights: Vec<u8>,
}

#[derive(Debug)]
pub enum ModelFileError {
    /// The file is not there.
    Missing { file: &'static str },
    /// The file is there but is not a regular file, or its size or SHA-256 differs.
    Unverified { file: &'static str, reason: String },
    /// The path to the file, or the file itself, is a symbolic link.
    Unsafe { path: PathBuf },
    Io {
        file: &'static str,
        error: io::Error,
    },
}

impl fmt::Display for ModelFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { file } => write!(formatter, "model file {file} is missing"),
            Self::Unverified { file, reason } => {
                write!(formatter, "model file {file} failed verification: {reason}")
            }
            Self::Unsafe { path } => {
                write!(
                    formatter,
                    "model path is a symbolic link: {}",
                    path.display()
                )
            }
            Self::Io { file, error } => {
                write!(formatter, "model file {file} is unreadable: {error}")
            }
        }
    }
}

impl std::error::Error for ModelFileError {}

/// Reads every file of `manifest` from its folder and checks its size and SHA-256.
pub fn read_verified_model(
    paths: &MemorySearchPaths,
    manifest: &'static ModelManifest,
) -> Result<VerifiedModel, ModelFileError> {
    let dir = paths.model_dir(manifest);
    let mut config = Vec::new();
    let mut tokenizer = Vec::new();
    let mut weights = Vec::new();
    for file in &manifest.files {
        let bytes = read_verified_file(paths, &dir, file)?;
        match file.name {
            CONFIG_FILE => config = bytes,
            TOKENIZER_FILE => tokenizer = bytes,
            WEIGHTS_FILE => weights = bytes,
            _ => {}
        }
    }
    Ok(VerifiedModel {
        manifest,
        config,
        tokenizer,
        weights,
    })
}

/// Whether the file at `path` is a regular file whose size and SHA-256 match `file`.
pub(super) fn file_matches(path: &Path, file: &ModelFile) -> Result<bool, ModelFileError> {
    match read_matching(path, file) {
        Ok(_) => Ok(true),
        Err(ModelFileError::Missing { .. } | ModelFileError::Unverified { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

fn read_verified_file(
    paths: &MemorySearchPaths,
    dir: &Path,
    file: &ModelFile,
) -> Result<Vec<u8>, ModelFileError> {
    let path = dir.join(file.name);
    let parent = path.parent().unwrap_or(dir);
    paths.make_existing_dirs_private(parent).map_err(|error| {
        if paths.check_no_symlinks(parent).is_err() {
            ModelFileError::Unsafe {
                path: parent.to_path_buf(),
            }
        } else {
            ModelFileError::Io {
                file: file.name,
                error,
            }
        }
    })?;
    paths
        .check_no_symlinks(&path)
        .map_err(|_| ModelFileError::Unsafe { path: path.clone() })?;
    read_matching(&path, file)
}

fn read_matching(path: &Path, file: &ModelFile) -> Result<Vec<u8>, ModelFileError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(ModelFileError::Unsafe {
                path: path.to_path_buf(),
            });
        }
        Ok(metadata) if !metadata.is_file() => return Err(not_regular(file)),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ModelFileError::Missing { file: file.name });
        }
        Err(error) => {
            return Err(ModelFileError::Io {
                file: file.name,
                error,
            })
        }
    }
    memory_search_paths::set_mode(path, 0o600).map_err(|error| ModelFileError::Io {
        file: file.name,
        error,
    })?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Opening a FIFO put here since the check above must not wait for a writer.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut handle = match options.open(path) {
        Ok(handle) => handle,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ModelFileError::Missing { file: file.name });
        }
        Err(error) if is_symlink_refusal(&error) => {
            return Err(ModelFileError::Unsafe {
                path: path.to_path_buf(),
            });
        }
        Err(error) => {
            return Err(ModelFileError::Io {
                file: file.name,
                error,
            })
        }
    };
    let metadata = handle.metadata().map_err(|error| ModelFileError::Io {
        file: file.name,
        error,
    })?;
    if !metadata.is_file() {
        return Err(not_regular(file));
    }
    if metadata.len() != file.size {
        return Err(ModelFileError::Unverified {
            file: file.name,
            reason: format!("size {} differs from {}", metadata.len(), file.size),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(file.size).unwrap_or(0));
    Read::by_ref(&mut handle)
        .take(file.size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| ModelFileError::Io {
            file: file.name,
            error,
        })?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() as u64 != file.size || digest != file.sha256 {
        return Err(ModelFileError::Unverified {
            file: file.name,
            reason: format!("SHA-256 {digest} differs from {}", file.sha256),
        });
    }
    Ok(bytes)
}

fn not_regular(file: &ModelFile) -> ModelFileError {
    ModelFileError::Unverified {
        file: file.name,
        reason: "not a regular file".to_owned(),
    }
}

/// `O_NOFOLLOW` refuses a symbolic link at the end of the path with `ELOOP`.
fn is_symlink_refusal(error: &io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::ELOOP)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}
