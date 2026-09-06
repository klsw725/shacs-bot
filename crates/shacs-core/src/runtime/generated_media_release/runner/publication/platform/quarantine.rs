use super::*;
use rustix::fs::fsync;
use std::io::Read;

const MAX_QUARANTINE_ATTEMPTS: usize = 8;

pub(super) fn quarantine_visible(
    parent: &File,
    leaf: &OsStr,
) -> Result<(), Spec034ReleaseArtifactError> {
    quarantine_visible_with(parent, leaf, random_rejected_name)
}

pub(super) fn quarantine_visible_with(
    parent: &File,
    leaf: &OsStr,
    mut next_name: impl FnMut() -> Result<OsString, Spec034ReleaseArtifactError>,
) -> Result<(), Spec034ReleaseArtifactError> {
    for _ in 0..MAX_QUARANTINE_ATTEMPTS {
        let rejected = next_name()?;
        match renameat_with(
            parent,
            leaf,
            parent,
            &rejected,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => {
                fsync(parent).map_err(|_| quarantine_failure())?;
                return Ok(());
            }
            Err(error) if error == rustix::io::Errno::EXIST => {}
            Err(_) => return Err(quarantine_failure()),
        }
    }
    Err(quarantine_failure())
}

fn random_rejected_name() -> Result<OsString, Spec034ReleaseArtifactError> {
    let mut random = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(Spec034ReleaseArtifactError::Io)?;
    let mut name = String::from(".spec034-rejected-");
    for byte in random {
        use std::fmt::Write;
        write!(&mut name, "{byte:02x}").map_err(|_| quarantine_failure())?;
    }
    Ok(OsString::from(name))
}

fn quarantine_failure() -> Spec034ReleaseArtifactError {
    Spec034ReleaseArtifactError::CommitStatusUnknown(PublicationStage::QuarantineFailure)
}
