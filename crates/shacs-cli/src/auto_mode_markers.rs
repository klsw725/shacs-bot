//! The data folder's auto mode marker files (Spec 048 PRD 050 scopes 3 and
//! 8). Auto mode refuses the assistant's writes to them; only the runtime
//! and the user's own commands change them.

use shacs_command::SetupHintSwitch;
use shacs_core::runtime::{auto_mode_marker_path, DEFAULT_NOTICE_MARKER, SETUP_HINT_OFF_MARKER};
use std::path::Path;
use std::{fs, io};

/// Creates the first notice's marker. Only the run that creates it sends
/// the notice, so runs that start together or after a restart do not.
pub(crate) fn claim_default_notice(data_folder: &Path) -> io::Result<bool> {
    let path = auto_mode_marker_path(data_folder, DEFAULT_NOTICE_MARKER);
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

/// A marker that cannot be checked counts as there: the hint is a nudge,
/// not a safety notice.
pub(crate) fn setup_hint_off(data_folder: &Path) -> bool {
    !matches!(
        fs::symlink_metadata(auto_mode_marker_path(data_folder, SETUP_HINT_OFF_MARKER)),
        Err(error) if error.kind() == io::ErrorKind::NotFound
    )
}

pub(crate) fn set_setup_hint(data_folder: &Path, switch: SetupHintSwitch) -> io::Result<()> {
    let path = auto_mode_marker_path(data_folder, SETUP_HINT_OFF_MARKER);
    match switch {
        SetupHintSwitch::Off => {
            if let Some(folder) = path.parent() {
                fs::create_dir_all(folder)?;
            }
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map(drop)
        }
        SetupHintSwitch::On => match fs::remove_file(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        },
    }
}
