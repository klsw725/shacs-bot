//! The view-only screen viewer of a browser generation (Spec 049 PRD 040):
//! `x11vnc` exports the generation's display on container loopback and
//! `websockify` serves noVNC on container port 6080. The password reaches
//! `x11vnc` only through a short-lived 0600 file inside the generation's 0700
//! folder; it is never in argv, child environment, status or diagnostics.

use super::{
    BrowserChild, BrowserChildKind, BrowserEnvironment, BrowserLifecycleFactory,
    BrowserLifecycleParticipant, BrowserProcessRunner, BrowserViewState, BrowserViewStatus,
    ViewerListener,
};
use shacs_config::{BrowserViewConfig, ConfigBundle, ProcessEnv};
use shacs_core::runtime::BrowserError;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

pub const X11VNC: &str = "/usr/bin/x11vnc";
pub const WEBSOCKIFY: &str = "/usr/bin/websockify";
pub const NOVNC_ROOT: &str = "/usr/share/novnc";
/// VNC listener inside the container, loopback only; websockify's target.
pub const VNC_PORT: u16 = 5900;
/// noVNC listener inside the container; the only port Compose publishes.
pub const VIEWER_PORT: u16 = 6080;
/// Host ports Compose publishes on 127.0.0.1 for the gateway and api services.
pub const GATEWAY_VIEWER_HOST_PORT: u16 = 6080;
pub const API_VIEWER_HOST_PORT: u16 = 6081;
/// The viewer's own start budget. It begins after the browser (Xvfb, Chromium,
/// DevTools) has started and is never charged to the browser startup limit.
pub const VIEWER_START_LIMIT: Duration = Duration::from_secs(10);
/// Lines x11vnc reads as instructions, not as a password, wherever they occur.
const PASSWORD_FILE_CONTROL_WORDS: [&str; 4] =
    ["__SKIP__", "__COMM__", "__EMPTY__", "__BEGIN_VIEWONLY__"];
pub const PASSWORD_MIN_CHARS: usize = 8;
/// Well under one x11vnc password-file record (a 1024-byte `fgets` buffer):
/// a longer line would be read as two passwords, both accepted.
pub const PASSWORD_MAX_CHARS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserViewPasswordProblem {
    Missing,
    TooShort,
    TooLong,
    FormatInvalid,
}

impl BrowserViewPasswordProblem {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Missing => "password_missing",
            Self::TooShort => "password_too_short",
            Self::TooLong => "password_too_long",
            Self::FormatInvalid => "password_format_invalid",
        }
    }
}

/// Whether a password is 8–128 printable ASCII characters (`0x21`–`0x7E`, so
/// no spaces or control characters), fits one x11vnc password-file line, and
/// cannot be read as one of the file's control lines. Only ASCII is accepted
/// because noVNC derives the DES key from UTF-16 code units while the file
/// holds UTF-8; a non-ASCII password could never authenticate.
pub fn validate_view_password(password: &str) -> Result<(), BrowserViewPasswordProblem> {
    if password.is_empty() {
        return Err(BrowserViewPasswordProblem::Missing);
    }
    let characters = password.chars().count();
    if characters < PASSWORD_MIN_CHARS {
        return Err(BrowserViewPasswordProblem::TooShort);
    }
    if characters > PASSWORD_MAX_CHARS {
        return Err(BrowserViewPasswordProblem::TooLong);
    }
    let unrepresentable = !password.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        || password.starts_with('#')
        || PASSWORD_FILE_CONTROL_WORDS
            .iter()
            .any(|word| password.contains(word));
    if unrepresentable {
        return Err(BrowserViewPasswordProblem::FormatInvalid);
    }
    Ok(())
}

pub fn view_url(host_port: u16) -> String {
    format!("http://127.0.0.1:{host_port}/vnc.html")
}

/// The host port Compose publishes for this process role: `serve` is the api
/// service on 6081; the gateway and a one-off CLI default to 6080.
pub fn published_viewer_port(owner_mode: Option<&str>) -> u16 {
    if owner_mode == Some("serve") {
        API_VIEWER_HOST_PORT
    } else {
        GATEWAY_VIEWER_HOST_PORT
    }
}

pub fn x11vnc_arguments(display: u16, xauthority: &Path, password_file: &Path) -> Vec<String> {
    vec![
        "-norc".to_owned(),
        "-noremote".to_owned(),
        "-display".to_owned(),
        format!(":{display}"),
        "-auth".to_owned(),
        xauthority.to_string_lossy().into_owned(),
        "-rfbport".to_owned(),
        VNC_PORT.to_string(),
        "-listen".to_owned(),
        "127.0.0.1".to_owned(),
        "-localhost".to_owned(),
        "-viewonly".to_owned(),
        "-nosel".to_owned(),
        "-noclipboard".to_owned(),
        "-forever".to_owned(),
        "-shared".to_owned(),
        "-passwdfile".to_owned(),
        format!("rm:{}", password_file.to_string_lossy()),
    ]
}

pub fn websockify_arguments() -> Vec<String> {
    vec![
        format!("--web={NOVNC_ROOT}"),
        format!("0.0.0.0:{VIEWER_PORT}"),
        format!("127.0.0.1:{VNC_PORT}"),
    ]
}

/// The viewer facts for a configuration: disabled, unavailable (with the
/// reason code only) or closed when it could start.
pub fn view_availability(view: &BrowserViewConfig, installed: bool) -> BrowserViewStatus {
    if !view.enable {
        return BrowserViewStatus::disabled();
    }
    if let Err(problem) = validate_view_password(&view.password) {
        return BrowserViewStatus::unavailable(problem.reason());
    }
    if !installed {
        return BrowserViewStatus::unavailable("package_missing");
    }
    BrowserViewStatus::closed()
}

/// Availability for a separate inspect, whose bundle keeps `${NAME}`
/// unresolved: the password is resolved here with the loader's own order
/// (process environment, then `.env.json`) and only its verdict is kept.
pub(crate) fn configured_view(bundle: &ConfigBundle, installed: bool) -> BrowserViewStatus {
    let mut view = bundle.config.tools.browser.view.clone();
    if view.enable {
        let resolved: Option<String> =
            shacs_config::config_env(&bundle.context.config_path, &ProcessEnv)
                .ok()
                .and_then(|environment| {
                    shacs_config::interpolate_env_with_source(&view.password, &environment).ok()
                });
        view.password = resolved.unwrap_or_default();
    }
    view_availability(&view, installed)
}

/// Creates one viewer participant per browser generation from the process
/// configuration captured at startup.
pub struct BrowserViewerFactory {
    view: BrowserViewConfig,
    host_port: u16,
}

impl BrowserViewerFactory {
    pub fn new(view: BrowserViewConfig, host_port: u16) -> Self {
        Self { view, host_port }
    }
}

impl BrowserLifecycleFactory for BrowserViewerFactory {
    fn create(&self) -> Box<dyn BrowserLifecycleParticipant> {
        Box::new(BrowserViewer::new(&self.view, self.host_port))
    }

    fn view(&self, runner: &dyn BrowserProcessRunner) -> BrowserViewStatus {
        view_availability(&self.view, runner.viewer_available())
    }
}

/// Why a viewer that should have run did not, with its diagnostic code.
enum ViewerStartFailure {
    /// Something else already listens on the VNC loopback port, before or
    /// while x11vnc starts; websockify would forward to that stranger.
    PortInUse,
    ProcessExited,
    Timeout,
    Spawn(BrowserError),
}

impl ViewerStartFailure {
    fn reason(&self) -> &'static str {
        match self {
            Self::PortInUse => "port_in_use",
            Self::ProcessExited => "process_exited",
            Self::Timeout => "startup_timeout",
            Self::Spawn(_) => "startup_failed",
        }
    }

    fn error(&self) -> BrowserError {
        match self {
            Self::PortInUse => BrowserError::Unavailable,
            Self::ProcessExited => BrowserError::ProcessExited,
            Self::Timeout => BrowserError::StartupTimeout,
            Self::Spawn(error) => *error,
        }
    }
}

/// The viewer of one generation. Holds the password only between `create`
/// and the moment the password file is written.
pub struct BrowserViewer {
    enabled: bool,
    host_port: u16,
    secret: Option<String>,
    problem: Option<BrowserViewPasswordProblem>,
    x11vnc: Option<Box<dyn BrowserChild>>,
    websockify: Option<Box<dyn BrowserChild>>,
    password_file: Option<PathBuf>,
    state: BrowserViewState,
    reason: Option<String>,
}

impl BrowserViewer {
    pub fn new(view: &BrowserViewConfig, host_port: u16) -> Self {
        let (secret, problem) = match validate_view_password(&view.password) {
            Ok(()) if view.enable => (Some(view.password.clone()), None),
            Ok(()) => (None, None),
            Err(problem) => (None, Some(problem)),
        };
        Self {
            enabled: view.enable,
            host_port,
            secret,
            problem,
            x11vnc: None,
            websockify: None,
            password_file: None,
            state: if view.enable {
                BrowserViewState::Closed
            } else {
                BrowserViewState::Disabled
            },
            reason: (!view.enable).then(|| "disabled".to_owned()),
        }
    }

    fn launch(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        deadline: Instant,
        secret: String,
    ) -> Result<(), ViewerStartFailure> {
        // An occupied port is refused before any file exists: the password
        // must not be written for an x11vnc that could not own the listener.
        if runner.viewer_listener(VNC_PORT, None) != ViewerListener::Absent {
            drop(secret);
            return Err(ViewerStartFailure::PortInUse);
        }
        let path =
            create_password_file(environment.root(), &secret).map_err(ViewerStartFailure::Spawn)?;
        drop(secret);
        self.password_file = Some(path.clone());
        let x11vnc = environment.child(
            BrowserChildKind::Viewer,
            X11VNC,
            x11vnc_arguments(
                environment.display(),
                &environment.root().join("Xauthority"),
                &path,
            ),
        );
        self.x11vnc = Some(
            runner
                .spawn(&x11vnc, deadline)
                .map_err(ViewerStartFailure::Spawn)?,
        );
        loop {
            let child = self
                .x11vnc
                .as_mut()
                .ok_or(ViewerStartFailure::Spawn(BrowserError::Protocol))?;
            if child.exited().map_err(ViewerStartFailure::Spawn)? {
                return Err(ViewerStartFailure::ProcessExited);
            }
            // Ready means: x11vnc consumed the file AND the IPv4 loopback
            // listener belongs to that x11vnc. x11vnc stays alive IPv6-only
            // when the IPv4 bind fails, so a listener alone proves nothing.
            match runner.viewer_listener(VNC_PORT, child.process_id()) {
                ViewerListener::Foreign => return Err(ViewerStartFailure::PortInUse),
                ViewerListener::OwnedByChild if fs::symlink_metadata(&path).is_err() => break,
                ViewerListener::OwnedByChild | ViewerListener::Absent => {}
            }
            if Instant::now() >= deadline {
                return Err(ViewerStartFailure::Timeout);
            }
            thread::sleep(Duration::from_millis(10));
        }
        // x11vnc read and removed the file; nothing is left for the host to delete.
        self.password_file = None;
        let websockify =
            environment.child(BrowserChildKind::Viewer, WEBSOCKIFY, websockify_arguments());
        self.websockify = Some(
            runner
                .spawn(&websockify, deadline)
                .map_err(ViewerStartFailure::Spawn)?,
        );
        Ok(())
    }

    /// Stops websockify, then x11vnc, and removes a password file x11vnc did
    /// not consume. Safe to call more than once.
    fn cleanup(&mut self) {
        if let Some(mut websockify) = self.websockify.take() {
            websockify.stop();
        }
        if let Some(mut x11vnc) = self.x11vnc.take() {
            x11vnc.stop();
        }
        if let Some(path) = self.password_file.take() {
            let _ = fs::remove_file(path);
        }
    }
}

impl BrowserLifecycleParticipant for BrowserViewer {
    fn start(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        deadline: Instant,
    ) -> Result<(), BrowserError> {
        if !self.enabled {
            return Ok(());
        }
        if let Some(problem) = self.problem {
            self.state = BrowserViewState::Unavailable;
            self.reason = Some(problem.reason().to_owned());
            return Ok(());
        }
        if !runner.viewer_available() {
            self.secret = None;
            self.state = BrowserViewState::Unavailable;
            self.reason = Some("package_missing".to_owned());
            return Ok(());
        }
        let Some(secret) = self.secret.take() else {
            self.state = BrowserViewState::Failed;
            self.reason = Some("startup_failed".to_owned());
            return Err(BrowserError::Protocol);
        };
        self.state = BrowserViewState::Starting;
        match self.launch(environment, runner, deadline, secret) {
            Ok(()) => {
                self.state = BrowserViewState::Open;
                self.reason = None;
                Ok(())
            }
            Err(failure) => {
                self.cleanup();
                self.state = BrowserViewState::Failed;
                self.reason = Some(failure.reason().to_owned());
                Err(failure.error())
            }
        }
    }

    fn failed(&mut self) -> bool {
        if self.state != BrowserViewState::Open {
            return false;
        }
        let dead = |child: &mut Option<Box<dyn BrowserChild>>| {
            child
                .as_mut()
                .is_none_or(|child| child.exited().unwrap_or(true))
        };
        if dead(&mut self.x11vnc) || dead(&mut self.websockify) {
            self.state = BrowserViewState::Failed;
            self.reason = Some("process_exited".to_owned());
            return true;
        }
        false
    }

    fn stop(&mut self) {
        self.cleanup();
        if matches!(
            self.state,
            BrowserViewState::Open | BrowserViewState::Starting
        ) {
            self.state = BrowserViewState::Closed;
            self.reason = None;
        }
    }

    fn view(&self) -> BrowserViewStatus {
        BrowserViewStatus {
            enabled: self.enabled,
            state: self.state,
            reason: self.reason.clone(),
            url: (self.state == BrowserViewState::Open).then(|| view_url(self.host_port)),
        }
    }
}

/// A new 0600 regular file with an unpredictable name inside `root`, holding
/// the password as its only line, closed before this returns.
fn create_password_file(root: &Path, secret: &str) -> Result<PathBuf, BrowserError> {
    let mut random = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| BrowserError::Unavailable)?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = root.join(format!("view-{suffix}"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).map_err(|_| BrowserError::Unavailable)?;
    let written = file
        .write_all(secret.as_bytes())
        .and_then(|()| file.write_all(b"\n"));
    drop(file);
    if written.is_err() {
        let _ = fs::remove_file(&path);
        return Err(BrowserError::Unavailable);
    }
    Ok(path)
}
