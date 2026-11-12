//! The lock files under `<data folder>/browser/` (Spec 050 PRD 020 scopes 3
//! and 5). Every browser generation holds `active.lock` shared. One that
//! keeps its login profile also holds `profile.lock` exclusively. `browser
//! forget` holds `forget.lock` exclusively and then waits for `active.lock`;
//! a generation takes `active.lock` first and only then looks at
//! `forget.lock`. So no browser starts while forget deletes, and forget never
//! deletes past a browser that is starting.
use super::{
    browser_profile_directory, BrowserClock, BrowserLoginProfile, BrowserLoginStatus,
    BrowserProcessRunner, BrowserProfileState,
};
use fs2::FileExt;
use shacs_core::runtime::BrowserError;
use std::fs::{self, DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const FOLDER: &str = "browser";
const ACTIVE_LOCK: &str = "active.lock";
const FORGET_LOCK: &str = "forget.lock";
const PROFILE_LOCK: &str = "profile.lock";
const PROFILE: &str = "profile";
const SINGLETON_LOCK: &str = "SingletonLock";
/// How many profile files a size is added up over.
const PROFILE_FILE_LIMIT: usize = 20_000;
/// How long between two tries at a lock someone else holds.
const RETRY_MS: u64 = 100;
/// How long a start keeps trying: inspect and the forget survey hold a lock
/// only for a moment.
const START_WAIT_MS: u64 = 500;

/// Which file a path leads to, whatever name it has now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub device: u64,
    pub inode: u64,
}

impl FileIdentity {
    pub(super) fn of(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
}

/// `<data folder>/browser`: the lock files and the login profile.
#[derive(Debug, Clone)]
pub struct BrowserFolder {
    root: PathBuf,
}

impl BrowserFolder {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join(FOLDER),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn active_lock(&self) -> PathBuf {
        self.root.join(ACTIVE_LOCK)
    }

    pub fn forget_lock(&self) -> PathBuf {
        self.root.join(FORGET_LOCK)
    }

    pub fn profile_lock(&self) -> PathBuf {
        self.root.join(PROFILE_LOCK)
    }

    pub fn profile(&self) -> PathBuf {
        self.root.join(PROFILE)
    }
}

/// Which profile a generation runs on (scope 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileUse {
    /// The 049 profile inside the generation's temporary folder.
    Temporary,
    /// `<data folder>/browser/profile`, kept between generations.
    Saved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Missing {
    Create,
    Leave,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LockKind {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathKind {
    Folder,
    File,
}

/// Opens one lock file without following a symlink at its name.
pub(super) fn open_lock(path: &Path, missing: Missing) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    if missing == Missing::Create {
        options.write(true).create(true).truncate(false);
    }
    let file = options.open(path)?;
    if file.metadata()?.file_type().is_file() {
        Ok(file)
    } else {
        Err(io::Error::other("browser lock is not a file"))
    }
}

/// Whether the lock was taken; `false` when someone else holds it.
pub(super) fn try_lock(file: &File, kind: LockKind) -> io::Result<bool> {
    let taken = match kind {
        LockKind::Shared => FileExt::try_lock_shared(file),
        LockKind::Exclusive => FileExt::try_lock_exclusive(file),
    };
    match taken {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(error) => Err(error),
    }
}

/// Tries every 100 ms for up to `limit_ms` of `clock`.
pub(super) fn lock_within(
    clock: &dyn BrowserClock,
    file: &File,
    kind: LockKind,
    limit_ms: u64,
) -> io::Result<bool> {
    let started = clock.now_ms();
    loop {
        if try_lock(file, kind)? {
            return Ok(true);
        }
        if clock.now_ms().saturating_sub(started) >= limit_ms {
            return Ok(false);
        }
        clock.pause(RETRY_MS);
    }
}

/// The file at `path` itself, never the one a symlink there leads to.
pub(super) fn identity_at(path: &Path, kind: PathKind) -> Option<FileIdentity> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let expected = match kind {
        PathKind::Folder => metadata.file_type().is_dir(),
        PathKind::File => metadata.file_type().is_file(),
    };
    expected.then(|| FileIdentity::of(&metadata))
}

fn own_user() -> u32 {
    // SAFETY: `geteuid` only reads this process's user id.
    unsafe { libc::geteuid() }
}

/// Creates a folder only this user can enter, when nothing is at `path`.
pub(super) fn create_private_folder(path: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

/// Start check 1: `browser/` is a real folder of this user with mode 0700.
fn prepare_folder(folder: &BrowserFolder) -> io::Result<FileIdentity> {
    if let Some(parent) = folder.root().parent() {
        fs::create_dir_all(parent)?;
    }
    create_private_folder(folder.root())?;
    let metadata = fs::symlink_metadata(folder.root())?;
    if !metadata.file_type().is_dir() || metadata.uid() != own_user() {
        return Err(io::Error::other("browser folder is not this user's folder"));
    }
    if metadata.mode() & 0o777 != 0o700 {
        fs::set_permissions(folder.root(), fs::Permissions::from_mode(0o700))?;
    }
    Ok(FileIdentity::of(&metadata))
}

/// Start check 4: the profile is a real folder this user reads and writes.
/// A missing one is made; an existing one keeps its mode.
fn prepare_profile(profile: &Path) -> io::Result<()> {
    if matches!(fs::symlink_metadata(profile), Err(error) if error.kind() == io::ErrorKind::NotFound)
    {
        create_private_folder(profile)?;
    }
    if profile_usable(profile) {
        Ok(())
    } else {
        Err(io::Error::other("login profile is not usable"))
    }
}

fn profile_usable(profile: &Path) -> bool {
    fs::symlink_metadata(profile).is_ok_and(|metadata| {
        metadata.file_type().is_dir()
            && metadata.uid() == own_user()
            && metadata.mode() & 0o700 == 0o700
    })
}

/// What Chromium's own `SingletonLock` in a profile says (scope 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingletonLock {
    Absent,
    /// Left by a Chromium of this container that no longer runs. Chromium
    /// clears it when it starts.
    Startable,
    /// A Chromium of this container runs on the profile.
    Live,
    /// Another host's lock, or one that cannot be read as a lock: nothing
    /// here can tell whether that browser still runs.
    Elsewhere,
}

/// Reads `SingletonLock`, a symlink named `<hostname>-<pid>`.
pub fn singleton_lock(profile: &Path, runner: &dyn BrowserProcessRunner) -> SingletonLock {
    let path = profile.join(SINGLETON_LOCK);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return SingletonLock::Absent,
        Err(_) => return SingletonLock::Elsewhere,
    };
    if !metadata.file_type().is_symlink() {
        return SingletonLock::Elsewhere;
    }
    let Ok(target) = fs::read_link(&path) else {
        return SingletonLock::Elsewhere;
    };
    let Some(target) = target.to_str() else {
        return SingletonLock::Elsewhere;
    };
    singleton_target(target, runner)
}

pub(super) fn singleton_open(directory: &File, runner: &dyn BrowserProcessRunner) -> SingletonLock {
    match browser_profile_directory::singleton(directory) {
        Ok(target) => singleton_target(&target, runner),
        Err(error) if error.kind() == io::ErrorKind::NotFound => SingletonLock::Absent,
        Err(_) => SingletonLock::Elsewhere,
    }
}

fn singleton_target(target: &str, runner: &dyn BrowserProcessRunner) -> SingletonLock {
    let Some((host, written)) = target.rsplit_once('-') else {
        return SingletonLock::Elsewhere;
    };
    // `+7` and `007` parse as numbers but are not how Chromium writes a pid.
    let pid = match written.parse::<u32>() {
        Ok(pid) if pid > 0 && pid.to_string() == written => pid,
        _ => return SingletonLock::Elsewhere,
    };
    if host.is_empty() || runner.hostname().as_deref() != Some(host) {
        return SingletonLock::Elsewhere;
    }
    if runner.chromium_running(pid) {
        SingletonLock::Live
    } else {
        SingletonLock::Startable
    }
}

/// The lock files of one generation as they were when it took them.
#[derive(Debug, Clone)]
pub struct WatchedLocks {
    root: (PathBuf, FileIdentity),
    active: (PathBuf, FileIdentity),
    forget: (PathBuf, FileIdentity),
    profile_lock: Option<(PathBuf, FileIdentity)>,
}

/// What the one-second look at the lock files found (scopes 3 and 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockSignal {
    Keep,
    /// `browser forget` holds `forget.lock`.
    Forget,
    /// A lock path no longer leads to the file this generation holds, so
    /// its locks protect nothing any more.
    Replaced,
}

impl WatchedLocks {
    pub fn check(&self) -> LockSignal {
        let same = identity_at(&self.root.0, PathKind::Folder) == Some(self.root.1)
            && [
                Some(&self.active),
                Some(&self.forget),
                self.profile_lock.as_ref(),
            ]
            .into_iter()
            .flatten()
            .all(|(path, identity)| identity_at(path, PathKind::File) == Some(*identity));
        if !same {
            return LockSignal::Replaced;
        }
        let Ok(file) = open_lock(&self.forget.0, Missing::Leave) else {
            return LockSignal::Replaced;
        };
        if !file
            .metadata()
            .is_ok_and(|metadata| FileIdentity::of(&metadata) == self.forget.1)
        {
            return LockSignal::Replaced;
        }
        match try_lock(&file, LockKind::Shared) {
            Ok(true) => LockSignal::Keep,
            Ok(false) => LockSignal::Forget,
            Err(_) => LockSignal::Replaced,
        }
    }
}

/// The locks of one generation. Dropping it releases them, so it is dropped
/// only after the generation's processes and temporary folder are gone.
pub struct BrowserLockHold {
    watched: WatchedLocks,
    profile: Option<PathBuf>,
    // Released in this order: the profile before the sign that a browser runs.
    _profile_lock: Option<File>,
    _profile_directory: Option<File>,
    _active: File,
    // Every generation keeps this stable inode live even if active.lock is
    // replaced. It is acquired last, before launch, and released last.
    _browser_directory: File,
}

impl BrowserLockHold {
    /// The saved profile this generation runs on, if it keeps its login.
    pub fn profile(&self) -> Option<&Path> {
        self.profile.as_deref()
    }

    pub fn watched(&self) -> WatchedLocks {
        self.watched.clone()
    }
}

/// The checks before any browser process starts (scope 3, rows 1 to 6).
/// A failed check leaves no lock held and the profile as it was.
pub fn acquire(
    folder: &BrowserFolder,
    profile: ProfileUse,
    runner: &dyn BrowserProcessRunner,
    clock: &dyn BrowserClock,
) -> Result<BrowserLockHold, BrowserError> {
    let unavailable = |_: io::Error| BrowserError::StateUnavailable;
    let root = prepare_folder(folder).map_err(unavailable)?;
    let active = open_lock(&folder.active_lock(), Missing::Create).map_err(unavailable)?;
    let forget = open_lock(&folder.forget_lock(), Missing::Create).map_err(unavailable)?;
    if !lock_within(clock, &active, LockKind::Shared, START_WAIT_MS).map_err(unavailable)? {
        return Err(BrowserError::ForgetInProgress);
    }
    if !lock_within(clock, &forget, LockKind::Shared, START_WAIT_MS).map_err(unavailable)? {
        return Err(BrowserError::ForgetInProgress);
    }
    let forget_identity = forget.metadata().map_err(unavailable)?;
    drop(forget);
    let identity = |path: PathBuf, file: &File| {
        file.metadata()
            .map(|metadata| (path, FileIdentity::of(&metadata)))
            .map_err(unavailable)
    };
    let mut watched = WatchedLocks {
        root: (folder.root().to_owned(), root),
        active: identity(folder.active_lock(), &active)?,
        forget: (folder.forget_lock(), FileIdentity::of(&forget_identity)),
        profile_lock: None,
    };
    if profile == ProfileUse::Temporary {
        let browser_directory = hold_browser_directory(folder, root, clock)?;
        return Ok(BrowserLockHold {
            watched,
            profile: None,
            _profile_lock: None,
            _profile_directory: None,
            _active: active,
            _browser_directory: browser_directory,
        });
    }
    let saved = folder.profile();
    prepare_profile(&saved).map_err(|_| BrowserError::ProfileUnreadable)?;
    let profile_lock = open_lock(&folder.profile_lock(), Missing::Create).map_err(unavailable)?;
    if !lock_within(clock, &profile_lock, LockKind::Exclusive, START_WAIT_MS)
        .map_err(unavailable)?
    {
        return Err(BrowserError::ProfileInUse);
    }
    let directory =
        browser_profile_directory::open(&saved).map_err(|_| BrowserError::ProfileUnreadable)?;
    if !lock_within(clock, &directory, LockKind::Exclusive, START_WAIT_MS)
        .map_err(|_| BrowserError::ProfileUnreadable)?
    {
        return Err(BrowserError::ProfileInUse);
    }
    // Waiting for flock may have outlived a forget that unlinked this
    // directory. Chromium opens the pathname, so a lock on the old inode
    // alone is not permission to start on that pathname.
    let opened = directory
        .metadata()
        .map_err(|_| BrowserError::ProfileUnreadable)?;
    if identity_at(&saved, PathKind::Folder) != Some(FileIdentity::of(&opened)) {
        return Err(BrowserError::ProfileUnreadable);
    }
    if !matches!(
        singleton_open(&directory, runner),
        SingletonLock::Absent | SingletonLock::Startable
    ) {
        return Err(BrowserError::ProfileLocked);
    }
    watched.profile_lock = Some(identity(folder.profile_lock(), &profile_lock)?);
    let browser_directory = hold_browser_directory(folder, root, clock)?;
    Ok(BrowserLockHold {
        watched,
        profile: Some(saved),
        _profile_lock: Some(profile_lock),
        _profile_directory: Some(directory),
        _active: active,
        _browser_directory: browser_directory,
    })
}

/// The last admission before any child starts. A saved profile is already
/// locked and checked, so forget never waits for this hold while holding a
/// profile lock that the same starting generation still needs.
fn hold_browser_directory(
    folder: &BrowserFolder,
    expected: FileIdentity,
    clock: &dyn BrowserClock,
) -> Result<File, BrowserError> {
    let unavailable = |_: io::Error| BrowserError::StateUnavailable;
    let directory = browser_profile_directory::open(folder.root()).map_err(unavailable)?;
    if !lock_within(clock, &directory, LockKind::Shared, START_WAIT_MS).map_err(unavailable)? {
        return Err(BrowserError::ForgetInProgress);
    }
    if FileIdentity::of(&directory.metadata().map_err(unavailable)?) != expected
        || identity_at(folder.root(), PathKind::Folder) != Some(expected)
    {
        return Err(BrowserError::StateUnavailable);
    }
    Ok(directory)
}

/// How much a profile holds, without following symlinks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProfileSize {
    pub bytes: u64,
    /// More than 20,000 files: the rest was not added.
    pub truncated: bool,
}

pub fn profile_size(profile: &Path) -> ProfileSize {
    let mut size = ProfileSize::default();
    if !fs::symlink_metadata(profile).is_ok_and(|metadata| metadata.is_dir()) {
        return size;
    }
    let mut files = 0;
    let mut folders = vec![profile.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.file_type().is_dir() {
                folders.push(entry.path());
                continue;
            }
            if files == PROFILE_FILE_LIMIT {
                size.truncated = true;
                return size;
            }
            files += 1;
            size.bytes = size.bytes.saturating_add(metadata.len());
        }
    }
    size
}

/// Whether anything is at the profile's path, a symlink included.
pub(super) fn profile_present(profile: &Path) -> bool {
    fs::symlink_metadata(profile).is_ok()
}

/// Whether another holder keeps `path` from being locked as `kind`. A file
/// that is not there is held by nobody, and none is made.
fn held_elsewhere(path: &Path, kind: LockKind) -> bool {
    match open_lock(path, Missing::Leave) {
        Ok(file) => !try_lock(&file, kind).unwrap_or(false),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// The file and directory liveness holds are both probed without creating
/// anything. A replaced active.lock must not make a live browser disappear
/// from forget's survey, including when no saved profile exists.
pub(super) fn browser_running(folder: &BrowserFolder) -> bool {
    if held_elsewhere(&folder.active_lock(), LockKind::Exclusive) {
        return true;
    }
    match browser_profile_directory::open(folder.root()) {
        Ok(directory) => !try_lock(&directory, LockKind::Exclusive).unwrap_or(false),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// The login facts `runtime inspect` shows (scope 7). It only reads: start
/// checks 4 to 6 without making a folder or a lock file, and without keeping
/// a lock.
pub fn login_status(
    data_dir: &Path,
    persist: bool,
    runner: &dyn BrowserProcessRunner,
) -> BrowserLoginStatus {
    let folder = BrowserFolder::new(data_dir);
    let profile = folder.profile();
    let exists = profile_present(&profile);
    let size = if exists {
        profile_size(&profile)
    } else {
        ProfileSize::default()
    };
    let state = if held_elsewhere(&folder.forget_lock(), LockKind::Shared) {
        BrowserProfileState::InUse
    } else if !exists {
        BrowserProfileState::Absent
    } else if !profile_usable(&profile) {
        BrowserProfileState::Unreadable
    } else if held_elsewhere(&folder.profile_lock(), LockKind::Shared)
        || browser_profile_directory::open(&profile)
            .is_ok_and(|directory| !try_lock(&directory, LockKind::Shared).unwrap_or(false))
    {
        BrowserProfileState::InUse
    } else if matches!(
        singleton_lock(&profile, runner),
        SingletonLock::Live | SingletonLock::Elsewhere
    ) {
        BrowserProfileState::Locked
    } else {
        BrowserProfileState::Ready
    };
    BrowserLoginStatus {
        persist,
        profile: BrowserLoginProfile {
            exists,
            path: profile.to_string_lossy().into_owned(),
            size_bytes: size.bytes,
            size_truncated: size.truncated,
            state,
        },
    }
}
