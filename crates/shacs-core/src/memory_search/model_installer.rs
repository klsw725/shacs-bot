use fs2::FileExt;
use sha2::{Digest, Sha256};
use shacs_security::{resolve_redirect_url, NetworkGuard};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use ureq::http::header::LOCATION;
use ureq::Agent;

use super::memory_search_paths::{self, MemorySearchPaths};
use super::model_manifest::{ModelFile, ModelManifest};
use super::verified_model::{self, ModelFileError};

const PARTIAL_MARKER: &str = ".partial-";
const INSTALL_LOCK: &str = ".install.lock";
const MAX_REDIRECTS: usize = 10;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

/// Where model files come from. The only network use of `memory_search`.
pub trait ModelFileSource {
    /// Writes the body found at `url` into `sink`, reading at most `limit` bytes.
    fn fetch(&self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<(), String>;
}

/// Downloads from Hugging Face, every request and redirect checked by the network guard. Only
/// connecting and the response head have time limits: a body of gigabytes may take long.
pub struct HuggingFaceSource {
    network_guard: NetworkGuard,
}

impl HuggingFaceSource {
    pub fn new(network_guard: NetworkGuard) -> Self {
        Self { network_guard }
    }
}

impl ModelFileSource for HuggingFaceSource {
    fn fetch(&self, url: &str, limit: u64, sink: &mut dyn Write) -> Result<(), String> {
        let agent: Agent = Agent::config_builder()
            .user_agent("shacs-bot memory model install")
            .max_redirects(0)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .new_agent();
        self.network_guard.validate_url_target(url)?;
        let mut current = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            let mut response = agent
                .get(&current)
                .call()
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| format!("redirect without a location from {current}"))?;
                let next = resolve_redirect_url(&current, location)?;
                self.network_guard.validate_url_target(&next)?;
                current = next;
                continue;
            }
            if status.as_u16() != 200 {
                return Err(format!("{current} answered {status}"));
            }
            let mut reader = response.body_mut().with_config().limit(limit).reader();
            io::copy(&mut reader, sink).map_err(|error| error.to_string())?;
            return Ok(());
        }
        Err(format!("more than {MAX_REDIRECTS} redirects from {url}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledFile {
    AlreadyVerified,
    Downloaded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub model_dir: PathBuf,
    pub files: Vec<(&'static str, InstalledFile)>,
}

#[derive(Debug)]
pub enum InstallError {
    Io(io::Error),
    Unsafe(PathBuf),
    Busy,
    Download { file: &'static str, reason: String },
    Verification { file: &'static str, reason: String },
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "model install failed: {error}"),
            Self::Unsafe(path) => {
                write!(
                    formatter,
                    "model path is a symbolic link: {}",
                    path.display()
                )
            }
            Self::Busy => formatter.write_str("another memory model install is running"),
            Self::Download { file, reason } => {
                write!(
                    formatter,
                    "model file {file} could not be downloaded: {reason}"
                )
            }
            Self::Verification { file, reason } => {
                write!(
                    formatter,
                    "downloaded model file {file} failed verification: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for InstallError {}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Puts every file of `manifest` into its folder. A file already there that matches the
/// manifest is kept; any other is removed, downloaded into a 0600 temporary file and renamed
/// into place only once its size and SHA-256 match. Temporary files left by an earlier run are
/// removed first, so an interrupted install never leaves an unverified file under a final
/// name. One install runs at a time, so none removes the temporary files of another.
pub fn install_model(
    paths: &MemorySearchPaths,
    manifest: &'static ModelManifest,
    source: &dyn ModelFileSource,
) -> Result<InstallReport, InstallError> {
    let _lock = lock_installs(paths)?;
    let model_dir = paths.model_dir(manifest);
    let mut files = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let path = model_dir.join(file.name);
        let parent = path.parent().unwrap_or(&model_dir);
        paths
            .create_private_dir(parent)
            .map_err(|error| unsafe_or_io(paths, parent, error))?;
        remove_partial_files(parent)?;
        paths
            .check_no_symlinks(&path)
            .map_err(|_| InstallError::Unsafe(path.clone()))?;
        let state = match verified_model::file_matches(&path, file) {
            Ok(true) => InstalledFile::AlreadyVerified,
            Ok(false) | Err(ModelFileError::Io { .. }) => {
                remove_unverified(file, &path)?;
                download(manifest, file, &path, source)?;
                InstalledFile::Downloaded
            }
            Err(ModelFileError::Unsafe { path }) => return Err(InstallError::Unsafe(path)),
            Err(error) => {
                return Err(InstallError::Verification {
                    file: file.name,
                    reason: error.to_string(),
                })
            }
        };
        files.push((file.name, state));
    }
    Ok(InstallReport { model_dir, files })
}

fn lock_installs(paths: &MemorySearchPaths) -> Result<File, InstallError> {
    let models_dir = paths.models_dir();
    paths
        .create_private_dir(&models_dir)
        .map_err(|error| unsafe_or_io(paths, &models_dir, error))?;
    let path = models_dir.join(INSTALL_LOCK);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    let file = memory_search_paths::open_private_file(&path, &mut options)
        .map_err(|error| unsafe_or_io(paths, &path, error))?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(InstallError::Busy),
        Err(error) => Err(InstallError::Io(error)),
    }
}

fn download(
    manifest: &ModelManifest,
    file: &'static ModelFile,
    path: &Path,
    source: &dyn ModelFileSource,
) -> Result<(), InstallError> {
    let partial = partial_path(path);
    let mut sink = HashingFile::create(&partial)?;
    let fetched = source.fetch(
        &manifest.download_url(file),
        file.size.saturating_add(1),
        &mut sink,
    );
    let finished = fetched
        .map_err(|reason| InstallError::Download {
            file: file.name,
            reason,
        })
        .and_then(|()| sink.finish(file));
    if let Err(error) = finished.and_then(|()| Ok(fs::rename(&partial, path)?)) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    sync_parent(path)?;
    Ok(())
}

/// Removes a regular file that failed verification, so that it is not left under its final
/// name when the download replacing it fails. Anything else in its place is refused.
fn remove_unverified(file: &'static ModelFile, path: &Path) -> Result<(), InstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(fs::remove_file(path)?),
        Ok(_) => Err(InstallError::Verification {
            file: file.name,
            reason: format!("{} is not a regular file", path.display()),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// The temporary name of a download of `path`: hidden, next to it, unique in this process.
fn partial_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(
        ".{name}{PARTIAL_MARKER}{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn remove_partial_files(dir: &Path) -> Result<(), InstallError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.contains(PARTIAL_MARKER) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

struct HashingFile {
    file: File,
    hasher: Sha256,
    written: u64,
}

impl HashingFile {
    fn create(path: &Path) -> Result<Self, InstallError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let file = memory_search_paths::open_private_file(path, &mut options)?;
        Ok(Self {
            file,
            hasher: Sha256::new(),
            written: 0,
        })
    }

    fn finish(&mut self, file: &ModelFile) -> Result<(), InstallError> {
        let digest = format!("{:x}", self.hasher.clone().finalize());
        if self.written != file.size || digest != file.sha256 {
            return Err(InstallError::Verification {
                file: file.name,
                reason: format!(
                    "{} bytes with SHA-256 {digest}, expected {} bytes with {}",
                    self.written, file.size, file.sha256
                ),
            });
        }
        self.file.sync_all()?;
        Ok(())
    }
}

impl Write for HashingFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.file.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        self.written = self.written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn sync_parent(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) => File::open(parent)?.sync_all(),
        None => Ok(()),
    }
}

fn unsafe_or_io(paths: &MemorySearchPaths, path: &Path, error: io::Error) -> InstallError {
    if paths.check_no_symlinks(path).is_err() {
        InstallError::Unsafe(path.to_path_buf())
    } else {
        InstallError::Io(error)
    }
}
