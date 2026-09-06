use super::{digest_bytes, spawn, PathChainSeal, Spec034ReleaseArtifactError};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

#[path = "linker/protocol.rs"]
mod protocol;
pub(super) use protocol::LinkerReceipts;
use protocol::{MODE_ENV, MODE_VALUE, NONCE_ENV, SOCKET_ENV};

pub(super) const MAX_RECEIPT_BYTES: usize = 4096;
const FIXED_LINKER: &str = "/usr/bin/clang";
const ACK: &[u8] = b"ACK\n";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LinkerReceipt {
    nonce: String,
    path: String,
    device: u64,
    inode: u64,
    size: u64,
    digest: String,
    cdhash: Vec<u8>,
}

pub(super) fn fixed_linker() -> Result<PathBuf, Spec034ReleaseArtifactError> {
    Path::new(FIXED_LINKER)
        .canonicalize()
        .map_err(Spec034ReleaseArtifactError::Io)
}

pub(super) fn prepare_wrapper(
    tools: &Path,
    image: Option<&Path>,
) -> Result<(PathBuf, PathChainSeal, spawn::ProcessIdentity), Spec034ReleaseArtifactError> {
    let current = match image {
        Some(path) => spawn::capture_static_identity(path)?,
        None => {
            let current = spawn::capture_process_identity(
                i32::try_from(std::process::id())
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?,
            )?;
            if !spawn::static_identity_matches(&current)? {
                return Err(Spec034ReleaseArtifactError::DigestMismatch);
            }
            current
        }
    };
    let target = tools
        .parent()
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?
        .join("spec034-linker-wrapper");
    copy_verified_process_image(&current, &target)?;
    let copied = capture_copied_identity(&target, &current)?;
    Ok((target.clone(), PathChainSeal::capture_digest_leaf(&target)?, copied))
}

fn copy_verified_process_image(
    source: &spawn::ProcessIdentity,
    target: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let mut input = File::open(&source.executable).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = input.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    if metadata.dev() != source.device || metadata.ino() != source.inode {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    let mut output = OpenOptions::new().write(true).create_new(true).open(target)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    std::io::copy(&mut input, &mut output).map_err(Spec034ReleaseArtifactError::Io)?;
    output.sync_all().map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::set_permissions(target, metadata.permissions()).map_err(Spec034ReleaseArtifactError::Io)
}

fn capture_copied_identity(
    target: &Path,
    source: &spawn::ProcessIdentity,
) -> Result<spawn::ProcessIdentity, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let mut file = File::open(target).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(Spec034ReleaseArtifactError::Io)?;
    let cdhash = spawn::static_cdhash(target)?;
    if digest_bytes(&bytes) != source.digest || cdhash != source.cdhash {
        return Err(Spec034ReleaseArtifactError::DigestMismatch);
    }
    Ok(spawn::ProcessIdentity {
        pid: 0,
        parent_pid: 0,
        start_seconds: 0,
        start_microseconds: 0,
        executable: target.to_path_buf(),
        device: metadata.dev(),
        inode: metadata.ino(),
        digest: source.digest.clone(),
        cdhash,
    })
}

#[cfg(test)]
pub(super) fn copy_verified_self_image(
    source: &Path,
    target: &Path,
) -> Result<(), Spec034ReleaseArtifactError> {
    let mut input = File::open(source).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = input.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(target)
        .map_err(Spec034ReleaseArtifactError::Io)?;
    std::io::copy(&mut input, &mut output).map_err(Spec034ReleaseArtifactError::Io)?;
    std::fs::set_permissions(target, metadata.permissions()).map_err(Spec034ReleaseArtifactError::Io)
}

pub fn run_wrapper() -> Result<(), Spec034ReleaseArtifactError> {
    if std::env::var(MODE_ENV).ok().as_deref() != Some(MODE_VALUE) {
        return Err(Spec034ReleaseArtifactError::InvalidConfig);
    }
    let target = PathBuf::from(std::env::var_os("SHACS_SPEC034_TARGET")
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?)
        .canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let socket = PathBuf::from(std::env::var_os(SOCKET_ENV)
        .ok_or(Spec034ReleaseArtifactError::InvalidConfig)?);
    let nonce = std::env::var(NONCE_ENV).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let output = args.windows(2).find(|pair| pair[0] == "-o")
        .map(|pair| PathBuf::from(&pair[1])).ok_or(Spec034ReleaseArtifactError::InvalidConfig)?;
    let status = std::process::Command::new(fixed_linker()?).args(&args).status()
        .map_err(Spec034ReleaseArtifactError::Io)?;
    if !status.success() {
        return Err(Spec034ReleaseArtifactError::CommandFailed);
    }
    let receipt = output_receipt(&target, &output, nonce)?;
    let mut stream = UnixStream::connect(socket).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut encoded = serde_json::to_vec(&receipt).map_err(Spec034ReleaseArtifactError::Json)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_RECEIPT_BYTES {
        return Err(Spec034ReleaseArtifactError::InvalidEvidence);
    }
    stream.write_all(&encoded).map_err(Spec034ReleaseArtifactError::Io)?;
    let mut ack = [0_u8; 4];
    stream.read_exact(&mut ack).map_err(Spec034ReleaseArtifactError::Io)?;
    (ack == ACK).then_some(()).ok_or(Spec034ReleaseArtifactError::InvalidEvidence)
}

fn output_receipt(
    target: &Path,
    output: &Path,
    nonce: String,
) -> Result<LinkerReceipt, Spec034ReleaseArtifactError> {
    use std::os::unix::fs::MetadataExt;
    let output = output.canonicalize().map_err(Spec034ReleaseArtifactError::Io)?;
    let relative = output.strip_prefix(target).map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
    let mut file = File::open(&output).map_err(Spec034ReleaseArtifactError::Io)?;
    let metadata = file.metadata().map_err(Spec034ReleaseArtifactError::Io)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(Spec034ReleaseArtifactError::Io)?;
    Ok(LinkerReceipt {
        nonce,
        path: relative.to_str().ok_or(Spec034ReleaseArtifactError::InvalidEvidence)?.to_owned(),
        device: metadata.dev(), inode: metadata.ino(), size: metadata.size(),
        digest: digest_bytes(&bytes), cdhash: spawn::static_cdhash(&output)?,
    })
}

#[cfg(test)]
#[path = "linker_test.rs"]
mod tests;
