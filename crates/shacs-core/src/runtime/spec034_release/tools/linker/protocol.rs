use super::{LinkerReceipt, MAX_RECEIPT_BYTES};
use crate::runtime::spec034_release::tools::{
    digest_bytes,
    monitor::{ExecutionLedger, ExecutionQueue},
    spawn, Spec034ReleaseArtifactError,
};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

pub(super) const SOCKET_ENV: &str = "SHACS_SPEC034_RECEIPT_SOCKET";
pub(super) const NONCE_ENV: &str = "SHACS_SPEC034_RECEIPT_NONCE";
pub(super) const MODE_ENV: &str = "SHACS_SPEC034_LINKER_MODE";
pub(super) const MODE_VALUE: &str = "self-image-v1";
const ACK: &[u8] = b"ACK\n";

struct VerifiedReceipt {
    receipt: LinkerReceipt,
    monitor: ExecutionLedger,
}

pub(in crate::runtime::spec034_release::tools) struct LinkerReceipts {
    listener: UnixListener,
    socket_path: PathBuf,
    nonce: String,
    target: PathBuf,
    wrapper: spawn::ProcessIdentity,
    compiler: spawn::ProcessIdentity,
    queue: ExecutionQueue,
    state: Mutex<BTreeMap<String, VerifiedReceipt>>,
}

impl LinkerReceipts {
    pub(in crate::runtime::spec034_release::tools) fn prepare(
        target: &Path,
        wrapper: spawn::ProcessIdentity,
        compiler: spawn::ProcessIdentity,
    ) -> Result<Self, Spec034ReleaseArtifactError> {
        #[cfg(not(target_vendor = "apple"))]
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
        #[cfg(target_vendor = "apple")]
        {
            let target = target.canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
            let socket_path = target.join(format!(".spec034-linker-{}.sock", std::process::id()));
            let listener = UnixListener::bind(&socket_path).map_err(Spec034ReleaseArtifactError::Io)?;
            listener.set_nonblocking(true).map_err(Spec034ReleaseArtifactError::Io)?;
            let nonce = digest_bytes(
                format!("{}:{}", socket_path.display(), wrapper.digest).as_bytes(),
            );
            let queue = ExecutionLedger::new_queue()?;
            Ok(Self {
                listener,
                socket_path,
                nonce,
                target,
                wrapper,
                compiler,
                queue,
                state: Mutex::new(BTreeMap::new()),
            })
        }
    }

    pub(in crate::runtime::spec034_release::tools) fn configure(&self, command: &mut std::process::Command) {
        command
            .env(MODE_ENV, MODE_VALUE)
            .env(SOCKET_ENV, &self.socket_path)
            .env(NONCE_ENV, &self.nonce);
    }

    pub(super) fn drain(&self) -> Result<(), Spec034ReleaseArtifactError> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => self.accept_receipt(stream)?,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(Spec034ReleaseArtifactError::Io(error)),
            }
        }
    }

    fn accept_receipt(&self, mut stream: UnixStream) -> Result<(), Spec034ReleaseArtifactError> {
        let peer_pid = peer_pid(&stream)?;
        let peer = spawn::capture_process_identity(peer_pid)?;
        if !self.wrapper.same_executable(&peer) {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let parent = spawn::capture_process_identity(peer.parent_pid)?;
        if !self.compiler.same_executable(&parent) {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .map_err(Spec034ReleaseArtifactError::Io)?;
        let mut encoded = Vec::new();
        BufReader::new(&stream)
            .take(u64::try_from(MAX_RECEIPT_BYTES + 1).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?)
            .read_until(b'\n', &mut encoded)
            .map_err(Spec034ReleaseArtifactError::Io)?;
        if encoded.last() != Some(&b'\n') || encoded.len() > MAX_RECEIPT_BYTES {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        let receipt: LinkerReceipt = serde_json::from_slice(&encoded[..encoded.len() - 1])
            .map_err(Spec034ReleaseArtifactError::Json)?;
        if receipt.nonce != self.nonce {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        let (receipt, output) = self.verify_receipt(receipt)?;
        let retained = self
            .state
            .lock()
            .map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?
            .len();
        ExecutionLedger::ensure_capacity(retained.saturating_add(1))?;
        let monitor = ExecutionLedger::arm_on(&[output], self.queue.clone())?;
        monitor.verify()?;
        let mut state = self.state.lock().map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
        if state
            .insert(receipt.path.clone(), VerifiedReceipt { receipt, monitor })
            .is_some()
        {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        stream.write_all(ACK).map_err(Spec034ReleaseArtifactError::Io)
    }

    fn verify_receipt(
        &self,
        receipt: LinkerReceipt,
    ) -> Result<(LinkerReceipt, PathBuf), Spec034ReleaseArtifactError> {
        use std::io::Read;
        use std::os::unix::fs::MetadataExt;
        let relative = Path::new(&receipt.path);
        if relative.is_absolute()
            || relative.components().any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        let original = self.target.join(relative);
        let (path, mut file, preserved_inode) = open_output(&original, relative)?;
        let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
        if metadata.size() != receipt.size
            || (preserved_inode && (metadata.dev() != receipt.device || metadata.ino() != receipt.inode))
        {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(Spec034ReleaseArtifactError::Io)?;
        if digest_bytes(&bytes) != receipt.digest || spawn::static_cdhash(&path)? != receipt.cdhash {
            return Err(Spec034ReleaseArtifactError::DigestMismatch);
        }
        Ok((receipt, path))
    }

    pub(in crate::runtime::spec034_release::tools) fn verify(&self) -> Result<(), Spec034ReleaseArtifactError> {
        self.drain()?;
        let state = self.state.lock().map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
        state.values().try_for_each(|receipt| receipt.monitor.verify())
    }

    pub(in crate::runtime::spec034_release::tools) fn verify_identity(
        &self,
        identity: &spawn::ProcessIdentity,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        self.drain()?;
        if !identity.executable.starts_with(&self.target) {
            return Ok(());
        }
        let state = self.state.lock().map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
        let receipt = state
            .values()
            .find(|receipt| receipt.receipt.digest == identity.digest && receipt.receipt.cdhash == identity.cdhash)
            .ok_or(Spec034ReleaseArtifactError::DigestMismatch)?;
        receipt.monitor.verify()
    }

    pub(in crate::runtime::spec034_release::tools) fn attestation_digest(&self) -> Result<String, Spec034ReleaseArtifactError> {
        self.verify()?;
        let state = self.state.lock().map_err(|_| Spec034ReleaseArtifactError::InvalidEvidence)?;
        #[cfg(not(test))]
        if state.is_empty() {
            return Err(Spec034ReleaseArtifactError::InvalidEvidence);
        }
        let receipts = state.values().map(|value| &value.receipt).collect::<Vec<_>>();
        Ok(digest_bytes(&serde_json::to_vec(&receipts).map_err(Spec034ReleaseArtifactError::Json)?))
    }
}

impl Drop for LinkerReceipts {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn open_output(
    original: &Path,
    relative: &Path,
) -> Result<(PathBuf, std::fs::File, bool), Spec034ReleaseArtifactError> {
    match std::fs::File::open(original) {
        Ok(file) => Ok((original.to_path_buf(), file, true)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let name = relative.file_name().and_then(|name| name.to_str())
                .ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?;
            if !name.starts_with("build_script_build-") {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
            let finalized = original.parent().ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?
                .join("build-script-build");
            let file = std::fs::File::open(&finalized)
                .map_err(|_| Spec034ReleaseArtifactError::DigestMismatch)?;
            Ok((finalized, file, false))
        }
        Err(error) => Err(Spec034ReleaseArtifactError::Io(error)),
    }
}

#[cfg(target_vendor = "apple")]
fn peer_pid(stream: &UnixStream) -> Result<libc::pid_t, Spec034ReleaseArtifactError> {
    let mut pid = 0;
    let mut length = libc::socklen_t::try_from(std::mem::size_of::<libc::pid_t>())
        .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    // SAFETY: [Category 8 - FFI boundary] `pid` and `length` are writable and the stream
    // owns a connected AF_UNIX descriptor; LOCAL_PEERPID writes one pid_t on success.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(), &mut length,
        )
    };
    if status != 0 || length as usize != std::mem::size_of::<libc::pid_t>() {
        return Err(Spec034ReleaseArtifactError::Io(std::io::Error::last_os_error()));
    }
    Ok(pid)
}

#[cfg(not(target_vendor = "apple"))]
fn peer_pid(_stream: &UnixStream) -> Result<libc::pid_t, Spec034ReleaseArtifactError> {
    Err(Spec034ReleaseArtifactError::InvalidConfig)
}

#[cfg(test)]
#[path = "protocol_test.rs"]
mod tests;
