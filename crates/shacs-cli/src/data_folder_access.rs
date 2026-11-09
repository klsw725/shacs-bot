//! Spec 052 PRD 030: a command that writes its data folder first checks that
//! this process can write the folder (or create it). When it cannot, the
//! command stops with who owns the folder, who runs the command and how to
//! fix it, instead of the raw `Permission denied` its first write ends with.

use super::discord_connect_cli::DiscordConnectAction;
use super::memory_cli::MemoryOptions;
use super::provider_status_cli::CommandForm;
use super::{
    ChannelsCommand, CliCommand, CliError, ConfigMigrationMode, PermissionsCommand, PluginsCommand,
    ProviderCommand, RuntimeMigrateMode,
};
use shacs_config::{config_context, default_config_path};
use shacs_core::runtime::Spec035TasksSemanticAction;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

/// The config path of a command that writes its data folder; `None` for the
/// commands this check leaves alone (scope 1).
pub(super) fn write_target(command: &CliCommand) -> Option<PathBuf> {
    let config_path = match command {
        CliCommand::Onboard(options) => &options.config_path,
        CliCommand::Run(options) => &options.config_path,
        CliCommand::Serve(options) => &options.config_path,
        CliCommand::Gateway(options) => &options.config_path,
        CliCommand::Web(options) => &options.config_path,
        CliCommand::Ask(options) => &options.config_path,
        CliCommand::AgentRepl(options) => &options.config_path,
        CliCommand::RuntimeStart(options) => &options.config_path,
        CliCommand::RuntimeStop(options) | CliCommand::RuntimeRestart(options) => {
            &options.config_path
        }
        CliCommand::RuntimeRecover(options) => &options.config_path,
        CliCommand::RuntimeUpdate(options) => &options.config_path,
        CliCommand::RuntimeMigrate(options) if options.mode != RuntimeMigrateMode::DryRun => {
            &options.config_path
        }
        CliCommand::ConfigMigrate(options) => {
            return (options.mode != ConfigMigrationMode::DryRun)
                .then(|| options.config_path.clone());
        }
        CliCommand::Provider(ProviderCommand::CodexLogin(options)) => &options.config_path,
        CliCommand::Provider(ProviderCommand::CodexImportToken(options)) => &options.config_path,
        CliCommand::Provider(ProviderCommand::CopilotImportToken(options)) => &options.config_path,
        CliCommand::Provider(ProviderCommand::ImportApiKey(options)) => &options.config_path,
        CliCommand::Channels(ChannelsCommand::Connect(options))
            if !matches!(options.action, DiscordConnectAction::Status) =>
        {
            &options.config_path
        }
        CliCommand::Plugins(PluginsCommand::Enable(options) | PluginsCommand::Disable(options)) => {
            &options.config_path
        }
        CliCommand::Permissions(PermissionsCommand::Revoke(options)) => &options.config_path,
        CliCommand::Permissions(
            PermissionsCommand::AutoModeReset(options) | PermissionsCommand::AutoModeSetup(options),
        ) => &options.config_path,
        CliCommand::Memory(
            MemoryOptions::ModelInstall { config_path, .. }
            | MemoryOptions::Index { config_path, .. },
        ) => config_path,
        CliCommand::Tasks(options)
            if options.workspace.is_none()
                && options.data_dir.is_none()
                && options.action == Some(Spec035TasksSemanticAction::RuntimeRecover) =>
        {
            &options.config_path
        }
        _ => return None,
    };
    Some(config_path.clone().unwrap_or_else(default_config_path))
}

/// Stops the command when this process cannot write the data folder of
/// `config_path` (or create it).
pub(super) fn check(config_path: &Path) -> Result<(), CliError> {
    let running = Ids::effective();
    let groups = supplementary_groups();
    let Some(mut denied) = probe(
        &data_folder(config_path),
        &data_folder(&default_config_path()),
        running,
        groups.as_deref().unwrap_or_default(),
        &effective_access,
    ) else {
        return Ok(());
    };
    if groups.is_none() {
        // Without the group list the mode class is a guess; give no command
        // that changes the owner.
        denied.default_folder = false;
    }
    Err(CliError::DataFolder(render(
        &denied,
        running,
        CommandForm::observed(),
    )))
}

/// The data folder the runtime uses for `config_path`, absolute.
fn data_folder(config_path: &Path) -> PathBuf {
    absolute(&config_context(Some(config_path.to_path_buf()), None).data_dir)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Ids {
    pub(super) uid: u32,
    pub(super) gid: u32,
}

impl Ids {
    fn effective() -> Self {
        // SAFETY: `geteuid` and `getegid` only read this process's ids.
        unsafe {
            Self {
                uid: libc::geteuid(),
                gid: libc::getegid(),
            }
        }
    }
}

impl fmt::Display for Ids {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.uid, self.gid)
    }
}

/// Why the data folder cannot be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Denied {
    /// The data folder, absolute.
    pub(super) data_folder: PathBuf,
    /// The folder asked: the data folder, or when it does not exist, the
    /// first existing folder above it.
    pub(super) asked: PathBuf,
    pub(super) exists: bool,
    /// Where the data folder points when it is a link.
    pub(super) link_target: Option<PathBuf>,
    pub(super) owner: Ids,
    pub(super) mode: u32,
    /// The mode bits that apply to this process already allow writing, so
    /// something else (an ACL, a security policy) denies it.
    pub(super) mode_allows: bool,
    /// The dedicated default data folder (`~/.shacs-bot`), the only one
    /// whose owner the advice may tell to change.
    pub(super) default_folder: bool,
}

/// Scope 3: `None` when the folder can be written or the answer is not a
/// plain permission denial (the command then runs and fails as before).
pub(super) fn probe(
    data_folder: &Path,
    default_folder: &Path,
    running: Ids,
    groups: &[u32],
    access: &dyn Fn(&Path) -> io::Result<()>,
) -> Option<Denied> {
    let data_folder = absolute(data_folder);
    let (asked, exists, link_target) = match fs::symlink_metadata(&data_folder) {
        Ok(entry) if entry.is_dir() => (data_folder.clone(), true, None),
        Ok(entry) if entry.file_type().is_symlink() => {
            if !fs::metadata(&data_folder).is_ok_and(|target| target.is_dir()) {
                return None;
            }
            let target = fs::canonicalize(&data_folder).ok()?;
            (data_folder.clone(), true, Some(target))
        }
        Ok(_) => return None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            (first_existing_folder(&data_folder)?, false, None)
        }
        Err(_) => return None,
    };
    let error = access(&asked).err()?;
    if error.raw_os_error() != Some(libc::EACCES) {
        return None;
    }
    let metadata = fs::metadata(&asked).ok()?;
    let owner = Ids {
        uid: metadata.uid(),
        gid: metadata.gid(),
    };
    let mode = metadata.mode() & 0o7777;
    let default_folder = link_target.is_none() && data_folder == absolute(default_folder);
    Some(Denied {
        data_folder,
        asked,
        exists,
        link_target,
        owner,
        mode,
        mode_allows: mode_allows(owner, mode, running, groups),
        default_folder,
    })
}

/// The folder a missing data folder would be made in: the first existing
/// entry above it, when that resolves to a folder.
fn first_existing_folder(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors().skip(1) {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                return fs::metadata(ancestor)
                    .is_ok_and(|entry| entry.is_dir())
                    .then(|| ancestor.to_path_buf());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    None
}

/// Whether the owner, group or other bits that apply to `running` grant
/// write and search.
fn mode_allows(owner: Ids, mode: u32, running: Ids, groups: &[u32]) -> bool {
    let class = if running.uid == owner.uid {
        mode >> 6
    } else if running.gid == owner.gid || groups.contains(&owner.gid) {
        mode >> 3
    } else {
        mode
    };
    class & 0o3 == 0o3
}

fn absolute(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    }
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn effective_access(folder: &Path) -> io::Result<()> {
    let path = CString::new(folder.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `path` is a NUL-terminated string that outlives the call.
    let result = unsafe {
        libc::faccessat(
            libc::AT_FDCWD,
            path.as_ptr(),
            libc::W_OK | libc::X_OK,
            libc::AT_EACCESS,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// This process's supplementary groups; `None` when they cannot be read.
fn supplementary_groups() -> Option<Vec<libc::gid_t>> {
    read_groups(
        || {
            // SAFETY: with a size of 0, `getgroups` only returns the number
            // of groups.
            let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
            usize::try_from(count).map_err(|_| io::Error::last_os_error())
        },
        |groups| {
            let size = libc::c_int::try_from(groups.len())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            // SAFETY: `groups` has room for `size` ids.
            let written = unsafe { libc::getgroups(size, groups.as_mut_ptr()) };
            usize::try_from(written).map_err(|_| io::Error::last_os_error())
        },
    )
}

/// Reads the group list with `count` and then `fill`. `fill` always gets at
/// least one slot (a size of 0 would only count again), and the list is
/// read again when it grew in between: `EINVAL`, or more groups than slots.
fn read_groups(
    mut count: impl FnMut() -> io::Result<usize>,
    mut fill: impl FnMut(&mut [libc::gid_t]) -> io::Result<usize>,
) -> Option<Vec<libc::gid_t>> {
    for _ in 0..3 {
        let mut groups = vec![0; count().ok()? + 1];
        match fill(&mut groups) {
            Ok(written) if written <= groups.len() => {
                groups.truncate(written);
                return Some(groups);
            }
            Ok(_) => {}
            Err(error) if error.raw_os_error() == Some(libc::EINVAL) => {}
            Err(_) => return None,
        }
    }
    None
}

/// Scope 4: the error text, one line each for what is wrong, why it may be
/// so, and the fix.
pub(super) fn render(denied: &Denied, running: Ids, form: CommandForm) -> String {
    let mut lines = vec![first_line(denied, running)];
    if denied.mode_allows {
        lines.push(
            "Its mode lets this user write, so an ACL or a security policy such as SELinux \
             blocks it."
                .to_owned(),
        );
    } else if denied.exists && denied.owner.uid == 0 {
        lines.push(
            "A folder owned by root is usually one Docker made because it did not exist when \
             a container mounted it."
                .to_owned(),
        );
    }
    lines.push(format!("fix: {}", fix(denied, running, form)));
    lines.join("\n")
}

fn first_line(denied: &Denied, running: Ids) -> String {
    let data_folder = denied.data_folder.display();
    let owner = denied.owner;
    let mode = denied.mode;
    if denied.exists {
        let link = denied
            .link_target
            .as_ref()
            .map(|target| format!(", a link to {}", target.display()))
            .unwrap_or_default();
        format!(
            "cannot write to the data folder {data_folder}{link}: it belongs to {owner} (mode \
             {mode:04o}) and this command runs as {running}"
        )
    } else {
        format!(
            "cannot create the data folder {data_folder}: {} belongs to {owner} (mode {mode:04o}) \
             and this command runs as {running}",
            denied.asked.display()
        )
    }
}

fn fix(denied: &Denied, running: Ids, form: CommandForm) -> String {
    let container = form == CommandForm::OfficialContainer;
    if denied.mode_allows {
        return if container {
            format!(
                "on the host, check the ACL and security labels of the folder mounted at {}; \
                 with SELinux, add :z to that volume in docker-compose.yml",
                denied.data_folder.display()
            )
        } else {
            "check the folder's ACL (ls -le on macOS, getfacl on Linux) and its security labels"
                .to_owned()
        };
    }
    if !denied.default_folder {
        return other_folder_fix(denied, running, container);
    }
    let path = shell_quote(&denied.data_folder);
    let owner = denied.owner;
    match (container, denied.exists) {
        (false, true) if owner.uid == running.uid => format!("chmod u+rwx {path}"),
        (false, true) => format!("sudo chown -R {running} {path} && chmod u+rwx {path}"),
        (false, false) => {
            format!("sudo mkdir -p {path} && sudo chown {running} {path} && chmod u+rwx {path}")
        }
        (true, true) if owner.uid == running.uid => {
            "on the host, run chmod u+rwx ~/.shacs-bot".to_owned()
        }
        (true, true) if owner.uid == 0 => {
            "on the host, run sudo chown -R \"$(id -u):$(id -g)\" ~/.shacs-bot && chmod u+rwx \
             ~/.shacs-bot, then ./scripts/docker-setup.sh in the shacs-bot repository; it sets \
             SHACS_UID and SHACS_GID in the repository .env so the containers run as you"
                .to_owned()
        }
        (true, true) => format!(
            "on the host, run ./scripts/docker-setup.sh in the shacs-bot repository as the user \
             who owns ~/.shacs-bot ({owner}), or set SHACS_UID={} and SHACS_GID={} in the \
             repository .env; then run this again",
            owner.uid, owner.gid
        ),
        (true, false) => "on the host, run ./scripts/docker-setup.sh in the shacs-bot \
                          repository; it creates ~/.shacs-bot as your user and sets SHACS_UID \
                          and SHACS_GID in the repository .env"
            .to_owned(),
    }
}

/// Advice for a folder that may not be shacs-bot's alone (a `--config`
/// folder or a link): no command that changes its owner.
fn other_folder_fix(denied: &Denied, running: Ids, container: bool) -> String {
    let data_folder = denied.data_folder.display();
    match (container, denied.exists) {
        (false, true) => format!(
            "make {} writable for {running}, or use --config with a path in a folder you own \
             (the default data folder is ~/.shacs-bot)",
            denied
                .link_target
                .as_deref()
                .unwrap_or(&denied.data_folder)
                .display()
        ),
        (false, false) => format!(
            "create {data_folder} for {running}, or use --config with a path in a folder you \
             own (the default data folder is ~/.shacs-bot)"
        ),
        (true, true) => format!(
            "on the host, make the folder mounted at {data_folder} writable for {running}, or \
             run the containers as its owner with SHACS_UID and SHACS_GID in the repository .env"
        ),
        (true, false) => "use --config with a path in a folder this container can write, such \
                          as the mounted data folder"
            .to_owned(),
    }
}

/// `path` as one shell word: as it is when it has only letters, digits and
/// `_-./:@%+=,`, otherwise in single quotes.
fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-./:@%+=,".contains(character));
    if plain {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
#[path = "data_folder_access_tests.rs"]
mod tests;
