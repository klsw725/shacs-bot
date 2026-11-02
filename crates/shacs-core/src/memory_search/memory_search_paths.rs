use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use super::model_manifest::ModelManifest;

const RUNTIME_DIR: &str = "runtime";
const MEMORY_SEARCH_DIR: &str = "memory-search";
const MODELS_DIR: &str = "models";
const INDEX_DIR: &str = "index";

/// The folders `memory_search` keeps under the data directory: `runtime/memory-search/models/`
/// and `runtime/memory-search/index/`.
///
/// The data directory is resolved once. Below it, no path component may be a symbolic link,
/// folders from `memory-search` down are 0700 and files are 0600.
#[derive(Debug, Clone)]
pub struct MemorySearchPaths {
    data_dir: PathBuf,
}

impl MemorySearchPaths {
    pub fn new(data_dir: &Path) -> io::Result<Self> {
        Ok(Self {
            data_dir: fs::canonicalize(data_dir)?,
        })
    }

    pub fn models_dir(&self) -> PathBuf {
        self.memory_search_dir().join(MODELS_DIR)
    }

    pub fn model_dir(&self, manifest: &ModelManifest) -> PathBuf {
        self.models_dir().join(manifest.directory_name())
    }

    pub fn index_dir(&self) -> PathBuf {
        self.memory_search_dir().join(INDEX_DIR)
    }

    /// The index of the workspace whose canonical path has `workspace_digest`.
    pub fn index_file(&self, workspace_digest: &str) -> PathBuf {
        self.index_dir().join(format!("{workspace_digest}.sqlite3"))
    }

    fn memory_search_dir(&self) -> PathBuf {
        self.data_dir.join(RUNTIME_DIR).join(MEMORY_SEARCH_DIR)
    }

    /// Creates `dir` and its missing parents. `runtime` keeps the permissions it is created
    /// with because other parts of the runtime share it; `memory-search` and everything below
    /// become 0700.
    pub(super) fn create_private_dir(&self, dir: &Path) -> io::Result<()> {
        self.make_private(dir, true)
    }

    pub(super) fn make_existing_dirs_private(&self, dir: &Path) -> io::Result<()> {
        self.make_private(dir, false)
    }

    fn make_private(&self, dir: &Path, create: bool) -> io::Result<()> {
        let relative = self.relative(dir)?;
        let mut current = self.data_dir.clone();
        for (depth, component) in relative.components().enumerate() {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(symlink_error(&current));
                }
                Ok(metadata) if !metadata.is_dir() => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("memory search path is not a folder: {}", current.display()),
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                            self.check_no_symlinks(&current)?;
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
            if depth > 0 {
                set_mode(&current, 0o700)?;
            }
        }
        Ok(())
    }

    /// Fails when any existing component of `path` below the data directory is a symbolic
    /// link.
    pub(super) fn check_no_symlinks(&self, path: &Path) -> io::Result<()> {
        let relative = self.relative(path)?;
        let mut current = self.data_dir.clone();
        for component in relative.components() {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(symlink_error(&current));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn relative<'a>(&self, path: &'a Path) -> io::Result<&'a Path> {
        path.strip_prefix(&self.data_dir).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "memory search path is outside the data directory: {}",
                    path.display()
                ),
            )
        })
    }
}

/// SHA-256 of the canonical workspace path, in lowercase hex: the name of its index.
pub fn workspace_digest(workspace: &Path) -> io::Result<String> {
    let canonical = fs::canonicalize(workspace)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical.as_os_str().as_encoded_bytes())
    ))
}

/// Opens `path` without following a symbolic link at its end. A new file is created 0600 and
/// an existing one is set to 0600. Anything there that is not a regular file is refused before
/// it is opened.
pub(super) fn open_private_file(path: &Path, options: &mut OpenOptions) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => return Err(not_a_file(path)),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Opening a FIFO put here since the check above must not wait for a reader.
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(not_a_file(path));
    }
    set_mode(path, 0o600)?;
    Ok(file)
}

fn not_a_file(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("memory search path is not a file: {}", path.display()),
    )
}

/// Sets the permissions of a file or folder this module owns. Changed by path: `path` was
/// checked not to be a symbolic link just before.
pub(super) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let current = fs::symlink_metadata(path)?.permissions().mode() & 0o777;
        if current != mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

pub(super) fn symlink_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("memory search path is a symbolic link: {}", path.display()),
    )
}
