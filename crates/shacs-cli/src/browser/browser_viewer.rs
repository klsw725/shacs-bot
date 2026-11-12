//! The screen viewer of a browser generation (Spec 049 PRD 040, Spec 050 PRD
//! 010): `x11vnc` exports the generation's display on container loopback and
//! `websockify` serves noVNC on container port 6080, finding its targets in
//! the generation's `view-routes` file. `x11vnc` is view-only except while
//! the user has taken the browser over. The password reaches `x11vnc` only
//! through a short-lived 0600 file inside the generation's 0700 folder; it is
//! never in argv, child environment, status, diagnostics or `Debug`.

use super::{
    BrowserChild, BrowserChildKind, BrowserClock, BrowserEnvironment, BrowserLifecycleFactory,
    BrowserLifecycleParticipant, BrowserProcessRunner, BrowserViewState, BrowserViewStatus,
    ViewerListenAddress, ViewerListener, ViewerRoutes, ViewerWait,
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
/// How long ending a viewer process may take to be confirmed before the
/// whole generation is closed instead (Spec 050 PRD 010 scope 4).
pub const VIEWER_STOP_LIMIT_MS: u64 = 2_000;
/// The token file websockify reads on every connection.
pub const VIEW_ROUTES: &str = "view-routes";
/// The paste listener's socket inside the generation folder.
pub const PASTE_SOCKET: &str = "paste.sock";
/// The page the visible address points at.
pub const VIEW_PAGE: &str = "shacs-view.html";
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

/// The address shown to the user: the configured base address when it is
/// usable, else the loopback port Compose publishes.
pub fn view_url(view: &BrowserViewConfig, host_port: u16) -> String {
    match view.base_url() {
        Some(base) => format!("{base}/{VIEW_PAGE}"),
        None => format!("http://127.0.0.1:{host_port}/{VIEW_PAGE}"),
    }
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

/// `input` leaves out `-viewonly` and nothing else: remote control and the X
/// selection stay closed in both control states.
pub fn x11vnc_arguments(
    display: u16,
    xauthority: &Path,
    password_file: &Path,
    input: bool,
) -> Vec<String> {
    let mut arguments = vec![
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
    ];
    if !input {
        arguments.push("-viewonly".to_owned());
    }
    arguments.extend([
        "-nosel".to_owned(),
        "-noclipboard".to_owned(),
        "-forever".to_owned(),
        "-shared".to_owned(),
        "-passwdfile".to_owned(),
        format!("rm:{}", password_file.to_string_lossy()),
    ]);
    arguments
}

/// websockify has no fixed target: each connection's token is looked up in
/// `routes`, and the Origin check runs before the lookup.
pub fn websockify_arguments(routes: &Path) -> Vec<String> {
    vec![
        format!("--web={NOVNC_ROOT}"),
        "--token-plugin=TokenFile".to_owned(),
        format!("--token-source={}", routes.to_string_lossy()),
        "--auth-plugin=shacs_view_auth.SameOrigin".to_owned(),
        format!("0.0.0.0:{VIEWER_PORT}"),
    ]
}

/// The text of `view-routes` for a generation folder. `None` when the folder's
/// path holds a `:`, which the token file format cannot carry; the viewer is
/// not started then.
pub fn view_routes(root: &Path, routes: ViewerRoutes) -> Option<String> {
    let root = root.to_str().filter(|root| !root.contains(':'))?;
    Some(match routes {
        ViewerRoutes::Empty => String::new(),
        ViewerRoutes::View => format!("view: 127.0.0.1:{VNC_PORT}\n"),
        ViewerRoutes::ViewAndPaste => {
            format!("view: 127.0.0.1:{VNC_PORT}\npaste: unix_socket:{root}/{PASTE_SOCKET}\n")
        }
    })
}

/// The viewer facts for a configuration: disabled, unavailable (with the
/// reason code only) or closed when it could start.
pub fn view_availability(view: &BrowserViewConfig, installed: bool) -> BrowserViewStatus {
    let status = if !view.enable {
        BrowserViewStatus::disabled()
    } else if let Err(problem) = validate_view_password(&view.password) {
        BrowserViewStatus::unavailable(problem.reason())
    } else if !installed {
        BrowserViewStatus::unavailable("package_missing")
    } else {
        BrowserViewStatus::closed()
    };
    status.with_url_ignored(view.url_ignored())
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
    /// Something else already listens on a viewer port, before or while the
    /// process starts; a connection would reach that stranger.
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

/// The viewer of one generation. It keeps the validated password for the
/// generation because every control switch starts a new `x11vnc`.
pub struct BrowserViewer {
    enabled: bool,
    url: String,
    url_ignored: bool,
    secret: Option<String>,
    problem: Option<BrowserViewPasswordProblem>,
    x11vnc: Option<Box<dyn BrowserChild>>,
    websockify: Option<Box<dyn BrowserChild>>,
    password_file: Option<PathBuf>,
    /// The generation folder, once started: where `view-routes` lives.
    root: Option<PathBuf>,
    state: BrowserViewState,
    reason: Option<String>,
}

impl std::fmt::Debug for BrowserViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserViewer")
            .field("enabled", &self.enabled)
            .field("url", &self.url)
            .field(
                "password",
                &if self.secret.is_some() {
                    "[REDACTED]"
                } else {
                    ""
                },
            )
            .field("state", &self.state)
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
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
            url: view_url(view, host_port),
            url_ignored: view.url_ignored(),
            secret,
            problem,
            x11vnc: None,
            websockify: None,
            password_file: None,
            root: None,
            state: if view.enable {
                BrowserViewState::Closed
            } else {
                BrowserViewState::Disabled
            },
            reason: (!view.enable).then(|| "disabled".to_owned()),
        }
    }

    /// Starts `x11vnc` and waits until it consumed its password file and owns
    /// the loopback listener. The spawned process stays attached on failure
    /// so whoever ends it can confirm that it is gone.
    fn screen(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        input: bool,
        deadline: Instant,
        expired: &dyn Fn() -> bool,
    ) -> Result<(), ViewerStartFailure> {
        // An occupied port is refused before any file exists: the password
        // must not be written for an x11vnc that could not own the listener.
        if runner.viewer_listener(ViewerListenAddress::Loopback, VNC_PORT, None)
            != ViewerListener::Absent
        {
            return Err(ViewerStartFailure::PortInUse);
        }
        let secret = self
            .secret
            .as_deref()
            .ok_or(ViewerStartFailure::Spawn(BrowserError::Protocol))?;
        let path =
            create_password_file(environment.root(), secret).map_err(ViewerStartFailure::Spawn)?;
        self.password_file = Some(path.clone());
        let x11vnc = environment.child(
            BrowserChildKind::Viewer,
            X11VNC,
            x11vnc_arguments(
                environment.display(),
                &environment.root().join("Xauthority"),
                &path,
                input,
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
            match runner.viewer_listener(
                ViewerListenAddress::Loopback,
                VNC_PORT,
                child.process_id(),
            ) {
                ViewerListener::Foreign => return Err(ViewerStartFailure::PortInUse),
                ViewerListener::OwnedByChild if fs::symlink_metadata(&path).is_err() => break,
                ViewerListener::OwnedByChild | ViewerListener::Absent => {}
            }
            if expired() {
                return Err(ViewerStartFailure::Timeout);
            }
            thread::sleep(Duration::from_millis(10));
        }
        // x11vnc read and removed the file; nothing is left for the host to delete.
        self.password_file = None;
        Ok(())
    }

    fn spawn_websockify(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        deadline: Instant,
    ) -> Result<(), ViewerStartFailure> {
        let websockify = environment.child(
            BrowserChildKind::Viewer,
            WEBSOCKIFY,
            websockify_arguments(&environment.root().join(VIEW_ROUTES)),
        );
        self.websockify = Some(
            runner
                .spawn(&websockify, deadline)
                .map_err(ViewerStartFailure::Spawn)?,
        );
        Ok(())
    }

    fn launch(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        deadline: Instant,
    ) -> Result<(), ViewerStartFailure> {
        self.screen(environment, runner, false, deadline, &|| {
            Instant::now() >= deadline
        })?;
        // websockify starts only after the file it reads its targets from.
        write_routes(environment.root(), ViewerRoutes::View).map_err(ViewerStartFailure::Spawn)?;
        self.spawn_websockify(environment, runner, deadline)
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
        self.discard_password_file();
    }

    fn discard_password_file(&mut self) {
        if let Some(path) = self.password_file.take() {
            let _ = fs::remove_file(path);
        }
    }

    /// A failed viewer offers no route.
    fn mark_failed(&mut self, reason: &str) {
        if let Some(root) = self.root.as_deref() {
            let _ = write_routes(root, ViewerRoutes::Empty);
        }
        self.state = BrowserViewState::Failed;
        self.reason = Some(reason.to_owned());
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
        if self.secret.is_none() || view_routes(environment.root(), ViewerRoutes::View).is_none() {
            self.state = BrowserViewState::Failed;
            self.reason = Some("startup_failed".to_owned());
            return Err(BrowserError::Protocol);
        }
        self.root = Some(environment.root().to_path_buf());
        self.state = BrowserViewState::Starting;
        match self.launch(environment, runner, deadline) {
            Ok(()) => {
                self.state = BrowserViewState::Open;
                self.reason = None;
                Ok(())
            }
            Err(failure) => {
                self.cleanup();
                self.mark_failed(failure.reason());
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
            self.mark_failed("process_exited");
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
            url: (self.state == BrowserViewState::Open).then(|| self.url.clone()),
            url_ignored: self.url_ignored,
        }
    }

    fn routes(
        &mut self,
        environment: &BrowserEnvironment,
        routes: ViewerRoutes,
    ) -> Result<(), BrowserError> {
        write_routes(environment.root(), routes)
    }

    fn stop_websockify(&mut self, runner: &dyn BrowserProcessRunner, wait: &ViewerWait) -> bool {
        if let Some(websockify) = self.websockify.as_mut() {
            websockify.stop();
        }
        loop {
            // A connection process that looked its target up and has not yet
            // connected lives in websockify's process group, so the group is
            // checked as well as the process itself.
            let gone = self.websockify.as_mut().is_none_or(|websockify| {
                websockify.exited().unwrap_or(false) && !websockify.group_alive()
            });
            if gone
                && runner.viewer_listener(ViewerListenAddress::Any, VIEWER_PORT, None)
                    == ViewerListener::Absent
            {
                self.websockify = None;
                return true;
            }
            if wait.expired() {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop_screen(&mut self, runner: &dyn BrowserProcessRunner, wait: &ViewerWait) -> bool {
        if let Some(x11vnc) = self.x11vnc.as_mut() {
            x11vnc.stop();
        }
        self.discard_password_file();
        loop {
            let gone = self
                .x11vnc
                .as_mut()
                .is_none_or(|x11vnc| x11vnc.exited().unwrap_or(false));
            if gone
                && runner.viewer_listener(ViewerListenAddress::Loopback, VNC_PORT, None)
                    == ViewerListener::Absent
            {
                self.x11vnc = None;
                return true;
            }
            if wait.expired() {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn start_screen(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        input: bool,
        clock: &dyn BrowserClock,
    ) -> Result<(), String> {
        let until = clock.now_ms() + VIEWER_START_LIMIT.as_millis() as u64;
        let started = self.screen(
            environment,
            runner,
            input,
            Instant::now() + VIEWER_START_LIMIT,
            &|| clock.now_ms() >= until,
        );
        started.map_err(|failure| {
            self.discard_password_file();
            failure.reason().to_owned()
        })
    }

    fn start_websockify(
        &mut self,
        environment: &BrowserEnvironment,
        runner: &dyn BrowserProcessRunner,
        clock: &dyn BrowserClock,
    ) -> Result<(), String> {
        let until = clock.now_ms() + VIEWER_START_LIMIT.as_millis() as u64;
        self.spawn_websockify(environment, runner, Instant::now() + VIEWER_START_LIMIT)
            .map_err(|failure| failure.reason().to_owned())?;
        loop {
            let Some(child) = self.websockify.as_mut() else {
                return Err(ViewerStartFailure::ProcessExited.reason().to_owned());
            };
            if child.exited().unwrap_or(true) {
                return Err(ViewerStartFailure::ProcessExited.reason().to_owned());
            }
            // websockify listens on every address, which a loopback-only
            // check never sees.
            match runner.viewer_listener(ViewerListenAddress::Any, VIEWER_PORT, child.process_id())
            {
                ViewerListener::OwnedByChild => {
                    self.state = BrowserViewState::Open;
                    self.reason = None;
                    return Ok(());
                }
                ViewerListener::Foreign => {
                    return Err(ViewerStartFailure::PortInUse.reason().to_owned())
                }
                ViewerListener::Absent => {}
            }
            if clock.now_ms() >= until {
                return Err(ViewerStartFailure::Timeout.reason().to_owned());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn fail(&mut self, _environment: &BrowserEnvironment, reason: &str) {
        self.cleanup();
        self.mark_failed(reason);
    }

    fn password(&self) -> Option<&str> {
        self.secret.as_deref()
    }
}

fn random_name(prefix: &str) -> Result<String, BrowserError> {
    let mut random = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|_| BrowserError::Unavailable)?;
    let suffix: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("{prefix}{suffix}"))
}

/// Replaces `view-routes` with one rename, so websockify, which reads the
/// file again for every connection, never sees half of it.
fn write_routes(root: &Path, routes: ViewerRoutes) -> Result<(), BrowserError> {
    let text = view_routes(root, routes).ok_or(BrowserError::Unavailable)?;
    let temporary = root.join(random_name("routes-")?);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(&temporary)
        .map_err(|_| BrowserError::Unavailable)?;
    let written = file.write_all(text.as_bytes());
    drop(file);
    if written.is_err() || fs::rename(&temporary, root.join(VIEW_ROUTES)).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(BrowserError::Unavailable);
    }
    Ok(())
}

/// A new 0600 regular file with an unpredictable name inside `root`, holding
/// the password as its only line, closed before this returns.
fn create_password_file(root: &Path, secret: &str) -> Result<PathBuf, BrowserError> {
    let path = root.join(random_name("view-")?);
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
