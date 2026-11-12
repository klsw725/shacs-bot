//! `shacs-bot browser forget` (Spec 050 PRD 020 scope 5): closes every
//! browser of one data folder, whatever profile it runs on, and removes the
//! saved login profile once nothing can still be using it.
use super::browser_lock::{
    browser_running, create_private_folder, identity_at, lock_within, open_lock, profile_present,
    profile_size, singleton_open, try_lock, BrowserFolder, FileIdentity, LockKind, Missing,
    PathKind, ProfileSize, SingletonLock,
};
use super::{browser_profile_directory, BrowserClock, BrowserProcessRunner};
use crate::consent_prompt::{Consent, Prompt};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const QUESTION: &str = "계속할까요? [y/N]";
const CLOSED: &str = "브라우저를 닫았습니다.";
const NOT_CLOSED: &str = "브라우저를 닫지 못해 지우지 않았습니다. 잠시 뒤 다시 하세요.";
/// A generation's one-second look at `forget.lock` may hold it for a moment.
const FORGET_WAIT_MS: u64 = 2_000;
/// A browser gets this long to close and let go of `active.lock`.
const CLOSE_WAIT_MS: u64 = 15_000;
const PROFILE_WAIT_MS: u64 = 500;

/// Whether the user is asked before anything is closed or removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgetConfirmation {
    Ask,
    /// `--yes`.
    Skip,
}

pub struct ForgetRequest<'a> {
    pub data_dir: &'a Path,
    pub confirmation: ForgetConfirmation,
    pub runner: &'a dyn BrowserProcessRunner,
    pub clock: &'a dyn BrowserClock,
}

/// Where the steps after the confirmation ended. Two of the refusals read
/// the same to the user, so this is what tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgetStage {
    ForgetLockBusy,
    ActiveLockBusy,
    ProfileLockBusy,
    ProfileDirectoryBusy,
    BrowserDirectoryBusy,
    LockReplaced,
    ProfileNotDirectory,
    SingletonLive,
    SingletonElsewhere,
    Removed,
    NothingToRemove,
}

impl ForgetStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ForgetLockBusy => "forget_lock_busy",
            Self::ActiveLockBusy => "active_lock_busy",
            Self::ProfileLockBusy => "profile_lock_busy",
            Self::ProfileDirectoryBusy => "profile_directory_busy",
            Self::BrowserDirectoryBusy => "browser_directory_busy",
            Self::LockReplaced => "lock_replaced",
            Self::ProfileNotDirectory => "profile_not_directory",
            Self::SingletonLive => "singleton_live",
            Self::SingletonElsewhere => "singleton_elsewhere",
            Self::Removed => "removed",
            Self::NothingToRemove => "nothing_to_remove",
        }
    }

    fn succeeded(self) -> bool {
        matches!(self, Self::Removed | Self::NothingToRemove)
    }

    /// Whether every browser was seen closed before this stage was reached.
    fn after_close(self) -> bool {
        matches!(
            self,
            Self::ProfileNotDirectory
                | Self::SingletonLive
                | Self::SingletonElsewhere
                | Self::Removed
                | Self::NothingToRemove
        )
    }

    fn text(self) -> &'static str {
        match self {
            Self::ForgetLockBusy => "다른 browser forget이 실행 중입니다.",
            Self::ActiveLockBusy | Self::ProfileLockBusy | Self::ProfileDirectoryBusy
                | Self::BrowserDirectoryBusy | Self::SingletonLive => NOT_CLOSED,
            Self::LockReplaced => {
                "브라우저 lock 파일이 바뀌어 지우지 않았습니다. 다시 실행하세요."
            }
            Self::ProfileNotDirectory => {
                "로그인 기록 폴더가 진짜 폴더가 아니어서 지우지 않았습니다."
            }
            Self::SingletonElsewhere => {
                "로그인 기록이 다른 곳에서 잠겨 있어 지우지 않았습니다. docs/USAGE.md의 브라우저 로그인 유지를 보세요."
            }
            Self::Removed => "로그인 기록을 지웠습니다.",
            Self::NothingToRemove => "지울 로그인 기록이 없습니다.",
        }
    }
}

/// The three lock files forget held when it looked at the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForgetHeld {
    pub forget: FileIdentity,
    pub active: FileIdentity,
    pub profile: FileIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgetReport {
    /// What the command prints, in order.
    pub lines: Vec<String>,
    /// Whether the command ends with exit code 0.
    pub succeeded: bool,
    /// Never printed. `None` when the steps after the confirmation did not
    /// run or broke on a file error.
    pub stage: Option<ForgetStage>,
    /// Set when the steps ended at the look at the profile.
    pub held: Option<ForgetHeld>,
}

impl ForgetReport {
    fn said(line: &str, succeeded: bool) -> Self {
        Self {
            lines: vec![line.to_owned()],
            succeeded,
            stage: None,
            held: None,
        }
    }
}

pub fn forget(request: &ForgetRequest<'_>, prompt: &mut dyn Prompt) -> ForgetReport {
    let folder = BrowserFolder::new(request.data_dir);
    let profile = folder.profile();
    let present = profile_present(&profile);
    let running = browser_running(&folder);
    if !present && !running {
        return ForgetReport {
            stage: Some(ForgetStage::NothingToRemove),
            ..ForgetReport::said(ForgetStage::NothingToRemove.text(), true)
        };
    }
    if request.confirmation == ForgetConfirmation::Ask {
        let target = if present {
            format!(
                "{} ({})",
                profile.display(),
                readable(profile_size(&profile))
            )
        } else {
            "없음".to_owned()
        };
        prompt.show(&format!("지울 폴더: {target}"));
        prompt.show(&format!(
            "떠 있는 브라우저: {}",
            if running { "닫습니다" } else { "없음" }
        ));
        match Consent::asked(prompt, QUESTION) {
            Consent::Given => {}
            Consent::Refused => return ForgetReport::said("지우지 않았습니다.", true),
            Consent::NoTerminal => {
                return ForgetReport::said(
                    "터미널이 아니어서 물어볼 수 없습니다. 지우려면 --yes를 붙여 다시 실행하세요.",
                    false,
                )
            }
        }
    }
    let Ok((stage, held)) = remove(&folder, request) else {
        return ForgetReport::said("브라우저 폴더를 쓸 수 없어 지우지 않았습니다.", false);
    };
    let mut lines = Vec::new();
    if running && stage.after_close() {
        lines.push(CLOSED.to_owned());
    }
    lines.push(stage.text().to_owned());
    ForgetReport {
        lines,
        succeeded: stage.succeeded(),
        stage: Some(stage),
        held,
    }
}

/// The steps after the confirmation. Every lock taken here is released when
/// this returns, wherever it stops.
fn remove(
    folder: &BrowserFolder,
    request: &ForgetRequest<'_>,
) -> io::Result<(ForgetStage, Option<ForgetHeld>)> {
    let clock = request.clock;
    create_private_folder(folder.root())?;
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(folder.root())?;
    let forget = open_lock(&folder.forget_lock(), Missing::Create)?;
    if !lock_within(clock, &forget, LockKind::Exclusive, FORGET_WAIT_MS)? {
        return Ok((ForgetStage::ForgetLockBusy, None));
    }
    // The active file and browser directory share one closing budget. The
    // directory also sees generations whose active.lock was replaced.
    let closing_started = clock.now_ms();
    let active = open_lock(&folder.active_lock(), Missing::Create)?;
    if !lock_within(clock, &active, LockKind::Exclusive, CLOSE_WAIT_MS)? {
        return Ok((ForgetStage::ActiveLockBusy, None));
    }
    let profile_lock = open_lock(&folder.profile_lock(), Missing::Create)?;
    let remaining = CLOSE_WAIT_MS.saturating_sub(clock.now_ms().saturating_sub(closing_started));
    if !lock_within(
        clock,
        &profile_lock,
        LockKind::Exclusive,
        PROFILE_WAIT_MS.min(remaining),
    )? {
        return Ok((ForgetStage::ProfileLockBusy, None));
    }
    // A lock on a file that is no longer at its path keeps nobody out.
    let held = ForgetHeld {
        forget: identity_of(&forget)?,
        active: identity_of(&active)?,
        profile: identity_of(&profile_lock)?,
    };
    let remaining = CLOSE_WAIT_MS.saturating_sub(clock.now_ms().saturating_sub(closing_started));
    if !lock_within(clock, &root, LockKind::Exclusive, remaining)? {
        return Ok((ForgetStage::BrowserDirectoryBusy, Some(held)));
    }
    // Check identities after the last wait, not before it: paths may have
    // changed while the directory's last generation was shutting down.
    let same = identity_at(folder.root(), PathKind::Folder) == Some(identity_of(&root)?)
        && identity_at(&folder.forget_lock(), PathKind::File) == Some(held.forget)
        && identity_at(&folder.active_lock(), PathKind::File) == Some(held.active)
        && identity_at(&folder.profile_lock(), PathKind::File) == Some(held.profile);
    if !same {
        return Ok((ForgetStage::LockReplaced, None));
    }
    let profile = folder.profile();
    match fs::symlink_metadata(&profile) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Ok((ForgetStage::ProfileNotDirectory, Some(held))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((ForgetStage::NothingToRemove, None))
        }
        Err(error) => return Err(error),
    }
    let directory = browser_profile_directory::profile(&root)?;
    // Keep decision (나) independently of flock: foreign SingletonLock remains
    // a refusal even on filesystems that do not share advisory locks.
    match singleton_open(&directory, request.runner) {
        SingletonLock::Absent | SingletonLock::Startable => {}
        SingletonLock::Live => return Ok((ForgetStage::SingletonLive, Some(held))),
        SingletonLock::Elsewhere => return Ok((ForgetStage::SingletonElsewhere, Some(held))),
    }
    // Unlike a sibling lock file, this inode cannot be replaced while keeping
    // the same profile contents. It is held before launch and through teardown.
    if !try_lock(&directory, LockKind::Exclusive)? {
        return Ok((ForgetStage::ProfileDirectoryBusy, Some(held)));
    }
    browser_profile_directory::remove(&directory, &root, &profile)?;
    Ok((ForgetStage::Removed, None))
}

fn identity_of(file: &File) -> io::Result<FileIdentity> {
    file.metadata().map(|metadata| FileIdentity::of(&metadata))
}

fn readable(size: ProfileSize) -> String {
    let mut whole = size.bytes;
    let mut tenth = 0;
    let mut unit = "B";
    for larger in ["KB", "MB", "GB", "TB"] {
        if whole < 1024 {
            break;
        }
        tenth = whole % 1024 * 10 / 1024;
        whole /= 1024;
        unit = larger;
    }
    let amount = if unit == "B" {
        format!("{whole} B")
    } else {
        format!("{whole}.{tenth} {unit}")
    };
    if size.truncated {
        format!("{amount} 이상")
    } else {
        amount
    }
}
