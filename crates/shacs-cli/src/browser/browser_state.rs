use crate::{RuntimeOwnershipMarker, RuntimeOwnershipState, RuntimeOwnershipStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_config::ConfigBundle;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserState {
    Disabled,
    Unavailable,
    Idle,
    Starting,
    Running,
    Busy,
    Failed,
    Unknown,
}

/// The screen viewer inside the browser status (Spec 049 PRD 040 scope 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserViewState {
    Disabled,
    Unavailable,
    Starting,
    Open,
    Failed,
    Closed,
    Unknown,
}

/// Viewer facts without the password, its variable name or child output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserViewStatus {
    pub enabled: bool,
    pub state: BrowserViewState,
    pub reason: Option<String>,
    pub url: Option<String>,
    /// `tools.browser.view.url` is set but unusable, so the default address
    /// is shown (Spec 050 PRD 010 scope 12).
    #[serde(default)]
    pub url_ignored: bool,
}

impl BrowserViewStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            state: BrowserViewState::Disabled,
            reason: Some("disabled".to_owned()),
            url: None,
            url_ignored: false,
        }
    }

    /// Enabled but not usable in this process; `reason` says why.
    pub fn unavailable(reason: &str) -> Self {
        Self {
            enabled: true,
            state: BrowserViewState::Unavailable,
            reason: Some(reason.to_owned()),
            url: None,
            url_ignored: false,
        }
    }

    /// Enabled and usable; no viewer is open right now.
    pub fn closed() -> Self {
        Self {
            enabled: true,
            state: BrowserViewState::Closed,
            reason: None,
            url: None,
            url_ignored: false,
        }
    }

    pub fn with_url_ignored(mut self, ignored: bool) -> Self {
        self.url_ignored = ignored;
        self
    }

    /// A live state that cannot be trusted keeps only the configuration facts.
    fn unknown(&mut self) {
        self.state = BrowserViewState::Unknown;
        self.url = None;
    }
}

/// Who may act on the browser (Spec 050 PRD 010 scope 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserControl {
    #[default]
    Assistant,
    User,
}

/// Whether a browser could start on the saved login profile right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfileState {
    Ready,
    Absent,
    InUse,
    Locked,
    Unreadable,
}

/// Where the saved login profile is and how much it holds. Nothing inside
/// it is named: no file, no site, no content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserLoginProfile {
    pub exists: bool,
    pub path: String,
    pub size_bytes: u64,
    /// The profile has more than 20,000 files; the size covers that many.
    #[serde(default)]
    pub size_truncated: bool,
    pub state: BrowserProfileState,
}

/// The saved login inside the browser status (Spec 050 PRD 020 scope 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserLoginStatus {
    /// `tools.browser.persistLogin` as configured.
    pub persist: bool,
    pub profile: BrowserLoginProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatus {
    pub enabled: bool,
    pub available: bool,
    pub reason: Option<String>,
    pub state: BrowserState,
    pub owner_observed: bool,
    pub updated_at_ms: u64,
    pub view: BrowserViewStatus,
    /// `tools.browser.replyWaitMinutes` as configured.
    #[serde(default)]
    pub reply_wait_minutes: u64,
    /// When the browser closes unless the session's user replies. Set only
    /// while it waits for that reply.
    #[serde(default)]
    pub reply_wait_deadline_ms: Option<u64>,
    /// Who controls the browser. Which session took it over, the episode
    /// and anything pasted are never recorded here.
    #[serde(default)]
    pub control: BrowserControl,
    /// `tools.browser.takeoverMinutes` as configured.
    #[serde(default)]
    pub takeover_minutes: u64,
    /// When the browser is taken back. Set only while the user controls it.
    #[serde(default)]
    pub takeover_deadline_ms: Option<u64>,
    /// Read from the data folder by `runtime inspect`; the browser's owner
    /// never writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<BrowserLoginStatus>,
}

impl BrowserStatus {
    pub fn new(enabled: bool, available: bool, now: u64) -> Self {
        Self {
            enabled,
            available,
            reason: if !enabled {
                Some("disabled".to_owned())
            } else if !available {
                Some("official_container_or_package_unavailable".to_owned())
            } else {
                None
            },
            state: if !enabled {
                BrowserState::Disabled
            } else if !available {
                BrowserState::Unavailable
            } else {
                BrowserState::Idle
            },
            owner_observed: false,
            updated_at_ms: now,
            view: BrowserViewStatus::disabled(),
            reply_wait_minutes: 0,
            reply_wait_deadline_ms: None,
            control: BrowserControl::Assistant,
            takeover_minutes: 0,
            takeover_deadline_ms: None,
            login: None,
        }
    }

    pub fn with_takeover(mut self, minutes: u64) -> Self {
        self.takeover_minutes = minutes;
        self
    }

    pub fn with_reply_wait(mut self, minutes: u64) -> Self {
        self.reply_wait_minutes = minutes;
        self
    }

    pub fn with_view(mut self, view: BrowserViewStatus) -> Self {
        self.view = view;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOwnerBinding {
    pub owner_id: String,
    pub pid: u32,
    pub started_at_ms: u64,
    pub config_path: String,
    pub workspace: String,
    pub config_digest: String,
}

impl BrowserOwnerBinding {
    pub fn from_marker(marker: &RuntimeOwnershipMarker) -> io::Result<Self> {
        Ok(Self {
            owner_id: marker.owner_id.clone(),
            pid: marker.pid,
            started_at_ms: marker.started_at_ms,
            config_path: marker.config_path.clone(),
            workspace: marker.workspace.clone(),
            config_digest: config_digest(Path::new(&marker.config_path))?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStateSnapshot {
    pub owner: BrowserOwnerBinding,
    pub generation: u64,
    pub holder_present: bool,
    pub status: BrowserStatus,
}

pub fn write_browser_snapshot(data_dir: &Path, snapshot: &BrowserStateSnapshot) -> io::Result<()> {
    let directory = data_dir.join("runtime");
    fs::create_dir_all(&directory)?;
    let temporary = directory.join(format!(
        ".browser-state-{}-{}.tmp",
        snapshot.owner.pid, snapshot.generation
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let result = (|| {
        let mut file = options.open(&temporary)?;
        let bytes = serde_json::to_vec(snapshot).map_err(io::Error::other)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, directory.join("browser-state.json"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn read_browser_snapshot(
    data_dir: &Path,
    ownership: &RuntimeOwnershipStatus,
    config_path: &Path,
    workspace: &Path,
    mut status: BrowserStatus,
    now: u64,
) -> BrowserStatus {
    status.state = BrowserState::Unknown;
    status.owner_observed = false;
    status.view.unknown();
    status.reply_wait_deadline_ms = None;
    status.control = BrowserControl::Assistant;
    status.takeover_deadline_ms = None;
    if status.reason.is_none() {
        status.reason = Some("owner_snapshot_unavailable".to_owned());
    }
    let Some(marker) = ownership.marker.as_ref().filter(|marker| {
        ownership.state == RuntimeOwnershipState::Active
            && marker.expires_at_ms > now
            && Path::new(&marker.config_path) == config_path
            && Path::new(&marker.workspace) == workspace
    }) else {
        return status;
    };
    let Ok(binding) = BrowserOwnerBinding::from_marker(marker) else {
        return status;
    };
    let Ok(bytes) = fs::read(data_dir.join("runtime/browser-state.json")) else {
        return status;
    };
    let Ok(snapshot) = serde_json::from_slice::<BrowserStateSnapshot>(&bytes) else {
        return status;
    };
    if snapshot.owner != binding
        || snapshot.generation == 0
        || snapshot.status.updated_at_ms < marker.started_at_ms
        || snapshot.status.updated_at_ms > now
    {
        return status;
    }
    let mut observed = snapshot.status;
    observed.owner_observed = true;
    if snapshot.holder_present {
        observed.state = BrowserState::Busy;
    }
    observed
}

pub(crate) fn inspect(bundle: &ConfigBundle, ownership: &RuntimeOwnershipStatus) -> BrowserStatus {
    use super::BrowserProcessRunner;
    let runner = super::LinuxBrowserProcessRunner;
    let available = crate::official_runtime_package_marker_observed()
        && crate::runtime_containment_inspect(bundle).contained == Some(true)
        && runner.available();
    let view = super::browser_viewer::configured_view(bundle, runner.viewer_available());
    let status = BrowserStatus::new(
        bundle.config.tools.browser.enable,
        available,
        crate::now_millis(),
    )
    .with_view(view)
    .with_reply_wait(bundle.config.tools.browser.reply_wait_minutes)
    .with_takeover(bundle.config.tools.browser.takeover_minutes);
    let mut status = read_browser_snapshot(
        &bundle.context.data_dir,
        ownership,
        &bundle.context.config_path,
        &bundle.context.workspace,
        status,
        crate::now_millis(),
    );
    status.login = Some(super::browser_lock::login_status(
        &bundle.context.data_dir,
        bundle.config.tools.browser.persist_login,
        &runner,
    ));
    status
}

/// The status as a diagnostics bundle carries it: of the login profile only
/// whether it is kept, whether it exists and its state.
pub(crate) fn diagnostics(status: &BrowserStatus) -> serde_json::Value {
    let mut value = serde_json::json!(status);
    if let Some(profile) = value
        .pointer_mut("/login/profile")
        .and_then(serde_json::Value::as_object_mut)
    {
        profile.retain(|key, _| matches!(key.as_str(), "exists" | "state"));
    }
    value
}

pub(crate) fn current_owner(data_dir: &Path) -> Option<BrowserOwnerBinding> {
    BrowserOwnerBinding::from_marker(&current_marker(data_dir)?).ok()
}

/// The active ownership marker of this process, if it holds one.
pub(crate) fn current_marker(data_dir: &Path) -> Option<RuntimeOwnershipMarker> {
    let ownership = crate::inspect_runtime_ownership(data_dir, crate::now_millis()).ok()?;
    ownership.marker.filter(|marker| {
        ownership.state == RuntimeOwnershipState::Active && marker.pid == std::process::id()
    })
}

fn config_digest(path: &Path) -> io::Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

pub struct BrowserSnapshotDestination {
    pub data_dir: PathBuf,
    pub owner: Option<BrowserOwnerBinding>,
    pub config_path: PathBuf,
    pub workspace: PathBuf,
}
